//! One ranked list from every catalogue. From `src/main/services/lyrics/online.ts`.

use super::ProviderResult;
use super::normalize::{canonical_text, trim};
use kairo_domain::lyrics::LyricsProvider;
use kairo_domain::lyrics::query::parse_song_query;
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

pub const MAX_RESULTS: usize = 10;
pub const MIN_QUERY_LENGTH: usize = 3;

const SCORE_LYRIC_PHRASE: f64 = 6.0;
const SCORE_LYRIC_WORDS: f64 = 2.5;
const SCORE_TITLE_EXACT: f64 = 5.0;
const SCORE_TITLE_PREFIX: f64 = 3.0;
const SCORE_TITLE_CONTAINS: f64 = 1.5;
const SCORE_TITLE_COVERAGE: f64 = 4.0;
const SCORE_ARTIST_COVERAGE: f64 = 1.5;
const SCORE_ARTIST_NAMED: f64 = 3.5;
const SCORE_RANK_WEIGHT: f64 = 2.0;
const SCORE_POPULARITY_MAX: f64 = 1.2;
const SCORE_COMPLETENESS_MAX: f64 = 2.5;
const WORD_MATCH_THRESHOLD: f64 = 0.75;
const LYRIC_IDENTITY_THRESHOLD: f64 = 0.75;

use kairo_domain::js::utf16_len as len16;

