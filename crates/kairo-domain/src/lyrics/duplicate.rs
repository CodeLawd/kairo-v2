//! Recognising a song the library already holds. Port of the functions in
//! `src/lib/lyrics-duplicate.ts` (its types are in [`super`]). Nothing here
//! decides anything: it reports what matched and why.

use super::js_text::{DOT, WS, collapse_ws, is_ws, re, trim};
use super::{DuplicateMatch, DuplicateReason, LyricsSong};
use regex::Regex;
use std::collections::{HashMap, HashSet};
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

struct DuplicateEntry {
    id: String,
    title: String,
    artist: String,
    fingerprint_len: usize,
}

impl DuplicateEntry {
    fn describe(&self, reason: DuplicateReason) -> DuplicateMatch {
        DuplicateMatch {
            song_id: self.id.clone(),
            title: self.title.clone(),
            artist: self.artist.clone(),
            reason,
        }
    }
}

/// Indexed equivalent of [`find_duplicate`] for bulk imports: `O(1)` for CCLI
/// and title/artist lookups, and inverted-index lookup for lyric containment.
#[derive(Default)]
pub struct DuplicateIndex {
    entries: Vec<DuplicateEntry>,
    by_ccli: HashMap<String, usize>,
    by_key: HashMap<String, usize>,
    by_line: HashMap<String, Vec<usize>>,
}

impl DuplicateIndex {
    pub fn new(library: &[LyricsSong]) -> Self {
        let mut index = Self::default();
        for song in library {
            index.insert(song);
        }
        index
    }

    pub fn insert(&mut self, song: &LyricsSong) {
        let idx = self.entries.len();
        let ccli = song.ccli_number.as_deref().map(trim).unwrap_or("");
        if !ccli.is_empty() {
            self.by_ccli.entry(ccli.to_owned()).or_insert(idx);
        }
        let key = song_key(&song.title, &song.artist);
        self.by_key.entry(key).or_insert(idx);

        let fingerprint = lyric_fingerprint(song);
        let fingerprint_len = fingerprint.len();
        if fingerprint_len >= MIN_LINES_FOR_LYRIC_MATCH {
            for line in fingerprint {
                self.by_line.entry(line).or_default().push(idx);
            }
        }
        self.entries.push(DuplicateEntry {
            id: song.id.clone(),
            title: song.title.clone(),
            artist: song.artist.clone(),
            fingerprint_len,
        });
    }

    pub fn find(&self, candidate: &LyricsSong) -> Option<DuplicateMatch> {
        let ccli = candidate.ccli_number.as_deref().map(trim).unwrap_or("");
        if !ccli.is_empty()
            && let Some(&idx) = self.by_ccli.get(ccli)
        {
            return Some(self.entries[idx].describe(DuplicateReason::Ccli));
        }

        let bare_title = canonical(&candidate.title);
        if !bare_title.is_empty() && !PLACEHOLDER_TITLE.is_match(&bare_title) {
            let key = song_key(&candidate.title, &candidate.artist);
            if let Some(&idx) = self.by_key.get(&key) {
                return Some(self.entries[idx].describe(DuplicateReason::TitleArtist));
            }
        }

        let fingerprint = lyric_fingerprint(candidate);
        if fingerprint.len() < MIN_LINES_FOR_LYRIC_MATCH {
            return None;
        }
        let mut shared: HashMap<usize, usize> = HashMap::new();
        for line in &fingerprint {
            if let Some(indices) = self.by_line.get(line) {
                for &idx in indices {
                    *shared.entry(idx).or_insert(0) += 1;
                }
            }
        }
        shared
            .into_iter()
            .filter(|&(idx, count)| {
                let small = fingerprint.len().min(self.entries[idx].fingerprint_len);
                small >= MIN_LINES_FOR_LYRIC_MATCH
                    && (count as f64 / small as f64) >= LYRIC_MATCH_THRESHOLD
            })
            .map(|(idx, _)| idx)
            .min()
            .map(|idx| self.entries[idx].describe(DuplicateReason::Lyrics))
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lyrics::{LyricsSectionType, LyricsSongSection, LyricsSource};

    fn song(id: &str, title: &str, artist: &str, ccli: Option<&str>, lines: &[&str]) -> LyricsSong {
        LyricsSong {
            id: id.into(),
            title: title.into(),
            artist: artist.into(),
            copyright: None,
            ccli_number: ccli.map(str::to_owned),
            is_favorite: None,
            source: Some(LyricsSource::Text),
            sections: vec![LyricsSongSection {
                kind: LyricsSectionType::Verse,
                label: "Verse 1".into(),
                lines: lines.iter().map(|s| (*s).to_owned()).collect(),
                line_colors: None,
                hotkey: None,
                extra: Default::default(),
            }],
            created_at: 0.0,
            updated_at: 0.0,
            extra: Default::default(),
        }
    }

    #[test]
    fn duplicate_index_matches_find_duplicate() {
        let lib = vec![
            song(
                "1",
                "Way Maker",
                "Sinach",
                Some("7115744"),
                &[
                    "You are here moving in our midst",
                    "I worship You I worship You",
                    "You are here working in this place",
                    "I worship You I worship You today",
                ],
            ),
            song(
                "2",
                "Amazing Grace",
                "John Newton",
                None,
                &[
                    "Amazing grace how sweet the sound",
                    "That saved a wretch like me",
                    "I once was lost but now am found",
                    "Was blind but now I see",
                ],
            ),
        ];
        let index = DuplicateIndex::new(&lib);
        let cases = [
            song("c1", "Different", "Other", Some("7115744"), &["x"]),
            song(
                "c2",
                "Amazing Grace (Live)",
                "John Newton feat. Choir",
                None,
                &["x"],
            ),
            song(
                "c3",
                "Untitled",
                "",
                None,
                &[
                    "Amazing grace how sweet the sound",
                    "That saved a wretch like me",
                    "I once was lost but now am found",
                    "Was blind but now I see",
                ],
            ),
            song(
                "c4",
                "Brand New Song",
                "Nobody",
                None,
                &["Line one", "Line two"],
            ),
        ];
        for cand in &cases {
            assert_eq!(index.find(cand), find_duplicate(cand, &lib));
        }
    }
}
