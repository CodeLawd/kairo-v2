//! `kairo-settings.json` and the launch-time repairs of
//! `src/main/db/index.ts`, in the same order, plus `settings:get`,
//! `settings:set` and `settings:getAll` from `src/main/ipc/index.ts`.
//!
//! The repairs work on the stored JSON as electron-store hands it over, not
//! on typed values: a store written by any older build, or edited by hand,
//! must come out exactly as Electron would leave it. Where Electron would
//! throw on a hostile shape (a section that is `null`, say), the value is read
//! as an empty object instead, and the launch goes on.

use crate::json_store::{JsonStore, ListenerId, OpenState};
use crate::paths::renamed_store;
use kairo_domain::cloud::SecretsConfigured;
use kairo_domain::cloud::{LYRICS_SECRET_KEYS, STT_SECRET_KEYS, merge_secret_section};
use kairo_domain::js::{trim, truthy, value_to_string};
use kairo_domain::json::to_js_value;
use kairo_domain::lyrics::style::DEFAULT_GLOSS_COLOR;
use kairo_domain::normalize::{as_object, is_hex_color};
use kairo_domain::overlay::{
    CustomOverlayTheme, OverlayContentKind, OverlayOutputKind, OverlayTheme,
};
use kairo_domain::overlay_defaults::{DEFAULT_OVERLAY_SETTINGS, normalize_overlay_settings};
use kairo_domain::pp_retirement::retire_pro_presenter_settings;
use kairo_domain::program::normalize_presentation_settings;
use kairo_domain::settings::{AppSettings, DEFAULT_TRANSLATION_ID, SettingsSecretClearKey};
use kairo_domain::theme_library::{
    assign_theme_to_output, is_untouched_theme_library, normalize_theme_library,
    seed_built_in_themes,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};
use std::path::Path;

type Data = Map<String, Value>;

/// The settings store and its legacy name.
pub const SETTINGS_STORE: &str = "kairo-settings";
pub const SETTINGS_LEGACY_STORE: &str = "proautomate-settings";
/// The migration flags store and its legacy name.
pub const MIGRATIONS_STORE: &str = "kairo-migrations";
pub const MIGRATIONS_LEGACY_STORE: &str = "proautomate-migrations";

/// The last release without the What's New sheet.
pub const LAST_VERSION_BEFORE_WHATS_NEW: &str = "1.0.28";
/// The old gold gloss default that the palette colour
/// ([`DEFAULT_GLOSS_COLOR`]) replaced.
pub const OLD_GOLD_GLOSS_COLOR: &str = "#D4A017";

/// Time and random ids for the repairs that create themes. Electron uses
/// `Date.now()` and `crypto.randomUUID()`; tests pin both.
pub struct LaunchContext<'a> {
    pub now_ms: f64,
    pub new_id: &'a mut dyn FnMut() -> String,
}

/// A v4 UUID, as `crypto.randomUUID()` makes.
pub fn random_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// The current time in milliseconds, as `Date.now()`.
pub fn now_ms() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64
}

/// `defaults` in `db/index.ts`, with its keys in the literal's order (the
/// order a fresh `kairo-settings.json` has, which `AppSettings` declares).
pub fn settings_defaults() -> Data {
    match to_js_value(&AppSettings::defaults(DEFAULT_OVERLAY_SETTINGS.clone()))
        .expect("defaults serialize")
    {
        Value::Object(all) => all,
        _ => unreachable!("settings serialize as an object"),
    }
}

/// The one-time repairs recorded in `kairo-migrations.json`, in the order of
/// its defaults literal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigrationFlag {
    NkjvDefault,
    KjvDefault,
    BuiltInThemes,
    GlossPalette,
    CustomThemeLibrary,
    CloudOnboarding,
    SongsFolderExport,
    DeepseekDefault,
}

impl MigrationFlag {
    pub const ALL: [MigrationFlag; 8] = [
        MigrationFlag::NkjvDefault,
        MigrationFlag::KjvDefault,
        MigrationFlag::BuiltInThemes,
        MigrationFlag::GlossPalette,
        MigrationFlag::CustomThemeLibrary,
        MigrationFlag::CloudOnboarding,
        MigrationFlag::SongsFolderExport,
        MigrationFlag::DeepseekDefault,
    ];

