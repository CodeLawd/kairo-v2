//! Lyrics from allowlisted gospel lyric sites and Blogger hymn archives.
//! From `src/main/services/lyrics/web-lyrics.ts`, with `scraper` in place of
//! `cheerio`. Only the hosts Electron allows are fetched.

use super::genius::{BROWSER_USER_AGENT, element_text};
use super::http::{Http, HttpError, HttpRequest, success};
use super::normalize::{structure_lyrics, trim};
use kairo_domain::lyrics::translate::{expand_inline_glosses_to_lines, is_gloss_line};
use regex::Regex;
use scraper::{ElementRef, Html, Selector};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(12);

const ALLOWED_HOSTS: [&str; 14] = [
    "africangospellyrics.com",
    "www.africangospellyrics.com",
    "lyricsom.com",
    "www.lyricsom.com",
    "tooxclusive.com",
    "www.tooxclusive.com",
    "mpmania.com",
    "www.mpmania.com",
    "ceenaija.com",
    "www.ceenaija.com",
    "gospelsongs.com.ng",
    "www.gospelsongs.com.ng",
    "notjustok.com",
    "www.notjustok.com",
];

#[derive(Clone, Debug, PartialEq)]
pub struct ScrapedLyricsPage {
    pub title: String,
    pub artist: String,
    pub lyrics: String,
    pub url: String,
}

/// Allowlisted hosts, plus Blogger hymn archives.
pub fn is_allowed_lyrics_url(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    let Some(host) = parsed.host_str().map(str::to_lowercase) else {
        return false;
    };
    ALLOWED_HOSTS.contains(&host.as_str())
        || host.ends_with(".blogspot.com")
        || host.ends_with(".blogspot.co.uk")
}

fn sel(css: &str) -> Selector {
    Selector::parse(css).expect("valid selector")
}

static REMOVED: LazyLock<Selector> = LazyLock::new(|| {
    sel(
        "script, style, noscript, nav, header, footer, aside, .comments, #comments, .sharedaddy, .jp-relatedposts",
    )
});
static REVIEW_REMOVED: LazyLock<Selector> = LazyLock::new(|| {
    sel(
        "script, style, noscript, nav, header, footer, aside, form, button, iframe, figure, figcaption, \
         .comments, #comments, .sharedaddy, .jp-relatedposts, .related-posts, .yarpp-related, \
         .post-tags, .post-meta, .author-box, .social-share, .newsletter, .ads, .advertisement",
    )
});
pub(crate) static GLOSS_REMOVED: LazyLock<Selector> = LazyLock::new(|| {
    sel("script, style, noscript, nav, header, footer, aside, .comments, #comments")
});

/// Whether `$(removed).remove()` would have taken this element out.
fn is_removed(element: ElementRef<'_>, removed: &Selector) -> bool {
    std::iter::once(element)
        .chain(element.ancestors().filter_map(ElementRef::wrap))
        .any(|e| removed.matches(&e))
}

/// The first element matching `css` that survives the removal pass.
pub(crate) fn first_kept<'a>(
    document: &'a Html,
    css: &Selector,
    removed: &Selector,
) -> Option<ElementRef<'a>> {
    document.select(css).find(|e| !is_removed(*e, removed))
}

/// `.text()` of an element after the removal pass, with `<br>` as newlines.
pub(crate) fn kept_text(element: ElementRef<'_>, removed: &Selector) -> String {
    element_text(element, &|e| removed.matches(&e))
}

static NBSP_SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t]+").unwrap());

