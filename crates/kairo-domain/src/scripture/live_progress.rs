//! The live progress model for detected scripture, from
//! `src/lib/scripture-live-progress.ts`: grouping suggestions into passages,
//! operator navigation, merging new detections, and following a preacher who
//! reads a passage aloud.

use super::ScriptureSuggestion;
use super::js;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScriptureSuggestionGroup {
    pub id: String,
    pub reference: String,
    pub suggestions: Vec<ScriptureSuggestion>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OperatorSuggestionSections {
    pub current: Vec<ScriptureSuggestionGroup>,
    pub history: Vec<ScriptureSuggestionGroup>,
}

/// Newest groups first; the first `visible_group_count` are current, the rest
/// history. The active group does not change the order (the source accepts
/// and ignores it, so a clicked passage is never reordered).
pub fn partition_operator_suggestion_groups(
    groups: &[ScriptureSuggestionGroup],
    _active_group_id: Option<&str>,
    visible_group_count: usize,
) -> OperatorSuggestionSections {
    let newest_first: Vec<_> = groups.iter().rev().cloned().collect();
    let current: Vec<_> = newest_first
        .iter()
        .take(visible_group_count)
        .cloned()
        .collect();
    let current_ids: HashSet<&str> = current.iter().map(|g| g.id.as_str()).collect();
    let history = newest_first
        .iter()
        .filter(|g| !current_ids.contains(g.id.as_str()))
        .cloned()
        .collect();
    OperatorSuggestionSections { current, history }
}

/// The source's default for `visible_group_count`.
pub const DEFAULT_VISIBLE_GROUP_COUNT: usize = 3;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptureReadingProgress {
    pub matched_index: usize,
    pub active_index: usize,
    pub active_suggestion_id: String,
    pub confidence: f64,
}

/// Where a preacher reading a passage aloud has got to.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PassageFollowState {
    pub passage_id: String,
    pub current_index: f64,
    /// Unused since reading is followed in order; kept for older callers.
    pub matched_token_indexes: Vec<f64>,
    /// How far into the current verse's words the reader has got, from finished phrases.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_to: Option<f64>,
    /// How many of those words were actually heard.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heard_count: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PassageFollowUpdate {
    pub state: PassageFollowState,
    pub next_suggestion_id: Option<String>,
}

/// "Leviticus 26:3-5, 9-10": a passage's verses, runs joined with a dash.
pub fn verse_list_reference(book: &str, chapter: f64, verses: &[f64]) -> String {
    let mut sorted = verses.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted.dedup();
    let mut runs: Vec<(f64, f64)> = Vec::new();
    for verse in sorted {
        match runs.last_mut() {
            Some((_, end)) if verse == *end + 1.0 => *end = verse,
            _ => runs.push((verse, verse)),
        }
    }
    let runs: Vec<String> = runs
        .into_iter()
        .map(|(start, end)| {
            if start == end {
                js::number_to_string(start)
            } else {
                format!(
                    "{}-{}",
                    js::number_to_string(start),
                    js::number_to_string(end)
                )
            }
        })
        .collect();
    format!(
        "{book} {}:{}",
        js::number_to_string(chapter),
        runs.join(", ")
    )
}

/// Marks a suggestion sent. The suggestions are returned unchanged: sending a
/// standalone verse keeps its card visible.
pub fn apply_operator_suggestion_sent(
    suggestions: Vec<ScriptureSuggestion>,
    sent_suggestion_ids: &HashSet<String>,
    suggestion_id: &str,
) -> (Vec<ScriptureSuggestion>, HashSet<String>) {
    let mut sent = sent_suggestion_ids.clone();
    sent.insert(suggestion_id.to_string());
    (suggestions, sent)
}

/// Groups suggestions by passage (or by their own id), keeping first-seen
/// order, one chip per verse and translation, sorted by passage position.
pub fn group_scripture_suggestions(
    suggestions: &[ScriptureSuggestion],
) -> Vec<ScriptureSuggestionGroup> {
    let mut groups: Vec<ScriptureSuggestionGroup> = Vec::new();
    for suggestion in suggestions {
        let id = suggestion
            .passage_id
            .clone()
            .unwrap_or_else(|| suggestion.id.clone());
        if let Some(existing) = groups.iter_mut().find(|g| g.id == id) {
            existing.suggestions.push(suggestion.clone());
            continue;
        }
        groups.push(ScriptureSuggestionGroup {
            id,
            reference: suggestion
                .passage_reference
                .clone()
                .unwrap_or_else(|| suggestion.reference.clone()),
            suggestions: vec![suggestion.clone()],
        });
    }
    for group in &mut groups {
        // A spoken step can land while the row switches translation, briefly
        // leaving two copies of one verse.
        let mut seen = HashSet::new();
        group
            .suggestions
            .retain(|item| seen.insert(format!("{}|{}", item.reference, item.translation)));
        js::sort_by_number(&mut group.suggestions, |l, r| {
            l.passage_index.unwrap_or(0.0) - r.passage_index.unwrap_or(0.0)
        });
    }
    groups
}