    /// The flag's key in the file.
    pub fn as_str(self) -> &'static str {
        match self {
            MigrationFlag::NkjvDefault => "nkjvDefaultV1",
            MigrationFlag::KjvDefault => "kjvDefaultV1",
            MigrationFlag::BuiltInThemes => "builtInThemesV1",
            MigrationFlag::GlossPalette => "glossPaletteV1",
            MigrationFlag::CustomThemeLibrary => "customThemeLibraryV1",
            MigrationFlag::CloudOnboarding => "cloudOnboardingV1",
            MigrationFlag::SongsFolderExport => "songsFolderExportV1",
            MigrationFlag::DeepseekDefault => "deepseekDefaultV1",
        }
    }
}

/// The flags `migrations` starts with, in the literal's order.
pub fn migration_defaults() -> Data {
    MigrationFlag::ALL
        .into_iter()
        .map(|flag| (flag.as_str().to_string(), Value::Bool(false)))
        .collect()
}

/// `{ ...value }`: an object's keys, an array's or string's by index, else nothing.
pub fn spread(value: Option<&Value>) -> Data {
    crate::json_store::js_assign_source(value.cloned().unwrap_or(Value::Null))
}

fn prop<'a>(value: Option<&'a Value>, key: &str) -> Option<&'a Value> {
    as_object(value).get(key)
}

/// The secret fields of `section` that never leave the store, if it has any.
fn secret_keys(section: &str) -> Option<&'static [SettingsSecretClearKey]> {
    match section {
        "stt" => Some(STT_SECRET_KEYS),
        "lyrics" => Some(LYRICS_SECRET_KEYS),
        _ => None,
    }
}

/// `section` with its secret fields blanked.
fn redacted(section: Option<&Value>, keys: &[SettingsSecretClearKey]) -> Value {
    let mut out = spread(section);
    for key in keys {
        out.insert(key.as_str().into(), json!(""));
    }
    Value::Object(out)
}

/// `upgradedFromBeforeWhatsNew`: a settings file that exists, parses and has
/// no `whatsNew` key belongs to an install from before 1.0.29.
pub fn upgraded_from_before_whats_new(file: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(file) else {
        return false;
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(raw)) => !raw.contains_key("whatsNew"),
        // `'whatsNew' in array` is false; `in` on anything else throws, which counts as no.
        Ok(Value::Array(_)) => true,
        _ => false,
    }
}

/// Something a `settings:set` asks the caller to do once the value is stored.
#[derive(Clone, Debug, PartialEq)]
pub enum SettingsEffect {
    /// Open, close or move screen windows (`surfaceManager.reconcile`). With
    /// outputs after an `overlay` set; without after a `presentation` set.
    ReconcileOutputs(Option<Value>),
    /// Send `settings:changed` with [`SettingsStore::for_renderer`].
    BroadcastSettingsChanged,
    /// `cloudSession.syncOrgProfile` with the church's name, timezone and service times.
    SyncChurchProfile(Value),
    /// Pass the new scripture settings to the scripture service and orchestrator.
    ApplyScripture(Value),
    /// `programService.pruneProps`.
    PruneProps,
}

/// Why a `settings:set` was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettingsSetError {
    /// Older renderer sessions cannot bring back retired settings.
    Retired,
}

impl std::fmt::Display for SettingsSetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SettingsSetError::Retired => write!(f, "ProPresenter settings are no longer supported"),
        }
    }
}

impl std::error::Error for SettingsSetError {}

/// Services a post-open repair needs that the store does not have.
pub trait PostOpenServices {
    /// Whether the local Bible pack for `translation` is installed.
    fn bible_pack_installed(&self, translation: &str) -> Result<bool, String>;
}

/// The settings store, opened and repaired.
pub struct SettingsStore {
    store: JsonStore,
    migrations: JsonStore,
    upgraded_from_before_whats_new: bool,
}

