//! Recognising a song the library already holds. Port of the functions in
//! `src/lib/lyrics-duplicate.ts` (its types are in [`super`]). Nothing here
//! decides anything: it reports what matched and why.

use super::js_text::{DOT, WS, collapse_ws, is_ws, re, trim};
use super::{DuplicateMatch, DuplicateReason, LyricsSong};
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;
use unicode_normalization::UnicodeNormalization;

/// Share of the shorter song's lines that must appear in the other.
const LYRIC_MATCH_THRESHOLD: f64 = 0.75;
/// Songs with fewer distinct lines are matched on title and artist only.
const MIN_LINES_FOR_LYRIC_MATCH: usize = 4;

static PLACEHOLDER_TITLE: LazyLock<Regex> = LazyLock::new(|| {
    re(r"^(?:untitled|unknown|song|lyrics|new song|document|untitled [0-9]+|untitled copy)$")
});

/// Lower case, unaccented and punctuation-free: the form two transcriptions
/// of one song share.
pub fn canonical(value: &str) -> String {
    let stripped: String = value
        .nfd()
        .filter(|c| !('\u{0300}'..='\u{036F}').contains(c))
        .collect();
    let mapped: String = stripped
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || is_ws(c) {
                c
            } else {
                ' '
            }
        })
        .collect();
    trim(&collapse_ws(&mapped)).to_string()
}

static BRACKETED: LazyLock<Regex> = LazyLock::new(|| re(r"\([^)]*\)|\[[^\]]*\]"));
static FEATURING: LazyLock<Regex> =
    LazyLock::new(|| re(&format!(r"(?-u:\b)(?i-u:feat|ft)\.?{WS}{DOT}*$")));
static VERSION_WORDS: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?-u:\b)(?i-u:live|acoustic|remix|reprise|official|version|cover)(?-u:\b)")
});
static ARTIST_SPLIT: LazyLock<Regex> =
    LazyLock::new(|| re(r"[;,/]|(?-u:\b)(?i-u:feat)\.?(?-u:\b)|(?-u:\b)(?i-u:ft)\.?(?-u:\b)|\("));

/// Title and lead artist, ignoring case, punctuation, a `(Live)` suffix and
/// featured credits.
pub fn song_key(title: &str, artist: &str) -> String {
    let bare = BRACKETED.replace_all(title, " ");
    let bare = FEATURING.replace(&bare, " ");
    let bare = VERSION_WORDS.replace_all(&bare, " ");
    let bare_title = canonical(&bare);
    let lead = ARTIST_SPLIT
        .find(artist)
        .map_or(artist, |m| &artist[..m.start()]);
    let lead_artist = canonical(lead);
    let title_part = if bare_title.is_empty() {
        canonical(title)
    } else {
        bare_title
    };
    format!("{title_part}|{lead_artist}")
}

/// The song's distinct lyric lines, as a comparable set.
pub fn lyric_fingerprint(song: &LyricsSong) -> HashSet<String> {
    song.sections
        .iter()
        .flat_map(|s| s.lines.iter())
        .map(|l| canonical(l))
        .filter(|l| !l.is_empty())
        .collect()
}

/// Share of the smaller set that also appears in the larger one.
pub fn containment(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    let (small, large) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    if small.is_empty() {
        return 0.0;
    }
    let shared = small.iter().filter(|l| large.contains(*l)).count();
    shared as f64 / small.len() as f64
}

fn describe(song: &LyricsSong, reason: DuplicateReason) -> DuplicateMatch {
    DuplicateMatch {
        song_id: song.id.clone(),
        title: song.title.clone(),
        artist: song.artist.clone(),
        reason,
    }
}

/// The song in `library` that `candidate` is a copy of: by CCLI number, then
/// title and artist (unless the title is a placeholder), then lyrics.
pub fn find_duplicate(candidate: &LyricsSong, library: &[LyricsSong]) -> Option<DuplicateMatch> {
    let ccli = candidate.ccli_number.as_deref().map(trim).unwrap_or("");
    if !ccli.is_empty()
        && let Some(found) = library
            .iter()
            .find(|s| s.ccli_number.as_deref().map(trim) == Some(ccli))
    {
        return Some(describe(found, DuplicateReason::Ccli));
    }

    let bare_title = canonical(&candidate.title);
    if !bare_title.is_empty() && !PLACEHOLDER_TITLE.is_match(&bare_title) {
        let key = song_key(&candidate.title, &candidate.artist);
        if let Some(found) = library
            .iter()
            .find(|s| song_key(&s.title, &s.artist) == key)
        {
            return Some(describe(found, DuplicateReason::TitleArtist));
        }
    }

    let fingerprint = lyric_fingerprint(candidate);
    if fingerprint.len() < MIN_LINES_FOR_LYRIC_MATCH {
        return None;
    }
    library
        .iter()
        .find(|song| {
            let other = lyric_fingerprint(song);
            other.len() >= MIN_LINES_FOR_LYRIC_MATCH
                && containment(&fingerprint, &other) >= LYRIC_MATCH_THRESHOLD
        })
        .map(|song| describe(song, DuplicateReason::Lyrics))
}

/// One line explaining a match, for the badge in the import list.
pub fn describe_duplicate(found: &DuplicateMatch) -> String {
    let who = if found.artist.is_empty() {
        String::new()
    } else {
        format!(" by {}", found.artist)
    };
    let title = &found.title;
    match found.reason {
        DuplicateReason::SameSong => format!("Already in your library as \"{title}\"{who}"),
        DuplicateReason::Ccli => format!("Same CCLI number as \"{title}\"{who}"),
        DuplicateReason::TitleArtist => format!("Same title and artist as \"{title}\"{who}"),
        DuplicateReason::Lyrics => format!("Same lyrics as \"{title}\"{who}"),
    }
}