fn tidy_block(text: &str) -> String {
    text.replace('\u{a0}', " ")
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .split('\n')
        .map(|line| trim(&NBSP_SPACES.replace_all(line, " ")).to_owned())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn extract_lyric_body_with(
    document: &Html,
    removed: &Selector,
    boilerplate: fn(&str) -> bool,
    filter_single_paragraph: bool,
) -> String {
    let p = sel("p");
    for css in [
        ".post-body.entry-content",
        ".post-body",
        ".entry-content",
        ".post-content",
        ".lyrics",
        "article .content",
        "article",
    ] {
        let Some(root) = first_kept(document, &sel(css), removed) else {
            continue;
        };
        // Paragraphs keep stanza breaks; <br> stays a real newline.
        let mut total_paragraphs = 0usize;
        let paragraphs: Vec<String> = root
            .select(&p)
            .filter(|e| !is_removed(*e, removed))
            .map(|e| tidy_block(&kept_text(e, removed)))
            .filter(|b| !b.is_empty())
            .inspect(|_| total_paragraphs += 1)
            .filter_map(|b| {
                if !filter_single_paragraph {
                    return (!boilerplate(&b)).then_some(b);
                }
                // Filter line-by-line so a heading or trailing download CTA inside
                // a <p> with <br> breaks never drops the song's first/last stanza.
                let kept: Vec<&str> = b.lines().filter(|l| !boilerplate(l)).collect();
                (!kept.is_empty()).then(|| kept.join("\n"))
            })
            .collect();
        if paragraphs.len() >= 2
            || (filter_single_paragraph && total_paragraphs >= 2 && !paragraphs.is_empty())
        {
            return paragraphs.join("\n\n");
        }
        let text = kept_text(root, removed)
            .replace('\u{a0}', " ")
            .replace("\r\n", "\n")
            .replace('\r', "\n");
        let text = if filter_single_paragraph {
            let joined = text
                .split('\n')
                .map(|l| trim(&NBSP_SPACES.replace_all(l, " ")).to_owned())
                .filter(|l| l.is_empty() || !boilerplate(l))
                .collect::<Vec<_>>()
                .join("\n");
            BLANKS3.replace_all(&joined, "\n\n").into_owned()
        } else {
            text
        };
        let text = trim(&text);
        if text.encode_utf16().count() > 80 {
            return text.to_owned();
        }
    }
    String::new()
}

static RELATED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^related\b").unwrap());
static COMMENTS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^comments?\b").unwrap());

fn is_boilerplate(line: &str) -> bool {
    let lower = line.to_lowercase();
    [
        "languages:",
        "discover more",
        "subscribe",
        "leave a comment",
        "share this",
        "type your email",
        "posted by",
        "email this",
    ]
    .iter()
    .any(|p| lower.starts_with(p))
        || lower.contains("wordpress.com account")
        || lower.contains("sign me up")
        || RELATED.is_match(&lower)
        || COMMENTS.is_match(&lower)
}

static DOWNLOAD_CTA: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^download\b.+\b(by|mp3|audio|video|song|here|below)\b").unwrap()
});
static LYRIC_CREDIT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^lyrics?\s+(for\b.+by\b|extracted|transcribed|courtesy|submitted)").unwrap()
});

/// Stricter blog/MP3-site CTA filter used before AI review so ads, download
/// prompts and social links are dropped up front.
fn is_review_boilerplate(block: &str) -> bool {
    if is_boilerplate(block) {
        return true;
    }
    let lower = block.to_lowercase();
    [
        "download mp3",
        "download audio",
        "download song",
        "download below",
        "stream below",
        "listen below",
        "watch video",
        "watch official",
        "click here to download",
        "join our whatsapp",
        "join our telegram",
        "follow us on",
        "drop your comment",
        "read also",
        "also read",
        "check out",
        "tags:",
        "filed under:",
        "categories:",
    ]
    .iter()
    .any(|p| lower.starts_with(p))
        || DOWNLOAD_CTA.is_match(block)
        || LYRIC_CREDIT.is_match(block)
        || lower.contains("all rights reserved")
        || lower.contains("download mp3 here")
        || lower.contains("join our whatsapp")
        || lower.contains("join our telegram")
}

