//! English glosses already published on gospel lyric sites. From
//! `src/main/services/lyrics/online-glosses.ts`. Preferred over machine
//! translation for African worship songs.

use super::genius::BROWSER_USER_AGENT;
use super::http::{Http, HttpRequest, ManagedGateway, success};
use super::normalize::trim;
use super::web_lyrics::{
    GLOSS_REMOVED, first_kept, is_allowed_lyrics_url, kept_text, structure_bilingual_hymn,
};
use super::web_search::{WebSearch, WebSearchResult};
use kairo_domain::lyrics::translate::{
    GlossPairs, clean_gloss_source, extract_inline_gloss_pairs, gloss_match_key, is_gloss_line,
    is_likely_english_lyric, lookup_online_gloss, should_apply_gloss,
};
use regex::Regex;
use scraper::{Html, Selector};
use std::collections::HashSet;
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use unicode_normalization::UnicodeNormalization;

const BRAVE_HTML_URL: &str = "https://search.brave.com/search";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(12);
const MAX_PAGES: usize = 3;

static LISTING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)/tag/|/category/|/page/").unwrap());
static NON_SLUG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^a-z0-9]+").unwrap());
static PAREN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\([^)]*\)").unwrap());
static WS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());
static IJA_DOPIN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)ija\s*d[''`]?opin|ijadopin|strife is o.?er|ogu+n si\s*tan").unwrap()
});
static NBSP_SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t]+").unwrap());

fn slug(value: &str) -> String {
    let folded: String = value
        .to_lowercase()
        .nfkd()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
        .collect();
    let plain: String = folded
        .chars()
        .filter(|c| !matches!(c, '\'' | '`' | '´'))
        .collect();
    NON_SLUG
        .replace_all(&plain, "-")
        .trim_matches('-')
        .to_owned()
}

/// Well-known gospel lyric URL shapes, tried before search.
fn seed_urls(title: &str, artist: Option<&str>) -> Vec<String> {
    let bare = trim(&WS.replace_all(&PAREN.replace_all(title, " "), " ")).to_owned();
    let t = Some(slug(&bare))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| slug(title));
    let a = artist.map(slug).unwrap_or_default();
    let hay = format!("{title} {bare}").to_lowercase();
    let mut urls = Vec::new();
    let ija = "https://yorubahymns.blogspot.com/2015/04/ija-dopin-ogun-si-tan.html";
    if IJA_DOPIN.is_match(&hay) {
        urls.push(ija.to_owned());
    }
    if !t.is_empty() {
        urls.push(format!("https://africangospellyrics.com/{t}/"));
        if t == "odudu" {
            urls.push("https://africangospellyrics.com/2026/04/02/odudu/".into());
        }
        urls.push(format!("https://yorubahymns.blogspot.com/2015/04/{t}.html"));
        if t.contains("ija") && (t.contains("dopin") || t.contains("opin")) {
            urls.push(ija.to_owned());
        }
        if !a.is_empty() {
            urls.push(format!("https://lyricsom.com/{t}-lyrics-by-{a}/"));
            urls.push(format!("https://gospelsongs.com.ng/{a}-{t}-lyrics/"));
        }
    }
    urls.retain(|u| is_allowed_lyrics_url(u));
    urls
}

fn unwrap_gloss(line: &str) -> String {
    let t = trim(line);
    let t = t.strip_prefix('(').unwrap_or(t);
    let t = t.strip_suffix(')').unwrap_or(t);
    trim(t).to_owned()
}

fn insert(pairs: &mut GlossPairs, key: String, gloss: String) {
    if !pairs.iter().any(|(k, _)| *k == key) {
        pairs.push((key, gloss));
    }
}

