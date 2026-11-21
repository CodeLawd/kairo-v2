//! Scripture search parsing, from `src/lib/scripture-query.ts`: book names and
//! abbreviations, reference normalisation, completions, phrase detection and
//! adjacent-verse navigation.

use super::js;
use super::versification::get_verse_numbering_override;
use super::{ScriptureResult, ScriptureTranslation};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::sync::LazyLock;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookCompletion {
    pub value: String,
    pub book: String,
    pub typed_book: String,
}

/// `(canonical name, aliases)` in canonical order. Aliases are lower case.
pub const BOOKS: &[(&str, &[&str])] = &[
    ("Genesis", &["gen", "ge", "gn"]),
    ("Exodus", &["exod", "exo", "ex"]),
    ("Leviticus", &["lev", "lv"]),
    ("Numbers", &["num", "nu"]),
    ("Deuteronomy", &["deut", "deu", "dt"]),
    ("Joshua", &["josh", "jos"]),
    ("Judges", &["judg", "jdg"]),
    ("Ruth", &["ruth", "ru"]),
    ("1 Samuel", &["1sam", "1 sam", "1sa"]),
    ("2 Samuel", &["2sam", "2 sam", "2sa"]),
    ("1 Kings", &["1kgs", "1 kgs", "1ki"]),
    ("2 Kings", &["2kgs", "2 kgs", "2ki"]),
    ("1 Chronicles", &["1chr", "1 chr", "1ch"]),
    ("2 Chronicles", &["2chr", "2 chr", "2ch"]),
    ("Ezra", &["ezra", "ezr", "ez"]),
    ("Nehemiah", &["neh"]),
    ("Esther", &["esth", "est"]),
    ("Job", &["job"]),
    ("Psalms", &["psalms", "psalm", "psa", "pss", "ps"]),
    ("Proverbs", &["prov", "pro"]),
    ("Ecclesiastes", &["eccl", "ecc"]),
    ("Song of Solomon", &["song", "song of songs", "sos"]),
    ("Isaiah", &["isa"]),
    ("Jeremiah", &["jer"]),
    ("Lamentations", &["lam"]),
    ("Ezekiel", &["ezek", "eze"]),
    ("Daniel", &["dan"]),
    ("Hosea", &["hos"]),
    ("Joel", &["joel"]),
    ("Amos", &["amos"]),
    ("Obadiah", &["obad", "oba"]),
    ("Jonah", &["jonah", "jon"]),
    ("Micah", &["mic"]),
    ("Nahum", &["nah"]),
    ("Habakkuk", &["hab"]),
    ("Zephaniah", &["zeph"]),
    ("Haggai", &["hag"]),
    ("Zechariah", &["zech"]),
    ("Malachi", &["mal"]),
    ("Matthew", &["matt", "mat"]),
    ("Mark", &["mark", "mrk"]),
    ("Luke", &["luke", "luk"]),
    ("John", &["john", "jhn", "jn"]),
    ("Acts", &["acts", "act"]),
    ("Romans", &["rom", "ro"]),
    ("1 Corinthians", &["1cor", "1 cor", "1co"]),
    ("2 Corinthians", &["2cor", "2 cor", "2co"]),
    ("Galatians", &["gal"]),
    ("Ephesians", &["eph"]),
    ("Philippians", &["phil", "php"]),
    ("Colossians", &["col"]),
    ("1 Thessalonians", &["1thess", "1 thess", "1th"]),
    ("2 Thessalonians", &["2thess", "2 thess", "2th"]),
    ("1 Timothy", &["1tim", "1 tim", "1ti"]),
    ("2 Timothy", &["2tim", "2 tim", "2ti"]),
    ("Titus", &["titus", "tit"]),
    ("Philemon", &["phlm", "phm"]),
    ("Hebrews", &["heb"]),
    ("James", &["jas", "jam"]),
    ("1 Peter", &["1pet", "1 pet", "1pe"]),
    ("2 Peter", &["2pet", "2 pet", "2pe"]),
    ("1 John", &["1john", "1 john", "1jn"]),
    ("2 John", &["2john", "2 john", "2jn"]),
    ("3 John", &["3john", "3 john", "3jn"]),
    ("Jude", &["jude"]),
    ("Revelation", &["rev"]),
];

/// Every book's full name, in canonical order.
pub fn bible_book_names() -> impl Iterator<Item = &'static str> {
    BOOKS.iter().map(|(name, _)| *name)
}

/// Lower-cased canonical names, computed once (`toLowerCase` of ASCII names).
static LOWER_NAMES: LazyLock<Vec<String>> = LazyLock::new(|| {
    BOOKS
        .iter()
        .map(|(name, _)| name.to_ascii_lowercase())
        .collect()
});

