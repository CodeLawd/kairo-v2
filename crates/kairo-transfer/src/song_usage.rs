//! One entry per song per service, for CCLI reporting (`song-usage.json`).
//! The store opens on first use and is read once, then cached.

use crate::{Clock, lock, open};
use kairo_domain::lyrics::LyricsSong;
use kairo_domain::usage::{
    SongUsageEntry, is_new_song_use, normalize_song_usage_entries, song_usage_between,
    song_usage_csv,
};
use kairo_store::json_store::JsonStore;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

struct Inner {
    user_data: PathBuf,
    store: Option<JsonStore>,
    cache: Option<Vec<SongUsageEntry>>,
}

impl Inner {
    fn db(&mut self) -> &mut JsonStore {
        let user_data = self.user_data.clone();
        self.store
            .get_or_insert_with(|| open(&user_data, "song-usage"))
    }

    fn entries(&mut self) -> &Vec<SongUsageEntry> {
        if self.cache.is_none() {
            let raw = self.db().get("entries").cloned().unwrap_or_default();
            self.cache = Some(normalize_song_usage_entries(&raw));
        }
        self.cache.as_ref().expect("cache filled")
    }

    fn save(&mut self, entries: Vec<SongUsageEntry>) {
        if let Ok(value) = kairo_domain::json::to_js_value(&entries) {
            self.db().set("entries", value);
        }
        self.cache = Some(entries);
    }
}

#[derive(Clone)]
pub struct SongUsageService {
    inner: Arc<Mutex<Inner>>,
    clock: Clock,
}

/// `song-usage-YYYY-MM-DD.csv`, the UTC date as `toISOString` gives it.
pub fn song_usage_file_name(now_ms: f64) -> String {
    let date = chrono::DateTime::from_timestamp_millis(now_ms as i64).map_or_else(
        || "1970-01-01".to_owned(),
        |d| d.format("%Y-%m-%d").to_string(),
    );
    format!("song-usage-{date}.csv")
}

impl SongUsageService {
    pub fn new(user_data: &Path, clock: Clock) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                user_data: user_data.to_owned(),
                store: None,
                cache: None,
            })),
            clock,
        }
    }

    /// Called after every lyric slide push; records the song once per service.
    /// Never fails: reporting must not get in the way of a slide going up.
    pub fn record(&self, song: &LyricsSong) {
        self.record_at(song, (self.clock)());
    }

    pub fn record_at(&self, song: &LyricsSong, now: f64) {
        let mut inner = lock(&self.inner);
        let entries = inner.entries();
        if !is_new_song_use(entries, &song.id, now) {
            return;
        }
        let mut next = entries.clone();
        next.push(SongUsageEntry {
            song_id: song.id.clone(),
            title: song.title.clone(),
            artist: song.artist.clone(),
            ccli_number: song.ccli_number.clone().unwrap_or_default(),
            copyright: song.copyright.clone().unwrap_or_default(),
            at: now,
            extra: Default::default(),
        });
        inner.save(next);
    }

    /// `songUsage:list`. Non-numeric bounds become 0 and now, as the handler does.
    pub fn list(&self, from: Option<f64>, to: Option<f64>) -> Vec<SongUsageEntry> {
        let from = from.filter(|v| v.is_finite()).unwrap_or(0.0);
        let to = to
            .filter(|v| v.is_finite())
            .unwrap_or_else(|| (self.clock)());
        let mut inner = lock(&self.inner);
        song_usage_between(inner.entries(), from, to)
            .into_iter()
            .cloned()
            .collect()
    }

    /// `songUsage:clear`. Keeps entries at or after `before`.
    pub fn clear_before(&self, before: Option<f64>) {
        let before = before.filter(|v| v.is_finite()).unwrap_or(0.0);
        let mut inner = lock(&self.inner);
        let next = inner
            .entries()
            .iter()
            .filter(|e| e.at >= before)
            .cloned()
            .collect();
        inner.save(next);
    }

    /// `songUsage:export`: writes the CSV to `path` (chosen by the save
    /// dialog; `None` when cancelled) and returns it.
    pub async fn export_csv<Tz: chrono::TimeZone>(
        &self,
        from: Option<f64>,
        to: Option<f64>,
        zone: &Tz,
        path: Option<PathBuf>,
    ) -> std::io::Result<Option<PathBuf>> {
        let entries = self.list(from, to);
        let Some(path) = path else { return Ok(None) };
        let (csv, target) = (song_usage_csv(&entries, zone), path.clone());
        tokio::task::spawn_blocking(move || {
            kairo_store::fs_atomic::write_atomic(&target, csv.as_bytes())
        })
        .await
        .map_err(std::io::Error::other)??;
        Ok(Some(path))
    }

    pub fn flush(&self) -> std::result::Result<(), String> {
        match &lock(&self.inner).store {
            Some(store) => store.flush(),
            None => Ok(()),
        }
    }
}
