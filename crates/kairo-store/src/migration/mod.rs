//! First-run migration from the Electron app (prompt 100).
//!
//! The native app shares Electron's userData, stores, databases and
//! workspace (architecture §7), so most data needs no conversion. On the
//! first native launch, before any store opens, this:
//!
//! 1. backs up the settings and migration files (every top-level `*.json`,
//!    `Preferences`, `cloud/` and `api-bible-cache.key`) to a dated folder;
//! 2. rewraps Electron's data keys into the native keyring (022), reading
//!    and deleting nothing they protect;
//! 3. imports the renderer's `localStorage` into `ui-state.json`;
//! 4. rewrites the saved microphone from Chromium's hashed id to the
//!    device's label, which the audio service turns into its id (074);
//! 5. checks every database and workspace folder.
//!
//! It never deletes Electron data. A run that cannot back up changes nothing
//! and tries again next launch. Afterwards `kairo-native-migration.json`
//! records the run, so it happens once.

pub mod devices;
pub mod local_storage;

use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::json_text::stringify_tabs_object;
use crate::secrets::{KeyMigration, KeyProtector, migrate_electron_keys};
use devices::{InputDevice, MicrophoneMatch, device_id_salt, match_microphone};
use local_storage::{FILE_ORIGIN, read_local_storage};

/// Written once the migration has run.
pub const MARKER_FILE: &str = "kairo-native-migration.json";
/// Where dated backups go, inside userData.
pub const BACKUP_DIR: &str = "Native Migration Backups";
/// The marker's format.
pub const MARKER_VERSION: u32 = 1;

const SETTINGS: &str = "kairo-settings";
const LEGACY_SETTINGS: &str = "proautomate-settings";
const UI_STATE: &str = "ui-state";
const DATABASES: &[&str] = &["lyrics.db", "bible.db", "api-bible-cache.db"];

/// One line of the result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    /// The operator should act on it; only these are shown.
    pub attention: bool,
    pub text: String,
}

impl Note {
    fn info(text: impl Into<String>) -> Self {
        Self {
            attention: false,
            text: text.into(),
        }
    }
    fn attention(text: impl Into<String>) -> Self {
        Self {
            attention: true,
            text: text.into(),
        }
    }
}

/// What a launch's migration did.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationReport {
    /// It ran this launch (false once the marker exists).
    pub ran: bool,
    /// Electron data was found (false on a fresh install).
    pub upgraded: bool,
    pub backup: Option<PathBuf>,
    pub notes: Vec<Note>,
    /// `localStorage` keys copied into `ui-state.json`.
    pub imported_keys: usize,
    /// Milliseconds per step (`backup`, `keys`, `localStorage`,
    /// `microphone`, `checks`), to tell work from a keychain prompt.
    #[serde(default)]
    pub steps_ms: Vec<(String, f64)>,
    pub elapsed_ms: f64,
}

impl MigrationReport {
    fn lap(&mut self, name: &str, since: Instant) {
        self.steps_ms
            .push((name.to_owned(), since.elapsed().as_secs_f64() * 1000.0));
    }

    /// The lines the operator should see; empty when all went well.
    pub fn attention(&self) -> Vec<&str> {
        self.notes
            .iter()
            .filter(|n| n.attention)
            .map(|n| n.text.as_str())
            .collect()
    }
}

/// What the migration needs from outside the store.
pub struct MigrationInput<'a> {
    pub user_data: &'a Path,
    /// `AppPaths::default_workspace_root`.
    pub default_workspace: PathBuf,
    pub protector: &'a dyn KeyProtector,
    /// The Electron product name for its wrapped keys (`Kairo`).
    pub electron_app: &'a str,
    /// Present audio inputs (cpal ids and labels); read only when the saved
    /// microphone is a Chromium id.
    pub devices: &'a dyn Fn() -> Vec<InputDevice>,
    /// Names the backup folder.
    pub now: chrono::DateTime<chrono::Local>,
}

