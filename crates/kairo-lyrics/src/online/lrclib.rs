//! LRCLIB, the free keyless lyrics database. From
//! `src/main/services/lyrics/lrclib.ts`.
//!
//! Its search matches track and artist metadata only, but every hit ships
//! lyrics inline, so a half-remembered line is matched locally. LRCLIB asks
//! clients to identify themselves, so the agent names Kairo.

use super::ProviderResult;
use super::cache::TtlCache;
use super::http::{Http, HttpError, HttpRequest, success};
use super::normalize::{canonical_text, structure_lyrics, trim};
use kairo_domain::lyrics::LyricsProvider;
use regex::Regex;
use serde::Deserialize;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

const SEARCH_URL: &str = "https://lrclib.net/api/search";
const GET_URL: &str = "https://lrclib.net/api/get";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(12);
/// GET can be slower than search.
const FETCH_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_RESULTS: usize = 8;
pub const USER_AGENT: &str = "Kairo (https://github.com/CodeLawd/kairo-presenter)";
const SEARCH_CACHE_TTL: Duration = Duration::from_secs(5 * 60);
const LYRICS_CACHE_TTL: Duration = Duration::from_secs(30 * 60);

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Track {
    id: Option<serde_json::Value>,
    track_name: Option<String>,
    artist_name: Option<String>,
    album_name: Option<String>,
    instrumental: Option<bool>,
    plain_lyrics: Option<String>,
}

impl Track {
    /// `typeof track.id === 'number'`.
    fn numeric_id(&self) -> Option<String> {
        let n = self.id.as_ref()?.as_f64()?;
        Some(kairo_domain::js::number_to_string(n))
    }
}

static PIPE_TAIL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*[-–—]?\s*(#[\w'’-]+\s*)+$").unwrap());
static DISTRIBUTION_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\s*[(\[]\s*(official\s*)?(music\s*)?(video|audio|lyrics?|lyric video|visualizer|hd|4k)\s*[)\]]").unwrap()
});
static MULTI_SPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s{2,}").unwrap());
static TRAILING_PUNCT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\s\-–—,]+$").unwrap());
static FEAT_SPLIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)[;,]|\bfeat\.?\b|\bft\.?\b").unwrap());
static GET_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"/api/get/(\d+)$").unwrap());

/// Strips YouTube-title noise crowdsourced entries carry.
pub fn clean_track_title(raw: &str) -> String {
    let original = trim(raw);
    let mut title = original.split('|').next().unwrap_or("").to_owned();
    title = PIPE_TAIL.replace(&title, "").into_owned();
    title = DISTRIBUTION_TAG.replace_all(&title, "").into_owned();
    title = MULTI_SPACE.replace_all(&title, " ").into_owned();
    title = trim(&TRAILING_PUNCT.replace(&title, "")).to_owned();
    if title.is_empty() {
        original.to_owned()
    } else {
        title
    }
}

fn dedupe_key(title: &str, artist: &str) -> String {
    let primary = FEAT_SPLIT.split(artist).next().unwrap_or("");
    format!("{}|{}", canonical_text(title), canonical_text(primary))
}

/// LRCLIB's friendly errors.
pub fn friendly_error(error: &HttpError, fetching: bool) -> String {
    match error {
        HttpError::Status { status: 404, .. } => {
            "That song is no longer available on LRCLIB.".into()
        }
        HttpError::Status { status: 429, .. } => {
            "LRCLIB is rate limiting this connection. Wait a minute and try again.".into()
        }
        HttpError::Timeout if fetching => "Timed out fetching lyrics from LRCLIB.".into(),
        HttpError::Timeout => "Timed out searching LRCLIB.".into(),
        HttpError::Offline => "No internet connection.".into(),
        other => {
            tracing::warn!(error = %other, "lrclib request failed");
            format!("Could not reach LRCLIB. {other}")
        }
    }
}