impl SettingsStore {
    /// Opens the stores in `user_data` and runs every launch repair of
    /// `db/index.ts`, in order.
    pub fn open(user_data: &Path, ctx: &mut LaunchContext<'_>) -> SettingsStore {
        let name = renamed_store(user_data, SETTINGS_LEGACY_STORE, SETTINGS_STORE);
        let upgraded =
            upgraded_from_before_whats_new(&crate::json_store::store_path(user_data, name));
        let mut store = JsonStore::open(user_data, name, settings_defaults());
        if upgraded {
            store.set(
                "whatsNew",
                json!({ "lastSeenVersion": LAST_VERSION_BEFORE_WHATS_NEW }),
            );
        }
        let migrations_name = renamed_store(user_data, MIGRATIONS_LEGACY_STORE, MIGRATIONS_STORE);
        let migrations = JsonStore::open(user_data, migrations_name, migration_defaults());
        let mut settings = SettingsStore {
            store,
            migrations,
            upgraded_from_before_whats_new: upgraded,
        };
        settings.repair(ctx);
        settings
    }

    fn flag(&self, flag: MigrationFlag) -> bool {
        truthy(self.migrations.get(flag.as_str()))
    }

    fn set_flag(&mut self, flag: MigrationFlag) {
        self.migrations.set(flag.as_str(), Value::Bool(true));
    }

    /// Whether a one-time service migration has been recorded.
    pub fn migration_done(&self, flag: MigrationFlag) -> bool {
        self.flag(flag)
    }

    /// Record a service migration after its data has been saved.
    pub fn mark_migration_done(&mut self, flag: MigrationFlag) {
        self.set_flag(flag);
    }

    /// `store.set(section, { ...store.get(section), ...fields })`, for
    /// services that update fields of a section and keep the rest as stored.
    pub fn merge_fields<'k>(
        &mut self,
        section: &str,
        fields: impl IntoIterator<Item = (&'k str, Value)>,
    ) {
        merge_fields(&mut self.store, section, fields);
    }

    /// `normalizeThemeLibrary(raw, theme, Date.now(), newId())`. The id is
    /// made on every call, used or not, as the default argument is.
    fn normalized_library(
        &self,
        raw: Option<&Value>,
        theme: &OverlayTheme,
        ctx: &mut LaunchContext<'_>,
    ) -> Vec<CustomOverlayTheme> {
        normalize_theme_library(raw, theme, ctx.now_ms, &(ctx.new_id)())
    }

    fn repair(&mut self, ctx: &mut LaunchContext<'_>) {
        // Every repair lands in one write of each file.
        let SettingsStore {
            store, migrations, ..
        } = self;
        store.batch(|store| migrations.batch(|migrations| repair(store, migrations, ctx)));
    }

    /// `migrateUnreadableNkjvDefault` from `src/main/index.ts`, run once the
    /// scripture service exists (prompt 067 calls it at app ready, before the
    /// window loads): an install left on NKJV with no local pack and no
    /// API.Bible key moves to the bundled KJV. A failure is logged and the
    /// flag is still set, as in Electron.
    pub fn migrate_unreadable_nkjv_default(&mut self, services: &dyn PostOpenServices) {
        if self.flag(MigrationFlag::KjvDefault) {
            return;
        }
        let result = (|| -> Result<(), String> {
            let scripture = self.store.get("scripture").cloned();
            let bible_key = prop(self.store.get("stt"), "bibleApiKey")
                .and_then(Value::as_str)
                .is_some_and(|k| !trim(k).is_empty());
            let readable = services.bible_pack_installed("NKJV")? || bible_key;
            let translation = prop(scripture.as_ref(), "defaultTranslation")
                .and_then(Value::as_str)
                .ok_or("scripture.defaultTranslation is not text")?;
            if translation.to_uppercase() == "NKJV" && !readable {
                self.merge_fields("scripture", [("defaultTranslation", json!("KJV"))]);
                tracing::info!("[Scripture] Default moved from unreadable NKJV to bundled KJV");
            }
            Ok(())
        })();
        if let Err(error) = result {
            tracing::warn!(%error, "[Scripture] KJV default migration skipped");
        }
        self.set_flag(MigrationFlag::KjvDefault);
    }

    /// Whether this launch upgraded a settings file from before What's New.
    pub fn upgraded_from_before_whats_new(&self) -> bool {
        self.upgraded_from_before_whats_new
    }

    /// The raw stored settings.
    pub fn data(&self) -> &Data {
        self.store.data()
    }

    /// What opening `kairo-settings.json` found.
    pub fn open_state(&self) -> &OpenState {
        self.store.open_state()
    }