fn extract_heading(document: &Html) -> String {
    first_kept(
        document,
        &sel("h1.entry-title, h1.post-title, article h1, .entry-header h1"),
        &REMOVED,
    )
    .map(|e| trim(&kept_text(e, &REMOVED)).to_owned())
    .filter(|t| !t.is_empty())
    .or_else(|| {
        document
            .select(&sel(r#"meta[property="og:title"]"#))
            .next()
            .and_then(|m| m.value().attr("content"))
            .map(|c| trim(c).to_owned())
            .filter(|t| !t.is_empty())
    })
    .unwrap_or_else(|| {
        document
            .select(&sel("title"))
            .next()
            .map(|t| trim(&t.text().collect::<String>()).to_owned())
            .unwrap_or_default()
    })
}

/// Title, artist and lyric body of an allowlisted page, given its HTML.
pub fn parse_lyrics_page(url: &str, html: &str) -> Result<ScrapedLyricsPage, String> {
    let document = Html::parse_document(html);
    let (title, artist) = parse_page_heading(&extract_heading(&document));
    let body = extract_lyric_body_with(&document, &REMOVED, is_boilerplate, false);
    if trim(&body).is_empty() {
        return Err("No lyrics found on that page.".into());
    }
    let cleaned = clean_scraped_lyrics(&body);
    let lyrics = structure_bilingual_hymn(&cleaned).unwrap_or_else(|| structure_lyrics(&cleaned));
    tracing::info!(
        url,
        title,
        artist,
        chars = lyrics.len(),
        "scraped lyrics page"
    );
    Ok(ScrapedLyricsPage {
        title,
        artist,
        lyrics,
        url: url.into(),
    })
}

/// Extracts an allowlisted page for AI review: strips non-content widgets and
/// MP3/blog CTAs, preserves bilingual hymn pairing, and avoids pre-slicing
/// prose into fake `[Verse]` blocks before the reviewer sees it.
pub fn parse_lyrics_for_review(url: &str, html: &str) -> Result<ScrapedLyricsPage, String> {
    let document = Html::parse_document(html);
    let (title, artist) = parse_page_heading(&extract_heading(&document));
    let body = extract_lyric_body_with(&document, &REVIEW_REMOVED, is_review_boilerplate, true);
    if trim(&body).is_empty() {
        return Err("No lyrics found on that page.".into());
    }
    let lyrics = structure_bilingual_hymn(&body).unwrap_or(body);
    tracing::info!(
        url,
        title,
        artist,
        chars = lyrics.len(),
        "scraped lyrics page for review"
    );
    Ok(ScrapedLyricsPage {
        title,
        artist,
        lyrics,
        url: url.into(),
    })
}

/// Formats raw web text when AI review is unavailable so unverified previews
/// still display with sections.
pub fn fallback_structure(raw: &str) -> String {
    if raw.contains("[Verse") {
        return raw.to_owned();
    }
    let cleaned = clean_scraped_lyrics(raw);
    structure_bilingual_hymn(&cleaned).unwrap_or_else(|| structure_lyrics(&cleaned))
}

pub fn friendly_error(error: &HttpError) -> String {
    match error {
        HttpError::Timeout => "Timed out fetching lyrics page.".into(),
        HttpError::Offline => "No internet connection.".into(),
        other => format!("Could not fetch lyrics page. {other}"),
    }
}

async fn fetch_allowed_html(http: &Arc<dyn Http>, url: &str) -> Result<String, String> {
    if !is_allowed_lyrics_url(url) {
        return Err("Refusing to scrape a non-allowlisted lyrics host.".into());
    }
    let request = HttpRequest::get(url, REQUEST_TIMEOUT)
        .header("User-Agent", BROWSER_USER_AGENT)
        .header("Accept", "text/html");
    Ok(http
        .get(request, success)
        .await
        .map_err(|e| friendly_error(&e))?
        .body)
}

/// Fetches and parses an allowlisted page.
pub async fn scrape_lyrics_page(
    http: &Arc<dyn Http>,
    url: &str,
) -> Result<ScrapedLyricsPage, String> {
    let html = fetch_allowed_html(http, url).await?;
    parse_lyrics_page(url, &html)
}

/// Fetches an allowlisted page prepared for AI review.
pub async fn scrape_lyrics_for_review(
    http: &Arc<dyn Http>,
    url: &str,
) -> Result<ScrapedLyricsPage, String> {
    let html = fetch_allowed_html(http, url).await?;
    parse_lyrics_for_review(url, &html)
}

static NUMBERED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d+\.\s*").unwrap());
static NUMBERED_DOT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d+\.").unwrap());
static NUMBERED_SPLIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\d+)\.\s*(.*)$").unwrap());
static ENGLISHY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(the|is|of|and|for|lord|battle|done|over|o'?er|strife|strive|song|hymn)\b")
        .unwrap()
});
static ALLELUIA: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(alleluia|hallelujah|amen)\b").unwrap());
static ENDS_SENTENCE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[.!?]$").unwrap());
static WS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());
static BLANKS3: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n{3,}").unwrap());