/// `source (gloss)` pairs from one page's HTML.
pub fn gloss_pairs_from_html(html: &str) -> GlossPairs {
    let document = Html::parse_document(html);
    let roots = Selector::parse(".entry-content, .post-content, .post-body, .lyrics, article")
        .expect("selector");
    let body = Selector::parse("body").expect("selector");
    let root = first_kept(&document, &roots, &GLOSS_REMOVED)
        .or_else(|| first_kept(&document, &body, &GLOSS_REMOVED));
    let Some(root) = root else {
        return Vec::new();
    };
    let text = kept_text(root, &GLOSS_REMOVED)
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .split('\n')
        .map(|l| trim(&NBSP_SPACES.replace_all(l, " ")).to_owned())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let mut pairs = extract_inline_gloss_pairs(&text);
    // Already expanded under-line glosses: source, then (gloss).
    let lines: Vec<&str> = text.split('\n').collect();
    for w in lines.windows(2) {
        if is_gloss_line(w[1]) && !is_gloss_line(w[0]) {
            let key = gloss_match_key(&clean_gloss_source(w[0]));
            let gloss = unwrap_gloss(w[1]);
            if key.encode_utf16().count() >= 3 && !gloss.is_empty() {
                insert(&mut pairs, key, gloss);
            }
        }
    }
    // Numbered bilingual hymns publish English as a second block: zip them.
    if let Some(hymn) = structure_bilingual_hymn(&text) {
        let hymn_lines: Vec<&str> = hymn.split('\n').collect();
        for w in hymn_lines.windows(2) {
            if is_gloss_line(w[1]) && !is_gloss_line(w[0]) && !trim(w[0]).starts_with('[') {
                let key = gloss_match_key(&clean_gloss_source(w[0]));
                let gloss = unwrap_gloss(w[1]);
                if key.encode_utf16().count() >= 3 && !gloss.is_empty() {
                    insert(&mut pairs, key, gloss);
                }
            }
        }
    }
    pairs
}

fn brave_hits(html: &str) -> Vec<WebSearchResult> {
    static SKIP: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)brave\.com|youtube\.com|youtu\.be|facebook\.com").unwrap()
    });
    let document = Html::parse_document(html);
    let links =
        Selector::parse(r#"div[data-type="web"] a[href^="http"], .snippet a[href^="http"]"#)
            .expect("selector");
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for a in document.select(&links) {
        let Some(href) = a.value().attr("href").filter(|h| !h.is_empty()) else {
            continue;
        };
        if seen.contains(href) || SKIP.is_match(href) {
            continue;
        }
        seen.insert(href.to_owned());
        out.push(WebSearchResult {
            title: Some(trim(&a.text().collect::<String>()).to_owned()),
            url: href.into(),
            description: None,
        });
    }
    out
}

pub struct GlossFinder {
    pub(crate) http: Arc<dyn Http>,
    pub(crate) gateway: Arc<dyn ManagedGateway>,
    pub(crate) search: Arc<WebSearch>,
}

impl GlossFinder {
    pub fn new(
        http: Arc<dyn Http>,
        gateway: Arc<dyn ManagedGateway>,
        search: Arc<WebSearch>,
    ) -> Self {
        Self {
            http,
            gateway,
            search,
        }
    }

    async fn harvest(&self, urls: &[String], seen: &mut HashSet<String>, merged: &mut GlossPairs) {
        for url in urls {
            if !seen.insert(url.clone()) {
                continue;
            }
            let request = HttpRequest::get(url, REQUEST_TIMEOUT)
                .header("User-Agent", BROWSER_USER_AGENT)
                .header("Accept", "text/html");
            match self.http.get(request, success).await {
                Ok(response) => {
                    for (key, gloss) in gloss_pairs_from_html(&response.body) {
                        insert(merged, key, gloss);
                    }
                }
                Err(error) => {
                    tracing::info!(url, status = ?error.status(), "skip unavailable gloss page")
                }
            }
        }
    }

    fn lyric_urls(hits: &[WebSearchResult]) -> Vec<String> {
        hits.iter()
            .map(|h| h.url.clone())
            .filter(|u| is_allowed_lyrics_url(u) && !LISTING.is_match(u))
            .take(MAX_PAGES)
            .collect()
    }

