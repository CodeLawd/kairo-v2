//! Song usage (CCLI reporting) and anonymous usage statistics, from
//! `src/lib/song-usage.ts`, `usage-ledger.ts` and `cloud/usage.ts`.

use crate::serde_util::{Extra, string_enum};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SongUsageEntry {
    pub song_id: String,
    pub title: String,
    pub artist: String,
    pub ccli_number: String,
    pub copyright: String,
    pub at: f64,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Persisted as `song-usage.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SongUsageStore {
    pub entries: Vec<SongUsageEntry>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SongUsageSummaryRow {
    pub song_id: String,
    pub title: String,
    pub artist: String,
    pub ccli_number: String,
    pub copyright: String,
    pub uses: f64,
    pub last_used: f64,
}

string_enum!(
    pub enum UsageFeature {
        ServiceStarted = "service_started",
        ServiceEnded = "service_ended",
        ListeningMinutes = "listening_minutes",
        ScriptureManual = "scripture_manual",
        ScriptureAuto = "scripture_auto",
        ScriptureSearch = "scripture_search",
        LyricsSlide = "lyrics_slide",
        SongImport = "song_import",
        SongOnlineSearch = "song_online_search",
        MediaLive = "media_live",
        DocumentPage = "document_page",
        TimerStart = "timer_start",
        MessageShown = "message_shown",
        OutputClear = "output_clear",
        BibleDownload = "bible_download",
        RecapUploaded = "recap_uploaded",
    }
);
string_enum!(
    pub enum UsageErrorKind {
        Uncaught = "uncaught",
        UnhandledRejection = "unhandled_rejection",
        RendererGone = "renderer_gone",
        ChildProcessGone = "child_process_gone",
    }
);

/// Counts for one day.
///
/// Keys are [`UsageFeature`] and [`UsageErrorKind`] names, kept as strings so
/// a count written by a newer version for a feature this build does not know
/// survives a load and save.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LedgerDay {
    pub counts: BTreeMap<String, f64>,
    pub errors: BTreeMap<String, f64>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Persisted as `kairo-usage.json`. Keyed by local day, `YYYY-MM-DD`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LedgerState {
    pub days: BTreeMap<String, LedgerDay>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSystem {
    pub os: String,
    pub os_version: String,
    pub arch: String,
    pub app_version: String,
    pub electron_version: String,
    pub locale: String,
    pub cpu_count: f64,
    pub memory_gb: f64,
    pub screens: f64,
    pub ndi_outputs: f64,
    pub propresenter: bool,
    pub transcription: bool,
    pub automation: bool,
    pub default_translation: String,
    pub theme: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UsageDayReport {
    pub day: String,
    pub counts: BTreeMap<String, f64>,
    pub errors: BTreeMap<String, f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageReport {
    pub install_id: String,
    pub days: Vec<UsageDayReport>,
    pub system: UsageSystem,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UsageReportResult {
    pub accepted: Vec<String>,
}