/// Numbered bilingual hymns (source stanzas, then English stanzas) to
/// `[Verse N]` blocks with English lines as `(gloss)` rows. `None` otherwise.
pub fn structure_bilingual_hymn(raw: &str) -> Option<String> {
    let lines: Vec<String> = raw
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .split('\n')
        .map(|l| trim(&NBSP_SPACES.replace_all(&l.replace('\u{a0}', " "), " ")).to_owned())
        .filter(|l| !l.is_empty() && !is_boilerplate(l))
        .collect();
    if lines.len() < 8 {
        return None;
    }
    let mut saw_numbered = false;
    let mut english_at = None;
    for (i, line) in lines.iter().enumerate() {
        if NUMBERED.is_match(line) {
            saw_numbered = true;
            continue;
        }
        if saw_numbered && is_secondary_hymn_title(line) {
            english_at = Some(i);
            break;
        }
    }
    let at = english_at?;
    let source = parse_numbered_verses(&lines[..at]);
    let english = parse_numbered_verses(&lines[at + 1..]);
    if source.is_empty() || english.is_empty() {
        return None;
    }
    let mut out: Vec<String> = Vec::new();
    for verse in &source {
        out.push(String::new());
        out.push(format!("[Verse {}]", verse.0));
        let gloss = english
            .iter()
            .find(|(n, _)| *n == verse.0)
            .map(|(_, l)| l.as_slice())
            .unwrap_or(&[]);
        for i in 0..verse.1.len().max(gloss.len()) {
            if let Some(s) = verse.1.get(i).filter(|s| !s.is_empty()) {
                out.push(s.clone());
            }
            if let Some(g) = gloss.get(i).filter(|g| !g.is_empty()) {
                out.push(format!("({g})"));
            }
        }
    }
    let structured = trim(&BLANKS3.replace_all(&out.join("\n"), "\n\n")).to_owned();
    structured.contains("[Verse").then_some(structured)
}

fn is_secondary_hymn_title(line: &str) -> bool {
    if line.encode_utf16().count() < 12 || NUMBERED_DOT.is_match(line) {
        return false;
    }
    let letters: Vec<char> = line.chars().filter(|c| c.is_ascii_alphabetic()).collect();
    if letters.len() < 10 {
        return false;
    }
    let upper = letters.iter().filter(|c| c.is_ascii_uppercase()).count();
    if (upper as f64) / (letters.len() as f64) < 0.8 {
        return false;
    }
    ENGLISHY.is_match(line)
}

fn parse_numbered_verses(lines: &[String]) -> Vec<(u32, Vec<String>)> {
    let mut verses: Vec<(u32, Vec<String>)> = Vec::new();
    let mut current: Option<(u32, Vec<String>)> = None;
    for line in lines {
        if let Some(c) = NUMBERED_SPLIT.captures(line) {
            if let Some((n, l)) = current.take() {
                verses.push((n, reflow_broken_lines(l)));
            }
            let rest = trim(&c[2]);
            let num = c[1]
                .parse::<u32>()
                .ok()
                .filter(|n| *n != 0)
                .unwrap_or(verses.len() as u32 + 1);
            current = Some((
                num,
                if rest.is_empty() {
                    Vec::new()
                } else {
                    vec![rest.into()]
                },
            ));
            continue;
        }
        let Some(cur) = current.as_mut() else {
            continue;
        };
        if is_secondary_hymn_title(line) {
            break;
        }
        cur.1.push(line.clone());
    }
    if let Some((n, l)) = current {
        verses.push((n, reflow_broken_lines(l)));
    }
    verses
}

