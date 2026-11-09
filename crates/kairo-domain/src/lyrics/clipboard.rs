//! Song text copied off a lyrics site. Port of `src/lib/lyrics-clipboard.ts`.
//!
//! Strips the page chrome a selection picks up (contributor counts, "You
//! might also like", the trailing "12345Embed"), lifts title and artist out
//! of the heading, and returns a clean body for a text import.

use super::js_text::{DOT, NOT_WS, WS, has_text, len16, normalize_newlines, re, split_ws, trim};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardSongDraft {
    pub title: String,
    pub artist: String,
    pub copyright: String,
    pub text: String,
    /// Lyric lines kept, so a bad paste is obvious.
    pub line_count: usize,
}

/// Lines that are page chrome wherever they appear.
static NOISE_LINE: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        format!(r"^[0-9]+{WS}+(?i-u:contributors?)(?-u:\b)"),
        format!(r"^(?:[0-9]+|(?i-u:one|two|few|several))?{WS}*(?i-u:translations?)$"),
        r"^(?i-u:you might also like)$".to_string(),
        format!(r"^(?i-u:see ){DOT}+(?i-u: live)$"),
        r"^(?i-u:get tickets as low as )\$[0-9]+".to_string(),
        format!(r"^(?i-u:read more){WS}*$"),
        r"^(?i-u:share|embed|print|copy|edit lyrics|add a comment)$".to_string(),
        r"^(?i-u:how to format lyrics)".to_string(),
        r"^(?i-u:lyrics licensed (?:and provided )?by)(?-u:\b)".to_string(),
        format!(r"^(?i-u:writer|writer/composer|composer|producer)(?i-u:s)?{WS}*:"),
        format!(r"^[0-9]+{WS}*(?i-u:views?|likes?)$"),
        r"^(?i-u:advertisement)$".to_string(),
        r"^(?i-u:submit corrections)$".to_string(),
        r"^(?i-u:we're doing our best to make sure)".to_string(),
        r"^(?i-u:sign up (?:and|to))(?-u:\b)".to_string(),
        r"^(?i-u:more on genius)$".to_string(),
        r"^\[?(?i-u:instrumental)\]?$".to_string(),
    ]
    .iter()
    .map(|p| re(p))
    .collect()
});

static TITLE_HEADING: LazyLock<Regex> =
    LazyLock::new(|| re(&format!(r"^({DOT}+?){WS}+(?i-u:lyrics)$")));
static ARTIST_TITLE: LazyLock<Regex> =
    LazyLock::new(|| re(&format!(r"^({DOT}+?){WS}+[–—\-]{WS}+({DOT}+?)$")));
static COPYRIGHT_LINE: LazyLock<Regex> =
    LazyLock::new(|| re(r"^(?:©|(?i-u:\(c\))|(?i-u:copyright)(?-u:\b)|(?i-u:ccli)(?-u:\b))"));
static COPYRIGHT_PREFIX: LazyLock<Regex> =
    LazyLock::new(|| re(&format!(r"^(?i-u:copyright)(?-u:\b)(?:[:]|{WS})*")));
static BYLINE: LazyLock<Regex> = LazyLock::new(|| {
    re(&format!(
        r"^(?i-u:by|artist|performed by){WS}*[:\-]?{WS}*({DOT}+)$"
    ))
});
static EMBED_COUNTER: LazyLock<Regex> = LazyLock::new(|| re(&format!(r"[0-9]*Embed{WS}*$")));
static MANY_NEWLINES: LazyLock<Regex> = LazyLock::new(|| re(r"\n{3,}"));
static MARKER: LazyLock<Regex> = LazyLock::new(|| re(&format!(r"^\[{DOT}+\]$")));
static URL_ONLY: LazyLock<Regex> = LazyLock::new(|| re(&format!(r"^(?i-u:https?://){NOT_WS}+$")));

fn is_noise(line: &str) -> bool {
    NOISE_LINE.iter().any(|r| r.is_match(line))
}

fn is_zero_width(c: char) -> bool {
    matches!(
        c,
        '\u{200B}'..='\u{200F}' | '\u{2028}' | '\u{2029}' | '\u{FEFF}'
    )
}

