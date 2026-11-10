//! The last-resort web tier for songs Genius and LRCLIB miss. From
//! `src/main/services/lyrics/snippet-resolver.ts` and `page-title.ts`.
//!
//! Managed web search first (DeepSeek, then Brave); keyless HTML engines
//! after. Every tier must produce verified lyric candidates: a page counts
//! only when its lyrics or title match the query.

use super::ProviderResult;
use super::cache::TtlCache;
use super::genius::{BROWSER_USER_AGENT, Genius};
use super::http::{Http, HttpError, HttpRequest, ManagedGateway};
use super::lrclib::Lrclib;
use super::normalize::{canonical_text, trim};
use super::throttle::Throttle;
use super::web_lyrics::{is_allowed_lyrics_url, scrape_lyrics_page};
use super::web_search::{WebSearch, WebSearchResult};
use kairo_domain::lyrics::LyricsProvider;
use regex::Regex;
use scraper::{Html, Selector};
use std::collections::HashSet;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(12);
const MIN_REQUEST_GAP: Duration = Duration::from_millis(1_100);
const MAX_WEB_RESULTS: usize = 8;
const MAX_CANDIDATES: usize = 4;
const MAX_DIRECT_PAGES: usize = 2;
const MAX_SCRAPE_PAGES: usize = 3;
/// Shortest query still worth a web round trip.
pub const MIN_WEB_QUERY_CHARS: usize = 3;
const CACHE_TTL: Duration = Duration::from_secs(10 * 60);

/// What a real browser sends; the keyless engines bot-check.
const BROWSER_HEADERS: [(&str, &str); 7] = [
    ("User-Agent", BROWSER_USER_AGENT),
    (
        "Accept",
        "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
    ),
    ("Accept-Language", "en-US,en;q=0.9"),
    ("Sec-Fetch-Dest", "document"),
    ("Sec-Fetch-Mode", "navigate"),
    ("Sec-Fetch-Site", "none"),
    ("Upgrade-Insecure-Requests", "1"),
];

type Parser = fn(&str) -> Vec<WebSearchResult>;

/// Keyless engines, in order, until one answers.
const HTML_ENGINES: [(&str, &str, Parser); 4] = [
    (
        "duckduckgo",
        "https://html.duckduckgo.com/html/",
        parse_duck_html,
    ),
    (
        "duckduckgo-lite",
        "https://lite.duckduckgo.com/lite/",
        parse_duck_html,
    ),
    ("brave", "https://search.brave.com/search", parse_brave_html),
    ("mojeek", "https://www.mojeek.com/search", parse_mojeek_html),
];

static PAGE_TITLE_NOISE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(download|d\s?load|free|full|official|new|latest|watch|listen|stream|audio|video|music|song|songs|track|mp3|mp4|m4a|hd|4k|lyrics?|lyric|letra|paroles|translation|meaning|chords|tab|karaoke|instrumental)\b").unwrap()
});
static LEAD_LABEL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[^:]{0,24}:\s*").unwrap());
static BRACKETS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\([^)]*\)|\[[^\]]*\]").unwrap());
static SEPARATORS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[–—|·:]+").unwrap());
static AMP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*&\s*").unwrap());
static HYPHENS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"-+").unwrap());
static MULTI_SPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s{2,}").unwrap());

/// A search-result page title reduced to a song-name query.
pub fn clean_page_title(raw: &str) -> String {
    let title = raw.split('|').next().unwrap_or("");
    let title = LEAD_LABEL.replace(title, "");
    let title = BRACKETS.replace_all(&title, " ");
    let title = PAGE_TITLE_NOISE.replace_all(&title, " ");
    let title = SEPARATORS.replace_all(&title, " ");
    let title = AMP.replace_all(&title, " ");
    let title = HYPHENS.replace_all(&title, " ");
    trim(&MULTI_SPACE.replace_all(&title, " ")).to_owned()
}

static NOT_NEEDLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:lyrics|lyric|official|video)\b").unwrap());