/// Name and aliases, lower case.
fn variants(index: usize) -> impl Iterator<Item = &'static str> {
    std::iter::once(LOWER_NAMES[index].as_str()).chain(BOOKS[index].1.iter().copied())
}

fn is_exact(index: usize, needle: &str) -> bool {
    variants(index).any(|variant| variant == needle)
}

fn is_prefix(index: usize, needle: &str) -> bool {
    variants(index).any(|variant| variant.starts_with(needle))
}

/// `typedBook` and the numeric `suffix` (with its leading space), from
/// `/^(.+?)(\s+\d[\d\s:\-\u2013\u2014]*)$/` on the trimmed, collapsed input.
fn split_book_and_numbers(input: &str) -> (String, String) {
    let normalized = js::collapse_spaces(js::trim(input));
    let chars: Vec<(usize, char)> = normalized.char_indices().collect();
    let allowed = |c: char| c.is_ascii_digit() || c == ' ' || matches!(c, ':' | '-' | '–' | '—');
    // Start of the longest tail made only of allowed characters.
    let mut tail = chars.len();
    while tail > 0 && allowed(chars[tail - 1].1) {
        tail -= 1;
    }
    // The lazy book group takes at least one character; spaces were collapsed,
    // so `\s+` is exactly one space.
    for i in tail.max(1)..chars.len().saturating_sub(1) {
        if chars[i].1 == ' ' && chars[i + 1].1.is_ascii_digit() {
            let at = chars[i].0;
            return (
                js::trim(&normalized[..at]).to_string(),
                normalized[at..].to_string(),
            );
        }
    }
    (normalized, String::new())
}

fn resolve_book_name(input: &str) -> Option<&'static str> {
    let needle = js::collapse_spaces(&js::trim(input).to_lowercase());
    if needle.is_empty() {
        return None;
    }
    // A recognised abbreviation beats a prefix: "ez" is Ezra, not Ezekiel.
    if let Some(index) = (0..BOOKS.len()).find(|i| is_exact(*i, &needle)) {
        return Some(BOOKS[index].0);
    }
    // Incomplete text expands only when every match points to one book.
    let mut candidates = (0..BOOKS.len()).filter(|i| is_prefix(*i, &needle));
    match (candidates.next(), candidates.next()) {
        (Some(index), None) => Some(BOOKS[index].0),
        _ => None,
    }
}

/// Book completions for the search box, sorted by book name.
pub fn get_book_completions(input: &str) -> Vec<BookCompletion> {
    let (typed_book, suffix) = split_book_and_numbers(input);
    let needle = typed_book.to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let matches: Vec<&'static str> =
        if let Some(index) = (0..BOOKS.len()).find(|i| is_exact(*i, &needle)) {
            vec![BOOKS[index].0]
        } else {
            (0..BOOKS.len())
                .filter(|i| is_prefix(*i, &needle))
                .map(|i| BOOKS[i].0)
                .collect()
        };
    if matches.len() == 1 && matches[0].to_ascii_lowercase() == needle {
        return Vec::new();
    }
    let mut completions: Vec<BookCompletion> = matches
        .into_iter()
        .map(|book| BookCompletion {
            value: if suffix.is_empty() {
                format!("{book} ")
            } else {
                format!("{book}{suffix}")
            },
            book: book.to_string(),
            typed_book: typed_book.clone(),
        })
        .collect();
    // `localeCompare` on these ASCII names (letters, digits, single spaces)
    // orders as case-insensitive ASCII; checked against the oracle.
    completions.sort_by_key(|c| c.book.to_ascii_lowercase());
    completions
}

struct BookPrefix {
    book: &'static str,
    remainder: String,
}

fn find_book_prefix(input: &str) -> Option<BookPrefix> {
    let (typed_book, suffix) = split_book_and_numbers(input);
    let book = resolve_book_name(&typed_book)?;
    Some(BookPrefix {
        book,
        remainder: js::trim(&suffix).to_string(),
    })
}

/// Splits `text` into maximal runs of ASCII digits and of other characters.
fn digit_runs(text: &str) -> Vec<(bool, &str)> {
    let mut runs = Vec::new();
    let mut start = 0;
    let mut digit = None;
    for (at, c) in text.char_indices() {
        let is_digit = c.is_ascii_digit();
        if digit.is_some_and(|d| d != is_digit) {
            runs.push((digit.unwrap_or(false), &text[start..at]));
            start = at;
        }
        digit = Some(is_digit);
    }
    if let Some(d) = digit {
        runs.push((d, &text[start..]));
    }
    runs
}

