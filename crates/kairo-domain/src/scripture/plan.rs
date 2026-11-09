//! Sermon playlist matching and resolving, from `src/lib/sermon-plan-match.ts`
//! and `src/lib/sermon-plan-resolve.ts`.

use super::live_progress::tokenize;
use super::translations::bundled_translation_ids_in_order;
use super::{
    ScriptureResult, ScriptureTranslation, ScriptureTranslationOption, ScriptureVerse, SermonPlan,
    SermonScriptureItem, js,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt::Display;
use std::future::Future;

/// One verse of one playlist item, resolved and ready to project.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanVerseEntry {
    pub plan_id: String,
    pub plan_item_id: String,
    /// The item's own label, such as `Romans 8:28–30`.
    pub item_reference: String,
    /// Canonical single-verse reference, such as `Romans 8:28`.
    pub reference: String,
    pub translation: ScriptureTranslation,
    pub verse: ScriptureVerse,
    /// Position of this verse in its item.
    pub index_in_item: usize,
    /// How many verses the item holds.
    pub item_length: usize,
}

/// A map that keeps first-insertion order, like a JavaScript `Map`.
#[derive(Clone, Debug, PartialEq)]
pub struct OrderedMap<V> {
    keys: Vec<String>,
    values: HashMap<String, V>,
}

impl<V> Default for OrderedMap<V> {
    fn default() -> Self {
        Self {
            keys: Vec::new(),
            values: HashMap::new(),
        }
    }
}

impl<V> OrderedMap<V> {
    pub fn get(&self, key: &str) -> Option<&V> {
        self.values.get(key)
    }

    /// Replaces the value in place; a new key goes last.
    pub fn set(&mut self, key: String, value: V) {
        if !self.values.contains_key(&key) {
            self.keys.push(key.clone());
        }
        self.values.insert(key, value);
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &V)> {
        self.keys.iter().map(|k| (k.as_str(), &self.values[k]))
    }
}

/// An entry whose text is distinct enough to match by quotation.
#[derive(Clone, Debug, PartialEq)]
pub struct QuotableEntry {
    pub entry: PlanVerseEntry,
    pub tokens: Vec<String>,
}

/// Lookup structures for one sermon playlist.
#[derive(Clone, Debug, PartialEq)]
pub struct SermonPlanIndex {
    pub plan_id: String,
    pub ordered: Vec<PlanVerseEntry>,
    /// Position in `ordered` of the verse expected next.
    pub expected_index: usize,
    /// [`verse_key`] to entry; the narrower item wins.
    pub by_verse_key: OrderedMap<PlanVerseEntry>,
    /// The same key plus `|TRANSLATION`, for a preacher who names one.
    pub by_verse_translation: OrderedMap<PlanVerseEntry>,
    pub quotable: Vec<QuotableEntry>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlanQuoteMatch {
    pub entry: PlanVerseEntry,
    pub coverage: f64,
}

/// Minimum distinct tokens a verse needs to be quote-matchable.
const MIN_VERSE_TOKENS: usize = 6;
/// Distinct-token coverage a transcript must reach to count as reading a verse.
pub const DEFAULT_MIN_COVERAGE: f64 = 0.6;
/// Distinct verse tokens that must be heard whatever the coverage.
const MIN_MATCHED_TOKENS: usize = 4;
/// Ordered run required so scattered common words cannot match.
const RUN_LENGTH: usize = 3;

/// `book|chapter|verse` with the book trimmed, lower-cased and single-spaced.
pub fn verse_key(book: &str, chapter: f64, verse: f64) -> String {
    format!(
        "{}|{}|{}",
        js::collapse_spaces(&js::trim(book).to_lowercase()),
        js::number_to_string(chapter),
        js::number_to_string(verse)
    )
}

fn item_verse_reference(verse: &ScriptureVerse) -> String {
    format!(
        "{} {}:{}",
        verse.book,
        js::number_to_string(verse.chapter),
        js::number_to_string(verse.verse)
    )
}

/// Builds the lookup structures. Keys come from the verse rows, which are
/// canonical, not from item references, which mix dash styles.
pub fn build_sermon_plan_index(plan: &SermonPlan) -> SermonPlanIndex {
    let mut by_verse_key = OrderedMap::default();
    let mut by_verse_translation: OrderedMap<PlanVerseEntry> = OrderedMap::default();
    let mut ordered = Vec::new();
    for item in &plan.items {
        if !item.available || item.verses.is_empty() {
            continue;
        }
        for (index_in_item, verse) in item.verses.iter().enumerate() {
            let entry = PlanVerseEntry {
                plan_id: plan.id.clone(),
                plan_item_id: item.id.clone(),
                item_reference: item.reference.clone(),
                reference: item_verse_reference(verse),
                translation: item.translation.clone(),
                verse: verse.clone(),
                index_in_item,
                item_length: item.verses.len(),
            };
            ordered.push(entry.clone());
            let key = verse_key(&verse.book, verse.chapter, verse.verse);
            // Narrower item wins: a single-verse call must not drag a whole
            // passage onto the screen when both are in the playlist.
            let translation_key = format!("{key}|{}", item.translation.to_uppercase());
            if by_verse_translation
                .get(&translation_key)
                .is_none_or(|same| same.item_length > entry.item_length)
            {
                by_verse_translation.set(translation_key, entry.clone());
            }
            if by_verse_key
                .get(&key)
                .is_some_and(|existing: &PlanVerseEntry| existing.item_length <= entry.item_length)
            {
                continue;
            }
            by_verse_key.set(key, entry);
        }
    }
    let quotable = by_verse_key
        .iter()
        .filter_map(|(_, entry)| {
            let tokens = tokenize(&entry.verse.text);
            let distinct: HashSet<&String> = tokens.iter().collect();
            (distinct.len() >= MIN_VERSE_TOKENS).then(|| QuotableEntry {
                entry: entry.clone(),
                tokens,
            })
        })
        .collect();
    SermonPlanIndex {
        plan_id: plan.id.clone(),
        ordered,
        expected_index: 0,
        by_verse_key,
        by_verse_translation,
        quotable,
    }
}

/// A detected reference to look up in the playlist.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanReference {
    pub book: String,
    pub chapter: f64,
    pub verse_start: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verse_end: Option<f64>,
}