/// Relevant music pages stay discoverable even when the source withholds
/// lyrics; they open in the browser (`lyricsAvailable: false`).
pub fn discover_song_pages(results: &[WebSearchResult], query: &str) -> Vec<ProviderResult> {
    let needle = trim(&NOT_NEEDLE.replace_all(query, " ")).to_owned();
    let mut found: Vec<ProviderResult> = Vec::new();
    for result in results {
        let Ok(url) = url::Url::parse(&result.url) else {
            continue;
        };
        if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
            continue;
        }
        let host = url.host_str().unwrap_or("").trim_start_matches("www.");
        let music = is_allowed_lyrics_url(url.as_str())
            || [
                "musixmatch.com",
                "shazam.com",
                "open.spotify.com",
                "music.apple.com",
                "youtube.com",
                "youtu.be",
                "genius.com",
            ]
            .contains(&host);
        let Some(title) = result.title.as_deref().filter(|t| !t.is_empty()) else {
            continue;
        };
        if !music || !title_matches_query(title, &needle) {
            continue;
        }
        let href = url.to_string();
        let page = ProviderResult {
            id: format!("web:{href}"),
            provider: LyricsProvider::Web,
            title: clean_page_title(title),
            artist: String::new(),
            url: href.clone(),
            lyrics_available: Some(false),
            ..ProviderResult::default()
        };
        match found.iter().position(|p| p.url == href) {
            Some(at) => found[at] = page,
            None => found.push(page),
        }
    }
    found.truncate(MAX_CANDIDATES);
    found
}

fn sel(css: &str) -> Selector {
    Selector::parse(css).expect("valid selector")
}

static BRAVE_SKIP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)brave\.com|youtube\.com|youtu\.be|scribd\.com|facebook\.com").unwrap()
});
static BRAVE_PREFIX_A: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\S+\s+\S+\s+›\s+\S+\s+").unwrap());
static BRAVE_PREFIX_B: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9 .-]+\s+\S+\s+›\s+").unwrap());
static WS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());

fn squash(text: &str) -> String {
    trim(&WS.replace_all(text, " ")).to_owned()
}

pub fn parse_brave_html(html: &str) -> Vec<WebSearchResult> {
    let document = Html::parse_document(html);
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for a in document.select(&sel(
        r#"div[data-type="web"] a[href^="http"], .snippet a[href^="http"]"#,
    )) {
        let Some(href) = a.value().attr("href").filter(|h| !h.is_empty()) else {
            continue;
        };
        if seen.contains(href) || BRAVE_SKIP.is_match(href) {
            continue;
        }
        seen.insert(href.to_owned());
        let text = squash(&a.text().collect::<String>());
        let once = BRAVE_PREFIX_A.replace(&text, "");
        let title = trim(&BRAVE_PREFIX_B.replace(&once, "")).to_owned();
        out.push(WebSearchResult {
            title: Some(if title.is_empty() { text } else { title }),
            url: href.into(),
            description: None,
        });
    }
    out
}

static JUNK_HOST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)brave\.com|duckduckgo\.com|mojeek\.com|google\.|bing\.com|youtube\.com|youtu\.be|scribd\.com|facebook\.com|instagram\.com|tiktok\.com|spotify\.com|apple\.com").unwrap()
});
static UDDG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[?&]uddg=([^&]+)").unwrap());

fn unwrap_duck_url(href: &str) -> String {
    if href.is_empty() {
        return String::new();
    }
    let absolute = if href.starts_with("//") {
        format!("https:{href}")
    } else {
        href.to_owned()
    };
    if let Some(c) = UDDG.captures(&absolute) {
        return percent_decode(&c[1]).unwrap_or_default();
    }
    if absolute.starts_with("http") {
        absolute
    } else {
        String::new()
    }
}

/// `decodeURIComponent`, failing on malformed sequences.
fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

