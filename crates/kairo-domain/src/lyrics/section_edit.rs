//! Section editing helpers. Port of `src/lib/lyrics-section-edit.ts`.

use super::LyricsSectionType;
use super::js_text::{WS, has_text, is_ws, len16, normalize_newlines, re, trim};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// A section as the song editor holds it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditableSection {
    #[serde(rename = "_key")]
    pub key: String,
    #[serde(rename = "type")]
    pub kind: LyricsSectionType,
    pub label: String,
    pub lines_text: String,
}

/// A section read back from `[Label]`-marked text.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MarkedSection {
    #[serde(rename = "type")]
    pub kind: LyricsSectionType,
    pub label: String,
    pub lines: Vec<String>,
}

/// `TYPE_LABEL`, in its TypeScript key order.
pub(crate) const TYPE_LABEL: [(LyricsSectionType, &str); 8] = [
    (LyricsSectionType::Verse, "Verse"),
    (LyricsSectionType::Chorus, "Chorus"),
    (LyricsSectionType::Bridge, "Bridge"),
    (LyricsSectionType::PreChorus, "Pre-Chorus"),
    (LyricsSectionType::Tag, "Tag"),
    (LyricsSectionType::Intro, "Intro"),
    (LyricsSectionType::Outro, "Outro"),
    (LyricsSectionType::Ending, "Ending"),
];

fn type_label(kind: LyricsSectionType) -> &'static str {
    TYPE_LABEL
        .iter()
        .find(|(k, _)| *k == kind)
        .map_or("Verse", |(_, label)| label)
}

fn type_value(kind: LyricsSectionType) -> &'static str {
    match kind {
        LyricsSectionType::Verse => "verse",
        LyricsSectionType::Chorus => "chorus",
        LyricsSectionType::Bridge => "bridge",
        LyricsSectionType::PreChorus => "pre-chorus",
        LyricsSectionType::Tag => "tag",
        LyricsSectionType::Intro => "intro",
        LyricsSectionType::Outro => "outro",
        LyricsSectionType::Ending => "ending",
    }
}

/// `Chorus`, or `Chorus 2` for an index above 1.
pub fn default_label_for_type(kind: LyricsSectionType, index: usize) -> String {
    let base = type_label(kind);
    if index > 1 {
        format!("{base} {index}")
    } else {
        base.to_string()
    }
}

/// Splits section text at the line under the cursor; that line and the rest
/// become `after`. `cursor` is a UTF-16 offset, as a text field reports it.
pub fn split_text_at_cursor(text: &str, cursor: f64) -> (String, String) {
    let units: Vec<u16> = text.encode_utf16().collect();
    let safe = super::js_text::max(0.0, super::js_text::min(cursor, units.len() as f64));
    let from = super::js_text::max(0.0, safe - 1.0);
    // `lastIndexOf(search, NaN)` searches the whole string.
    let from = if from.is_nan() {
        units.len()
    } else {
        (from.trunc() as usize).min(units.len())
    };
    let line_start = units[..units.len().min(from + 1)]
        .iter()
        .rposition(|&u| u == u16::from(b'\n'))
        .map_or(0, |i| i + 1);
    let split = super::js_text::byte_offset(text, line_start);
    let before = text[..split].trim_end_matches('\n').to_string();
    let after = text[split..].trim_start_matches('\n').to_string();
    (before, after)
}

static JAMMED: LazyLock<Regex> = LazyLock::new(|| re(&format!("{WS}{{2,}}")));

/// Breaks lines that jam phrases together with two or more spaces into one
/// row each. Blank lines are kept.
pub fn expand_jammed_lines(text: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for line in text.split('\n') {
        let trimmed = line.trim_end_matches(is_ws);
        if !has_text(trimmed) {
            out.push(String::new());
        } else if JAMMED.is_match(trimmed) {
            out.extend(
                JAMMED
                    .split(trimmed)
                    .map(trim)
                    .filter(|p| !p.is_empty())
                    .map(str::to_string),
            );
        } else {
            out.push(trimmed.to_string());
        }
    }
    out.join("\n")
}

/// Inserts a new section after `index`, moving the text from the cursor line
/// onward into it. `None` when the index is out of range or either half would
/// be blank.
pub fn split_section_at_cursor(
    sections: &[EditableSection],
    index: usize,
    cursor: f64,
    make_key: impl FnOnce() -> String,
    new_type: Option<LyricsSectionType>,
) -> Option<Vec<EditableSection>> {
    let new_type = new_type.unwrap_or(LyricsSectionType::Verse);
    let current = sections.get(index)?;
    let (before, after) = split_text_at_cursor(&current.lines_text, cursor);
    if !has_text(&after) || !has_text(&before) {
        return None;
    }
    let type_count = sections.iter().filter(|s| s.kind == new_type).count() + 1;
    let next = EditableSection {
        key: make_key(),
        kind: new_type,
        label: default_label_for_type(new_type, type_count),
        lines_text: after,
    };
    let mut updated = sections.to_vec();
    updated[index].lines_text = before;
    updated.insert(index + 1, next);
    Some(updated)
}

/// `/^\[([^\]]{1,40})\]$/` on an already trimmed line; the 40 counts UTF-16
/// code units.
pub(crate) fn bracket_label(trimmed: &str) -> Option<&str> {
    let inner = trimmed.strip_prefix('[')?.strip_suffix(']')?;
    let len = len16(inner);
    ((1..=40).contains(&len) && !inner.contains(']')).then_some(inner)
}

/// Reads `[Label]`-marked text back into sections. Lines before the first
/// marker become a verse.
pub fn parse_marked_sections(text: &str) -> Vec<MarkedSection> {
    let mut sections: Vec<MarkedSection> = Vec::new();
    let mut label: Option<String> = None;
    let mut lines: Vec<String> = Vec::new();

    let flush = |sections: &mut Vec<MarkedSection>, label: &Option<String>, lines: &[String]| {
        let mut trimmed = lines;
        while trimmed.last().is_some_and(|l| !has_text(l)) {
            trimmed = &trimmed[..trimmed.len() - 1];
        }
        while trimmed.first().is_some_and(|l| !has_text(l)) {
            trimmed = &trimmed[1..];
        }
        if trimmed.is_empty() && label.is_none() {
            return;
        }
        let resolved = label.clone().unwrap_or_else(|| {
            default_label_for_type(LyricsSectionType::Verse, sections.len() + 1)
        });
        sections.push(MarkedSection {
            kind: section_type_from_label(&resolved),
            label: resolved,
            lines: trimmed.to_vec(),
        });
    };

    for raw in normalize_newlines(text).split('\n') {
        if let Some(marker) = bracket_label(trim(raw)) {
            flush(&mut sections, &label, &lines);
            label = Some(trim(marker).to_string());
            lines.clear();
            continue;
        }
        lines.push(raw.to_string());
    }
    flush(&mut sections, &label, &lines);
    sections
}

static TRAILING_NUMBER: LazyLock<Regex> = LazyLock::new(|| re(&format!("{WS}*[0-9]+{WS}*$")));

/// `Pre-Chorus 2` → pre-chorus. Anything unrecognised is a verse.
pub fn section_type_from_label(label: &str) -> LyricsSectionType {
    let lower = label.to_lowercase();
    let base = TRAILING_NUMBER.replace(&lower, "");
    let base = trim(&base);
    TYPE_LABEL
        .iter()
        .find(|(kind, text)| text.to_lowercase() == base || type_value(*kind) == base)
        .map_or(LyricsSectionType::Verse, |(kind, _)| *kind)
}
