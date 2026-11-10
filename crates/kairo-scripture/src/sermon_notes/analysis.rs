//! `analyzeScriptureReferences`: every reference written in sermon notes,
//! with list continuations ("Acts 2:14, 41"), ranges and translation tags.
//!
//! The patterns are built from the same tables as the TypeScript: book names,
//! abbreviations and aliases from `BOOKS` in `bible-db.ts`, and translation
//! ids and aliases from the translation registry. JavaScript semantics kept:
//! `\b` and `\d` are ASCII, `\s` is JavaScript's set, and the `i` flag folds
//! ASCII letters only (each letter becomes a two-letter class).

use kairo_domain::js::is_whitespace;
use kairo_domain::scripture::translations::build_translation_alias_map;
use kairo_domain::scripture::{SermonNotesAnalysis, SermonPlanDraftItem, SermonReferenceMatch};
use regex::Regex;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::LazyLock;

/// JavaScript's `\s` set as a `regex` class body.
const SPACE_CLASS: &str = r"\t\n\x0B\x0C\r \u{A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}";

/// `value.replace(/\s+/g, ' ')`.
fn collapse_spaces(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut space = false;
    for c in value.chars() {
        if is_whitespace(c) {
            if !space {
                out.push(' ');
            }
            space = true;
        } else {
            out.push(c);
            space = false;
        }
    }
    out
}

struct Patterns {
    reference: Regex,
    continuation: fancy_regex::Regex,
    book_start: fancy_regex::Regex,
    split_translations: Regex,
    /// Alias without spaces, lower case, to the canonical name. The first
    /// alias in longest-first order wins, as `Array.find` does.
    book_lookup: HashMap<String, String>,
    translation_aliases: BTreeMap<String, &'static str>,
}

#[derive(serde::Deserialize)]
struct BookSeed {
    name: String,
    abbr: String,
    aliases: Vec<String>,
}

/// `escapeRegex`, then each ASCII letter as `[xX]` for the `i` flag.
fn literal(value: &str) -> String {
    let mut out = String::new();
    for c in value.chars() {
        if c.is_ascii_alphabetic() {
            out.push('[');
            out.push(c.to_ascii_lowercase());
            out.push(c.to_ascii_uppercase());
            out.push(']');
        } else if c == ' ' {
            out.push_str("\\x20");
        } else {
            out.push_str(&regex::escape(&c.to_string()));
        }
    }
    out
}

use kairo_domain::js::utf16_len;

static PATTERNS: LazyLock<Patterns> = LazyLock::new(|| {
    let translation_aliases = build_translation_alias_map();
    let mut translation_keys: Vec<&String> = translation_aliases.keys().collect();
    translation_keys.sort_by_key(|k| std::cmp::Reverse(utf16_len(k)));
    let translation = translation_keys
        .iter()
        .map(|k| literal(k))
        .collect::<Vec<_>>()
        .join("|");

    let seeds: Vec<BookSeed> =
        serde_json::from_str(kairo_store::db::BOOKS_JSON).expect("bundled books table");
    let mut book_aliases: Vec<(String, String)> = seeds
        .iter()
        .flat_map(|book| {
            std::iter::once(&book.name)
                .chain(std::iter::once(&book.abbr))
                .chain(book.aliases.iter())
                .map(|alias| (alias.clone(), book.name.clone()))
        })
        .collect();
    book_aliases.sort_by_key(|(alias, _)| std::cmp::Reverse(utf16_len(alias)));
    let book = book_aliases
        .iter()
        .map(|(alias, _)| literal(alias))
        .collect::<Vec<_>>()
        .join("|");

    let s = format!("[{SPACE_CLASS}]");
    let b = r"(?-u:\b)";
    let trailing = format!("(?:{translation})(?:{s}*[,/]{s}*(?:{translation}))*");
    let reference = format!(
        "(?:{b}({translation}){b}[{SPACE_CLASS}:,\\-]*)?{b}({book}){s}+([0-9]{{1,3}})(?:{s}*:{s}*|{s}+)([0-9]{{1,3}})(?:{s}*[-–—]{s}*([0-9]{{1,3}}))?(?:{s}*[(\\[]?({trailing})[)\\]]?)?"
    );
    let continuation = format!(
        "^{s}*([,;]){s}*(?:([0-9]{{1,3}}){s}*:{s}*)?([0-9]{{1,3}})(?:{s}*[-–—]{s}*([0-9]{{1,3}}))?(?![0-9:])"
    );
    let book_start = format!("^(?:{book})(?![a-zA-Z])");
    let mut book_lookup = HashMap::new();
    for (alias, name) in &book_aliases {
        book_lookup
            .entry(strip_spaces_lower(alias))
            .or_insert_with(|| name.clone());
    }
    Patterns {
        reference: Regex::new(&reference).expect("reference pattern"),
        continuation: fancy_regex::Regex::new(&continuation).expect("continuation pattern"),
        book_start: fancy_regex::Regex::new(&book_start).expect("book start pattern"),
        split_translations: Regex::new(&format!("{s}*[,/]{s}*")).expect("split pattern"),
        book_lookup,
        translation_aliases,
    }
});

fn strip_spaces_lower(value: &str) -> String {
    value
        .chars()
        .filter(|c| !is_whitespace(*c))
        .flat_map(char::to_lowercase)
        .collect()
}

fn canonical_book(p: &Patterns, value: &str) -> String {
    p.book_lookup
        .get(&strip_spaces_lower(value))
        .map_or_else(|| value.to_owned(), Clone::clone)
}