/// The playlist entries covering a reference, in order; empty when any verse
/// of the range is missing, so a partial hit falls through to the normal
/// Bible lookup. With a (non-empty) `translation`, only its entries count.
pub fn match_plan_reference(
    index: Option<&SermonPlanIndex>,
    reference: &PlanReference,
    translation: Option<&str>,
) -> Vec<PlanVerseEntry> {
    let start = reference.verse_start;
    let end = match reference.verse_end {
        Some(end) if end > start => end,
        _ => start,
    };
    let Some(index) = index else {
        return Vec::new();
    };
    let translation = translation.filter(|t| !t.is_empty());
    let mut entries = Vec::new();
    let mut verse = start;
    while verse <= end {
        let key = verse_key(&reference.book, reference.chapter, verse);
        let entry = match translation {
            Some(t) => index
                .by_verse_translation
                .get(&format!("{key}|{}", t.to_uppercase())),
            None => index.by_verse_key.get(&key),
        };
        let Some(entry) = entry else {
            return Vec::new();
        };
        entries.push(entry.clone());
        verse += 1.0;
    }
    entries
}

fn has_ordered_run(verse_tokens: &[String], heard: &[String], run: usize) -> bool {
    if verse_tokens.len() < run || heard.len() < run {
        return false;
    }
    let heard_runs: HashSet<String> = heard.windows(run).map(|w| w.join(" ")).collect();
    verse_tokens
        .windows(run)
        .any(|w| heard_runs.contains(&w.join(" ")))
}

/// The playlist verse a transcript reads aloud, if any. Coverage alone lets
/// commentary score highly by reusing a verse's words, so a contiguous run of
/// the verse's own wording is also required. The expected verse wins a tie.
pub fn match_plan_quote(
    index: Option<&SermonPlanIndex>,
    transcript: &str,
    min_coverage: f64,
) -> Option<PlanQuoteMatch> {
    let index = index?;
    let heard_sequence = tokenize(transcript);
    if heard_sequence.len() < MIN_MATCHED_TOKENS {
        return None;
    }
    let heard: HashSet<&String> = heard_sequence.iter().collect();
    let expected = index.ordered.get(index.expected_index);
    let bonus = |candidate: &PlanVerseEntry| {
        if expected.is_some_and(|e| e.reference == candidate.reference) {
            0.04
        } else {
            0.0
        }
    };
    let mut best: Option<PlanQuoteMatch> = None;
    for QuotableEntry { entry, tokens } in &index.quotable {
        let mut seen = HashSet::new();
        let distinct: Vec<&String> = tokens.iter().filter(|t| seen.insert(*t)).collect();
        let matched = distinct.iter().filter(|t| heard.contains(**t)).count();
        if matched < MIN_MATCHED_TOKENS {
            continue;
        }
        let coverage = matched as f64 / distinct.len() as f64;
        if coverage < min_coverage {
            continue;
        }
        if !has_ordered_run(tokens, &heard_sequence, RUN_LENGTH) {
            continue;
        }
        if best
            .as_ref()
            .is_none_or(|b| coverage + bonus(entry) > b.coverage + bonus(&b.entry))
        {
            best = Some(PlanQuoteMatch {
                entry: entry.clone(),
                coverage,
            });
        }
    }
    best
}