/// Arrow-key navigation through the visible verse order.
pub fn navigate_operator_suggestion_id(
    visible_ids: &[String],
    selected_id: Option<&str>,
    delta: i8,
) -> Option<String> {
    let first = visible_ids.first()?;
    let current = selected_id.and_then(|id| visible_ids.iter().position(|v| v == id));
    let Some(current) = current else {
        return Some(first.clone());
    };
    let last = visible_ids.len() as isize - 1;
    let next = (current as isize + isize::from(delta)).clamp(0, last);
    Some(visible_ids[next as usize].clone())
}

fn verse_text(suggestion: Option<&ScriptureSuggestion>) -> &str {
    suggestion
        .and_then(|s| s.verses.first())
        .map_or("", |v| v.text.as_str())
}

fn distinct(tokens: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    tokens
        .into_iter()
        .filter(|t| seen.insert(t.clone()))
        .collect()
}

/// Which suggestion a transcript is reading, by word coverage. Advances to the
/// next card once most of a verse has been heard.
pub fn find_reading_progress(
    transcript: &str,
    suggestions: &[ScriptureSuggestion],
) -> Option<ScriptureReadingProgress> {
    let heard: HashSet<String> = tokenize(transcript).into_iter().collect();
    if heard.len() < 3 {
        return None;
    }
    let mut best_index = None;
    let mut best_coverage = 0.0;
    let mut best_matched = 0;
    for (index, suggestion) in suggestions.iter().enumerate() {
        let verse_tokens = distinct(tokenize(verse_text(Some(suggestion))));
        if verse_tokens.len() < 2 {
            continue;
        }
        let matched = verse_tokens.iter().filter(|t| heard.contains(*t)).count();
        let coverage = matched as f64 / verse_tokens.len() as f64;
        if matched > best_matched || (matched == best_matched && coverage > best_coverage) {
            best_matched = matched;
            best_coverage = coverage;
            best_index = Some(index);
        }
    }
    let best_index = best_index?;
    if best_coverage < 0.45 {
        return None;
    }
    let advance = best_coverage >= 0.72 && best_index < suggestions.len() - 1;
    let active_index = if advance { best_index + 1 } else { best_index };
    Some(ScriptureReadingProgress {
        matched_index: best_index,
        active_index,
        active_suggestion_id: suggestions[active_index].id.clone(),
        confidence: best_coverage,
    })
}

/// `^(.+?)\s+(\d+):(\d+)(?:[-–—](\d+))?$` on a passage reference.
fn parse_passage_reference(text: &str) -> Option<(&str, f64, f64, f64)> {
    for (at, c) in text.char_indices().skip(1) {
        // The lazy book group cannot cross a line terminator.
        if text[..at].chars().any(js::is_line_terminator) {
            return None;
        }
        if !js::is_space(c) {
            continue;
        }
        if let Some(numbers) = parse_tail(&text[at..]) {
            return Some((&text[..at], numbers.0, numbers.1, numbers.2));
        }
    }
    None
}

/// `\s+(\d+):(\d+)(?:[-–—](\d+))?$`.
fn parse_tail(tail: &str) -> Option<(f64, f64, f64)> {
    let rest = tail.trim_start_matches(js::is_space);
    if rest.len() == tail.len() {
        return None;
    }
    let digits = |s: &str| -> usize { s.bytes().take_while(u8::is_ascii_digit).count() };
    let a = digits(rest);
    if a == 0 {
        return None;
    }
    let rest2 = rest[a..].strip_prefix(':')?;
    let b = digits(rest2);
    if b == 0 {
        return None;
    }
    let chapter = js::number_from_digits(&rest[..a]);
    let start = js::number_from_digits(&rest2[..b]);
    let after = &rest2[b..];
    if after.is_empty() {
        return Some((chapter, start, start));
    }
    let dash = after.chars().next()?;
    if !matches!(dash, '-' | '–' | '—') {
        return None;
    }
    let end_text = &after[dash.len_utf8()..];
    let c = digits(end_text);
    if c == 0 || c != end_text.len() {
        return None;
    }
    Some((chapter, start, js::number_from_digits(end_text)))
}