    /// Published glosses keyed by line, enough to cover `source_lines` when
    /// possible: seed pages, managed search, then keyless Brave.
    pub async fn fetch(
        &self,
        title: &str,
        artist: Option<&str>,
        source_lines: Option<&[String]>,
    ) -> GlossPairs {
        let title = trim(title);
        if title.encode_utf16().count() < 2 {
            return Vec::new();
        }
        let artist = artist.map(trim).filter(|a| !a.is_empty());
        let query = [Some(title), artist, Some("lyrics")]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ");
        let gloss_query = format!(
            "{query} (site:africangospellyrics.com OR site:lyricsom.com OR site:yorubahymns.blogspot.com OR site:blogspot.com)"
        );
        let requested: Option<Vec<&String>> = source_lines.map(|lines| {
            lines
                .iter()
                .filter(|l| !trim(l).is_empty() && !is_likely_english_lyric(l))
                .collect()
        });
        let covered = |merged: &GlossPairs| match &requested {
            Some(lines) if !lines.is_empty() => lines.iter().all(|line| {
                lookup_online_gloss(line, merged)
                    .is_some_and(|g| should_apply_gloss(line, &g, Some("und")))
            }),
            _ => !merged.is_empty(),
        };
        let mut seen = HashSet::new();
        let mut merged: GlossPairs = Vec::new();
        self.harvest(&seed_urls(title, artist), &mut seen, &mut merged)
            .await;
        let queries = vec![gloss_query.clone(), query.clone()];
        let mut cursor = self.search.batches(self.gateway.as_ref(), &queries);
        while let Some(batch) = cursor.next().await {
            self.harvest(&Self::lyric_urls(&batch.results), &mut seen, &mut merged)
                .await;
            if covered(&merged) {
                return merged;
            }
        }
        for q in [&gloss_query, &query] {
            let request = HttpRequest::get(BRAVE_HTML_URL, REQUEST_TIMEOUT)
                .query("q", q.as_str())
                .header("User-Agent", BROWSER_USER_AGENT)
                .header("Accept", "text/html");
            match self.http.get(request, success).await {
                Ok(response) => {
                    self.harvest(
                        &Self::lyric_urls(&brave_hits(&response.body)),
                        &mut seen,
                        &mut merged,
                    )
                    .await;
                    if covered(&merged) {
                        break;
                    }
                }
                Err(_) => tracing::warn!("keyless gloss search failed"),
            }
        }
        tracing::info!(
            query,
            pages = seen.len(),
            pairs = merged.len(),
            "glosses harvested"
        );
        merged
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_cover_known_hymn_and_site_shapes() {
        let urls = seed_urls("Ija D'opin (The Strife is O'er)", None);
        assert_eq!(
            urls[0],
            "https://yorubahymns.blogspot.com/2015/04/ija-dopin-ogun-si-tan.html"
        );
        assert!(urls.contains(&"https://africangospellyrics.com/ija-dopin/".to_string()));
        let urls = seed_urls("Odudu", Some("Theophilus Sunday"));
        assert!(urls.contains(&"https://africangospellyrics.com/2026/04/02/odudu/".to_string()));
        assert!(
            urls.contains(&"https://lyricsom.com/odudu-lyrics-by-theophilus-sunday/".to_string())
        );
    }

    #[test]
    fn inline_and_underline_glosses_are_collected() {
        let html = "<article><p>Odudu dabu Jesus no (There is no other Name like Jesus)<br>Amioluwa<br>(The mark of God)</p></article>";
        let pairs = gloss_pairs_from_html(html);
        assert!(
            pairs
                .iter()
                .any(|(_, g)| g == "There is no other Name like Jesus")
        );
        assert!(pairs.iter().any(|(_, g)| g == "The mark of God"));
    }
}