/// Runs the first-run migration once per profile.
pub fn run(input: &MigrationInput<'_>) -> MigrationReport {
    let started = Instant::now();
    let user_data = input.user_data;
    let mut report = MigrationReport::default();
    if user_data.join(MARKER_FILE).exists() {
        report.elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
        return report;
    }
    report.ran = true;
    let settings_file = [SETTINGS, LEGACY_SETTINGS]
        .iter()
        .map(|name| user_data.join(format!("{name}.json")))
        .find(|p| p.is_file());
    report.upgraded = settings_file.is_some();
    if report.upgraded {
        let step = Instant::now();
        let backed_up = back_up(user_data, input.now);
        report.lap("backup", step);
        match backed_up {
            Ok(dir) => report.backup = Some(dir),
            Err(error) => {
                tracing::error!(%error, "[Migration] backup failed; nothing changed");
                report.notes.push(Note::attention(format!(
                    "Kairo could not back up your settings before upgrading ({error}). Nothing was changed; it will try again next time it opens."
                )));
                report.elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
                return report;
            }
        }
        let step = Instant::now();
        migrate_keys(input, &mut report);
        report.lap("keys", step);
        let step = Instant::now();
        import_local_storage(user_data, &mut report);
        report.lap("localStorage", step);
        // Settings repairs adopt the legacy name later; read whichever exists.
        if let Some(file) = &settings_file {
            let step = Instant::now();
            migrate_microphone(input, file, &mut report);
            report.lap("microphone", step);
        }
    }
    let step = Instant::now();
    verify(input, settings_file.as_deref(), &mut report);
    report.lap("checks", step);
    report.elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    if let Err(error) = write_marker(user_data, &report) {
        tracing::warn!(%error, "[Migration] could not record the run; it will repeat");
    }
    for note in &report.notes {
        if note.attention {
            tracing::warn!("[Migration] {}", note.text);
        } else {
            tracing::info!("[Migration] {}", note.text);
        }
    }
    tracing::info!(
        ms = report.elapsed_ms,
        upgraded = report.upgraded,
        steps = ?report.steps_ms,
        "[Migration] first run done"
    );
    report
}

