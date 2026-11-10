//! Small expiring caches. From the provider caches and `online-cache.ts`.

use super::ProviderResult;
use kairo_domain::lyrics::LyricsOnlineResult;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Key to value with a per-entry expiry; an expired entry is dropped on read.
pub struct TtlCache<T> {
    entries: Mutex<HashMap<String, (T, Instant)>>,
}

impl<T: Clone> Default for TtlCache<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone> TtlCache<T> {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, (T, Instant)>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn get(&self, key: &str) -> Option<T> {
        let mut entries = self.lock();
        match entries.get(key) {
            Some((_, expires)) if *expires < Instant::now() => {
                entries.remove(key);
                None
            }
            Some((value, _)) => Some(value.clone()),
            None => None,
        }
    }

    /// Ignores expiry, for a last-resort fallback.
    pub fn get_stale(&self, key: &str) -> Option<T> {
        self.lock().get(key).map(|(v, _)| v.clone())
    }

    pub fn put(&self, key: String, value: T, ttl: Duration) {
        self.lock().insert(key, (value, Instant::now() + ttl));
    }

    pub fn clear(&self) {
        self.lock().clear();
    }
}

const TEXT_TTL: Duration = Duration::from_secs(10 * 60);
const TEXT_CAPACITY: usize = 50;

fn source_key(provider: &str, url: &str, title: &str) -> String {
    serde_json::json!([provider, url, title]).to_string()
}

/// Lyric bodies search already fetched, kept for preview and import.
/// At most 50, ten minutes each, oldest evicted first.
#[derive(Default)]
pub struct OnlineLyricsTextCache {
    entries: Mutex<Vec<(String, String, Instant)>>,
}

impl OnlineLyricsTextCache {
    pub fn remember(&self, provider: &str, url: &str, title: &str, text: &str) {
        if super::normalize::trim(text).is_empty() {
            return;
        }
        let now = Instant::now();
        let key = source_key(provider, url, title);
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        entries.retain(|(k, _, expires)| *expires > now && *k != key);
        entries.push((key, text.into(), now + TEXT_TTL));
        while entries.len() > TEXT_CAPACITY {
            entries.remove(0);
        }
    }

    pub fn remember_result(&self, result: &ProviderResult, text: &str) {
        self.remember(result.provider.as_str(), &result.url, &result.title, text);
    }

    pub fn get(&self, source: &LyricsOnlineResult) -> Option<String> {
        let key = source_key(source.provider.as_str(), &source.url, &source.title);
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        let at = entries.iter().position(|(k, _, _)| *k == key)?;
        if entries[at].2 <= Instant::now() {
            entries.remove(at);
            return None;
        }
        Some(entries[at].1.clone())
    }
}