fn normalize_translation(p: &Patterns, value: Option<&str>, fallback: &str) -> String {
    let Some(value) = value.filter(|v| !v.is_empty()) else {
        return fallback.to_owned();
    };
    let key = collapse_spaces(&value.to_uppercase());
    p.translation_aliases
        .get(&key)
        .map_or_else(|| fallback.to_owned(), |id| (*id).to_owned())
}

/// Byte offset to UTF-16 offset, walking forward only.
struct Utf16Cursor<'a> {
    text: &'a str,
    byte: usize,
    units: usize,
}

impl Utf16Cursor<'_> {
    fn at(&mut self, byte: usize) -> usize {
        if byte < self.byte {
            self.byte = 0;
            self.units = 0;
        }
        self.units += utf16_len(&self.text[self.byte..byte]);
        self.byte = byte;
        self.units
    }
}

fn now_ms() -> u128 {
    kairo_domain::js::now_ms() as u128
}

pub fn extract_scripture_references(
    text: &str,
    default_translation: &str,
) -> Vec<SermonPlanDraftItem> {
    analyze_scripture_references(text, default_translation).items
}

pub fn analyze_scripture_references(text: &str, default_translation: &str) -> SermonNotesAnalysis {
    let p = &*PATTERNS;
    let mut items: Vec<SermonPlanDraftItem> = Vec::new();
    let mut matches: Vec<SermonReferenceMatch> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut cursor = Utf16Cursor {
        text,
        byte: 0,
        units: 0,
    };
    let stamp = now_ms();

    let mut add =
        |items: &mut Vec<SermonPlanDraftItem>, reference: &str, translations: &[String]| {
            for translation in translations {
                if seen.insert(format!("{reference}|{translation}")) {
                    items.push(SermonPlanDraftItem {
                        id: format!("verse-{}-{stamp}", items.len() + 1),
                        reference: reference.to_owned(),
                        translation: translation.clone(),
                    });
                }
            }
        };

    let mut pos = 0;
    while pos <= text.len() {
        let Some(caps) = p.reference.captures_at(text, pos) else {
            break;
        };
        let whole = caps.get(0).expect("group 0");
        let book = canonical_book(p, &caps[2]);
        let range = caps
            .get(5)
            .map_or(String::new(), |m| format!("–{}", m.as_str()));
        let reference = format!("{book} {}:{}{range}", &caps[3], &caps[4]);
        let translations: Vec<String> = match caps.get(6) {
            Some(trailing) => p
                .split_translations
                .split(trailing.as_str())
                .map(|v| normalize_translation(p, Some(v), default_translation))
                .collect(),
            None => vec![normalize_translation(
                p,
                caps.get(1).map(|m| m.as_str()),
                default_translation,
            )],
        };
        let start = cursor.at(whole.start());
        let end = cursor.at(whole.end());
        matches.push(SermonReferenceMatch {
            start: start as f64,
            end: end as f64,
            text: whole.as_str().to_owned(),
            reference: reference.clone(),
            translations: translations.clone(),
        });
        add(&mut items, &reference, &translations);

        let mut chapter = caps[3].to_owned();
        let mut at = whole.end();
        while let Ok(Some(more)) = p.continuation.captures(&text[at..]) {
            let all = more.get(0).expect("group 0");
            let whole_text = all.as_str();
            let Some(digit) = whole_text.find(|c: char| c.is_ascii_digit()) else {
                break;
            };
            let number_start = at + digit;
            if matches!(p.book_start.is_match(&text[number_start..]), Ok(true)) {
                break;
            }
            let separator = more.get(1).map_or("", |m| m.as_str());
            let next_chapter = more.get(2).map(|m| m.as_str());
            if separator == ";" && next_chapter.is_none() {
                break;
            }
            if let Some(c) = next_chapter {
                chapter = c.to_owned();
            }
            let verse = more.get(3).map_or("", |m| m.as_str());
            let end_part = more
                .get(4)
                .map_or(String::new(), |m| format!("–{}", m.as_str()));
            let continued = format!("{book} {chapter}:{verse}{end_part}");
            let stop = at + all.end();
            let start = cursor.at(number_start);
            let end = cursor.at(stop);
            matches.push(SermonReferenceMatch {
                start: start as f64,
                end: end as f64,
                text: text[number_start..stop].to_owned(),
                reference: continued.clone(),
                translations: translations.clone(),
            });
            add(&mut items, &continued, &translations);
            at = stop;
        }
        pos = whole.end().max(at);
        if whole.is_empty() {
            pos += text[pos..].chars().next().map_or(1, char::len_utf8);
        }
    }
    SermonNotesAnalysis { matches, items }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refs(text: &str) -> Vec<(String, String)> {
        analyze_scripture_references(text, "KJV")
            .items
            .into_iter()
            .map(|i| (i.reference, i.translation))
            .collect()
    }

    #[test]
    fn continuations_ranges_and_translations() {
        assert_eq!(
            refs("Read Acts 2:14, 41 and Romans 8:28-30 (NIV); 9:1"),
            vec![
                ("Acts 2:14".into(), "KJV".into()),
                ("Acts 2:41".into(), "KJV".into()),
                ("Romans 8:28–30".into(), "NIV".into()),
                ("Romans 9:1".into(), "NIV".into()),
            ]
        );
        assert_eq!(refs("1 Corinthians 13:4, 2 Peter 1:3").len(), 2);
        assert_eq!(
            refs("Psalm 23:1; 4 more"),
            vec![("Psalms 23:1".into(), "KJV".into())]
        );
    }

    #[test]
    fn offsets_are_utf16() {
        let a = analyze_scripture_references("😀 John 3:16", "KJV");
        assert_eq!(a.matches[0].start, 3.0);
        assert_eq!(a.matches[0].end, 12.0);
    }
}