/// `\s*(?:X)\s*` where `X` is one of `seps`, on a remainder whose spaces are
/// single: the gap is the separator with an optional space either side.
fn is_gap(gap: &str, seps: &[char]) -> bool {
    let inner = gap.strip_prefix(' ').unwrap_or(gap);
    let inner = inner.strip_suffix(' ').unwrap_or(inner);
    let mut chars = inner.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => seps.contains(&c),
        // A bare single space is the `\s` alternative itself.
        (None, None) => gap == " ",
        _ => false,
    }
}

/// A typed reference in canonical form ("rom 8 28" is "Romans 8:28"), or
/// `None` when the input is not a reference.
pub fn normalize_scripture_query(input: &str) -> Option<String> {
    let prefix = find_book_prefix(input)?;
    let remainder = prefix.remainder.as_str();
    if remainder.is_empty() {
        return None;
    }
    let runs = digit_runs(remainder);
    // Chapter only ("Psalms 23") is the whole chapter.
    if let [(true, digits)] = runs.as_slice() {
        let chapter = js::number_from_digits(digits);
        if !(1.0..=150.0).contains(&chapter) {
            return None;
        }
        return Some(format!("{} {}", prefix.book, js::number_to_string(chapter)));
    }
    let n = |digits: &str| js::number_to_string(js::number_from_digits(digits));
    match runs.as_slice() {
        [(true, a), (false, g1), (true, b)] if is_gap(g1, &[':', ' ']) => {
            Some(format!("{} {}:{}", prefix.book, n(a), n(b)))
        }
        [(true, a), (false, g1), (true, b), (false, g2), (true, c)]
            if is_gap(g1, &[':', ' ']) && is_gap(g2, &['-', '–', '—', ' ']) =>
        {
            Some(format!("{} {}:{}–{}", prefix.book, n(a), n(b), n(c)))
        }
        _ => None,
    }
}

/// The single completion, if exactly one book matches.
pub fn get_book_completion(input: &str) -> Option<BookCompletion> {
    let mut completions = get_book_completions(input);
    if completions.len() == 1 {
        completions.pop()
    } else {
        None
    }
}

/// Remembered verse words rather than a reference.
pub fn is_likely_phrase_query(input: &str) -> bool {
    let normalized = js::collapse_spaces(js::trim(input));
    if normalize_scripture_query(&normalized).is_some() || find_book_prefix(&normalized).is_some() {
        return false;
    }
    let mut count = 0;
    let mut letters = 0;
    let mut in_word = false;
    for c in normalized.chars() {
        let word_char = c.is_ascii_alphabetic() || c == '\'';
        if word_char {
            letters += 1;
            if !in_word {
                count += 1;
            }
        }
        in_word = word_char;
    }
    count >= 3 && letters >= 8
}

/// Whether the search box should fetch live matches while typing.
pub fn should_live_suggest_scripture_query(input: &str) -> bool {
    let normalized = js::collapse_spaces(js::trim(input));
    if js::utf16_len(&normalized) < 3 {
        return false;
    }
    if is_likely_phrase_query(&normalized) || normalize_scripture_query(&normalized).is_some() {
        return true;
    }
    // A book plus numbers ("John 3", "1 cor 13"); a bare book name is not enough.
    find_book_prefix(&normalized).is_some_and(|book| !book.remainder.is_empty())
}

/// What Enter submits: a pending book completion is applied first, so "psa 23"
/// searches "Psalms 23" rather than keywords.
pub fn resolve_submitted_scripture_query(input: &str) -> String {
    let completed = match get_book_completion(input) {
        Some(completion) => completion.value,
        None => js::trim(input).to_string(),
    };
    normalize_scripture_query(&completed).unwrap_or_else(|| js::trim(&completed).to_string())
}

/// Where a playlist keeps a verse's own reference line, exactly as it is
/// shown (translation and all).
pub const VERSE_LABEL_KEY: &str = "label";

/// The reference line a playlist gave this verse, if any.
pub fn custom_reference_line(verse: &super::ScriptureVerse) -> Option<&str> {
    verse
        .extra
        .get(VERSE_LABEL_KEY)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|l| !l.is_empty())
}

/// The reference line over a verse: the playlist's own line when it gave
/// one, else `reference` with the translation as the options say.
pub fn verse_reference_line(
    verse: Option<&super::ScriptureVerse>,
    reference: &str,
    translation: &str,
    show_translation: bool,
) -> String {
    verse
        .and_then(custom_reference_line)
        .map(str::to_owned)
        .unwrap_or_else(|| {
            crate::overlay_content::format_overlay_reference(
                reference,
                translation,
                show_translation,
            )
        })
}

pub fn verse_reference(book: &str, chapter: f64, verse: f64) -> String {
    format!(
        "{book} {}:{}",
        js::number_to_string(chapter),
        js::number_to_string(verse)
    )
}