/// Reading or presenting a verse moves the expectation past it; order alone
/// never produces a match. Returns whether the expectation moved. An empty
/// `item_id` counts as none, as in the source.
pub fn advance_plan_expectation(
    index: &mut SermonPlanIndex,
    reference: &str,
    item_id: Option<&str>,
) -> bool {
    let item_id = item_id.filter(|id| !id.is_empty());
    let fits = |entry: &PlanVerseEntry| {
        entry.reference == reference && item_id.is_none_or(|id| entry.plan_item_id == id)
    };
    let current = index
        .expected_index
        .checked_sub(1)
        .and_then(|i| index.ordered.get(i));
    if current.is_some_and(fits) {
        return false;
    }
    let candidates: Vec<usize> = index
        .ordered
        .iter()
        .enumerate()
        .filter(|(_, entry)| fits(entry))
        .map(|(position, _)| position)
        .collect();
    let Some(position) = candidates
        .iter()
        .find(|p| **p >= index.expected_index)
        .or(candidates.first())
        .copied()
    else {
        return false;
    };
    if index.expected_index == position + 1 {
        return false;
    }
    index.expected_index = position + 1;
    true
}

/// The requested translation, then the operator's default, then bundled local
/// Bibles in registry order, then any other available translation; no repeats.
pub fn translation_fallback_order(
    requested: &str,
    preferred: &str,
    options: &[ScriptureTranslationOption],
) -> Vec<ScriptureTranslation> {
    let mut ordered: Vec<ScriptureTranslation> = Vec::new();
    let candidates = [requested, preferred]
        .into_iter()
        .chain(bundled_translation_ids_in_order())
        .chain(
            options
                .iter()
                .filter(|o| o.available)
                .map(|o| o.id.as_str()),
        );
    for candidate in candidates {
        if !ordered.iter().any(|t| t == candidate) {
            ordered.push(candidate.to_string());
        }
    }
    ordered
}

/// Fills a playlist item that has no text by trying each translation in
/// [`translation_fallback_order`]. An item that already has text is returned
/// as it is. On success from another translation, `error` says which was
/// shown; on failure the last lookup error is kept.
pub async fn resolve_sermon_plan_item<F, Fut, E>(
    item: &SermonScriptureItem,
    preferred_translation: &str,
    search: &mut F,
    options: &[ScriptureTranslationOption],
) -> SermonScriptureItem
where
    F: FnMut(String, ScriptureTranslation) -> Fut,
    Fut: Future<Output = Result<Vec<ScriptureResult>, E>>,
    E: Display,
{
    if item.available && !item.verses.is_empty() {
        return item.clone();
    }
    let mut last_error = format!("{} text is unavailable", item.translation);
    for translation in translation_fallback_order(&item.translation, preferred_translation, options)
    {
        match search(item.reference.clone(), translation.clone()).await {
            Ok(results) => {
                let Some(result) = results.into_iter().next().filter(|r| !r.verses.is_empty())
                else {
                    last_error = format!("{translation} text is unavailable");
                    continue;
                };
                return SermonScriptureItem {
                    reference: result.reference,
                    translation: result.translation,
                    verses: result.verses,
                    available: true,
                    error: (translation != item.translation).then(|| {
                        format!("{} unavailable — showing {translation}", item.translation)
                    }),
                    ..item.clone()
                };
            }
            Err(error) => last_error = error.to_string(),
        }
    }
    SermonScriptureItem {
        verses: Vec::new(),
        available: false,
        error: Some(last_error),
        ..item.clone()
    }
}

/// [`resolve_sermon_plan_item`] for every item, in order. The source runs the
/// items concurrently; each is independent, so the results are the same.
pub async fn resolve_sermon_plan_items<F, Fut, E>(
    items: &[SermonScriptureItem],
    preferred_translation: &str,
    mut search: F,
    options: &[ScriptureTranslationOption],
) -> Vec<SermonScriptureItem>
where
    F: FnMut(String, ScriptureTranslation) -> Fut,
    Fut: Future<Output = Result<Vec<ScriptureResult>, E>>,
    E: Display,
{
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        out.push(resolve_sermon_plan_item(item, preferred_translation, &mut search, options).await);
    }
    out
}