/// Copies the settings and migration files, key wraps and `Preferences` into
/// `<userData>/Native Migration Backups/<yyyy-mm-dd hh-mm-ss>/`.
pub fn back_up(user_data: &Path, now: chrono::DateTime<chrono::Local>) -> std::io::Result<PathBuf> {
    let root = user_data.join(BACKUP_DIR);
    let stamp = now.format("%Y-%m-%d %H-%M-%S").to_string();
    let mut dir = root.join(&stamp);
    let mut n = 2;
    while dir.exists() {
        dir = root.join(format!("{stamp} ({n})"));
        n += 1;
    }
    std::fs::create_dir_all(&dir)?;
    for entry in std::fs::read_dir(user_data)? {
        let entry = entry?;
        let name = entry.file_name();
        let text = name.to_string_lossy();
        let wanted = entry.file_type()?.is_file()
            && ((text.ends_with(".json") && !text.contains(".tmp-"))
                || text == "Preferences"
                || text == "api-bible-cache.key");
        if wanted {
            std::fs::copy(entry.path(), dir.join(&name))?;
        }
    }
    let cloud = user_data.join("cloud");
    if cloud.is_dir() {
        copy_dir(&cloud, &dir.join("cloud"))?;
    }
    Ok(dir)
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn migrate_keys(input: &MigrationInput<'_>, report: &mut MigrationReport) {
    for (purpose, result) in
        migrate_electron_keys(input.user_data, input.protector, input.electron_app)
    {
        match (purpose, result) {
            (_, KeyMigration::Rewrapped) => {
                report.notes.push(Note::info(format!("{purpose} key moved to the keychain")))
            }
            (_, KeyMigration::AlreadyNative | KeyMigration::NoElectronKey) => {}
            ("cloud", KeyMigration::Failed(error)) => report.notes.push(Note::attention(format!(
                "Your saved sign-in could not be moved to this version ({error}). You may need to sign in again."
            ))),
            (_, KeyMigration::Failed(error)) => report.notes.push(Note::info(format!(
                "{purpose} key not moved ({error}); the scripture cache will be rebuilt"
            ))),
        }
    }
}

fn read_object(path: &Path) -> Option<Map<String, Value>> {
    let text = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str(&text).ok()? {
        Value::Object(map) => Some(map),
        _ => None,
    }
}

fn write_object(path: &Path, map: &Map<String, Value>) -> std::io::Result<()> {
    crate::fs_atomic::write_atomic(path, stringify_tabs_object(map).as_bytes())
}

/// `localStorage` into `ui-state.json`, as the exact strings Electron kept;
/// every reader accepts that form. A value already in `ui-state` wins. A
/// failure leaves the layout at its defaults.
fn import_local_storage(user_data: &Path, report: &mut MigrationReport) {
    let entries = match read_local_storage(user_data, FILE_ORIGIN) {
        Ok(entries) => entries,
        Err(error) => {
            report.notes.push(Note::info(format!(
                "window layout not imported ({error}); it starts from the defaults"
            )));
            return;
        }
    };
    if entries.is_empty() {
        return;
    }
    let path = user_data.join(format!("{UI_STATE}.json"));
    let mut state = read_object(&path).unwrap_or_default();
    let mut added = 0;
    for (key, value) in entries {
        if !state.contains_key(&key) {
            state.insert(key, Value::String(value));
            added += 1;
        }
    }
    if added == 0 {
        return;
    }
    match write_object(&path, &state) {
        Ok(()) => {
            report.imported_keys = added;
            report
                .notes
                .push(Note::info(format!("{added} view preferences imported")));
        }
        Err(error) => report.notes.push(Note::info(format!(
            "window layout not imported ({error}); it starts from the defaults"
        ))),
    }
}

fn migrate_microphone(
    input: &MigrationInput<'_>,
    settings_file: &Path,
    report: &mut MigrationReport,
) {
    let Some(mut settings) = read_object(settings_file) else {
        return;
    };
    let saved = settings
        .get("audio")
        .and_then(|a| a.get("deviceId"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let hashed = saved.len() == 64 && saved.bytes().all(|b| b.is_ascii_hexdigit());
    if !hashed {
        return;
    }
    let salt = device_id_salt(input.user_data);
    let devices = (input.devices)();
    match match_microphone(&saved, salt.as_deref(), FILE_ORIGIN, &devices) {
        MicrophoneMatch::Unchanged => {}
        MicrophoneMatch::Found(device) => {
            let mut audio = settings
                .get("audio")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            audio.insert("deviceId".into(), Value::String(device.label.clone()));
            settings.insert("audio".into(), Value::Object(audio));
            match write_object(settings_file, &settings) {
                Ok(()) => report
                    .notes
                    .push(Note::info(format!("microphone matched: {}", device.label))),
                Err(error) => report.notes.push(Note::attention(format!(
                    "Your microphone choice could not be carried over ({error}). Choose it again in Settings › Audio."
                ))),
            }
        }
        MicrophoneMatch::NotConnected => report.notes.push(Note::attention(
            "The microphone chosen in the previous version isn’t connected. Kairo uses the system default until you choose one in Settings › Audio.",
        )),
    }
}

/// A folder from settings, trimmed; `None` when unset.
fn folder(settings: &Map<String, Value>, section: &str) -> Option<PathBuf> {
    settings
        .get(section)?
        .get("folder")?
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| crate::workspace::resolve(Path::new(s)))
}

/// Every database opens, and every chosen folder is there.
fn verify(input: &MigrationInput<'_>, settings_file: Option<&Path>, report: &mut MigrationReport) {
    for name in DATABASES {
        let path = input.user_data.join(name);
        if !path.exists() {
            continue;
        }
        let opened = rusqlite::Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .and_then(|c| c.query_row("PRAGMA schema_version", [], |r| r.get::<_, i64>(0)));
        if let Err(error) = opened {
            report.notes.push(Note::attention(format!(
                "{name} could not be opened ({error}). Your data is in the backup; contact support before changing anything."
            )));
        }
    }
    let settings = settings_file.and_then(read_object).unwrap_or_default();
    match folder(&settings, "workspace") {
        Some(root) if !root.is_dir() => report.notes.push(Note::attention(format!(
            "Your Kairo folder isn’t available: {}. Reconnect its drive, or choose the folder again in Settings.",
            root.display()
        ))),
        Some(_) => {}
        // The default is created when needed, inside a folder that must exist.
        None => {
            let root = &input.default_workspace;
            if !root.is_dir() && root.parent().is_some_and(|p| !p.is_dir()) {
                report.notes.push(Note::attention(format!(
                    "Kairo cannot create its folder at {}.",
                    root.display()
                )));
            }
        }
    }
    for (section, what) in [("media", "media"), ("tracks", "tracks")] {
        if let Some(path) = folder(&settings, section)
            && !path.is_dir()
        {
            report.notes.push(Note::attention(format!(
                "The {what} folder isn’t available: {}. If it is on an external drive, connect it.",
                path.display()
            )));
        }
    }
}

fn write_marker(user_data: &Path, report: &MigrationReport) -> std::io::Result<()> {
    let mut map = Map::new();
    map.insert("version".into(), Value::from(MARKER_VERSION));
    map.insert(
        "completedAt".into(),
        Value::String(chrono::Utc::now().to_rfc3339()),
    );
    map.insert(
        "report".into(),
        serde_json::to_value(report).unwrap_or(Value::Null),
    );
    write_object(&user_data.join(MARKER_FILE), &map)
}