    /// The migration flags.
    pub fn migrations(&self) -> &Data {
        self.migrations.data()
    }

    /// A section read as `T`.
    pub fn section<T: DeserializeOwned>(&self, key: &str) -> Result<T, serde_json::Error> {
        self.store.get_as(key)
    }

    /// Writes a section from `T`.
    pub fn set_section<T: Serialize>(
        &mut self,
        key: &str,
        value: &T,
    ) -> Result<(), serde_json::Error> {
        self.store.set_as(key, value)
    }

    /// `store.set(key, value)` with the JSON as given, for services that
    /// update one field of a section and keep the rest as stored.
    pub fn set_raw(&mut self, key: &str, value: Value) {
        self.store.set(key, value);
    }

    /// `store.onDidChange(section, callback)`.
    pub fn on_did_change(
        &mut self,
        section: &str,
        callback: impl FnMut(&Value, &Value) + Send + 'static,
    ) -> ListenerId {
        self.store.on_did_change(section, callback)
    }

    pub fn remove_listener(&mut self, id: ListenerId) {
        self.store.remove_listener(id);
    }

    /// Waits for both files to reach disk.
    pub fn flush(&self) -> Result<(), String> {
        self.store.flush().and(self.migrations.flush())
    }

    /// The latest write failure of either file.
    pub fn last_write_error(&self) -> Option<String> {
        self.store
            .last_write_error()
            .or_else(|| self.migrations.last_write_error())
    }

    fn overlay(&self) -> kairo_domain::settings::OverlaySettings {
        normalize_overlay_settings(self.store.get("overlay"))
    }

    /// `settingsForRenderer`: the stored settings with overlay, presentation
    /// and theme library normalised, secrets blanked, and which provider keys
    /// are configured (from the licence, prompt 032).
    pub fn for_renderer(
        &self,
        secrets_configured: &SecretsConfigured,
        ctx: &mut LaunchContext<'_>,
    ) -> Value {
        let mut raw = self.store.data().clone();
        let overlay = self.overlay();
        let library = self.normalized_library(self.store.get("themeLibrary"), &overlay.theme, ctx);
        raw.insert("overlay".into(), to_js_value(&overlay).expect("overlay"));
        raw.insert(
            "presentation".into(),
            to_js_value(&normalize_presentation_settings(
                self.store.get("presentation").unwrap_or(&Value::Null),
            ))
            .expect("presentation"),
        );
        raw.insert(
            "themeLibrary".into(),
            to_js_value(&library).expect("library"),
        );
        for section in ["stt", "lyrics"] {
            let keys = secret_keys(section).expect("a secret section");
            raw.insert(section.into(), redacted(raw.get(section), keys));
        }
        raw.insert(
            "secretsConfigured".into(),
            to_js_value(secrets_configured).expect("secrets"),
        );
        Value::Object(raw)
    }

    /// `settings:get`. Absent keys are `null`.
    pub fn get_for_renderer(&self, key: &str, ctx: &mut LaunchContext<'_>) -> Value {
        if let Some(keys) = secret_keys(key) {
            return redacted(self.store.get(key), keys);
        }
        match key {
            "overlay" => to_js_value(&self.overlay()).expect("overlay"),
            "themeLibrary" => {
                let library = self.normalized_library(
                    self.store.get("themeLibrary"),
                    &self.overlay().theme,
                    ctx,
                );
                to_js_value(&library).expect("library")
            }
            _ => self.store.get(key).cloned().unwrap_or(Value::Null),
        }
    }

