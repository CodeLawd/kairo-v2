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

pub const SONG_USAGE_DEDUPE_MS: f64 = 4.0 * 60.0 * 60.0 * 1000.0;
pub const LEDGER_MAX_DAYS: usize = 30;
pub const USAGE_MAX_DAYS: usize = 31;
pub const USAGE_MAX_COUNT: usize = 100_000;
pub const USAGE_FEATURES: &[UsageFeature] = UsageFeature::ALL;
pub const USAGE_ERRORS: &[UsageErrorKind] = UsageErrorKind::ALL;

pub fn is_new_song_use(entries: &[SongUsageEntry], song_id: &str, now: f64) -> bool {
    entries
        .iter()
        .rev()
        .find(|entry| entry.song_id == song_id)
        .is_none_or(|entry| now - entry.at >= SONG_USAGE_DEDUPE_MS)
}

pub fn song_usage_between(entries: &[SongUsageEntry], from: f64, to: f64) -> Vec<&SongUsageEntry> {
    let mut entries: Vec<_> = entries
        .iter()
        .filter(|entry| entry.at >= from && entry.at < to)
        .collect();
    entries.sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
    entries
}

pub fn summarize_song_usage(
    entries: &[SongUsageEntry],
    compare_titles: impl Fn(&str, &str) -> std::cmp::Ordering,
) -> Vec<SongUsageSummaryRow> {
    let mut indices = std::collections::HashMap::new();
    let mut rows: Vec<SongUsageSummaryRow> = Vec::new();
    for entry in entries {
        let index = *indices.entry(entry.song_id.as_str()).or_insert(rows.len());
        if let Some(row) = rows.get_mut(index) {
            row.uses += 1.0;
            if entry.at > row.last_used {
                row.last_used = entry.at;
                row.title.clone_from(&entry.title);
                row.artist.clone_from(&entry.artist);
                if !entry.ccli_number.is_empty() {
                    row.ccli_number.clone_from(&entry.ccli_number);
                }
                if !entry.copyright.is_empty() {
                    row.copyright.clone_from(&entry.copyright);
                }
            }
        } else {
            rows.push(SongUsageSummaryRow {
                song_id: entry.song_id.clone(),
                title: entry.title.clone(),
                artist: entry.artist.clone(),
                ccli_number: entry.ccli_number.clone(),
                copyright: entry.copyright.clone(),
                uses: 1.0,
                last_used: entry.at,
            });
        }
    }
    rows.sort_by(|a, b| {
        b.uses
            .total_cmp(&a.uses)
            .then_with(|| compare_titles(&a.title, &b.title))
    });
    rows
}

pub fn local_day(date: impl chrono::Datelike) -> String {
    format!("{}-{:02}-{:02}", date.year(), date.month(), date.day())
}

pub fn iso_date<T: chrono::TimeZone>(ms: f64, zone: &T) -> String {
    if !ms.is_finite() || ms.abs() > 8.64e15 {
        return "NaN-NaN-NaN".into();
    }
    zone.timestamp_millis_opt(ms as i64)
        .single()
        .map_or_else(|| "NaN-NaN-NaN".into(), local_day)
}

pub fn song_usage_csv<T: chrono::TimeZone>(entries: &[SongUsageEntry], zone: &T) -> String {
    fn cell(text: &str) -> String {
        let mut safe = String::new();
        if text.starts_with(['=', '+', '-', '@', '\t', '\r']) {
            safe.push('\'');
        }
        safe.push_str(text);
        if safe.contains(['"', ',', '\n', '\r']) {
            format!("\"{}\"", safe.replace('"', "\"\""))
        } else {
            safe
        }
    }
    let mut csv = String::from("Date,Title,Artist,CCLI Song #,Copyright\r\n");
    for entry in entries {
        let date = iso_date(entry.at, zone);
        let row = [
            date.as_str(),
            &entry.title,
            &entry.artist,
            &entry.ccli_number,
            &entry.copyright,
        ];
        csv.push_str(&row.map(cell).join(","));
        csv.push_str("\r\n");
    }
    csv
}

pub fn normalize_song_usage_entries(raw: &serde_json::Value) -> Vec<SongUsageEntry> {
    raw.as_array()
        .into_iter()
        .flatten()
        .filter_map(|raw| {
            let string = |key| raw[key].as_str().unwrap_or_default().to_owned();
            Some(SongUsageEntry {
                song_id: raw["songId"].as_str()?.into(),
                at: raw["at"].as_f64()?,
                title: string("title"),
                artist: string("artist"),
                ccli_number: string("ccliNumber"),
                copyright: string("copyright"),
                extra: Extra::new(),
            })
        })
        .collect()
}

pub fn is_usage_day(day: &str) -> bool {
    day.len() == 10
        && day.bytes().enumerate().all(|(i, b)| {
            if i == 4 || i == 7 {
                b == b'-'
            } else {
                b.is_ascii_digit()
            }
        })
}

pub fn usage_page_section(path: &str) -> Option<&str> {
    static SECTION: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"^/(dashboard|admin)(/[a-z-]+)?").unwrap());
    let clean = path.split(['?', '#']).next()?.trim_end_matches('/');
    SECTION.find(clean).map(|matched| matched.as_str())
}

pub trait LedgerStorage {
    fn read(&self) -> LedgerState;
    fn write(&mut self, state: LedgerState);
}

pub struct UsageLedger<S, N> {
    storage: S,
    today: N,
}

impl<S: LedgerStorage, N: Fn() -> String> UsageLedger<S, N> {
    pub fn new(storage: S, today: N) -> Self {
        Self { storage, today }
    }

    pub fn track(&mut self, feature: UsageFeature, count: f64) {
        if count.is_nan() || count <= 0.0 {
            return;
        }
        self.update(|day| {
            *day.counts.entry(feature.as_str().into()).or_default() += crate::js::math_round(count)
        });
    }

    pub fn error(&mut self, kind: UsageErrorKind) {
        self.update(|day| *day.errors.entry(kind.as_str().into()).or_default() += 1.0);
    }

    pub fn pending(&self) -> Vec<UsageDayReport> {
        self.storage
            .read()
            .days
            .into_iter()
            .map(|(day, totals)| UsageDayReport {
                day,
                counts: totals.counts,
                errors: totals.errors,
            })
            .collect()
    }

    pub fn mark_accepted(&mut self, accepted: &[String]) {
        let today = (self.today)();
        let mut state = self.storage.read();
        for day in accepted.iter().filter(|day| **day != today) {
            state.days.remove(day);
        }
        self.storage.write(state);
    }

    pub fn clear(&mut self) {
        self.storage.write(LedgerState::default());
    }

    fn update(&mut self, update: impl FnOnce(&mut LedgerDay)) {
        let mut state = self.storage.read();
        let today = (self.today)();
        update(state.days.entry(today).or_default());
        while state.days.len() > LEDGER_MAX_DAYS {
            state.days.pop_first();
        }
        self.storage.write(state);
    }
}