pub fn parse_duck_html(html: &str) -> Vec<WebSearchResult> {
    let document = Html::parse_document(html);
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for a in document.select(&sel("a.result__a, a.result-link, h2 a[href]")) {
        let href = unwrap_duck_url(a.value().attr("href").unwrap_or(""));
        if href.is_empty() || seen.contains(&href) || JUNK_HOST.is_match(&href) {
            continue;
        }
        seen.insert(href.clone());
        out.push(WebSearchResult {
            title: Some(squash(&a.text().collect::<String>())),
            url: href,
            description: None,
        });
    }
    out
}

pub fn parse_mojeek_html(html: &str) -> Vec<WebSearchResult> {
    let document = Html::parse_document(html);
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for a in document.select(&sel(
        r#"ul.results-standard li h2 a[href^="http"], li.result h2 a[href^="http"]"#,
    )) {
        let href = a.value().attr("href").unwrap_or("");
        if href.is_empty() || seen.contains(href) || JUNK_HOST.is_match(href) {
            continue;
        }
        seen.insert(href.to_owned());
        let title = squash(&a.text().collect::<String>());
        if title.is_empty() {
            continue;
        }
        out.push(WebSearchResult {
            title: Some(title),
            url: href.into(),
            description: None,
        });
    }
    out
}

static TITLE_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)<title[^>]*>([^<]*)</title>").unwrap());
static CHALLENGE_TITLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)captcha|robot|unusual traffic|are you human|access denied|just a moment")
        .unwrap()
});
static CHALLENGE_BODY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)enable javascript and cookies to continue|verify you are (a )?human").unwrap()
});

/// An engine served a captcha or interstitial instead of results.
pub fn looks_like_challenge(html: &str) -> bool {
    let units: Vec<u16> = html.encode_utf16().take(4000).collect();
    let head = String::from_utf16_lossy(&units);
    let title = TITLE_TAG
        .captures(&head)
        .map(|c| c[1].to_owned())
        .unwrap_or_default();
    CHALLENGE_TITLE.is_match(&title) || CHALLENGE_BODY.is_match(&head)
}

fn matches_query(lyrics: &str, snippet: &str) -> bool {
    if snippet.is_empty() {
        return false;
    }
    let body = canonical_text(lyrics);
    if body.contains(snippet) {
        return true;
    }
    let words: Vec<&str> = snippet
        .split(' ')
        .filter(|w| w.encode_utf16().count() > 1)
        .collect();
    if words.len() < 3 {
        return false;
    }
    words.iter().filter(|w| body.contains(*w)).count() as f64 / words.len() as f64 >= 0.75
}

static ORIGIN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"https?://[^/]+/").unwrap());
static SLUG_SEPS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[-_/]").unwrap());

pub(crate) fn title_matches_query(title_or_url: &str, raw_query: &str) -> bool {
    let stripped = ORIGIN.replace(title_or_url, " ");
    let title = canonical_text(&SLUG_SEPS.replace_all(&stripped, " "));
    let query = canonical_text(raw_query);
    if title.is_empty() || query.is_empty() {
        return false;
    }
    if title.contains(&query) || query.contains(&title) {
        return true;
    }
    let ct = title.replace(' ', "");
    let cq = query.replace(' ', "");
    if ct.contains(&cq) || cq.contains(&ct) {
        return true;
    }
    let words: Vec<&str> = query
        .split(' ')
        .filter(|w| w.encode_utf16().count() > 2)
        .collect();
    if words.is_empty() {
        return false;
    }
    words
        .iter()
        .filter(|w| title.contains(*w) || ct.contains(*w))
        .count() as f64
        / words.len() as f64
        >= 0.75
}

fn first_matching_line(lyrics: &str, snippet: &str) -> Option<String> {
    let first = snippet.split(' ').next().unwrap_or("");
    for line in lyrics.split('\n') {
        let t = trim(line);
        if !t.is_empty()
            && canonical_text(t).contains(first)
            && (canonical_text(t).contains(snippet) || snippet.split(' ').count() <= 2)
        {
            return Some(t.into());
        }
    }
    lyrics
        .split('\n')
        .map(trim)
        .find(|l| !l.is_empty())
        .map(str::to_owned)
}

