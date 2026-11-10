//! Local quote matching within a chapter already named by the speaker.

use std::{
    collections::{HashMap, HashSet},
    sync::LazyLock,
};

use kairo_domain::{
    js::{is_whitespace, number_to_string, string_to_number},
    scripture::{live_progress::tokenize, small::spoken_verse_text},
};
use regex::Regex;

use crate::{service::QuoteCandidate, verse_bounds::is_valid_scripture_reference};

/// One verse's wording, tokenized once for repeated matching.
pub struct ChapterVerseTokens {
    verse: u32,
    tokens: Vec<String>,
}

pub fn tokenize_chapter<'a>(
    candidates: impl IntoIterator<Item = (u32, &'a str)>,
) -> Vec<ChapterVerseTokens> {
    let mut seen = HashSet::new();
    candidates
        .into_iter()
        .filter_map(|(verse, text)| {
            let tokens = tokenize(&spoken_verse_text(text));
            seen.insert((verse, tokens.join(" ")))
                .then_some(ChapterVerseTokens { verse, tokens })
        })
        .collect()
}

#[derive(Clone, Copy, Default)]
struct Run {
    exact: u16,
    before_gap: u16,
    after_gap: u16,
}

fn shared_runs(heard: &[String], verse: &[String]) -> (u16, u32) {
    let mut previous = vec![Run::default(); verse.len() + 1];
    let mut current = previous.clone();
    let mut exact_best = 0;
    let mut gap_best = 0;
    for word in heard {
        for (index, verse_word) in verse.iter().enumerate() {
            let prior = previous[index];
            let next = if word == verse_word {
                Run {
                    exact: prior.exact.wrapping_add(1),
                    before_gap: prior.before_gap,
                    after_gap: if prior.before_gap == 0 {
                        0
                    } else {
                        prior.after_gap.wrapping_add(1)
                    },
                }
            } else {
                Run {
                    before_gap: prior.exact,
                    ..Run::default()
                }
            };
            exact_best = exact_best.max(next.exact);
            if next.before_gap >= 2 && next.after_gap >= 2 {
                gap_best = gap_best.max(u32::from(next.before_gap) + u32::from(next.after_gap));
            }
            current[index + 1] = next;
        }
        std::mem::swap(&mut previous, &mut current);
    }
    (exact_best, gap_best)
}

fn unique_best<T: Copy + Ord>(scores: HashMap<u32, T>, minimum: T) -> Option<(u32, T)> {
    let mut best = None;
    let mut tied = false;
    for (verse, score) in scores {
        if score < minimum {
            continue;
        }
        match best {
            Some((_, top)) if top > score => {}
            Some((_, top)) if top == score => tied = true,
            _ => {
                best = Some((verse, score));
                tied = false;
            }
        }
    }
    if tied { None } else { best }
}

/// Returns the uniquely matched verse and its confidence. Alternate translations
/// of one verse share a score, while equally good matches in two verses are ambiguous.
pub fn match_chapter_quote(text: &str, verses: &[ChapterVerseTokens]) -> Option<(u32, f64)> {
    let heard = tokenize(text);
    if heard.len() < 4 {
        return None;
    }

    let mut exact = HashMap::<u32, u16>::new();
    let mut gap = HashMap::<u32, u32>::new();
    for verse in verses {
        let (run, broken_run) = shared_runs(&heard, &verse.tokens);
        let score = exact.entry(verse.verse).or_default();
        *score = (*score).max(run);
        let score = gap.entry(verse.verse).or_default();
        *score = (*score).max(broken_run);
    }

    if let Some((verse, score)) = unique_best(exact, 4) {
        return Some((verse, if score >= 6 { 0.85 } else { 0.65 }));
    }
    unique_best(gap, 6).map(|(verse, _)| (verse, 0.65))
}

static DAMAGED: LazyLock<Regex> = LazyLock::new(|| {
    let pattern = r"(?i-u:\b([1-3]?\s*[a-z]+)\s+([a-z0-9]*\d[a-z0-9]*))(?:[-–]((?-u:\w)+))?(?i-u:\s+says[,.:]?\s+)([^\r\n\u{2028}\u{2029}]+)".replace(
        r"\s",
        r"(?u:[\x09-\x0D\x20\u{00A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}])",
    );
    Regex::new(&pattern).expect("valid damaged citation pattern")
});
static UNITS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new("one|two|three|four|five|six|seven|eight|nine").expect("valid unit pattern")
});
static QUOTE_WORDS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("[a-z]+").expect("valid quote word pattern"));
const NUMBERS: [&str; 20] = [
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
    "twenty",
];

fn spoken_number(word: &str) -> Option<usize> {
    NUMBERS
        .iter()
        .position(|name| *name == word)
        .map(|index| index + 1)
}

fn quote_words(text: &str) -> String {
    static VARIANTS: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?-u:\b(?:hath|you|me)\b)").expect("valid quote variants"));
    let lower = text.to_lowercase();
    let normalized = VARIANTS.replace_all(
        &lower,
        |word: &regex::Captures<'_>| {
            if &word[0] == "hath" { "has" } else { "us" }
        },
    );
    QUOTE_WORDS
        .find_iter(&normalized)
        .map(|word| word.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

/// A damaged citation confirmed by the words and numbering of a local verse.
#[derive(Debug, PartialEq)]
pub struct RecoveredQuote {
    pub book: String,
    pub chapter: f64,
    pub verse_start: f64,
    pub verse_end: Option<f64>,
}

pub fn recover_damaged_quote(
    text: &str,
    search: impl FnOnce(&str) -> Vec<QuoteCandidate>,
) -> Option<RecoveredQuote> {
    let citation = DAMAGED.captures(text)?;
    let encoded = citation[2].to_ascii_lowercase();
    if !encoded.bytes().any(|byte| byte.is_ascii_alphabetic()) {
        return None;
    }
    let raw_quote = citation[4].split(['.', '!', '?']).next()?;
    let quote = quote_words(raw_quote);
    if quote.split(' ').count() < 7 {
        return None;
    }
    let encoded = UNITS.replace_all(&encoded, |unit: &regex::Captures<'_>| {
        spoken_number(&unit[0]).expect("matched unit").to_string()
    });
    if !encoded.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let verse_end = citation.get(3).map(|end| {
        let end = end.as_str().to_ascii_lowercase();
        spoken_number(&end)
            .map(|number| number as f64)
            .unwrap_or_else(|| string_to_number(&end))
    });
    let book = citation[1].trim_matches(is_whitespace).to_ascii_lowercase();
    let book = if book == "psalm" { "psalms" } else { &book };
    let mut result = None;
    for candidate in search(raw_quote) {
        if !candidate.book.eq_ignore_ascii_case(book)
            || !quote_words(&candidate.text).contains(&quote)
            || format!(
                "{}{}",
                number_to_string(candidate.chapter),
                number_to_string(candidate.verse)
            ) != encoded
            || !is_valid_scripture_reference(
                &candidate.book,
                candidate.chapter,
                candidate.verse,
                verse_end,
            )
        {
            continue;
        }
        let found = RecoveredQuote {
            book: candidate.book,
            chapter: candidate.chapter,
            verse_start: candidate.verse,
            verse_end,
        };
        if result.as_ref().is_some_and(|previous| *previous != found) {
            return None;
        }
        result = Some(found);
    }
    result
}