/// Adds a detection, keeping the list free of duplicate verses: a single
/// verse already on screen is not repeated; a passage that another passage
/// already covers is dropped; otherwise the passage replaces standalone or
/// narrower cards inside its range in the same translation.
pub fn merge_scripture_suggestion(
    suggestions: &[ScriptureSuggestion],
    incoming: &ScriptureSuggestion,
) -> Vec<ScriptureSuggestion> {
    if suggestions.iter().any(|item| item.id == incoming.id) {
        return suggestions.to_vec();
    }
    let single = incoming.passage_length.unwrap_or(1.0) <= 1.0;
    if single {
        let reference = js::trim(&incoming.reference).to_lowercase();
        if suggestions.iter().any(|item| {
            item.translation == incoming.translation
                && js::trim(&item.reference).to_lowercase() == reference
        }) {
            return suggestions.to_vec();
        }
    }
    let passage = incoming
        .passage_reference
        .as_deref()
        .and_then(parse_passage_reference);
    let Some((book, chapter, start, end)) = passage.filter(|_| !single) else {
        let mut out = suggestions.to_vec();
        out.push(incoming.clone());
        return out;
    };
    let book = book.to_lowercase();
    fn same_place<'a>(
        item: &'a ScriptureSuggestion,
        book: &str,
        chapter: f64,
    ) -> Option<&'a super::ScriptureVerse> {
        item.verses
            .first()
            .filter(|v| v.book.to_lowercase() == book && v.chapter == chapter)
    }
    // A passage already holding every verse wins: a later, narrower guess at
    // the same reading must not split it into two cards.
    let mut covering: Vec<(&str, Vec<f64>)> = Vec::new();
    for item in suggestions {
        let Some(passage_id) = item.passage_id.as_deref() else {
            continue;
        };
        if passage_id.is_empty()
            || Some(passage_id) == incoming.passage_id.as_deref()
            || item.translation != incoming.translation
        {
            continue;
        }
        let Some(verse) = same_place(item, &book, chapter) else {
            continue;
        };
        match covering.iter_mut().find(|(id, _)| *id == passage_id) {
            Some((_, verses)) => verses.push(verse.verse),
            None => covering.push((passage_id, vec![verse.verse])),
        }
    }
    for (_, verses) in &covering {
        let low = verses.iter().copied().fold(f64::INFINITY, js::min);
        let high = verses.iter().copied().fold(f64::NEG_INFINITY, js::max);
        if low <= start && high >= end {
            return suggestions.to_vec();
        }
    }
    let mut retained: Vec<ScriptureSuggestion> = suggestions
        .iter()
        .filter(|item| {
            if item.passage_id == incoming.passage_id {
                return true;
            }
            // The same passage in another translation sits beside it.
            if item.translation != incoming.translation {
                return true;
            }
            match same_place(item, &book, chapter) {
                Some(verse) => !(verse.verse >= start && verse.verse <= end),
                None => true,
            }
        })
        .cloned()
        .collect();
    retained.push(incoming.clone());
    retained
}

/// The armed passage's next card to send automatically, if it is the active
/// one, not yet sent, and past every card already sent from that passage.
pub fn next_auto_follow_suggestion<'a>(
    suggestions: &'a [ScriptureSuggestion],
    active_suggestion_id: &str,
    armed_passage_id: &str,
    sent_suggestion_ids: &HashSet<String>,
) -> Option<&'a ScriptureSuggestion> {
    let candidate = suggestions.iter().find(|item| {
        item.id == active_suggestion_id
            && item.passage_id.as_deref() == Some(armed_passage_id)
            && !sent_suggestion_ids.contains(&item.id)
    })?;
    let furthest_sent = suggestions
        .iter()
        .filter(|item| {
            item.passage_id.as_deref() == Some(armed_passage_id)
                && sent_suggestion_ids.contains(&item.id)
        })
        .fold(-1.0, |furthest, item| {
            js::max(furthest, item.passage_index.unwrap_or(-1.0))
        });
    (candidate.passage_index.unwrap_or(0.0) > furthest_sent).then_some(candidate)
}

/// How many verse words ahead a heard word may match: a skipped or misheard word or two.
const FOLLOW_LOOKAHEAD: f64 = 4.0;

struct Reading {
    read_to: f64,
    heard_count: f64,
    /// Index of the last heard word that matched, or -1.
    last_heard: isize,
}