/// Whether text looks like a song rather than an arbitrary clipboard payload.
pub fn looks_like_lyrics(raw: &str) -> bool {
    let text = normalize_newlines(raw);
    let lines: Vec<&str> = text
        .split('\n')
        .map(trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.len() < 4 {
        return false;
    }
    !(URL_ONLY.is_match(lines[0]) && lines.len() < 6)
}

/// Cleans clipboard text into an importable draft. `None` when there is no
/// lyric body.
pub fn parse_clipboard_song(raw: &str) -> Option<ClipboardSongDraft> {
    let normalized: String = normalize_newlines(raw)
        .chars()
        .filter(|c| !is_zero_width(*c))
        .collect();
    if !has_text(&normalized) {
        return None;
    }
    let mut title = String::new();
    let mut artist = String::new();
    let mut copyright = String::new();
    let mut kept: Vec<String> = Vec::new();
    // Metadata is only trusted above the first lyric line.
    let mut in_header = true;

    for raw_line in normalized.split('\n') {
        let line = EMBED_COUNTER.replace(raw_line, "");
        let line = trim(&line);
        if line.is_empty() {
            if !kept.is_empty() {
                kept.push(String::new());
            }
            continue;
        }
        if is_noise(line) {
            continue;
        }
        if COPYRIGHT_LINE.is_match(line) {
            if copyright.is_empty() {
                copyright = trim(&COPYRIGHT_PREFIX.replace(line, "")).to_string();
            }
            continue;
        }
        if in_header {
            if let Some(heading) = TITLE_HEADING.captures(line) {
                let head = trim(&heading[1]);
                if let Some(split) = ARTIST_TITLE.captures(head) {
                    if artist.is_empty() {
                        artist = trim(&split[1]).to_string();
                    }
                    if title.is_empty() {
                        title = trim(&split[2]).to_string();
                    }
                } else if title.is_empty() {
                    title = head.to_string();
                }
                continue;
            }
            if let Some(byline) = BYLINE.captures(line)
                && !title.is_empty()
                && artist.is_empty()
            {
                artist = trim(&byline[1]).to_string();
                continue;
            }
        }
        in_header = false;
        kept.push(line.to_string());
    }

    let joined = kept.join("\n");
    let text = trim(&MANY_NEWLINES.replace_all(&joined, "\n\n")).to_string();
    let line_count = text
        .split('\n')
        .filter(|l| has_text(l) && !MARKER.is_match(trim(l)))
        .count();
    if line_count == 0 {
        return None;
    }
    Some(ClipboardSongDraft {
        title,
        artist,
        copyright,
        text,
        line_count,
    })
}

static BRACKETED_LINE: LazyLock<Regex> = LazyLock::new(|| re(&format!(r"^\[{DOT}*\]$")));

/// A line that could be a song name or a credit, not a lyric.
fn looks_like_heading_line(line: &str) -> bool {
    let trimmed = trim(line);
    !trimmed.is_empty()
        && !BRACKETED_LINE.is_match(trimmed)
        && len16(trimmed) <= 60
        && split_ws(trimmed).len() <= 10
}

/// A title (and artist) lifted off the top of a document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LiftedHeading {
    pub title: String,
    pub artist: String,
    pub text: String,
}

/// Reads the title, and the artist when present, off the top of a song sheet
/// typed by hand: a heading line, an optional artist line, then a blank line.
/// Returns empty fields and the text untouched otherwise.
pub fn lift_heading_title(text: &str) -> LiftedHeading {
    let unchanged = || LiftedHeading {
        title: String::new(),
        artist: String::new(),
        text: text.to_string(),
    };
    let normalized = normalize_newlines(text);
    let lines: Vec<&str> = normalized.split('\n').collect();
    let Some(first) = lines.iter().position(|l| has_text(l)) else {
        return unchanged();
    };
    if !looks_like_heading_line(lines[first]) {
        return unchanged();
    }
    let second = lines.get(first + 1);
    let third = lines.get(first + 2);
    let rest_from = |from: usize| trim(&lines[from.min(lines.len())..].join("\n")).to_string();

    if let Some(second) = second
        && !has_text(second)
    {
        let rest = rest_from(first + 2);
        return if rest.is_empty() {
            unchanged()
        } else {
            LiftedHeading {
                title: trim(lines[first]).to_string(),
                artist: String::new(),
                text: rest,
            }
        };
    }
    if let (Some(second), Some(third)) = (second, third)
        && looks_like_heading_line(second)
        && !has_text(third)
    {
        let rest = rest_from(first + 3);
        if !rest.is_empty() {
            return LiftedHeading {
                title: trim(lines[first]).to_string(),
                artist: trim(second).to_string(),
                text: rest,
            };
        }
    }
    unchanged()
}

static SONG_META_KEY: LazyLock<Regex> = LazyLock::new(|| {
    re(&format!(
        r"^{WS}*(?i-u:title|song ?title|author|artist|writer|songwriter|copyright|ccli ?#?|ccli ?number|ccli ?song ?number){WS}*[=:]"
    ))
});

/// Whether a file opens (first 8 lines) with a metadata block such as
/// `Title=…` or `Artist: …`.
pub fn has_song_metadata(text: &str) -> bool {
    normalize_newlines(text)
        .split('\n')
        .take(8)
        .any(|l| SONG_META_KEY.is_match(l))
}
