//! `AppSettings` and the settings file, from `src/lib/ipc.ts` and
//! `src/main/db/index.ts`.
//!
//! Two layers:
//!
//! - [`AppSettings`] is the canonical shape, exactly as TypeScript declares it.
//!   Every nested object keeps unknown keys in `extra`.
//! - [`StoredSettings`] is a settings file as found on disk. Files written by
//!   older builds lack sections and fields, and carry retired ones; electron-store
//!   only fills missing top-level keys, and the normalisers repair the rest
//!   (prompts 013 and 021). Each section is therefore [`Stored::Typed`] when it
//!   already has the canonical shape, and [`Stored::Raw`] otherwise, so any
//!   JSON object loads and saves without loss and nothing is normalised here.

use crate::capture::CaptureSettings;
use crate::cloud::SecretsConfigured;
use crate::documents::DocumentsSettings;
use crate::media::{MediaSettings, TracksSettings};
use crate::overlay::{CustomOverlayTheme, OverlayOutput, OverlayTheme};
use crate::program::PresentationSettings;
use crate::serde_util::{Extra, present, required, string_enum};
use crate::status::{AutoPresentDelaySec, LlmProvider, SttProvider};
use crate::web::WebSettings;
use crate::workspace::WorkspaceSettings;
use serde::{Deserialize, Serialize};