    /// `settings:set`: stores `value` under `key` the way the IPC handler
    /// does, and returns what the caller must then do.
    pub fn apply_set(
        &mut self,
        key: &str,
        value: Value,
        ctx: &mut LaunchContext<'_>,
    ) -> Result<Vec<SettingsEffect>, SettingsSetError> {
        if key == "propresenter" || key == "propresenterResources" {
            return Err(SettingsSetError::Retired);
        }
        let mut effects = Vec::new();
        match key {
            "overlay" => {
                // Merged with what is stored, so a page editing one slice cannot drop the rest.
                let mut merged = spread(self.store.get("overlay"));
                merged.extend(spread(Some(&value)));
                let normalized = normalize_overlay_settings(Some(&Value::Object(merged)));
                let outputs = to_js_value(&normalized.outputs).expect("outputs");
                self.store.set_as("overlay", &normalized).expect("overlay");
                effects.push(SettingsEffect::ReconcileOutputs(Some(outputs)));
            }
            "themeLibrary" => {
                let library = self.normalized_library(Some(&value), &self.overlay().theme, ctx);
                self.store
                    .set_as("themeLibrary", &library)
                    .expect("library");
            }
            "stt" | "lyrics" => {
                let keys = secret_keys(key).expect("a secret section");
                let merged =
                    merge_secret_section(&spread(self.store.get(key)), &spread(Some(&value)), keys);
                // Provider keys live in the church account, never in this file.
                self.store
                    .set(key, redacted(Some(&Value::Object(merged)), keys));
                effects.push(SettingsEffect::BroadcastSettingsChanged);
            }
            "capture" => {
                let mut merged = spread(self.store.get("capture"));
                merged.extend(spread(Some(&value)));
                let capture =
                    kairo_domain::capture::normalize_capture_settings(&Value::Object(merged));
                self.store.set_as("capture", &capture).expect("capture");
            }
            "web" => {
                let web = kairo_domain::web::normalize_web_settings(&value);
                self.store.set_as("web", &web).expect("web");
            }
            "documents" => {
                let documents = kairo_domain::documents::normalize_documents_settings(&value);
                self.store
                    .set_as("documents", &documents)
                    .expect("documents");
            }
            "presentation" => {
                let presentation = normalize_presentation_settings(&value);
                self.store
                    .set_as("presentation", &presentation)
                    .expect("presentation");
                effects.push(SettingsEffect::PruneProps);
                effects.push(SettingsEffect::ReconcileOutputs(None));
            }
            _ => {
                self.store.set(key, value.clone());
                if key == "church" {
                    effects.push(SettingsEffect::SyncChurchProfile(json!({
                        "name": prop(Some(&value), "name").cloned().unwrap_or(Value::Null),
                        "timezone": prop(Some(&value), "timezone").cloned().unwrap_or(Value::Null),
                        "serviceTimes": prop(Some(&value), "serviceTimes").cloned().unwrap_or(Value::Null),
                    })));
                }
            }
        }
        if key == "scripture" {
            effects.push(SettingsEffect::ApplyScripture(value));
        }
        tracing::debug!(key, "Setting updated");
        Ok(effects)
    }
}

/// `store.set(section, { ...store.get(section), ...fields })`.
fn merge_fields<'k>(
    store: &mut JsonStore,
    section: &str,
    fields: impl IntoIterator<Item = (&'k str, Value)>,
) {
    let mut next = spread(store.get(section));
    for (field, value) in fields {
        next.insert(field.into(), value);
    }
    store.set(section, Value::Object(next));
}