/// Joins short wrapped fragments back into singable lines.
fn reflow_broken_lines(lines: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in lines {
        let alleluia = ALLELUIA.is_match(&line);
        match out.last_mut() {
            Some(prev)
                if !alleluia
                    && !ALLELUIA.is_match(prev)
                    && prev.encode_utf16().count() < 28
                    && !ENDS_SENTENCE.is_match(prev) =>
            {
                *prev = trim(&WS.replace_all(&format!("{prev} {line}"), " ")).to_owned();
            }
            _ => out.push(line),
        }
    }
    out
}

static TIMES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bx\d+\b").unwrap());
static MID_PAREN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\S)[^\S\n]*\([^)\n]{0,120}\)").unwrap());
static LABEL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?im)^\s*(spoken|refrain|chorus|verse|bridge|pre-?chorus)\s*:?\s*").unwrap()
});
static PRE_CHORUS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^pre-?chorus$").unwrap());
static SECTION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\[.+\]$").unwrap());
static DOUBLE_SPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s{2,}").unwrap());
static TRAILING_BLANKS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t]+\n").unwrap());

/// Drops inline translation parentheticals and normalizes section labels.
pub fn clean_scraped_lyrics(raw: &str) -> String {
    let unix = raw.replace("\r\n", "\n").replace('\r', "\n");
    let expanded = expand_inline_glosses_to_lines(&TIMES.replace_all(&unix, ""));
    let no_paren = MID_PAREN.replace_all(&expanded, "$1");
    let labelled = LABEL.replace_all(&no_paren, |c: &regex::Captures<'_>| {
        let label: String = c[0]
            .chars()
            .filter(|ch| *ch != ':' && !kairo_domain::js::is_whitespace(*ch))
            .collect();
        let lower = label.to_lowercase();
        if lower == "spoken" {
            "[Verse]\n".into()
        } else if lower == "refrain" || lower == "chorus" {
            "[Chorus]\n".into()
        } else if PRE_CHORUS.is_match(&label) {
            "[Pre-Chorus]\n".into()
        } else if lower == "bridge" {
            "[Bridge]\n".into()
        } else if lower == "verse" {
            "[Verse]\n".into()
        } else {
            c[0].to_owned()
        }
    });
    let lines: Vec<String> = labelled
        .split('\n')
        .flat_map(|line| {
            let trimmed = line.trim_end_matches(kairo_domain::js::is_whitespace);
            if trim(trimmed).is_empty() {
                return vec![String::new()];
            }
            let t = trim(trimmed);
            if SECTION.is_match(t) || is_gloss_line(t) {
                return vec![t.to_owned()];
            }
            if DOUBLE_SPACE.is_match(trimmed) {
                return DOUBLE_SPACE
                    .split(trimmed)
                    .map(trim)
                    .filter(|p| !p.is_empty())
                    .map(str::to_owned)
                    .collect();
            }
            vec![trimmed.to_owned()]
        })
        .collect();
    let joined = TRAILING_BLANKS
        .replace_all(&lines.join("\n"), "\n")
        .into_owned();
    trim(&BLANKS3.replace_all(&joined, "\n\n")).to_owned()
}

static LYRICS_BY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(.*?)\s+lyrics(?:\s+by)?\s+(.+)$").unwrap());
static LYRICS_TAIL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\s+lyrics\s*$").unwrap());
static BY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(.*?)\s+by\s+(.+)$").unwrap());
static DASH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(.+?)\s+[-–—]\s+(.+)$").unwrap());
static TRAILING_ARTISTS: LazyLock<Regex> = LazyLock::new(|| {
    let name = r"[A-Z][A-Za-z0-9_'.-]+(?:\s+[A-Z][A-Za-z0-9_'.-]+){0,3}";
    Regex::new(&format!(
        r"^(.*?)[\s,]+({name}(?:\s+(?:and|&|,)\s+{name})+)$"
    ))
    .unwrap()
});
static AND: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\s+and\s+").unwrap());
static SITE_TAIL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*[-–—]\s*[A-Za-z0-9_.-]+\.(com|ng|net|org)\b.*$").unwrap());
static PAREN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\([^)]*\)").unwrap());
static MULTI_SPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s{2,}").unwrap());