pub struct Lrclib {
    http: Arc<dyn Http>,
    search_cache: TtlCache<Vec<ProviderResult>>,
    /// Plain lyrics by track id, filled by search and reused by fetch.
    lyrics_cache: TtlCache<String>,
}

impl Lrclib {
    pub fn new(http: Arc<dyn Http>) -> Self {
        Self {
            http,
            search_cache: TtlCache::new(),
            lyrics_cache: TtlCache::new(),
        }
    }

    pub async fn search(&self, query: &str) -> Result<Vec<ProviderResult>, String> {
        let q = trim(query);
        if q.encode_utf16().count() < 3 {
            return Ok(Vec::new());
        }
        self.run_search(vec![("q".into(), q.into())], q).await
    }

    /// Track and artist sent separately; free text misses "<title> by <artist>".
    pub async fn search_structured(
        &self,
        track: &str,
        artist: &str,
    ) -> Result<Vec<ProviderResult>, String> {
        let track = trim(track);
        let artist = trim(artist);
        if track.encode_utf16().count() < 2 {
            return Ok(Vec::new());
        }
        let mut params = vec![("track_name".to_owned(), track.to_owned())];
        if !artist.is_empty() {
            params.push(("artist_name".into(), artist.into()));
        }
        self.run_search(params, &format!("{track} | {artist}"))
            .await
    }

    async fn run_search(
        &self,
        params: Vec<(String, String)>,
        label: &str,
    ) -> Result<Vec<ProviderResult>, String> {
        let key = trim(label).to_lowercase();
        if let Some(cached) = self.search_cache.get(&key) {
            // Re-warm the lyrics cache so import still skips /api/get.
            for result in &cached {
                if let Some(lyrics) = result.lyrics.as_deref().map(trim).filter(|l| !l.is_empty()) {
                    let id = result.id.trim_start_matches("lrclib:");
                    self.lyrics_cache
                        .put(id.into(), lyrics.into(), LYRICS_CACHE_TTL);
                }
            }
            return Ok(cached);
        }
        let mut request = HttpRequest::get(SEARCH_URL, REQUEST_TIMEOUT)
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/json");
        request.query = params;
        let response = self
            .http
            .get(request, success)
            .await
            .map_err(|e| friendly_error(&e, false))?;
        let tracks: Vec<Track> = match serde_json::from_str::<serde_json::Value>(&response.body) {
            Ok(serde_json::Value::Array(items)) => items
                .into_iter()
                .map(|t| serde_json::from_value(t).unwrap_or_default())
                .collect(),
            _ => Vec::new(),
        };
        let raw = tracks.len();
        let mut by_key: Vec<(String, ProviderResult)> = Vec::new();
        for track in tracks {
            let Some(id) = track.numeric_id() else {
                continue;
            };
            if track.instrumental == Some(true) {
                continue;
            }
            let Some(lyrics) = track
                .plain_lyrics
                .as_deref()
                .map(trim)
                .filter(|l| !l.is_empty())
            else {
                continue;
            };
            let title = clean_track_title(track.track_name.as_deref().unwrap_or(""));
            let artist = trim(track.artist_name.as_deref().unwrap_or("")).to_owned();
            if title.is_empty() {
                continue;
            }
            let key = dedupe_key(&title, &artist);
            let existing = by_key.iter().position(|(k, _)| *k == key);
            // Variants of one recording differ mainly in completeness.
            if let Some(at) = existing
                && by_key[at]
                    .1
                    .lyrics
                    .as_ref()
                    .map_or(0, |l| l.encode_utf16().count())
                    >= lyrics.encode_utf16().count()
            {
                continue;
            }
            self.remember_plain_lyrics(&id, lyrics);
            let result = ProviderResult {
                id: format!("lrclib:{id}"),
                provider: LyricsProvider::Lrclib,
                title,
                artist,
                url: format!("{GET_URL}/{id}"),
                album: track
                    .album_name
                    .as_deref()
                    .map(trim)
                    .filter(|a| !a.is_empty())
                    .map(str::to_owned),
                lyrics: Some(lyrics.into()),
                ..ProviderResult::default()
            };
            match existing {
                Some(at) => by_key[at].1 = result,
                None => by_key.push((key, result)),
            }
        }
        let results: Vec<ProviderResult> = by_key
            .into_iter()
            .map(|(_, r)| r)
            .take(MAX_RESULTS)
            .collect();
        self.search_cache
            .put(key, results.clone(), SEARCH_CACHE_TTL);
        tracing::info!(query = label, raw, results = results.len(), "lrclib search");
        Ok(results)
    }