/// `DEFAULT_TRANSLATION_ID` in `bible-translations.ts`.
pub const DEFAULT_TRANSLATION_ID: &str = "KJV";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhatsNewSettings {
    /// `''` on a fresh install.
    pub last_seen_version: String,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioSettings {
    pub device_id: String,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SttSettings {
    pub provider: SttProvider,
    pub api_key: String,
    pub anthropic_api_key: String,
    pub deepseek_api_key: String,
    pub llm_provider: LlmProvider,
    /// Empty means the provider's default.
    pub llm_model: String,
    pub bible_api_key: String,
    pub language: String,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for SttSettings {
    fn default() -> Self {
        Self {
            provider: SttProvider::None,
            api_key: String::new(),
            anthropic_api_key: String::new(),
            deepseek_api_key: String::new(),
            llm_provider: LlmProvider::Deepseek,
            llm_model: String::new(),
            bible_api_key: String::new(),
            language: "en-US".into(),
            extra: Extra::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptureSettings {
    pub default_translation: String,
    pub show_verse_numbers: bool,
    pub auto_mode: bool,
    pub confidence_threshold: f64,
    pub auto_present_delay_sec: AutoPresentDelaySec,
    pub debounce_interval: f64,
    pub context_window_size: f64,
    /// API.Bible ids whose licence permits whole-translation offline download.
    pub offline_download_bible_ids: Vec<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for ScriptureSettings {
    fn default() -> Self {
        Self {
            default_translation: DEFAULT_TRANSLATION_ID.into(),
            show_verse_numbers: true,
            auto_mode: true,
            confidence_threshold: 0.7,
            auto_present_delay_sec: AutoPresentDelaySec::One,
            debounce_interval: 8.0,
            context_window_size: 90.0,
            offline_download_bible_ids: Vec::new(),
            extra: Extra::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsSettings {
    pub brave_api_key: String,
    pub google_translate_api_key: String,
    pub gloss_color: String,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for LyricsSettings {
    fn default() -> Self {
        Self {
            brave_api_key: String::new(),
            google_translate_api_key: String::new(),
            gloss_color: "#AABED7".into(),
            extra: Extra::new(),
        }
    }
}

string_enum!(
    pub enum ShortcutCommand {
        Search = "search",
        Previous = "previous",
        Next = "next",
        Approve = "approve",
        Dismiss = "dismiss",
        Clear = "clear",
        Auto = "auto",
    }
);

/// `ShortcutBindings = Partial<Record<ShortcutCommand, string[]>>`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ShortcutBindings {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub search: Option<Vec<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub previous: Option<Vec<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub next: Option<Vec<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub approve: Option<Vec<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub dismiss: Option<Vec<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub clear: Option<Vec<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub auto: Option<Vec<String>>,
    #[serde(flatten)]
    pub extra: Extra,
}

string_enum!(
    pub enum DisplayTheme {
        Dark = "dark",
        Light = "light",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplaySettings {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub shortcuts: Option<ShortcutBindings>,
    pub theme: DisplayTheme,
    pub font_size: f64,
    pub transcription_font_size: f64,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            shortcuts: None,
            theme: DisplayTheme::Dark,
            font_size: 16.0,
            transcription_font_size: 18.0,
            extra: Extra::new(),
        }
    }
}

/// `AppSettings.overlay`. Its default, `DEFAULT_OVERLAY_SETTINGS`, is built by
/// `overlay-defaults.ts`, which prompt 013 ports.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlaySettings {
    pub template: String,
    pub show_translation: bool,
    pub show_verse_numbers: bool,
    /// 0 means no cap.
    pub max_verses: f64,
    pub outputs: Vec<OverlayOutput>,
    /// Deprecated: replaced by the `ndi` output's theme; kept for migration.
    pub theme: OverlayTheme,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChurchServiceTime {
    /// 0 is Sunday.
    pub day: f64,
    /// 24h `HH:MM`.
    pub time: String,
    pub label: String,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChurchProfile {
    pub name: String,
    /// IANA zone, or `''` until chosen.
    pub timezone: String,
    pub role: String,
    pub service_times: Vec<ChurchServiceTime>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSettings {
    pub share_stats: bool,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub send_crash_reports: Option<bool>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for UsageSettings {
    fn default() -> Self {
        Self {
            share_stats: true,
            send_crash_reports: Some(true),
            extra: Extra::new(),
        }
    }
}

/// `Partial<Record<OverlayContentKind, string>>`: theme ids in `themeLibrary`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ThemeDefaults {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub scripture: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub lyrics: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Declares every settings section once: [`AppSettings`], [`StoredSettings`],
/// its streaming `Deserialize`, and the conversions between them.
macro_rules! settings_sections {
    ($($field:ident: $ty:ty = $key:literal,)*) => {
        /// The number of sections in a settings file.
        pub const SECTION_COUNT: usize = [$($key),*].len();

        /// The canonical settings shape.
        #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
        pub struct AppSettings {
            $(#[serde(rename = $key)] pub $field: $ty,)*
            #[serde(flatten)]
            pub extra: Extra,
        }

        /// A settings file (`kairo-settings.json`, or `proautomate-settings.json`
        /// before the rename) as found on disk. Absent sections stay absent; a
        /// section that is present, even as `null`, is kept. As with
        /// `JSON.parse`, the last of two equal keys wins.
        #[derive(Clone, Debug, Default, PartialEq, Serialize)]
        pub struct StoredSettings {
            $(
                #[serde(rename = $key, skip_serializing_if = "Option::is_none")]
                pub $field: Option<Stored<$ty>>,
            )*
            /// Retired keys (`propresenter`, `propresenterResources`) and keys
            /// from newer builds.
            #[serde(flatten)]
            pub extra: Extra,
        }

        /// Hand-written so sections are read straight from the input. The
        /// derived version, with `flatten` for `extra`, buffers the whole file.
        impl<'de> Deserialize<'de> for StoredSettings {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                struct Fields;
                impl<'de> serde::de::Visitor<'de> for Fields {
                    type Value = StoredSettings;

                    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                        f.write_str("a settings object")
                    }

                    fn visit_map<A: serde::de::MapAccess<'de>>(
                        self,
                        mut map: A,
                    ) -> Result<StoredSettings, A::Error> {
                        let mut out = StoredSettings::default();
                        while let Some(key) = map.next_key::<String>()? {
                            match key.as_str() {
                                $($key => out.$field = Some(map.next_value()?),)*
                                _ => {
                                    let value = map.next_value()?;
                                    out.extra.insert(key, value);
                                }
                            }
                        }
                        Ok(out)
                    }
                }
                deserializer.deserialize_map(Fields)
            }
        }

        impl StoredSettings {
            /// Names of sections that are absent or not yet canonical, in field order.
            pub fn non_canonical_sections(&self) -> Vec<&'static str> {
                let mut out = Vec::new();
                $(
                    if !matches!(self.$field, Some(Stored::Typed(_))) {
                        out.push($key);
                    }
                )*
                out
            }

            /// The canonical settings, when every section is canonical.
            /// Otherwise the file back unchanged, for the normalisers to
            /// repair; see [`Self::non_canonical_sections`].
            pub fn into_canonical(self) -> Result<AppSettings, Box<Self>> {
                match self {
                    StoredSettings { $($field: Some(Stored::Typed($field)),)* extra } => {
                        Ok(AppSettings { $($field,)* extra })
                    }
                    other => Err(Box::new(other)),
                }
            }
        }

        impl From<AppSettings> for StoredSettings {
            fn from(settings: AppSettings) -> Self {
                Self {
                    $($field: Some(Stored::Typed(settings.$field)),)*
                    extra: settings.extra,
                }
            }
        }
    };
}

settings_sections! {
    whats_new: WhatsNewSettings = "whatsNew",
    capture: CaptureSettings = "capture",
    web: WebSettings = "web",
    audio: AudioSettings = "audio",
    stt: SttSettings = "stt",
    scripture: ScriptureSettings = "scripture",
    lyrics: LyricsSettings = "lyrics",
    display: DisplaySettings = "display",
    overlay: OverlaySettings = "overlay",
    theme_library: Vec<CustomOverlayTheme> = "themeLibrary",
    documents: DocumentsSettings = "documents",
    workspace: WorkspaceSettings = "workspace",
    media: MediaSettings = "media",
    tracks: TracksSettings = "tracks",
    church: ChurchProfile = "church",
    presentation: PresentationSettings = "presentation",
    usage: UsageSettings = "usage",
    theme_defaults: ThemeDefaults = "themeDefaults",
}

impl AppSettings {
    /// The `defaults` object in `src/main/db/index.ts`, given the overlay
    /// default (`DEFAULT_OVERLAY_SETTINGS`, prompt 013).
    pub fn defaults(overlay: OverlaySettings) -> Self {
        Self {
            whats_new: WhatsNewSettings::default(),
            capture: CaptureSettings::default(),
            web: WebSettings::default(),
            audio: AudioSettings::default(),
            stt: SttSettings::default(),
            scripture: ScriptureSettings::default(),
            lyrics: LyricsSettings::default(),
            display: DisplaySettings::default(),
            overlay,
            theme_library: Vec::new(),
            documents: DocumentsSettings::default(),
            workspace: WorkspaceSettings::default(),
            media: MediaSettings::default(),
            tracks: TracksSettings::default(),
            church: ChurchProfile::default(),
            presentation: PresentationSettings::default(),
            usage: UsageSettings::default(),
            theme_defaults: ThemeDefaults::default(),
            extra: Extra::new(),
        }
    }
}

string_enum!(
    /// Secret fields that `settings:set` can clear without revealing them.
    pub enum SettingsSecretClearKey {
        ApiKey = "apiKey",
        AnthropicApiKey = "anthropicApiKey",
        DeepseekApiKey = "deepseekApiKey",
        BibleApiKey = "bibleApiKey",
        BraveApiKey = "braveApiKey",
        GoogleTranslateApiKey = "googleTranslateApiKey",
    }
);

/// `AppSettings & { secretsConfigured }`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsWithSecretsStatus {
    pub secrets_configured: SecretsConfigured,
    #[serde(flatten)]
    pub settings: AppSettings,
}

/// A section of a settings file: canonical when it parses as `T`, the JSON as
/// found otherwise.
///
/// `Raw` holds a `serde_json::Value` because the section's shape is not known:
/// it was written by an older build or edited by hand, and only the
/// normalisers of prompt 013 may reinterpret it.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Stored<T> {
    Typed(T),
    Raw(serde_json::Value),
}

/// Captures the section as JSON text, then parses `T` straight from the text,
/// building a tree only when the section is not canonical. Faster than serde's
/// `untagged`, which buffers the section and then walks the buffer.
impl<'de, T: serde::de::DeserializeOwned> Deserialize<'de> for Stored<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = Box::<serde_json::value::RawValue>::deserialize(deserializer)?;
        match serde_json::from_str::<T>(raw.get()) {
            Ok(typed) => Ok(Stored::Typed(typed)),
            Err(_) => serde_json::from_str(raw.get())
                .map(Stored::Raw)
                .map_err(serde::de::Error::custom),
        }
    }
}

impl<T> Stored<T> {
    pub fn typed(&self) -> Option<&T> {
        match self {
            Stored::Typed(value) => Some(value),
            Stored::Raw(_) => None,
        }
    }
}

/// `kairo-migrations.json`: one-time migrations already applied. Missing flags
/// read as not applied, as electron-store's defaults do.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationFlags {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present",
        rename = "nkjvDefaultV1"
    )]
    pub nkjv_default_v1: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present",
        rename = "kjvDefaultV1"
    )]
    pub kjv_default_v1: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present",
        rename = "builtInThemesV1"
    )]
    pub built_in_themes_v1: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present",
        rename = "glossPaletteV1"
    )]
    pub gloss_palette_v1: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present",
        rename = "customThemeLibraryV1"
    )]
    pub custom_theme_library_v1: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present",
        rename = "cloudOnboardingV1"
    )]
    pub cloud_onboarding_v1: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present",
        rename = "songsFolderExportV1"
    )]
    pub songs_folder_export_v1: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present",
        rename = "deepseekDefaultV1"
    )]
    pub deepseek_default_v1: Option<bool>,
    #[serde(flatten)]
    pub extra: Extra,
}