/// One result per verse, each sendable on its own.
pub fn expand_scripture_result(result: &ScriptureResult) -> Vec<ScriptureResult> {
    result
        .verses
        .iter()
        .map(|verse| ScriptureResult {
            reference: verse_reference(&verse.book, verse.chapter, verse.verse),
            translation: result.translation.clone(),
            verses: vec![verse.clone()],
        })
        .collect()
}

/// Joins loaded results into one passage, or `None` when they hold no verses.
pub fn combine_scripture_results(results: &[ScriptureResult]) -> Option<ScriptureResult> {
    let verses: Vec<_> = results.iter().flat_map(|r| r.verses.clone()).collect();
    let (first, last) = (verses.first()?, verses.last()?);
    let head = results.first()?;
    let num = js::number_to_string;
    let reference = if first.book != last.book {
        results
            .iter()
            .map(|r| r.reference.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    } else if first.chapter != last.chapter {
        format!(
            "{} {}:{}–{}:{}",
            first.book,
            num(first.chapter),
            num(first.verse),
            num(last.chapter),
            num(last.verse)
        )
    } else if first.verse != last.verse {
        format!(
            "{} {}:{}–{}",
            first.book,
            num(first.chapter),
            num(first.verse),
            num(last.verse)
        )
    } else {
        verse_reference(&first.book, first.chapter, first.verse)
    };
    Some(ScriptureResult {
        reference,
        translation: head.translation.clone(),
        verses,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Previous,
    Next,
}

/// Queries for the verse before or after the loaded queue's edge. Skips verses
/// a translation omits. Before verse 1 it asks for the previous chapter,
/// capped at 176 verses (Psalm 119) so API.Bible is never asked for 1–999.
pub fn get_adjacent_verse_queries(loaded: &[ScriptureResult], direction: Direction) -> Vec<String> {
    let edge_result = match direction {
        Direction::Next => loaded.last(),
        Direction::Previous => loaded.first(),
    };
    let Some(edge_result) = edge_result else {
        return Vec::new();
    };
    let edge_verse = match direction {
        Direction::Next => edge_result.verses.last(),
        Direction::Previous => edge_result.verses.first(),
    };
    let Some(edge_verse) = edge_verse else {
        return Vec::new();
    };
    let (book, chapter, verse) = (&edge_verse.book, edge_verse.chapter, edge_verse.verse);
    // An unknown book gives id 0, as `findIndex` of nothing plus one.
    let book_id = resolve_book_name(book)
        .and_then(|name| BOOKS.iter().position(|(n, _)| *n == name))
        .map_or(0.0, |index| index as f64 + 1.0);
    let omitted = get_verse_numbering_override(&edge_result.translation, book_id, chapter)
        .map(|o| o.omitted)
        .unwrap_or(&[]);
    let step = if direction == Direction::Next {
        1.0
    } else {
        -1.0
    };
    let mut adjacent = verse + step;
    while omitted.iter().any(|v| f64::from(*v) == adjacent) {
        adjacent += step;
    }
    if direction == Direction::Next {
        return vec![
            verse_reference(book, chapter, adjacent),
            verse_reference(book, chapter + 1.0, 1.0),
        ];
    }
    if adjacent > 0.0 {
        return vec![verse_reference(book, chapter, adjacent)];
    }
    // False for NaN, as in the source, which then asks for "NaN:1–176".
    if chapter <= 1.0 {
        return Vec::new();
    }
    vec![format!(
        "{book} {}:1–176",
        js::number_to_string(chapter - 1.0)
    )]
}

/// Result of re-looking-up passages in another translation.
#[derive(Clone, Debug, PartialEq)]
pub struct ReloadedPassages {
    pub results: Vec<ScriptureResult>,
    /// References with no text in the new translation; those passages stay as they were.
    pub unavailable: Vec<String>,
}

/// Re-looks-up each loaded passage in `translation`. A passage with no text
/// there, or whose lookup fails, stays as it was and is listed in `unavailable`.
pub async fn reload_passages_in_translation<F, Fut, E>(
    passages: &[ScriptureResult],
    translation: &ScriptureTranslation,
    mut search: F,
) -> ReloadedPassages
where
    F: FnMut(String, ScriptureTranslation) -> Fut,
    Fut: Future<Output = Result<Vec<ScriptureResult>, E>>,
{
    let mut out = ReloadedPassages {
        results: Vec::new(),
        unavailable: Vec::new(),
    };
    for passage in passages {
        let hit = match search(passage.reference.clone(), translation.clone()).await {
            Ok(hits) => hits.into_iter().find(|item| !item.verses.is_empty()),
            Err(_) => None,
        };
        match hit {
            Some(hit) => out.results.push(hit),
            None => {
                out.unavailable.push(passage.reference.clone());
                out.results.push(passage.clone());
            }
        }
    }
    out
}