    /// One record's lyrics, section-marked. Search-time lyrics first; a stale
    /// cached copy beats a hard timeout.
    pub async fn fetch_lyrics(&self, url: &str) -> Result<String, String> {
        let Some(id) = GET_ID.captures(url).map(|c| c[1].to_owned()) else {
            return Err("Refusing to fetch a non-LRCLIB URL.".into());
        };
        if let Some(cached) = self.lyrics_cache.get(&id) {
            return Ok(structure_lyrics(&cached));
        }
        match self.fetch_plain(&id).await {
            Ok(plain) => {
                self.remember_plain_lyrics(&id, &plain);
                Ok(structure_lyrics(&plain))
            }
            Err(error) => match self.lyrics_cache.get_stale(&id) {
                Some(stale) => {
                    tracing::warn!(id, "lrclib fetch failed; using stale search cache");
                    Ok(structure_lyrics(&stale))
                }
                None => Err(error),
            },
        }
    }

    pub fn remember_plain_lyrics(&self, url_or_id: &str, plain: &str) {
        let id = GET_ID
            .captures(url_or_id)
            .map(|c| c[1].to_owned())
            .unwrap_or_else(|| trim(url_or_id.trim_start_matches("lrclib:")).to_owned());
        let text = trim(plain);
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) || text.is_empty() {
            return;
        }
        self.lyrics_cache.put(id, text.into(), LYRICS_CACHE_TTL);
    }

    async fn fetch_plain(&self, id: &str) -> Result<String, String> {
        let mut last: Option<HttpError> = None;
        // One retry: LRCLIB intermittently stalls on /api/get.
        for attempt in 0..2 {
            let request = HttpRequest::get(format!("{GET_URL}/{id}"), FETCH_TIMEOUT)
                .header("User-Agent", USER_AGENT)
                .header("Accept", "application/json");
            match self.http.get(request, success).await {
                Ok(response) => {
                    let track: Track = serde_json::from_str(&response.body).unwrap_or_default();
                    return match track
                        .plain_lyrics
                        .as_deref()
                        .map(trim)
                        .filter(|l| !l.is_empty())
                    {
                        Some(plain) => Ok(plain.into()),
                        None => Err(
                            "That LRCLIB entry has no plain lyrics — it may be instrumental."
                                .into(),
                        ),
                    };
                }
                Err(error) => {
                    let timeout = error == HttpError::Timeout;
                    last = Some(error);
                    if !timeout || attempt == 1 {
                        break;
                    }
                    tracing::warn!(id, attempt = attempt + 1, "lrclib get timed out; retrying");
                }
            }
        }
        Err(friendly_error(
            &last.unwrap_or(HttpError::Other("Unknown error.".into())),
            true,
        ))
    }

    pub fn clear_cache(&self) {
        self.search_cache.clear();
        self.lyrics_cache.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn youtube_noise_is_removed_from_titles() {
        assert_eq!(
            clean_track_title(
                "Ese (Thank You) | NATHANIEL BASSEY feat. AIDEE IME - #nathanielbassey"
            ),
            "Ese (Thank You)"
        );
        assert_eq!(
            clean_track_title("Way Maker (Official Video) #sinach #worship"),
            "Way Maker"
        );
        assert_eq!(clean_track_title("  | "), "|");
    }
}