/// "Odudu (Spirit) Lyrics by Theophilus Sunday | African Gospel Lyrics"
/// → ("Odudu", "Theophilus Sunday").
pub fn parse_page_heading(raw: &str) -> (String, String) {
    let mut heading = trim(raw.split('|').next().unwrap_or("")).to_owned();
    if let Some(c) = LYRICS_BY.captures(&heading) {
        return (strip_parenthetical(&c[1]), clean_artist(&c[2]));
    }
    heading = trim(&LYRICS_TAIL.replace(&heading, "")).to_owned();
    if let Some(c) = BY.captures(&heading) {
        return (strip_parenthetical(&c[1]), clean_artist(&c[2]));
    }
    // "Dunsin Oyekan - Baruch Hashem Adonai": artist first.
    if let Some(c) = DASH.captures(&heading) {
        let artist = clean_artist(&c[1]);
        let title = strip_parenthetical(&c[2]);
        if artist.encode_utf16().count() >= 2 && title.encode_utf16().count() >= 2 {
            return (title, artist);
        }
    }
    if let Some(c) = TRAILING_ARTISTS.captures(&heading)
        && trim(&c[1]).encode_utf16().count() >= 3
    {
        return (strip_parenthetical(&c[1]), clean_artist(&c[2]));
    }
    let title = strip_parenthetical(&heading);
    (
        if title.is_empty() {
            "Untitled".into()
        } else {
            title
        },
        String::new(),
    )
}

fn clean_artist(value: &str) -> String {
    let joined = AND.replace_all(value, ", ");
    trim(&SITE_TAIL.replace(&joined, "")).to_owned()
}

fn strip_parenthetical(value: &str) -> String {
    let spaced = PAREN.replace_all(value, " ");
    trim(&MULTI_SPACE.replace_all(&spaced, " ")).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_split_title_and_artist() {
        assert_eq!(
            parse_page_heading(
                "Odudu (Spirit) Lyrics by Theophilus Sunday | African Gospel Lyrics"
            ),
            ("Odudu".into(), "Theophilus Sunday".into())
        );
        assert_eq!(
            parse_page_heading("Dunsin Oyekan - Baruch Hashem Adonai"),
            ("Baruch Hashem Adonai".into(), "Dunsin Oyekan".into())
        );
        assert_eq!(
            parse_page_heading("Amioluwa (The Mark of God) Sunmisola Agbebi and Yinka Okeleye"),
            ("Amioluwa".into(), "Sunmisola Agbebi, Yinka Okeleye".into())
        );
        assert_eq!(parse_page_heading(""), ("Untitled".into(), String::new()));
    }

    #[test]
    fn only_allowlisted_hosts() {
        assert!(is_allowed_lyrics_url("https://www.ceenaija.com/x"));
        assert!(is_allowed_lyrics_url(
            "https://yorubahymns.blogspot.com/2015/04/x.html"
        ));
        assert!(!is_allowed_lyrics_url("https://example.com/"));
        assert!(!is_allowed_lyrics_url("not a url"));
    }

    #[test]
    fn review_scrape_drops_mp3_blog_cta_and_skips_fake_verse_chunking() {
        let html = r#"<html><body><article>
            <h1 class="entry-title">Ese Lyrics by Nathaniel Bassey | CeeNaija</h1>
            <p>Download MP3 below and enjoy this powerful worship song.</p>
            <p>Lyrics for Ese By Nathaniel Bassey<br>Ese ese ese o<br>Thank you Lord<br>Download Ese By Nathaniel Bassey</p>
            <p>Join our WhatsApp channel for updates</p>
            <div class="related-posts"><p>Another Song Lyrics</p></div>
        </article></body></html>"#;
        let page = parse_lyrics_for_review("https://www.ceenaija.com/ese/", html).unwrap();
        assert_eq!(page.title, "Ese");
        assert_eq!(page.artist, "Nathaniel Bassey");
        assert_eq!(page.lyrics, "Ese ese ese o\nThank you Lord");
    }
}
