//! Scoring from `global-quote.ts`: confident quotations by ordered runs, and
//! paraphrases by shared rare words, always marked ambiguous.

use crate::api_bible::cache::CacheQuoteSource;
use kairo_domain::scripture::plan::verse_key;
use kairo_domain::scripture::small::spoken_verse_text;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// A verse offered to matching.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalQuoteCandidate {
    pub book: String,
    pub chapter: f64,
    pub verse: f64,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_source: Option<QuoteSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote_corpus_version: Option<u64>,
}

/// `CacheQuoteSource` with Electron's JSON field names.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuoteSource {
    pub bible_id: String,
    pub book_id: i64,
    pub chapter: i64,
    pub verse: i64,
    pub fetched_at: i64,
    pub expires_at: i64,
}

impl From<CacheQuoteSource> for QuoteSource {
    fn from(s: CacheQuoteSource) -> Self {
        Self {
            bible_id: s.bible_id,
            book_id: s.book_id,
            chapter: s.chapter,
            verse: s.verse,
            fetched_at: s.fetched_at,
            expires_at: s.expires_at,
        }
    }
}

impl From<QuoteSource> for CacheQuoteSource {
    fn from(s: QuoteSource) -> Self {
        Self {
            bible_id: s.bible_id,
            book_id: s.book_id,
            chapter: s.chapter,
            verse: s.verse,
            fetched_at: s.fetched_at,
            expires_at: s.expires_at,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalQuoteMatch {
    #[serde(flatten)]
    pub candidate: GlobalQuoteCandidate,
    pub score: f64,
    pub ambiguous: bool,
}

const STOP: &[&str] = &[
    "a", "an", "and", "as", "at", "be", "for", "in", "is", "it", "of", "that", "the", "to", "was",
    "with", "he", "she", "they", "we", "you", "i", "his", "her", "their", "our", "your", "my",
    "its",
];
/// Words common to biblical narration: shared alone, not evidence of a verse.
const FORMULA: &[&str] = &[
    "came", "pass", "said", "unto", "them", "behold", "shall", "hath", "has", "have", "been",
    "will", "there", "which", "god", "lord", "good",
];
/// Everyday words that say nothing about which verse is meant.
const COMMON: &[&str] = &[
    "about", "after", "again", "against", "also", "always", "another", "because", "before",
    "being", "between", "both", "came", "come", "comes", "could", "days", "does", "done", "down",
    "each", "even", "ever", "every", "first", "from", "give", "given", "going", "gone", "great",
    "have", "here", "into", "just", "keep", "know", "known", "last", "left", "life", "like",
    "little", "long", "look", "made", "make", "many", "more", "most", "much", "must", "never",
    "next", "nothing", "only", "other", "over", "people", "place", "said", "same", "says",
    "should", "since", "some", "still", "such", "take", "than", "that", "their", "them", "then",
    "there", "these", "they", "thing", "things", "think", "this", "those", "through", "time",
    "today", "told", "took", "turn", "under", "upon", "very", "want", "way", "well", "went",
    "were", "what", "when", "where", "which", "while", "will", "with", "without", "word", "words",
    "world", "would", "year", "years", "your",
];
/// Words skipped from the relaxed OR retrieval.
pub(crate) const RELAXED_SKIP: &[&str] = &[
    "said", "shall", "unto", "have", "been", "will", "them", "this", "from", "there", "which",
];

/// An exact run of this many words, as most of the utterance, is confident.
const EXACT_UTTERANCE_MIN_RUN: usize = 8;
/// Shared rare words a paraphrase needs before it is offered.
const POSSIBLE_MIN_SHARED: usize = 3;
/// A word is rare when at most this many verses of one translation hold it.
pub const RARE_VERSE_COUNT: f64 = 150.0;

/// How many verses of one translation contain a word (or its plural).
pub type WordRarity<'a> = &'a mut dyn FnMut(&str) -> f64;

/// `quoteTokens`: maximal runs of `[a-z0-9]` after lower-casing.
pub fn quote_tokens(text: &str) -> Vec<String> {
    spoken_verse_text(text)
        .to_lowercase()
        .split(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit()))
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .collect()
}

pub fn evidence_tokens(text: &str) -> Vec<String> {
    quote_tokens(text)
        .into_iter()
        .filter(|t| !STOP.contains(&t.as_str()))
        .collect()
}

/// "garments" and "garment" are the same evidence in a paraphrase.
fn stem(token: &str) -> &str {
    if token.len() > 4 && token.ends_with('s') && !token.ends_with("ss") {
        &token[..token.len() - 1]
    } else {
        token
    }
}

fn tail<T: Clone>(items: &[T], n: usize) -> Vec<T> {
    items[items.len().saturating_sub(n)..].to_vec()
}

fn unique(items: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    items
        .into_iter()
        .filter(|t| seen.insert(t.clone()))
        .collect()
}

fn rare_words(tokens: &[String], rarity: WordRarity<'_>) -> Vec<String> {
    let candidates = tokens
        .iter()
        .filter(|t| t.len() > 2 && !FORMULA.contains(&t.as_str()) && !COMMON.contains(&t.as_str()))
        .map(|t| stem(t).to_owned());
    unique(candidates.filter(|t| rarity(t) <= RARE_VERSE_COUNT))
}

/// Keeps the first position of each key and the best-scoring candidate.
fn group(
    grouped: &mut Vec<GlobalQuoteMatch>,
    index: &mut HashMap<String, usize>,
    m: GlobalQuoteMatch,
) {
    let key = verse_key(&m.candidate.book, m.candidate.chapter, m.candidate.verse);
    match index.get(&key) {
        Some(&i) => {
            if grouped[i].score < m.score {
                grouped[i] = m;
            }
        }
        None => {
            index.insert(key, grouped.len());
            grouped.push(m);
        }
    }
}

fn rank(mut grouped: Vec<GlobalQuoteMatch>) -> Vec<GlobalQuoteMatch> {
    grouped.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    grouped
}

fn possible_matches(
    text: &str,
    candidates: &[GlobalQuoteCandidate],
    rarity: WordRarity<'_>,
) -> Vec<GlobalQuoteMatch> {
    let heard = rare_words(&tail(&evidence_tokens(text), 24), rarity);
    if heard.len() < POSSIBLE_MIN_SHARED {
        return Vec::new();
    }
    let (mut grouped, mut index) = (Vec::new(), HashMap::new());
    for candidate in candidates {
        let verse: HashSet<String> = evidence_tokens(&candidate.text)
            .iter()
            .map(|t| stem(t).to_owned())
            .collect();
        let score = heard.iter().filter(|t| verse.contains(*t)).count();
        if score < POSSIBLE_MIN_SHARED {
            continue;
        }
        group(
            &mut grouped,
            &mut index,
            GlobalQuoteMatch {
                candidate: candidate.clone(),
                score: score as f64,
                ambiguous: true,
            },
        );
    }
    let ranked = rank(grouped);
    let Some(top) = ranked.first().map(|r| r.score) else {
        return Vec::new();
    };
    ranked
        .into_iter()
        .filter(|r| r.score >= top - 1.0)
        .take(3)
        .collect()
}

#[derive(Clone, Copy, Debug)]
struct AlignedRun {
    run: usize,
    heard_start: isize,
    verse_start: isize,
}

/// Token ids for one utterance: "not" is 0, words never heard are `UNHEARD`,
/// so they can never match. Integer compares keep the alignment loop cheap.
const NOT: u32 = 0;
const UNHEARD: u32 = u32::MAX;

struct Vocabulary(HashMap<String, u32>);

impl Vocabulary {
    fn new<'a>(heard: impl IntoIterator<Item = &'a String>) -> Self {
        let mut ids = HashMap::from([("not".to_owned(), NOT)]);
        for token in heard {
            let next = ids.len() as u32;
            ids.entry(token.clone()).or_insert(next);
        }
        Self(ids)
    }

    fn ids(&self, tokens: &[String]) -> Vec<u32> {
        tokens
            .iter()
            .map(|t| self.0.get(t).copied().unwrap_or(UNHEARD))
            .collect()
    }
}

/// Longest ordered run allowing one substitution, never across a "not".
fn aligned_run(heard: &[u32], verse: &[u32]) -> AlignedRun {
    let mut best = AlignedRun {
        run: 0,
        heard_start: -1,
        verse_start: -1,
    };
    for i in 0..heard.len() {
        for j in 0..verse.len() {
            let (mut matches, mut errors) = (0, 0);
            for k in 0..(heard.len() - i).min(verse.len() - j) {
                if heard[i + k] == verse[j + k] {
                    matches += 1;
                } else if heard[i + k] == NOT || verse[j + k] == NOT {
                    break;
                } else {
                    errors += 1;
                    if errors > 1 {
                        break;
                    }
                }
                if matches >= 4 && matches > best.run {
                    best = AlignedRun {
                        run: matches,
                        heard_start: i as isize,
                        verse_start: j as isize,
                    };
                }
            }
        }
    }
    best
}

fn window_has_not(tokens: &[u32], at: isize) -> bool {
    // `slice(Math.max(0, at - 2), at)`: a negative `at` counts from the end.
    let len = tokens.len() as isize;
    let end = if at < 0 {
        (len + at).max(0)
    } else {
        at.min(len)
    };
    let start = (at - 2).max(0).min(end);
    tokens[start as usize..end as usize].contains(&NOT)
}

/// "Do not give…" negates the quotation; an earlier "not" does not.
fn negated_before(heard: &[u32], verse: &[u32], at: AlignedRun) -> bool {
    window_has_not(heard, at.heard_start) && !window_has_not(verse, at.verse_start)
}

/// `matchGlobalQuote`. With `rarity`, speech that only paraphrases a verse can
/// return up to three possible verses, always ambiguous.
pub fn match_global_quote(
    text: &str,
    candidates: &[GlobalQuoteCandidate],
    rarity: Option<WordRarity<'_>>,
) -> Vec<GlobalQuoteMatch> {
    let heard = tail(&evidence_tokens(text), 40);
    if heard.len() < 3 {
        return Vec::new();
    }
    let heard_all = tail(&quote_tokens(text), 60);
    let vocabulary = Vocabulary::new(heard.iter().chain(&heard_all));
    let (heard_ids, heard_all_ids) = (vocabulary.ids(&heard), vocabulary.ids(&heard_all));
    let anchors = unique(
        heard
            .iter()
            .filter(|t| t.len() > 2 && !FORMULA.contains(&t.as_str()))
            .cloned(),
    );
    let (mut grouped, mut index) = (Vec::new(), HashMap::new());
    for candidate in candidates {
        let mut verse = evidence_tokens(&candidate.text);
        verse.truncate(180);
        let words: HashSet<&str> = verse.iter().map(String::as_str).collect();
        if anchors
            .iter()
            .filter(|t| words.contains(t.as_str()))
            .count()
            < 2
        {
            continue;
        }
        let verse_ids = vocabulary.ids(&verse);
        let content = aligned_run(&heard_ids, &verse_ids);
        if negated_before(&heard_ids, &verse_ids, content) {
            continue;
        }
        let mut all = quote_tokens(&candidate.text);
        all.truncate(240);
        let full_run = aligned_run(&heard_all_ids, &vocabulary.ids(&all)).run;
        // Verses of short common words have little content to run on; an
        // exact run that is most of what was said counts.
        let exact =
            full_run >= EXACT_UTTERANCE_MIN_RUN && full_run as f64 >= heard_all.len() as f64 * 0.75;
        let score = (content.run as f64)
            .max(if full_run >= 10 {
                full_run as f64 / 2.0
            } else {
                0.0
            })
            .max(if exact { 5.0 } else { 0.0 });
        if score < 5.0 {
            continue;
        }
        // Translations of one verse are one candidate, not competitors.
        group(
            &mut grouped,
            &mut index,
            GlobalQuoteMatch {
                candidate: candidate.clone(),
                score,
                ambiguous: false,
            },
        );
    }
    let ranked = rank(grouped);
    let Some(top) = ranked.first().map(|r| r.score) else {
        return match rarity {
            Some(rarity) => possible_matches(text, candidates, rarity),
            None => Vec::new(),
        };
    };
    let matches: Vec<_> = ranked
        .into_iter()
        .filter(|r| r.score >= top - 1.0)
        .take(3)
        .collect();
    let ambiguous = matches.len() > 1;
    matches
        .into_iter()
        .map(|r| GlobalQuoteMatch { ambiguous, ..r })
        .collect()
}

/// Whether the newest piece of speech belongs to this verse: it shares two
/// meaningful words, or every meaningful word in it is the verse's.
pub fn fresh_speech_supports(fresh: &str, verse_text: &str) -> bool {
    let words: HashSet<String> = evidence_tokens(verse_text).into_iter().collect();
    let meaningful = unique(evidence_tokens(fresh).into_iter().filter(|t| t.len() > 2));
    let shared = meaningful.iter().filter(|t| words.contains(*t)).count();
    shared >= 2 || (shared > 0 && shared == meaningful.len())
}
