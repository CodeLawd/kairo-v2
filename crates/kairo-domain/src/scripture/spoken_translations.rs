//! Bible translations named aloud in a sermon, from
//! `src/lib/spoken-translations.ts`: "John 3:16 in the NLT", "the New Living
//! Translation says", "in the N-I-V". Derived from the registry.
//!
//! - Letters count in any case or spelled out, except an id that is also an
//!   English word: WEB counts only in capitals.
//! - A short everyday name ("new living") counts only when used as a name:
//!   followed by translation, version or bible, a verb such as "says", or the
//!   end of the phrase.
//! - Names that are ordinary sermon words ("the message", "amplified") count
//!   only in forms nobody says by accident.

use super::js;
use super::translations::{BIBLE_TRANSLATIONS, js_regex};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// Ids that are also English words, so only their capitalised form counts.
const WORD_IDS: &[&str] = &["WEB"];

/// Spoken forms replacing the registry's name patterns for common-word names.
const SPOKEN_NAME_OVERRIDES: &[(&str, &[&str])] = &[
    (
        "MSG",
        &[
            "the message (?:bible|translation|version)",
            "message bible",
            "message translation",
        ],
    ),
    (
        "AMPC",
        &[
            "the amplified",
            "amplified (?:bible|version|translation|classic)",
        ],
    ),
];

const NAME_WORDS: &[&str] = &["translation", "version", "bible"];
const NAME_VERBS: &[&str] = &[
    "says", "said", "puts", "put", "reads", "read", "renders", "has", "and", "or", "too", "again",
    "instead",
];

struct SpokenPattern {
    id: &'static str,
    regex: Regex,
    /// Matched against ASCII-lower-cased text (JavaScript's `i` flag folds
    /// ASCII only, and every source is lower case).
    case_insensitive: bool,
    /// The source's `NAME_ENDING` lookahead applies.
    name_ending: bool,
}

fn escape_regex(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        if ".*+?^${}()|[]\\".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn build_patterns() -> Vec<SpokenPattern> {
    let mut patterns = Vec::new();
    for entry in BIBLE_TRANSLATIONS {
        let id = entry.id.to_uppercase();
        let word_id = WORD_IDS.contains(&id.as_str());
        // Abbreviation as written ("NLT", "niv").
        patterns.push(SpokenPattern {
            id: entry.id,
            regex: js_regex(&format!(
                r"\b{}\b",
                escape_regex(&if word_id {
                    id.clone()
                } else {
                    id.to_lowercase()
                })
            )),
            case_insensitive: !word_id,
            name_ending: false,
        });
        // Letter by letter ("n l t", "N.I.V.").
        if (2..=5).contains(&id.len()) && id.chars().all(|c| c.is_ascii_uppercase()) {
            let spelled = id
                .to_lowercase()
                .chars()
                .map(String::from)
                .collect::<Vec<_>>()
                .join(r"[\s.\-]+");
            patterns.push(SpokenPattern {
                id: entry.id,
                regex: js_regex(&format!(r"\b{spelled}\b\.?")),
                case_insensitive: true,
                name_ending: false,
            });
        }
        // Names: the full name, then the registry's shorter name patterns.
        let mut names: Vec<(String, bool)> = Vec::new();
        if let Some((_, overrides)) = SPOKEN_NAME_OVERRIDES.iter().find(|(o, _)| *o == id) {
            names.extend(overrides.iter().map(|s| (s.to_string(), false)));
        } else {
            let full = escape_regex(&entry.name.replace(',', "").to_lowercase());
            let full = full.split_whitespace().collect::<Vec<_>>().join(r"\s+");
            names.push((full, false));
            for source in entry.name_patterns {
                let unambiguous = source.to_ascii_lowercase().contains("king james");
                names.push((format!("(?:{source})"), !unambiguous));
            }
        }
        for (source, name_ending) in names {
            patterns.push(SpokenPattern {
                id: entry.id,
                regex: js_regex(&format!(r"\b(?:{source})")),
                case_insensitive: true,
                name_ending,
            });
        }
    }
    patterns
}

static PATTERNS: LazyLock<Vec<SpokenPattern>> = LazyLock::new(build_patterns);

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// The source's `NAME_ENDING` lookahead, on lower-cased text after a match.
fn name_ending_follows(rest: &str) -> bool {
    let after_spaces = rest.trim_start_matches(js::is_space);
    // `\s*(?:[,.;:!?)–—-]|$)`
    match after_spaces.chars().next() {
        None => return true,
        Some(c) if ",.;:!?)–—-".contains(c) => return true,
        _ => {}
    }
    // `\s+(?:word)\b`
    if after_spaces.len() == rest.len() {
        return false;
    }
    NAME_WORDS.iter().chain(NAME_VERBS).any(|word| {
        after_spaces
            .strip_prefix(word)
            .is_some_and(|tail| !tail.chars().next().is_some_and(is_word_char))
    })
}

/// A translation named in text, with UTF-16 offsets as the source reports.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpokenTranslationMatch {
    pub id: String,
    pub start: usize,
    pub end: usize,
}

/// Every translation named in `text`, in spoken order. Overlapping names
/// resolve to the longest, so "New King James" is NKJV, not KJV.
pub fn find_spoken_translation_matches(text: &str) -> Vec<SpokenTranslationMatch> {
    let lower = text.to_ascii_lowercase();
    let mut found = Vec::new();
    for pattern in PATTERNS.iter() {
        let hay = if pattern.case_insensitive {
            lower.as_str()
        } else {
            text
        };
        let mut from = 0;
        while from <= hay.len() {
            let Some(m) = pattern.regex.find_at(hay, from) else {
                break;
            };
            if pattern.name_ending && !name_ending_follows(&hay[m.end()..]) {
                // Every source starts with an ASCII letter, so one byte on is
                // the next character, as JavaScript retries one unit on.
                from = m.start() + 1;
                continue;
            }
            found.push(SpokenTranslationMatch {
                id: pattern.id.to_string(),
                start: js::utf16_offset(text, m.start()),
                end: js::utf16_offset(text, m.end()),
            });
            from = m.end().max(m.start() + 1);
        }
    }
    found.sort_by(|a, b| {
        a.start
            .cmp(&b.start)
            .then((b.end - b.start).cmp(&(a.end - a.start)))
    });
    let mut kept: Vec<SpokenTranslationMatch> = Vec::new();
    for candidate in found {
        let overlapping = kept
            .iter()
            .position(|other| candidate.start < other.end && other.start < candidate.end);
        match overlapping {
            None => kept.push(candidate),
            Some(at) => {
                let other = &kept[at];
                if candidate.end - candidate.start > other.end - other.start {
                    kept[at] = candidate;
                }
            }
        }
    }
    kept.sort_by_key(|m| m.start);
    kept
}

/// Ids of the translations named in `text`, in spoken order, without repeats.
pub fn find_spoken_translations(text: &str) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for found in find_spoken_translation_matches(text) {
        if !ids.contains(&found.id) {
            ids.push(found.id);
        }
    }
    ids
}