static GENIUS_PREFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^https://(www\.)?genius\.com/").unwrap());
static LYRICS_SUFFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"-lyrics/?$").unwrap());
static GENIUS_SONG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^https://(www\.)?genius\.com/[^/]+-lyrics/?$").unwrap());
static LISTING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)/tag/|/category/|/page/").unwrap());

fn title_from_genius_url(url: &str) -> (String, String) {
    let slug = LYRICS_SUFFIX
        .replace(&GENIUS_PREFIX.replace(url, ""), "")
        .into_owned();
    let words: Vec<&str> = slug.split('-').filter(|w| !w.is_empty()).collect();
    let title_case = |parts: &[&str]| {
        parts
            .iter()
            .map(|p| {
                let mut c = p.chars();
                c.next().map_or(String::new(), |f| {
                    f.to_uppercase().collect::<String>() + c.as_str()
                })
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    let split = 2.min(1.max(words.len().saturating_sub(1)));
    let split = split.min(words.len());
    (title_case(&words[split..]), title_case(&words[..split]))
}

fn dedupe_queries(queries: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    queries
        .into_iter()
        .filter(|q| {
            let k = canonical_text(q);
            !k.is_empty() && seen.insert(k)
        })
        .collect()
}

pub fn friendly_error(error: &HttpError) -> String {
    let code = match error {
        HttpError::Status { body, .. } => serde_json::from_str::<serde_json::Value>(body)
            .ok()
            .and_then(|v| v["error"]["code"].as_str().map(str::to_owned)),
        _ => None,
    };
    let status = error.status();
    if code.as_deref() == Some("SUBSCRIPTION_TOKEN_INVALID") || matches!(status, Some(401 | 403)) {
        return "The Brave Search API key was rejected. Check it in Settings → API Keys.".into();
    }
    if code.as_deref() == Some("RATE_LIMITED") || status == Some(429) {
        return "Web search is rate-limiting this machine right now.".into();
    }
    format!("Web search failed. {error}")
}

pub struct SnippetResolver {
    http: Arc<dyn Http>,
    gateway: Arc<dyn ManagedGateway>,
    genius: Arc<Genius>,
    lrclib: Arc<Lrclib>,
    search: Arc<WebSearch>,
    cache: TtlCache<Vec<ProviderResult>>,
    throttle: Throttle,
}

impl SnippetResolver {
    pub fn new(
        http: Arc<dyn Http>,
        gateway: Arc<dyn ManagedGateway>,
        genius: Arc<Genius>,
        lrclib: Arc<Lrclib>,
        search: Arc<WebSearch>,
    ) -> Self {
        Self {
            http,
            gateway,
            genius,
            lrclib,
            search,
            cache: TtlCache::new(),
            throttle: Throttle::spaced(MIN_REQUEST_GAP),
        }
    }

    /// Verified web candidates for a query the catalogues could not place;
    /// failing that, unverified music pages to open in the browser.
    pub async fn resolve(&self, snippet: &str) -> Vec<ProviderResult> {
        let query = trim(snippet).to_owned();
        if query.encode_utf16().count() < MIN_WEB_QUERY_CHARS {
            return Vec::new();
        }
        let key = query.to_lowercase();
        if let Some(cached) = self.cache.get(&key) {
            return cached;
        }
        let canonical = canonical_text(&query);
        let mut discovered: Vec<ProviderResult> = Vec::new();
        let queries = vec![format!("{query} lyrics")];
        let mut cursor = self.search.batches(self.gateway.as_ref(), &queries);
        while let Some(batch) = cursor.next().await {
            for page in discover_song_pages(&batch.results, &query) {
                upsert(&mut discovered, page);
            }
            let verified = self
                .gather_candidates(&batch.results, &canonical, &query)
                .await;
            if verified.is_empty() {
                continue;
            }
            tracing::info!(query, web = batch.results.len(), verified = verified.len(), via = ?batch.provider, "snippet resolved");
            self.cache.put(key, verified.clone(), CACHE_TTL);
            return verified;
        }
        match self.search_web_html(&query).await {
            Ok(results) => {
                for page in discover_song_pages(&results, &query) {
                    upsert(&mut discovered, page);
                }
                let verified = if results.is_empty() {
                    Vec::new()
                } else {
                    self.gather_candidates(&results, &canonical, &query).await
                };
                // Failures are not cached: recovery takes effect immediately.
                if !verified.is_empty() {
                    self.cache.put(key, verified.clone(), CACHE_TTL);
                    return verified;
                }
                discovered.truncate(MAX_CANDIDATES);
                discovered
            }
            Err(_) => {
                tracing::warn!("keyless web search failed");
                discovered.truncate(MAX_CANDIDATES);
                discovered
            }
        }
    }

    async fn gather_candidates(
        &self,
        results: &[WebSearchResult],
        snippet: &str,
        raw: &str,
    ) -> Vec<ProviderResult> {
        let mut found: Vec<ProviderResult> = Vec::new();
        let mut seen = HashSet::new();
        let mut push = |r: ProviderResult, found: &mut Vec<ProviderResult>| {
            if seen.insert(format!(
                "{}|{}",
                canonical_text(&r.title),
                canonical_text(&r.artist)
            )) {
                found.push(r);
            }
        };
        let genius_urls: Vec<String> = results
            .iter()
            .map(|r| r.url.clone())
            .filter(|u| GENIUS_SONG.is_match(u))
            .take(MAX_DIRECT_PAGES)
            .collect();
        let scrape_urls: Vec<String> = results
            .iter()
            .map(|r| r.url.clone())
            .filter(|u| is_allowed_lyrics_url(u) && !LISTING.is_match(u))
            .take(MAX_SCRAPE_PAGES)
            .collect();
        // The small, bounded set of pages is fetched together; results keep search order.
        let genius = genius_urls.iter().map(|url| async move {
            let (title, artist) = title_from_genius_url(url);
            match self.genius.fetch_lyrics(url, Some(&title)).await {
                Ok(lyrics) if matches_query(&lyrics, snippet) || title_matches_query(url, raw) => {
                    Some(ProviderResult {
                        id: format!("genius:{url}"),
                        provider: LyricsProvider::Genius,
                        title,
                        artist,
                        url: url.clone(),
                        lyrics: Some(lyrics),
                        ..ProviderResult::default()
                    })
                }
                Ok(_) => None,
                Err(_) => {
                    tracing::warn!(url, "genius page failed");
                    None
                }
            }
        });
        let scraped = scrape_urls.iter().map(|url| async move {
            match scrape_lyrics_page(&self.http, url).await {
                Ok(page) => {
                    let lyric_hit = matches_query(&page.lyrics, snippet);
                    let title_hit = title_matches_query(&page.title, raw)
                        || title_matches_query(&format!("{} {}", page.title, page.artist), raw);
                    if !lyric_hit && !title_hit {
                        return None;
                    }
                    Some(ProviderResult {
                        id: format!("web:{url}"),
                        provider: LyricsProvider::Web,
                        snippet: if lyric_hit {
                            first_matching_line(&page.lyrics, snippet)
                        } else {
                            None
                        },
                        title: page.title,
                        artist: page.artist,
                        url: page.url,
                        lyrics: Some(page.lyrics),
                        ..ProviderResult::default()
                    })
                }
                Err(_) => {
                    tracing::warn!(url, "scrape failed");
                    None
                }
            }
        });
        let (genius, scraped) = tokio::join!(
            futures_util::future::join_all(genius),
            futures_util::future::join_all(scraped)
        );
        for candidate in genius.into_iter().chain(scraped).flatten() {
            push(candidate, &mut found);
        }
        // Re-query LRCLIB with cleaned page titles once the real name is known.
        if found.is_empty() {
            let queries = dedupe_queries(
                results
                    .iter()
                    .map(|r| clean_page_title(r.title.as_deref().unwrap_or("")))
                    .collect(),
            );
            for candidate in queries.iter().take(MAX_CANDIDATES) {
                let Ok(hits) = self.lrclib.search(candidate).await else {
                    continue;
                };
                for hit in hits {
                    let Some(lyrics) = hit.lyrics.clone() else {
                        continue;
                    };
                    if !matches_query(&lyrics, snippet) && !title_matches_query(&hit.title, raw) {
                        continue;
                    }
                    let snippet_line = first_matching_line(&lyrics, snippet);
                    push(
                        ProviderResult {
                            snippet: snippet_line,
                            ..hit
                        },
                        &mut found,
                    );
                }
                if !found.is_empty() {
                    break;
                }
            }
        }
        found
    }

    /// Each engine once; a block, challenge or empty parse moves on. Only a
    /// clean sweep fails, with the first engine's reason.
    async fn search_web_html(&self, query: &str) -> Result<Vec<WebSearchResult>, String> {
        let mut first_error: Option<String> = None;
        for (name, url, parse) in HTML_ENGINES {
            self.throttle.wait().await;
            let mut request =
                HttpRequest::get(url, REQUEST_TIMEOUT).query("q", format!("{query} lyrics"));
            for (k, v) in BROWSER_HEADERS {
                request = request.header(k, v);
            }
            // A challenge page is a 202 here, not an error status.
            match self.http.get(request, |s| (200..400).contains(&s)).await {
                Ok(response) => {
                    if looks_like_challenge(&response.body) {
                        tracing::warn!(engine = name, "web engine served a challenge page");
                        continue;
                    }
                    let mut results = parse(&response.body);
                    results.truncate(MAX_WEB_RESULTS);
                    if !results.is_empty() {
                        tracing::info!(
                            engine = name,
                            results = results.len(),
                            "web engine answered"
                        );
                        return Ok(results);
                    }
                    tracing::warn!(
                        engine = name,
                        status = response.status,
                        "web engine returned nothing"
                    );
                }
                Err(error) => {
                    let friendly = friendly_error(&error);
                    tracing::warn!(engine = name, error = friendly, "web engine failed");
                    first_error.get_or_insert(friendly);
                }
            }
        }
        match first_error {
            Some(e) => Err(e),
            None => Ok(Vec::new()),
        }
    }

    pub fn clear_cache(&self) {
        self.cache.clear();
    }
}

fn upsert(list: &mut Vec<ProviderResult>, page: ProviderResult) {
    match list.iter().position(|p| p.url == page.url) {
        Some(at) => list[at] = page,
        None => list.push(page),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_titles_lose_site_and_format_noise() {
        assert_eq!(
            clean_page_title("DOWNLOAD SONG: Nathaniel Bassey - Ese (Mp3 & Lyrics) | CeeNaija"),
            "Nathaniel Bassey Ese"
        );
    }

    #[test]
    fn duck_links_are_unwrapped() {
        let html = r#"<a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fceenaija.com%2Fese%2F&rut=x">Ese  Lyrics</a>
            <a class="result__a" href="https://www.youtube.com/watch?v=1">Video</a>"#;
        let results = parse_duck_html(html);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://ceenaija.com/ese/");
        assert_eq!(results[0].title.as_deref(), Some("Ese Lyrics"));
    }

    #[test]
    fn challenge_pages_are_recognized() {
        assert!(looks_like_challenge(
            "<html><title>Just a moment...</title>"
        ));
        assert!(looks_like_challenge("<p>Please verify you are a human</p>"));
        assert!(!looks_like_challenge(
            "<title>ese lyrics at DuckDuckGo</title>"
        ));
    }

    #[test]
    fn genius_slugs_split_artist_and_title() {
        assert_eq!(
            title_from_genius_url("https://genius.com/Nathaniel-bassey-ese-lyrics"),
            ("Ese".into(), "Nathaniel Bassey".into())
        );
    }
}