fn words(text: &str) -> Vec<String> {
    text.split(' ')
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

#[derive(Clone, Debug, PartialEq)]
pub enum LyricMatch {
    Phrase(String),
    Words(String),
    None,
}

/// Where the query sits in a lyric body, and the line that carried it.
pub fn find_lyric_match(lyrics: &str, canonical_query: &str, query_words: &[String]) -> LyricMatch {
    if canonical_query.is_empty() {
        return LyricMatch::None;
    }
    let lines: Vec<&str> = lyrics
        .split('\n')
        .map(trim)
        .filter(|l| !l.is_empty())
        .collect();
    for line in &lines {
        if canonical_text(line).contains(canonical_query) {
            return LyricMatch::Phrase((*line).into());
        }
    }
    // A couplet remembered as one breath straddles a line break.
    if canonical_text(lyrics).contains(canonical_query) {
        let anchor = query_words.first().map(String::as_str).unwrap_or("");
        let line = lines
            .iter()
            .find(|l| canonical_text(l).contains(anchor))
            .or(lines.first());
        return LyricMatch::Phrase(line.map(|l| (*l).to_owned()).unwrap_or_default());
    }
    let collapsed = canonical_query.replace(' ', "");
    if len16(&collapsed) >= 4 {
        for line in &lines {
            if canonical_text(line).replace(' ', "").contains(&collapsed) {
                return LyricMatch::Phrase((*line).into());
            }
        }
    }
    if query_words.is_empty() {
        return LyricMatch::None;
    }
    let canonical = canonical_text(lyrics);
    let present: Vec<&String> = query_words
        .iter()
        .filter(|w| canonical.contains(w.as_str()))
        .collect();
    if present.len() as f64 / query_words.len() as f64 >= WORD_MATCH_THRESHOLD {
        let need = present.len().min(2);
        if let Some(line) = lines.iter().find(|l| {
            let c = canonical_text(l);
            present.iter().filter(|w| c.contains(w.as_str())).count() >= need
        }) {
            return LyricMatch::Words((*line).into());
        }
    }
    LyricMatch::None
}

static QUALIFIERS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(live|acoustic|remix|remaster(ed)?|radio edit|extended|version|reprise|instrumental|demo|session|mix)\b").unwrap()
});
static BRACKETS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\([^)]*\)|\[[^\]]*\]").unwrap());
static FEAT_TAIL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(feat|ft)\.?\s.*$").unwrap());
static DASH_TAIL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[-–—]\s*$").unwrap());
static ARTIST_SPLIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)[;,/]|\bfeat\.?\b|\bft\.?\b|\(").unwrap());

/// The song under a tagging variant: "Ese - Live", "ESE (feat. …)" → "ese".
pub fn core_title(title: &str) -> String {
    let stripped = BRACKETS.replace_all(title, " ");
    let stripped = FEAT_TAIL.replace(&stripped, " ");
    let stripped = QUALIFIERS.replace_all(&stripped, " ");
    let stripped = DASH_TAIL.replace(&stripped, " ");
    let core = canonical_text(&stripped);
    if core.is_empty() {
        canonical_text(title)
    } else {
        core
    }
}

fn primary_artist_key(artist: &str) -> String {
    canonical_text(ARTIST_SPLIT.split(artist).next().unwrap_or(""))
}

fn word_coverage(field: &str, query_words: &[String]) -> f64 {
    if field.is_empty() || query_words.is_empty() {
        return 0.0;
    }
    query_words
        .iter()
        .filter(|w| field.contains(w.as_str()))
        .count() as f64
        / query_words.len() as f64
}

fn matches_named_artist(artist: &str, named: &[String]) -> bool {
    let canonical = canonical_text(artist);
    len16(&canonical) >= 3
        && named
            .iter()
            .any(|n| canonical.contains(n.as_str()) || n.contains(canonical.as_str()))
}

/// Whether the catalogues failed to place the query with any confidence.
pub fn needs_web_resolution(query: &str, gathered: &[ProviderResult]) -> bool {
    if len16(trim(query)) < super::resolver::MIN_WEB_QUERY_CHARS {
        return false;
    }
    if gathered.is_empty() {
        return true;
    }
    let canonical_query = canonical_text(query);
    let query_words = words(&canonical_query);
    let collapsed = canonical_query.replace(' ', "");
    let phrase_hits: Vec<&ProviderResult> = gathered
        .iter()
        .filter(|r| {
            r.lyrics
                .as_deref()
                .filter(|l| !l.is_empty())
                .is_some_and(|l| {
                    matches!(
                        find_lyric_match(l, &canonical_query, &query_words),
                        LyricMatch::Phrase(_)
                    )
                })
        })
        .collect();
    if !phrase_hits.is_empty() {
        let longest = phrase_hits
            .iter()
            .map(|r| r.lyrics.as_deref().map_or(0, len16))
            .max()
            .unwrap_or(0);
        return longest < 700;
    }
    let strong_title = gathered.iter().any(|r| {
        let title = canonical_text(&r.title);
        let ct = title.replace(' ', "");
        if title == canonical_query
            || ct == collapsed
            || title.contains(&canonical_query)
            || ct.contains(&collapsed)
        {
            return true;
        }
        if query_words.is_empty() {
            return false;
        }
        let covered = query_words
            .iter()
            .filter(|w| title.contains(w.as_str()) || ct.contains(w.as_str()))
            .count();
        covered as f64 / query_words.len() as f64 >= 0.8
    });
    if !strong_title {
        return true;
    }
    let longest = gathered
        .iter()
        .map(|r| r.lyrics.as_deref().map_or(0, len16))
        .max()
        .unwrap_or(0);
    longest > 0 && longest < 700
}

#[derive(Clone)]
struct Scored {
    result: ProviderResult,
    score: f64,
}

struct Profile {
    named_artists: Vec<String>,
    title_words: Vec<String>,
    artist_words: Vec<String>,
}

fn score_result(
    result: &ProviderResult,
    canonical_query: &str,
    query_words: &[String],
    rank: usize,
    profile: &Profile,
) -> Scored {
    let mut score = SCORE_RANK_WEIGHT / (1.0 + rank as f64);
    let mut enriched = result.clone();
    let title = canonical_text(&result.title);
    let collapsed_query = canonical_query.replace(' ', "");
    let collapsed_title = title.replace(' ', "");
    if title == canonical_query || collapsed_title == collapsed_query {
        score += SCORE_TITLE_EXACT;
    } else if title.starts_with(canonical_query) || collapsed_title.starts_with(&collapsed_query) {
        score += SCORE_TITLE_PREFIX;
    } else if title.contains(canonical_query) || collapsed_title.contains(&collapsed_query) {
        score += SCORE_TITLE_CONTAINS;
    }
    score += SCORE_TITLE_COVERAGE * word_coverage(&core_title(&result.title), &profile.title_words);
    score += SCORE_ARTIST_COVERAGE
        * word_coverage(&primary_artist_key(&result.artist), &profile.artist_words);
    if let Some(lyrics) = result.lyrics.as_deref().filter(|l| !l.is_empty()) {
        match find_lyric_match(lyrics, canonical_query, query_words) {
            LyricMatch::Phrase(line) => {
                score += SCORE_LYRIC_PHRASE;
                enriched.snippet = Some(line);
            }
            LyricMatch::Words(line) => {
                score += SCORE_LYRIC_WORDS;
                enriched.snippet = Some(line);
            }
            LyricMatch::None => {}
        }
    }
    if !profile.named_artists.is_empty()
        && matches_named_artist(&result.artist, &profile.named_artists)
    {
        score += SCORE_ARTIST_NAMED;
    }
    if let Some(p) = result.popularity.filter(|p| *p != 0.0) {
        score += SCORE_POPULARITY_MAX.min((p + 1.0).log10() / 5.0);
    }
    if let Some(lyrics) = result.lyrics.as_deref()
        && len16(lyrics) > 120
    {
        score += SCORE_COMPLETENESS_MAX.min((len16(lyrics) as f64).log10() - 2.0);
    }
    Scored {
        result: enriched,
        score,
    }
}

fn provider_rank(p: LyricsProvider) -> u8 {
    match p {
        LyricsProvider::Genius => 3,
        LyricsProvider::Lrclib => 2,
        LyricsProvider::Web => 1,
    }
}

/// The better of two rows for one song, pooling their metadata. Genius wins
/// ties across providers: its pages carry real section headers.
fn merge(a: Scored, b: Scored) -> Scored {
    let a_wins = if a.result.provider == b.result.provider {
        a.score >= b.score
    } else {
        provider_rank(a.result.provider) > provider_rank(b.result.provider)
    };
    let score = a.score.max(b.score);
    let (mut winner, loser) = if a_wins { (a, b) } else { (b, a) };
    winner.result.snippet = winner.result.snippet.or(loser.result.snippet);
    winner.result.album = winner.result.album.or(loser.result.album);
    winner.result.release_year = winner.result.release_year.or(loser.result.release_year);
    winner.result.lyrics = winner.result.lyrics.or(loser.result.lyrics);
    winner.score = score;
    winner
}

fn line_set(lyrics: &str) -> HashSet<String> {
    lyrics
        .split('\n')
        .map(canonical_text)
        .filter(|c| !c.is_empty())
        .collect()
}

fn containment(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    let (large, small) = if a.len() >= b.len() { (a, b) } else { (b, a) };
    if small.is_empty() {
        return 0.0;
    }
    small.iter().filter(|l| large.contains(*l)).count() as f64 / small.len() as f64
}

/// `Array.prototype.sort` by descending score: stable.
fn sort_desc(list: &mut [Scored]) {
    list.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

fn dedupe(scored: Vec<Scored>) -> Vec<Scored> {
    let mut by_key: Vec<(String, Scored)> = Vec::new();
    for entry in scored {
        let key = format!(
            "{}|{}",
            core_title(&entry.result.title),
            primary_artist_key(&entry.result.artist)
        );
        match by_key.iter().position(|(k, _)| *k == key) {
            None => by_key.push((key, entry)),
            Some(at) => {
                let existing = by_key[at].1.clone();
                by_key[at].1 = merge(existing, entry);
            }
        }
    }
    let mut kept: Vec<(Scored, Option<HashSet<String>>)> = Vec::new();
    for (_, entry) in by_key {
        let fingerprint = entry
            .result
            .lyrics
            .as_deref()
            .filter(|l| !l.is_empty())
            .map(line_set);
        let twin = fingerprint.as_ref().and_then(|f| {
            kept.iter().position(|(_, cf)| {
                cf.as_ref()
                    .is_some_and(|cf| containment(cf, f) >= LYRIC_IDENTITY_THRESHOLD)
            })
        });
        match twin {
            None => kept.push((entry, fingerprint)),
            Some(at) => {
                let twin = kept[at].0.clone();
                kept[at].0 = merge(twin, entry);
            }
        }
    }
    let mut out: Vec<Scored> = kept.into_iter().map(|(s, _)| s).collect();
    sort_desc(&mut out);
    out
}

/// Once a clearly relevant hit exists, catalogue noise that matched a stray word goes.
fn drop_weak(scored: Vec<Scored>) -> Vec<Scored> {
    let Some(best) = scored.first().map(|s| s.score) else {
        return scored;
    };
    if best < SCORE_TITLE_CONTAINS + 1.0 {
        return scored;
    }
    let floor = (best * 0.4).max(SCORE_TITLE_CONTAINS + 0.5);
    scored.into_iter().filter(|s| s.score >= floor).collect()
}

/// Pure ranking over already-fetched provider results, best first, at most 10.
pub fn rank_results(query: &str, results: &[ProviderResult]) -> Vec<ProviderResult> {
    let canonical_query = canonical_text(query);
    let query_words = words(&canonical_query);
    let parsed = parse_song_query(query);
    let named_artists: Vec<String> = parsed
        .pairs
        .iter()
        .map(|p| canonical_text(&p.artist))
        .filter(|a| len16(a) >= 3)
        .collect();
    let (title_words, artist_words) = match parsed.pairs.first() {
        Some(pair) => (
            words(&canonical_text(&pair.title)),
            words(&canonical_text(&pair.artist)),
        ),
        None => (query_words.clone(), query_words.clone()),
    };
    let profile = Profile {
        named_artists,
        title_words,
        artist_words,
    };
    // Provider relevance is positional: rank per provider.
    let mut ranks: Vec<(LyricsProvider, usize)> = Vec::new();
    let mut scored: Vec<Scored> = results
        .iter()
        .map(|r| {
            let rank = match ranks.iter_mut().find(|(p, _)| *p == r.provider) {
                Some((_, n)) => {
                    *n += 1;
                    *n - 1
                }
                None => {
                    ranks.push((r.provider, 1));
                    0
                }
            };
            score_result(r, &canonical_query, &query_words, rank, &profile)
        })
        .collect();
    sort_desc(&mut scored);
    drop_weak(dedupe(scored))
        .into_iter()
        .take(MAX_RESULTS)
        .map(|s| s.result)
        .collect()
}

/// Bounded Levenshtein distance (`<= max_dist`) over Unicode scalar values.
fn edit_distance_within(a: &str, b: &str, max_dist: usize) -> bool {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > max_dist {
        return false;
    }
    if a == b {
        return true;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0usize; b.len() + 1];
    for (i, &ca) in a.iter().enumerate() {
        curr[0] = i + 1;
        let mut row_min = curr[0];
        for (j, &cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            let val = (prev[j + 1] + 1)
                .min(curr[j] + 1)
                .min(prev[j] + cost);
            curr[j + 1] = val;
            row_min = row_min.min(val);
        }
        if row_min > max_dist {
            return false;
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()] <= max_dist
}

fn max_typos_for(len: usize) -> usize {
    match len {
        0..=3 => 0,
        4..=5 => 1,
        6..=11 => 2,
        _ => 3,
    }
}

/// Typo-tolerant match between a single canonical query token and a candidate token.
pub(crate) fn fuzzy_word_matches(needle: &str, candidate: &str) -> bool {
    if needle == candidate {
        return true;
    }
    let n_len = needle.chars().count();
    if n_len >= 4 && candidate.contains(needle) {
        return true;
    }
    let max_dist = max_typos_for(n_len);
    max_dist > 0 && edit_distance_within(needle, candidate, max_dist)
}

fn token_in_haystack(needle: &str, haystack_words: &[String]) -> bool {
    haystack_words
        .iter()
        .any(|cand| fuzzy_word_matches(needle, cand))
}

/// Fraction of query words matched (exact, 1–3 char typo, or adjacent-word join
/// like `"nara ekele"` ↔ `"narekele"`) across title, artist and lyrics.
pub(crate) fn fuzzy_query_coverage(
    query: &str,
    title: &str,
    artist: &str,
    lyrics: Option<&str>,
) -> f64 {
    let q_words = words(&canonical_text(query));
    if q_words.is_empty() {
        return 0.0;
    }
    let mut haystack = words(&canonical_text(title));
    haystack.extend(words(&canonical_text(artist)));
    if let Some(body) = lyrics {
        haystack.extend(words(&canonical_text(body)));
    }
    if haystack.is_empty() {
        return 0.0;
    }
    // Also include adjacent haystack pairs joined ("ami" + "oluwa" → "amioluwa").
    let pair_count = haystack.len().saturating_sub(1);
    let mut joined_haystack = Vec::with_capacity(pair_count);
    for pair in haystack.windows(2) {
        if pair[0].len() + pair[1].len() <= 24 {
            joined_haystack.push(format!("{}{}", pair[0], pair[1]));
        }
    }
    let mut matched = vec![false; q_words.len()];
    for (i, w) in q_words.iter().enumerate() {
        if token_in_haystack(w, &haystack) || token_in_haystack(w, &joined_haystack) {
            matched[i] = true;
        }
    }
    // Check adjacent query pairs joined ("nara" + "ekele" → "naraekele" ↔ "narekele").
    for i in 0..q_words.len().saturating_sub(1) {
        if matched[i] && matched[i + 1] {
            continue;
        }
        let joined = format!("{}{}", q_words[i], q_words[i + 1]);
        if joined.chars().count() >= 5
            && (token_in_haystack(&joined, &haystack)
                || token_in_haystack(&joined, &joined_haystack))
        {
            matched[i] = true;
            matched[i + 1] = true;
        }
    }
    matched.iter().filter(|&&m| m).count() as f64 / q_words.len() as f64
}

/// Like [`find_lyric_match`], falling back to typo- and spacing-tolerant line
/// matching (`"nara ekele"` ↔ `"Narekele mo"`, `"amzing grace"` ↔ `"Amazing grace"`).
pub(crate) fn fuzzy_find_lyric_match(
    lyrics: &str,
    canonical_query: &str,
    query_words: &[String],
) -> LyricMatch {
    let exact = find_lyric_match(lyrics, canonical_query, query_words);
    if !matches!(exact, LyricMatch::None) || query_words.is_empty() {
        return exact;
    }
    let lines: Vec<&str> = lyrics
        .split('\n')
        .map(trim)
        .filter(|l| !l.is_empty())
        .collect();
    // Try collapsed phrase / adjacent joined pairs against each line first.
    let collapsed = canonical_query.replace(' ', "");
    if collapsed.chars().count() >= 5 {
        for line in &lines {
            let l_words = words(&canonical_text(line));
            let l_collapsed = l_words.concat();
            if fuzzy_word_matches(&collapsed, &l_collapsed)
                || l_words
                    .iter()
                    .any(|w| fuzzy_word_matches(&collapsed, w))
            {
                return LyricMatch::Phrase((*line).to_owned());
            }
        }
    }
    for pair in query_words.windows(2) {
        let joined = format!("{}{}", pair[0], pair[1]);
        if joined.chars().count() < 5 {
            continue;
        }
        for line in &lines {
            let l_words = words(&canonical_text(line));
            if l_words.iter().any(|w| fuzzy_word_matches(&joined, w)) {
                return LyricMatch::Words((*line).to_owned());
            }
        }
    }
    let body_words = words(&canonical_text(lyrics));
    let present: Vec<&String> = query_words
        .iter()
        .filter(|w| token_in_haystack(w, &body_words))
        .collect();
    if present.len() as f64 / query_words.len() as f64 >= WORD_MATCH_THRESHOLD {
        let need = present.len().min(2);
        if let Some(line) = lines.iter().find(|l| {
            let lw = words(&canonical_text(l));
            present.iter().filter(|w| token_in_haystack(w, &lw)).count() >= need
        }) {
            return LyricMatch::Words((*line).to_owned());
        }
    }
    LyricMatch::None
}

/// Fuzzy-aware counterpart to [`needs_web_resolution`] for `OnlineLyrics::search_all`:
/// skips web blog scraping when a catalogue already returned full lyrics whose
/// title/artist/lyrics strongly fuzzy-match the user's query.
pub(crate) fn needs_web_resolution_fuzzy(query: &str, gathered: &[ProviderResult]) -> bool {
    if !needs_web_resolution(query, gathered) {
        return false;
    }
    if gathered.is_empty() {
        return true;
    }
    let canonical_query = canonical_text(query);
    let query_words = words(&canonical_query);
    !gathered.iter().any(|r| {
        let Some(lyrics) = r.lyrics.as_deref().filter(|l| len16(l) >= 700) else {
            return false;
        };
        let cov = fuzzy_query_coverage(query, &r.title, &r.artist, Some(lyrics));
        if cov < 0.75 {
            return false;
        }
        let title_cov = fuzzy_query_coverage(query, &r.title, "", None);
        title_cov >= 0.75
            || !matches!(
                fuzzy_find_lyric_match(lyrics, &canonical_query, &query_words),
                LyricMatch::None
            )
    })
}

/// Fuzzy-aware counterpart to [`rank_results`] for `OnlineLyrics::search_all`:
/// adds typo- and spacing-tolerant lyric/title scoring while keeping
/// [`rank_results`] exact for oracle parity.
pub(crate) fn rank_results_fuzzy(query: &str, results: &[ProviderResult]) -> Vec<ProviderResult> {
    let canonical_query = canonical_text(query);
    let query_words = words(&canonical_query);
    let parsed = parse_song_query(query);
    let named_artists: Vec<String> = parsed
        .pairs
        .iter()
        .map(|p| canonical_text(&p.artist))
        .filter(|a| len16(a) >= 3)
        .collect();
    let (title_words, artist_words) = match parsed.pairs.first() {
        Some(pair) => (
            words(&canonical_text(&pair.title)),
            words(&canonical_text(&pair.artist)),
        ),
        None => (query_words.clone(), query_words.clone()),
    };
    let profile = Profile {
        named_artists,
        title_words,
        artist_words,
    };
    let mut ranks: Vec<(LyricsProvider, usize)> = Vec::new();
    let mut scored: Vec<Scored> = results
        .iter()
        .map(|r| {
            let rank = match ranks.iter_mut().find(|(p, _)| *p == r.provider) {
                Some((_, n)) => {
                    *n += 1;
                    *n - 1
                }
                None => {
                    ranks.push((r.provider, 1));
                    0
                }
            };
            let mut s = score_result(r, &canonical_query, &query_words, rank, &profile);
            // Boost fuzzy lyric / title matches that exact substring matching missed.
            if s.result.snippet.is_none()
                && let Some(lyrics) = r.lyrics.as_deref().filter(|l| !l.is_empty())
            {
                match fuzzy_find_lyric_match(lyrics, &canonical_query, &query_words) {
                    LyricMatch::Phrase(line) => {
                        s.score += SCORE_LYRIC_PHRASE * 0.9;
                        s.result.snippet = Some(line);
                    }
                    LyricMatch::Words(line) => {
                        s.score += SCORE_LYRIC_WORDS * 0.9;
                        s.result.snippet = Some(line);
                    }
                    LyricMatch::None => {}
                }
            }
            let exact_title_cov = word_coverage(&core_title(&r.title), &profile.title_words);
            let fuzzy_title_cov = fuzzy_query_coverage(query, &r.title, "", None);
            if fuzzy_title_cov > exact_title_cov {
                s.score += SCORE_TITLE_COVERAGE * 0.85 * (fuzzy_title_cov - exact_title_cov);
            }
            s
        })
        .collect();
    sort_desc(&mut scored);
    drop_weak(dedupe(scored))
        .into_iter()
        .take(MAX_RESULTS)
        .map(|s| s.result)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(
        provider: LyricsProvider,
        id: &str,
        title: &str,
        artist: &str,
        lyrics: Option<&str>,
    ) -> ProviderResult {
        ProviderResult {
            id: id.into(),
            provider,
            title: title.into(),
            artist: artist.into(),
            url: format!("https://x/{id}"),
            lyrics: lyrics.map(str::to_owned),
            ..ProviderResult::default()
        }
    }

    #[test]
    fn a_confirmed_lyric_phrase_wins_and_genius_wins_ties() {
        let lyrics = "Ese! Ese! Ese o\nThank you Lord";
        let ranked = rank_results(
            "ese ese ese o",
            &[
                result(
                    LyricsProvider::Genius,
                    "g1",
                    "Something Else",
                    "Other",
                    None,
                ),
                result(
                    LyricsProvider::Lrclib,
                    "l1",
                    "Ese (Thank You)",
                    "Nathaniel Bassey",
                    Some(lyrics),
                ),
                result(
                    LyricsProvider::Genius,
                    "g2",
                    "Ese",
                    "Nathaniel Bassey",
                    None,
                ),
            ],
        );
        assert_eq!(ranked[0].id, "g2");
        assert_eq!(ranked[0].snippet.as_deref(), Some("Ese! Ese! Ese o"));
        assert!(ranked.iter().all(|r| r.id != "g1"), "noise dropped");
    }

    #[test]
    fn core_titles_ignore_tagging_variants() {
        assert_eq!(core_title("Ese (Thank You)"), "ese");
        assert_eq!(core_title("Ese - Live"), "ese");
        assert_eq!(core_title("ESE feat. AIDEE IME"), "ese");
    }

    #[test]
    fn web_resolution_runs_on_misses_and_stub_lyrics() {
        assert!(needs_web_resolution("ami oluwa", &[]));
        assert!(!needs_web_resolution("ab", &[]));
        let full = "line\n".repeat(200);
        assert!(!needs_web_resolution(
            "ami oluwa",
            &[result(
                LyricsProvider::Lrclib,
                "l",
                "Amioluwa",
                "S",
                Some(&full)
            )]
        ));
        assert!(needs_web_resolution(
            "ami oluwa",
            &[result(
                LyricsProvider::Lrclib,
                "l",
                "Amioluwa",
                "S",
                Some("short")
            )]
        ));
    }

    #[test]
    fn fuzzy_matching_handles_phonetic_spacing_and_typos() {
        let nara_lyrics = format!(
            "You've done so much for me\nNarekele mo\n{}",
            "What shall I render to Jehovah\n".repeat(30)
        );
        // "Nara Ekele" ↔ "Narekele" + artist match
        assert!(
            fuzzy_query_coverage(
                "Nara Ekele Tim Godfrey",
                "Tim Godfrey ft Travis Greene - Nara",
                "Tim Godfrey",
                Some(&nara_lyrics)
            ) >= 0.99
        );
        // Unrelated track by the same artist does NOT satisfy the query.
        assert!(
            fuzzy_query_coverage(
                "Nagode Solomon Lange",
                "You Are So Good",
                "Solomon Lange",
                Some("Jesus you're so good")
            ) < 0.70
        );
        // Fuzzy lyric match finds the line even with a vowel/spacing difference.
        assert!(matches!(
            fuzzy_find_lyric_match(&nara_lyrics, "nara ekele", &["nara".into(), "ekele".into()]),
            LyricMatch::Phrase(line) | LyricMatch::Words(line) if line == "Narekele mo"
        ));
        // Full-lyric fuzzy hit satisfies needs_web_resolution_fuzzy.
        assert!(!needs_web_resolution_fuzzy(
            "Nara Ekele Tim Godfrey",
            &[result(
                LyricsProvider::Lrclib,
                "nara",
                "Tim Godfrey ft Travis Greene - Nara",
                "Tim Godfrey",
                Some(&nara_lyrics)
            )]
        ));
    }
}