/// Every launch repair of `db/index.ts`, in order.
fn repair(store: &mut JsonStore, migrations: &mut JsonStore, ctx: &mut LaunchContext<'_>) {
    let flag = |migrations: &JsonStore, flag: MigrationFlag| truthy(migrations.get(flag.as_str()));
    let set_flag = |migrations: &mut JsonStore, flag: MigrationFlag| {
        migrations.set(flag.as_str(), Value::Bool(true))
    };

    // AI detection moved to DeepSeek, once; a model name from the old provider is cleared.
    if !flag(migrations, MigrationFlag::DeepseekDefault) {
        let stt = store.get("stt");
        if prop(stt, "llmProvider") != Some(&json!("deepseek")) || truthy(prop(stt, "llmModel")) {
            merge_fields(
                store,
                "stt",
                [("llmProvider", json!("deepseek")), ("llmModel", json!(""))],
            );
        }
        set_flag(migrations, MigrationFlag::DeepseekDefault);
    }

    if !flag(migrations, MigrationFlag::NkjvDefault) {
        merge_fields(
            store,
            "scripture",
            [("defaultTranslation", json!(DEFAULT_TRANSLATION_ID))],
        );
        set_flag(migrations, MigrationFlag::NkjvDefault);
    }

    // Shallow-merge heal: scripture from before the offline cache.
    if !prop(store.get("scripture"), "offlineDownloadBibleIds").is_some_and(Value::is_array) {
        merge_fields(store, "scripture", [("offlineDownloadBibleIds", json!([]))]);
    }

    // Lyrics: a translate key that is not text, and a gloss colour that is not a hex colour.
    {
        let lyrics = store.get("lyrics");
        let mut fixes = Vec::new();
        if !prop(lyrics, "googleTranslateApiKey").is_some_and(Value::is_string) {
            fixes.push(("googleTranslateApiKey", json!("")));
        }
        if !prop(lyrics, "glossColor")
            .and_then(Value::as_str)
            .is_some_and(|color| is_hex_color(trim(color)))
        {
            fixes.push(("glossColor", json!(DEFAULT_GLOSS_COLOR)));
        }
        if !fixes.is_empty() {
            merge_fields(store, "lyrics", fixes);
        }
    }

    // Workspace from before the Songs folder: keep the default Documents location.
    {
        let workspace = store.get("workspace");
        if !truthy(workspace) || !prop(workspace, "folder").is_some_and(Value::is_string) {
            store.set("workspace", json!({ "folder": "" }));
        }
    }

    // Church from before the onboarding wizard.
    {
        let church = store.get("church");
        let text = |key| {
            prop(church, key)
                .filter(|v| v.is_string())
                .cloned()
                .unwrap_or(json!(""))
        };
        let times = prop(church, "serviceTimes").filter(|v| v.is_array());
        if !truthy(church) || !prop(church, "name").is_some_and(Value::is_string) || times.is_none()
        {
            let healed = json!({
                "name": text("name"),
                "timezone": text("timezone"),
                "role": text("role"),
                "serviceTimes": times.cloned().unwrap_or(json!([])),
            });
            store.set("church", healed);
        }
    }

    // Retired ProPresenter settings, before any normaliser reads old destinations.
    let retired = retire_pro_presenter_settings(Some(&Value::Object(store.data().clone())));
    store.replace(retired);

    let mut overlay = normalize_overlay_settings(store.get("overlay"));
    store
        .set_as("overlay", &overlay)
        .expect("overlay serializes");
    let presentation =
        normalize_presentation_settings(store.get("presentation").unwrap_or(&Value::Null));
    store
        .set_as("presentation", &presentation)
        .expect("presentation serializes");

    // `normalizeThemeLibrary(raw, theme, now = Date.now(), legacyId = newId())`:
    // the default id is made on every call, used or not.
    let library_migrated = flag(migrations, MigrationFlag::CustomThemeLibrary);
    let raw_library = if library_migrated {
        store.get("themeLibrary")
    } else {
        None
    };
    let legacy_id = (ctx.new_id)();
    let library = normalize_theme_library(raw_library, &overlay.theme, ctx.now_ms, &legacy_id);
    store
        .set_as("themeLibrary", &library)
        .expect("library serializes");
    if !library_migrated {
        set_flag(migrations, MigrationFlag::CustomThemeLibrary);
    }

    // A new install gets every built-in theme, once; a shaped library is left alone.
    if !flag(migrations, MigrationFlag::BuiltInThemes) {
        if is_untouched_theme_library(&library) {
            let seeded = seed_built_in_themes(&library, ctx.now_ms, &mut *ctx.new_id);
            let broadcast = seeded
                .iter()
                .find(|t| t.kind == OverlayContentKind::Scripture);
            let stage_lyrics = seeded.iter().find(|t| t.kind == OverlayContentKind::Lyrics);
            for output in &mut overlay.outputs {
                if output.kind != OverlayOutputKind::Screen {
                    continue;
                }
                if let Some(theme) = broadcast {
                    *output = assign_theme_to_output(output, theme);
                }
                if let Some(theme) = stage_lyrics {
                    *output = assign_theme_to_output(output, theme);
                }
            }
            store
                .set_as("themeLibrary", &seeded)
                .expect("library serializes");
            let raw = to_js_value(&overlay).expect("overlay serializes");
            store
                .set_as("overlay", &normalize_overlay_settings(Some(&raw)))
                .expect("overlay serializes");
        }
        set_flag(migrations, MigrationFlag::BuiltInThemes);
    }

    // The old gold gloss default moves to the palette blue; a picked colour stays.
    if !flag(migrations, MigrationFlag::GlossPalette) {
        let color = prop(store.get("lyrics"), "glossColor")
            .filter(|v| !v.is_null())
            .map(value_to_string)
            .unwrap_or_default();
        if color.to_uppercase() == OLD_GOLD_GLOSS_COLOR {
            merge_fields(
                store,
                "lyrics",
                [("glossColor", json!(DEFAULT_GLOSS_COLOR))],
            );
        }
        set_flag(migrations, MigrationFlag::GlossPalette);
    }
}

