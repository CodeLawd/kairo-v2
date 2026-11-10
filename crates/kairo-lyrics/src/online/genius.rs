//! Genius search and lyric pages. From `src/main/services/lyrics/scraper.ts`,
//! with `scraper` in place of `cheerio`.
//!
//! Genius is the only free catalogue that indexes lyric bodies. It serves a
//! bot interstitial to unknown agents, so requests use a stock desktop agent,
//! and outbound requests are spaced 350 ms apart.

use super::ProviderResult;
use super::cache::TtlCache;
use super::http::{Http, HttpError, HttpRequest, success};
use super::normalize::{canonical_text, structure_lyrics, trim};
use super::throttle::Throttle;
use kairo_domain::lyrics::LyricsProvider;
use regex::Regex;
use scraper::{ElementRef, Html, Node, Selector};
use std::collections::HashSet;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

const SEARCH_URL: &str = "https://genius.com/api/search/multi";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(12);
const MAX_RESULTS: usize = 8;
pub const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";
const MIN_REQUEST_GAP: Duration = Duration::from_millis(350);
const SEARCH_CACHE_TTL: Duration = Duration::from_secs(5 * 60);
const LYRICS_CACHE_TTL: Duration = Duration::from_secs(60 * 60);

static GENIUS_URL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^https://(www\.)?genius\.com/").unwrap());
static MIGHT_LIKE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"You might also like").unwrap());
static TICKETS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"See .{1,60} LiveGet tickets as low as \$\d+").unwrap());

/// Raw container text to section-marked lyrics.
pub fn normalize_genius_lyrics(raw: &str) -> String {
    let unix = raw.replace("\r\n", "\n").replace('\r', "\n");
    let cleaned = MIGHT_LIKE.replace_all(&unix, "\n");
    let cleaned = TICKETS.replace_all(&cleaned, "\n");
    structure_lyrics(&cleaned)
}

/// `node.text()` after removing excluded nodes and turning `<br>` into `\n`.
pub(crate) fn element_text(
    element: ElementRef<'_>,
    skip: &dyn Fn(ElementRef<'_>) -> bool,
) -> String {
    let mut out = String::new();
    collect_text(element, skip, &mut out);
    out
}

fn collect_text(element: ElementRef<'_>, skip: &dyn Fn(ElementRef<'_>) -> bool, out: &mut String) {
    for child in element.children() {
        match child.value() {
            Node::Text(text) => out.push_str(text),
            Node::Element(e) => {
                if e.name() == "br" {
                    out.push('\n');
                    continue;
                }
                if let Some(child) = ElementRef::wrap(child)
                    && !skip(child)
                {
                    collect_text(child, skip, out);
                }
            }
            _ => {}
        }
    }
}

