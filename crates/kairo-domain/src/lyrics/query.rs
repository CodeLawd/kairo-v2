//! Splits a typed song search into the parts a catalogue can match. Port of
//! `src/lib/lyrics-query.ts`.

use super::js_text::{WS, collapse_ws, len16, re, trim};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::LazyLock;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TitleArtist {
    pub title: String,
    pub artist: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SongQueryParts {
    /// The whole query, trimmed; always searched as typed as well.
    pub full: String,
    /// Title and artist candidates, most likely first.
    pub pairs: Vec<TitleArtist>,
    /// Bare title guesses, most likely first, free of artist words.
    pub titles: Vec<String>,
}

static BY_SPLIT: LazyLock<Regex> = LazyLock::new(|| re(&format!(r"{WS}+(?i-u:by){WS}+")));
static DASH_SPLIT: LazyLock<Regex> =
    LazyLock::new(|| re(&format!(r"{WS}+[\-–—]{WS}+|{WS}*\|{WS}*|{WS}*:{WS}+")));
static EDGE_PUNCT: LazyLock<Regex> =
    LazyLock::new(|| re(&format!(r#"^(?:{WS}|["'(\[])+|(?:{WS}|["')\]])+$"#)));

const NOISE_WORDS: [&str; 6] = ["lyrics", "lyric", "song", "official", "video", "audio"];

fn clean(value: &str) -> String {
    let collapsed = collapse_ws(value);
    let stripped = EDGE_PUNCT.replace_all(&collapsed, "");
    let trimmed = trim(&stripped);
    let words: Vec<&str> = trimmed
        .split(' ')
        .filter(|w| !NOISE_WORDS.contains(&w.to_lowercase().as_str()))
        .collect();
    let out = if words.is_empty() {
        trimmed.to_string()
    } else {
        words.join(" ")
    };
    trim(&out).to_string()
}

fn usable(value: &str) -> bool {
    len16(&clean(value)) >= 2
}

/// Decomposes a typed query into title and artist candidates. ` by ` gives
/// one pair; a dash, bar or colon gives both orders.
pub fn parse_song_query(raw: &str) -> SongQueryParts {
    let full = trim(&collapse_ws(raw)).to_string();
    let mut parts = SongQueryParts {
        full: full.clone(),
        pairs: Vec::new(),
        titles: Vec::new(),
    };
    if full.is_empty() {
        return parts;
    }
    let by: Vec<&str> = BY_SPLIT.split(&full).collect();
    if by.len() == 2 && usable(by[0]) && usable(by[1]) {
        let title = clean(by[0]);
        let artist = clean(by[1]);
        parts.titles.push(title.clone());
        parts.pairs.push(TitleArtist { title, artist });
        return parts;
    }
    let dash: Vec<&str> = DASH_SPLIT
        .split(&full)
        .filter(|p| !trim(p).is_empty())
        .collect();
    if dash.len() == 2 && usable(dash[0]) && usable(dash[1]) {
        let left = clean(dash[0]);
        let right = clean(dash[1]);
        parts.pairs.push(TitleArtist {
            title: right.clone(),
            artist: left.clone(),
        });
        parts.pairs.push(TitleArtist {
            title: left.clone(),
            artist: right.clone(),
        });
        parts.titles.push(right);
        parts.titles.push(left);
        return parts;
    }
    let cleaned = clean(&full);
    if !cleaned.is_empty() && cleaned != full {
        parts.titles.push(cleaned);
    }
    parts
}

/// Catalogue queries to try for one typed query, in order: as typed, each
/// title guess, then a short two- or three-word query collapsed into one word.
pub fn query_variants(query: &str) -> Vec<String> {
    let parsed = parse_song_query(query);
    if parsed.full.is_empty() {
        return Vec::new();
    }
    let mut variants = vec![parsed.full.clone()];
    variants.extend(parsed.titles.iter().cloned());
    let words: Vec<&str> = parsed.full.split(' ').filter(|w| !w.is_empty()).collect();
    if (2..=3).contains(&words.len()) && words.iter().all(|w| len16(w) <= 10) {
        let collapsed = words.concat();
        if len16(&collapsed) >= 4 {
            variants.push(collapsed);
        }
    }
    let mut seen = HashSet::new();
    variants
        .into_iter()
        .filter(|v| !v.is_empty() && seen.insert(v.to_lowercase()))
        .collect()
}