string_enum!(
    pub enum OnboardingStepId {
        Welcome = "welcome",
        Account = "account",
        Output = "output",
        ApiKeys = "apiKeys",
        Church = "church",
    }
);
string_enum!(
    pub enum OnboardingSource {
        Fresh = "fresh",
        Legacy = "legacy",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingState {
    pub completed_steps: Vec<OnboardingStepId>,
    pub skipped_steps: Vec<OnboardingStepId>,
    pub current_step: OnboardingStepId,
    /// `null` while the wizard is open.
    #[serde(deserialize_with = "required")]
    pub completed_at: Option<f64>,
    pub source: OnboardingSource,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `DEFAULT_ONBOARDING_STATE`.
impl Default for OnboardingState {
    fn default() -> Self {
        Self {
            completed_steps: Vec::new(),
            skipped_steps: Vec::new(),
            current_step: OnboardingStepId::Welcome,
            completed_at: None,
            source: OnboardingSource::Fresh,
            extra: Extra::new(),
        }
    }
}

/// `kairo-onboarding.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct OnboardingStore {
    pub onboarding: OnboardingState,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OnboardingSummaryLine {
    pub step: OnboardingStepId,
    pub label: String,
    pub detail: String,
    pub done: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OnboardingReadiness {
    pub lines: Vec<OnboardingSummaryLine>,
    pub outstanding: Vec<OnboardingSummaryLine>,
    pub ready: bool,
}