/// The verse word at a JavaScript array index (`undefined` off the end or
/// at a fractional index).
fn at(words: &[String], index: f64) -> Option<&str> {
    if index >= 0.0 && index.fract() == 0.0 && index < words.len() as f64 {
        Some(words[index as usize].as_str())
    } else {
        None
    }
}

/// Walks heard words through the verse in order; each may match one of the
/// next few verse words, moving the reader past it.
fn read_through(verse: &[String], heard: &[String], from: f64, heard_before: f64) -> Reading {
    let mut reading = Reading {
        read_to: from,
        heard_count: heard_before,
        last_heard: -1,
    };
    for (index, token) in heard.iter().enumerate() {
        let limit = js::min(verse.len() as f64, reading.read_to + FOLLOW_LOOKAHEAD);
        let mut position = reading.read_to;
        while position < limit {
            if at(verse, position) == Some(token.as_str()) {
                reading.read_to = position + 1.0;
                reading.heard_count += 1.0;
                reading.last_heard = index as isize;
                break;
            }
            position += 1.0;
        }
    }
    reading
}

/// Follows a preacher reading a passage aloud, verse by verse.
///
/// A verse is done once its closing words were heard in order (allowing a
/// misheard word or two) and at least half of it was heard; or early, when
/// the next verse clearly begins in words heard after the current verse's
/// last match. `final_text` is false for interim text, which is re-read from
/// the last finished phrase rather than added on.
pub fn update_passage_follow(
    state: &PassageFollowState,
    transcript: &str,
    suggestions: &[ScriptureSuggestion],
    final_text: bool,
) -> PassageFollowUpdate {
    follow(
        state,
        transcript,
        suggestions,
        final_text,
        FollowRule::Electron,
    )
}

/// [`update_passage_follow`] with the native operator's stricter rule, so the
/// screen never moves on while the current verse is still being read. A
/// preacher often stops short of a verse's last words to explain, then comes
/// back to finish it: the verse is done only once its last word was heard
/// (and most of it was), plurals aside. Should the last word be misheard, the
/// next verse counts as begun once three quarters of this one was read, on
/// finished speech.
pub fn update_passage_follow_strict(
    state: &PassageFollowState,
    transcript: &str,
    suggestions: &[ScriptureSuggestion],
    final_text: bool,
) -> PassageFollowUpdate {
    follow(
        state,
        transcript,
        suggestions,
        final_text,
        FollowRule::Strict,
    )
}

/// Share of a verse that must be read before follow-along may leave it.
const STRICT_READ_SHARE: f64 = 0.6;
/// Share of a verse read before the next verse's opening words may move on,
/// for when recognition never catches the last word.
const STRICT_NEXT_SHARE: f64 = 0.75;

/// "sins" and "sin": recognition often changes a verse's closing plural.
fn fold_plural(token: String) -> String {
    if token.len() > 3 && token.ends_with('s') && !token.ends_with("ss") {
        token[..token.len() - 1].to_owned()
    } else {
        token
    }
}

#[derive(Clone, Copy, PartialEq)]
enum FollowRule {
    /// `updatePassageFollow` exactly, as the Electron oracle pins it.
    Electron,
    Strict,
}