/// A store file other than settings: its name, the name it had before the
/// rename (copied once, see [`renamed_store`]) and its defaults.
#[derive(Clone, Debug)]
pub struct StoreSpec {
    pub name: &'static str,
    pub legacy: Option<&'static str>,
    pub defaults: fn() -> Data,
}

const fn spec(name: &'static str, defaults: fn() -> Data) -> StoreSpec {
    StoreSpec {
        name,
        legacy: None,
        defaults,
    }
}

fn object(value: Value) -> Data {
    match value {
        Value::Object(map) => map,
        _ => Data::new(),
    }
}

fn library_defaults() -> Data {
    let empty = || json!({ "libraries": [], "assignments": {} });
    object(json!({ "songs": empty(), "documents": empty(), "scripture": empty() }))
}

/// Every electron-store file the Electron app keeps besides settings.
pub const STORES: &[StoreSpec] = &[
    spec("song-usage", || object(json!({ "entries": [] }))),
    spec("kairo-crash-reports", || object(json!({ "reports": [] }))),
    spec("kairo-libraries", library_defaults),
    spec("kairo-usage", || object(json!({ "days": {} }))),
    spec("kairo-setlist", || {
        object(json!({ "lists": [], "activeId": null }))
    }),
    spec("kairo-services", || {
        object(json!({ "activeId": null, "services": [] }))
    }),
    StoreSpec {
        name: "kairo-onboarding",
        legacy: Some("proautomate-onboarding"),
        defaults: || {
            object(json!({ "onboarding": {
                "completedSteps": [], "skippedSteps": [], "currentStep": "welcome", "completedAt": null, "source": "fresh"
            } }))
        },
    },
    StoreSpec {
        name: "kairo-sermon-plans",
        legacy: Some("proautomate-sermon-plans"),
        defaults: || object(json!({ "plans": [], "livePlanId": null })),
    },
    spec("kairo-passages", || object(json!({ "passages": [] }))),
];

/// The spec named `name`.
pub fn store_spec(name: &str) -> Option<&'static StoreSpec> {
    STORES.iter().find(|spec| spec.name == name)
}

/// Opens one of [`STORES`], adopting its legacy file first.
pub fn open_store(user_data: &Path, spec: &StoreSpec) -> JsonStore {
    let name = match spec.legacy {
        Some(legacy) => renamed_store(user_data, legacy, spec.name),
        None => spec.name,
    };
    JsonStore::open(user_data, name, (spec.defaults)())
}

/// Per-machine view preferences that Electron kept in the renderer's
/// `localStorage` (panel widths, collapsed sections, last tabs). Prompt 100
/// imports the old values; the keys stay the ones the renderer used.
pub const UI_STATE_STORE: &str = "ui-state";

/// Opens `ui-state.json`.
pub fn open_ui_state(user_data: &Path) -> JsonStore {
    JsonStore::open(user_data, UI_STATE_STORE, Data::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_ids() -> impl FnMut() -> String {
        kairo_oracle::pinned_uuids()
    }

    #[test]
    fn spread_follows_javascript() {
        assert_eq!(Value::Object(spread(Some(&json!(null)))), json!({}));
        assert_eq!(
            Value::Object(spread(Some(&json!({ "a": 1 })))),
            json!({ "a": 1 })
        );
        assert_eq!(Value::Object(spread(None)), json!({}));
    }

    #[test]
    fn retired_keys_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let mut ids = ctx_ids();
        let mut ctx = LaunchContext {
            now_ms: 1.0,
            new_id: &mut ids,
        };
        let mut settings = SettingsStore::open(dir.path(), &mut ctx);
        assert_eq!(
            settings.apply_set("propresenter", json!({}), &mut ctx),
            Err(SettingsSetError::Retired)
        );
        assert!(!settings.data().contains_key("propresenter"));
    }
}