/// The text of every `div[data-lyrics-container="true"]`, in document order.
pub fn extract_lyric_containers(html: &str) -> Vec<String> {
    static CONTAINER: LazyLock<Selector> =
        LazyLock::new(|| Selector::parse(r#"div[data-lyrics-container="true"]"#).unwrap());
    let document = Html::parse_document(html);
    let excluded =
        |e: ElementRef<'_>| e.value().attr("data-exclude-from-selection") == Some("true");
    document
        .select(&CONTAINER)
        .map(|node| element_text(node, &excluded))
        .collect()
}

const TITLE_STOPWORDS: [&str; 12] = [
    "the",
    "and",
    "for",
    "you",
    "your",
    "feat",
    "featuring",
    "official",
    "lyrics",
    "with",
    "from",
    "live",
];

fn stopword(w: &str) -> bool {
    TITLE_STOPWORDS.contains(&w) || w == "remix"
}

use kairo_domain::js::utf16_len as len16;

fn title_tokens(title: &str) -> Vec<String> {
    canonical_text(title)
        .split(' ')
        .map(trim)
        .filter(|w| len16(w) >= 3 && !stopword(w))
        .map(str::to_owned)
        .collect()
}

fn score_segment(body: &str, tokens: &[String]) -> u32 {
    let canon = canonical_text(body);
    tokens
        .iter()
        .filter(|t| canon.contains(t.as_str()))
        .map(|t| match len16(t) {
            n if n >= 6 => 4,
            n if n >= 4 => 2,
            _ => 1,
        })
        .sum()
}

fn significant_words(text: &str) -> HashSet<String> {
    canonical_text(text)
        .split(' ')
        .map(trim)
        .filter(|w| len16(w) >= 4 && !stopword(w))
        .map(str::to_owned)
        .collect()
}

fn overlap_ratio(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let (small, large) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    small.iter().filter(|w| large.contains(*w)).count() as f64 / small.len() as f64
}

/// Medley pages pack several songs into containers split by empty ones.
/// Keeps too much rather than cutting the real song.
pub fn select_primary_lyric_segment(containers: &[String], title: &str) -> String {
    let mut segments: Vec<Vec<&str>> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for raw in containers {
        let text = trim(raw);
        if text.is_empty() {
            if !current.is_empty() {
                segments.push(std::mem::take(&mut current));
            }
            continue;
        }
        current.push(text);
    }
    if !current.is_empty() {
        segments.push(current);
    }
    if segments.is_empty() {
        return String::new();
    }
    let bodies: Vec<String> = segments.iter().map(|s| s.join("\n")).collect();
    if bodies.len() == 1 {
        return bodies[0].clone();
    }
    if bodies.len() < 3 {
        return bodies.join("\n");
    }
    let tokens = title_tokens(title);
    let scores: Vec<u32> = bodies
        .iter()
        .map(|b| {
            if tokens.is_empty() {
                0
            } else {
                score_segment(b, &tokens)
            }
        })
        .collect();
    let mut best = 0;
    let mut best_score: i64 = -1;
    for (i, s) in scores.iter().enumerate() {
        if i64::from(*s) > best_score {
            best_score = i64::from(*s);
            best = i;
        }
    }
    if best_score <= 0 {
        return bodies.join("\n");
    }
    let primary = significant_words(&bodies[best]);
    let foreign: Vec<bool> = bodies
        .iter()
        .enumerate()
        .map(|(i, body)| {
            i != best && scores[i] == 0 && overlap_ratio(&primary, &significant_words(body)) < 0.18
        })
        .collect();
    if !foreign.iter().any(|f| *f) {
        return bodies.join("\n");
    }
    let (mut start, mut end) = (best, best);
    while start > 0 && !foreign[start - 1] {
        start -= 1;
    }
    while end < bodies.len() - 1 && !foreign[end + 1] {
        end += 1;
    }
    bodies[start..=end].join("\n")
}

/// "https://genius.com/Apostle-bamilaw-ijadopin-lyrics" → "Apostle bamilaw ijadopin".
fn title_from_url(url: &str) -> String {
    let Ok(parsed) = url::Url::parse(url) else {
        return String::new();
    };
    let slug = parsed
        .path()
        .split('/')
        .rfind(|s| !s.is_empty())
        .unwrap_or("");
    let bare = if slug.len() >= 7
        && slug.is_char_boundary(slug.len() - 7)
        && slug[slug.len() - 7..].eq_ignore_ascii_case("-lyrics")
    {
        &slug[..slug.len() - 7]
    } else {
        slug
    };
    trim(&bare.replace('-', " ")).to_owned()
}

pub fn friendly_error(error: &HttpError, action: &str) -> String {
    match error {
        HttpError::Status {
            status: 429 | 403, ..
        } => "Genius is rate limiting this connection. Wait a minute and try again.".into(),
        HttpError::Status { status: 404, .. } => {
            "That song is no longer available on Genius.".into()
        }
        HttpError::Timeout => format!("Timed out trying to {action}."),
        HttpError::Offline => "No internet connection.".into(),
        other => {
            tracing::warn!(error = %other, action, "genius request failed");
            format!("Could not {action}. {other}")
        }
    }
}

pub struct Genius {
    http: Arc<dyn Http>,
    search_cache: TtlCache<Vec<ProviderResult>>,
    lyrics_cache: TtlCache<String>,
    throttle: Throttle,
}

impl Genius {
    pub fn new(http: Arc<dyn Http>) -> Self {
        Self {
            http,
            search_cache: TtlCache::new(),
            lyrics_cache: TtlCache::new(),
            throttle: Throttle::spaced(MIN_REQUEST_GAP),
        }
    }

    /// Title, artist or a fragment of the lyrics, in Genius relevance order.
    pub async fn search(&self, query: &str) -> Result<Vec<ProviderResult>, String> {
        let q = trim(query);
        if len16(q) < 3 {
            return Ok(Vec::new());
        }
        let key = q.to_lowercase();
        if let Some(cached) = self.search_cache.get(&key) {
            return Ok(cached);
        }
        self.throttle.wait().await;
        // The multi endpoint rejects per_page with a 422; trimmed below.
        let request = HttpRequest::get(SEARCH_URL, REQUEST_TIMEOUT)
            .query("q", q)
            .header("User-Agent", BROWSER_USER_AGENT)
            .header("Accept", "application/json");
        let response = self
            .http
            .get(request, success)
            .await
            .map_err(|e| friendly_error(&e, "search Genius"))?;
        let data: serde_json::Value = serde_json::from_str(&response.body).unwrap_or_default();
        let empty = Vec::new();
        let sections = data["response"]["sections"].as_array().unwrap_or(&empty);
        let mut seen = HashSet::new();
        let mut results = Vec::new();
        'outer: for section in sections {
            for hit in section["hits"].as_array().unwrap_or(&empty) {
                let song = &hit["result"];
                let Some(url) = song["url"].as_str().filter(|u| !u.is_empty()) else {
                    continue;
                };
                if song["_type"].as_str() != Some("song") {
                    continue;
                }
                if let Some(state) = song["lyrics_state"].as_str()
                    && !state.is_empty()
                    && state != "complete"
                {
                    continue;
                }
                if song["instrumental"].as_bool() == Some(true) {
                    continue;
                }
                let Some(id) = song["id"].as_f64() else {
                    continue;
                };
                if !seen.insert(id.to_bits()) {
                    continue;
                }
                let title = trim(song["title"].as_str().unwrap_or(""));
                let artist = song["artist_names"]
                    .as_str()
                    .or_else(|| song["primary_artist"]["name"].as_str())
                    .unwrap_or("");
                let year = song["release_date_components"]["year"]
                    .as_f64()
                    .filter(|y| *y != 0.0);
                results.push(ProviderResult {
                    id: format!("genius:{}", kairo_domain::js::number_to_string(id)),
                    provider: LyricsProvider::Genius,
                    title: if title.is_empty() {
                        "Untitled".into()
                    } else {
                        title.into()
                    },
                    artist: trim(artist).into(),
                    url: url.into(),
                    release_year: year.map(kairo_domain::js::number_to_string),
                    popularity: Some(song["stats"]["pageviews"].as_f64().unwrap_or(0.0)),
                    ..ProviderResult::default()
                });
                if results.len() >= MAX_RESULTS {
                    break 'outer;
                }
            }
        }
        self.search_cache
            .put(key, results.clone(), SEARCH_CACHE_TTL);
        tracing::info!(query = q, results = results.len(), "genius search");
        Ok(results)
    }

    /// A Genius song page as section-marked lyrics. With `title`, a medley
    /// page keeps only the segment that matches it.
    pub async fn fetch_lyrics(&self, url: &str, title: Option<&str>) -> Result<String, String> {
        if !GENIUS_URL.is_match(url) {
            return Err("Refusing to fetch a non-Genius URL.".into());
        }
        let key = format!("{url}::{}", trim(title.unwrap_or("")).to_lowercase());
        if let Some(cached) = self.lyrics_cache.get(&key) {
            return Ok(cached);
        }
        self.throttle.wait().await;
        let request = HttpRequest::get(url, REQUEST_TIMEOUT)
            .header("User-Agent", BROWSER_USER_AGENT)
            .header("Accept", "text/html");
        let html = self
            .http
            .get(request, success)
            .await
            .map_err(|e| friendly_error(&e, "fetch lyrics from Genius"))?
            .body;
        let containers = extract_lyric_containers(&html);
        let title = title
            .map(trim)
            .filter(|t| !t.is_empty())
            .map_or_else(|| title_from_url(url), str::to_owned);
        let extracted = select_primary_lyric_segment(&containers, &title);
        if trim(&extracted).is_empty() {
            return Err("No lyrics found on that Genius page. It may be instrumental, or the page layout changed.".into());
        }
        let normalized = normalize_genius_lyrics(&extracted);
        self.lyrics_cache
            .put(key, normalized.clone(), LYRICS_CACHE_TTL);
        Ok(normalized)
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
    fn containers_drop_excluded_nodes_and_keep_breaks() {
        let html = r#"<html><body>
            <div data-lyrics-container="true">[Verse 1]<br>Amazing grace<br><span data-exclude-from-selection="true">Ad</span>How sweet</div>
            <div data-lyrics-container="true"></div>
            <div data-lyrics-container="true">I once was lost</div>
        </body></html>"#;
        assert_eq!(
            extract_lyric_containers(html),
            ["[Verse 1]\nAmazing grace\nHow sweet", "", "I once was lost"]
        );
    }

    #[test]
    fn title_comes_from_the_slug() {
        assert_eq!(
            title_from_url("https://genius.com/Apostle-bamilaw-ijadopin-lyrics"),
            "Apostle bamilaw ijadopin"
        );
    }
}