fn follow(
    state: &PassageFollowState,
    transcript: &str,
    suggestions: &[ScriptureSuggestion],
    final_text: bool,
    rule: FollowRule,
) -> PassageFollowUpdate {
    let mut passage: Vec<&ScriptureSuggestion> = suggestions
        .iter()
        .filter(|item| item.passage_id.as_deref() == Some(state.passage_id.as_str()))
        .collect();
    js::sort_by_number(&mut passage, |l, r| {
        l.passage_index.unwrap_or(0.0) - r.passage_index.unwrap_or(0.0)
    });
    let current = passage
        .iter()
        .find(|item| item.passage_index == Some(state.current_index));
    let fold = |tokens: Vec<String>| -> Vec<String> {
        if rule == FollowRule::Strict {
            tokens.into_iter().map(fold_plural).collect()
        } else {
            tokens
        }
    };
    let verse_tokens = fold(tokenize(verse_text(current.copied())));
    let unchanged = || PassageFollowUpdate {
        state: state.clone(),
        next_suggestion_id: None,
    };
    if current.is_none() || verse_tokens.len() < 2 {
        return unchanged();
    }

    let heard = fold(tokenize(transcript));
    let reading = read_through(
        &verse_tokens,
        &heard,
        state.read_to.unwrap_or(0.0),
        state.heard_count.unwrap_or(0.0),
    );
    let length = verse_tokens.len() as f64;
    let closing_slack = if verse_tokens.len() <= 8 { 1.0 } else { 2.0 };
    let heard_share = if rule == FollowRule::Strict {
        STRICT_READ_SHARE
    } else {
        0.5
    };
    let closing = if rule == FollowRule::Strict {
        0.0
    } else {
        closing_slack
    };
    let finished =
        reading.read_to >= length - closing && reading.heard_count >= (length * heard_share).ceil();

    let next = passage
        .iter()
        .find(|item| item.passage_index == Some(state.current_index + 1.0));
    let mut next_lead = fold(tokenize(verse_text(next.copied())));
    next_lead.truncate(4);
    let after_current = &heard[(reading.last_heard + 1) as usize..];
    // At the closing words only a misheard last word stands between the
    // reader and the next verse, so its opening counts sooner: on live text,
    // from two of its first three words. Earlier, mid-verse, it needs
    // finished speech and three of four, so a pause to explain never jumps.
    let at_close = reading.read_to >= length - closing_slack;
    let (may_begin_next, lead_len, lead_needed) = match rule {
        FollowRule::Electron => (true, 4, 3.0),
        FollowRule::Strict if at_close => (true, 3, 2.0),
        FollowRule::Strict => (
            final_text && reading.read_to >= length * STRICT_NEXT_SHARE,
            4,
            3.0,
        ),
    };
    next_lead.truncate(lead_len);
    let next_started = may_begin_next
        && next_lead.len() as f64 >= lead_needed
        && read_through(&next_lead, after_current, 0.0, 0.0).heard_count >= lead_needed;

    if let Some(next) = next.filter(|_| finished || next_started) {
        return PassageFollowUpdate {
            state: PassageFollowState {
                passage_id: state.passage_id.clone(),
                current_index: state.current_index + 1.0,
                matched_token_indexes: Vec::new(),
                read_to: Some(0.0),
                heard_count: Some(0.0),
            },
            next_suggestion_id: Some(next.id.clone()),
        };
    }
    if !final_text {
        // Live text is provisional: only a finished phrase moves the start.
        return unchanged();
    }
    PassageFollowUpdate {
        state: PassageFollowState {
            read_to: Some(reading.read_to),
            heard_count: Some(reading.heard_count),
            ..state.clone()
        },
        next_suggestion_id: None,
    }
}

/// The reader has finished the passage being followed: its last verse was
/// read to its last word, by the strict rule that moves on from any other verse.
/// A one-verse passage counts; its only verse is its last.
pub fn passage_read_through(
    state: &PassageFollowState,
    suggestions: &[ScriptureSuggestion],
) -> bool {
    let passage: Vec<&ScriptureSuggestion> = suggestions
        .iter()
        .filter(|item| item.passage_id.as_deref() == Some(state.passage_id.as_str()))
        .collect();
    let last = passage
        .iter()
        .map(|item| item.passage_index.unwrap_or(0.0))
        .fold(f64::NEG_INFINITY, f64::max);
    if passage.is_empty() || state.current_index != last {
        return false;
    }
    let current = passage
        .iter()
        .find(|item| item.passage_index.unwrap_or(0.0) == state.current_index);
    let length = tokenize(verse_text(current.copied())).len() as f64;
    if length < 2.0 {
        return false;
    }
    // Its last word heard, as the strict follow rule requires: a pause to
    // explain before the closing words must not start the clear.
    state.read_to.unwrap_or(0.0) >= length
        && state.heard_count.unwrap_or(0.0) >= (length * STRICT_READ_SHARE).ceil()
}

const STOP_WORDS: &[&str] = &[
    "a", "an", "and", "as", "at", "be", "for", "in", "is", "it", "of", "that", "the", "to", "was",
    "with",
];

const SPOKEN_ALIASES: &[(&str, &str)] = &[
    ("forth", "out"),
    ("spake", "spoke"),
    ("thee", "you"),
    ("thou", "you"),
    ("unto", "to"),
];

/// Lower-cased content words with archaic forms mapped to modern speech.
/// A token that was only apostrophes stays as an empty string, as in the source.
pub fn tokenize(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let mut words: Vec<String> = Vec::new();
    let mut current = String::new();
    for c in lower.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '\'' {
            current.push(c);
        } else if !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
        .into_iter()
        .map(|token| {
            let token = token.strip_suffix("'s").unwrap_or(&token).replace('\'', "");
            SPOKEN_ALIASES
                .iter()
                .find(|(from, _)| *from == token)
                .map_or(token, |(_, to)| (*to).to_string())
        })
        .filter(|token| !STOP_WORDS.contains(&token.as_str()))
        .collect()
}
