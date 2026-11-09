//! Song text: `[Label]` lines start sections, blank lines break slides.
//! Port of `src/lib/lyrics-reflow.ts`.

use super::LyricsSectionType;
use super::js_text::{has_text, normalize_newlines, trim, trim_end};
use super::section_edit::{bracket_label, default_label_for_type, section_type_from_label};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReflowSlide {
    /// Line index in the text of this slide's first lyric line.
    pub start_line: usize,
    pub lines: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReflowSection {
    pub label: String,
    #[serde(rename = "type")]
    pub kind: LyricsSectionType,
    /// Line index of the `[Label]` line; `None` for lyrics before any header.
    pub header_line: Option<usize>,
    /// The body as stored: lines with single blank lines between slides.
    pub lines_text: String,
    pub slides: Vec<ReflowSlide>,
}

/// `[Chorus]` → `Chorus`; anything else, including `[ ]`, → `None`.
pub fn reflow_header_label(line: &str) -> Option<String> {
    let inner = trim(bracket_label(trim(line))?);
    (!inner.is_empty()).then(|| inner.to_string())
}

struct Pending<'a> {
    label: Option<String>,
    header_line: Option<usize>,
    body: Vec<(&'a str, usize)>,
}

fn flush(current: Option<Pending<'_>>, sections: &mut Vec<ReflowSection>) {
    let Some(current) = current else {
        return;
    };
    let mut body = &current.body[..];
    while body.first().is_some_and(|(t, _)| !has_text(t)) {
        body = &body[1..];
    }
    while body.last().is_some_and(|(t, _)| !has_text(t)) {
        body = &body[..body.len() - 1];
    }
    // Lyrics typed before any header still count, as the first verse.
    if body.is_empty() && current.label.is_none() {
        return;
    }
    let label = current.label.unwrap_or_else(|| {
        let verses = sections
            .iter()
            .filter(|s| s.kind == LyricsSectionType::Verse)
            .count();
        format!("Verse {}", verses + 1)
    });
    let mut slides = Vec::new();
    let mut stored: Vec<&str> = Vec::new();
    let mut slide: Option<ReflowSlide> = None;
    for &(raw, line) in body {
        let lyric = trim_end(raw);
        if !has_text(lyric) {
            if let Some(done) = slide.take() {
                slides.push(done);
                stored.push("");
            }
            continue;
        }
        slide
            .get_or_insert_with(|| ReflowSlide {
                start_line: line,
                lines: Vec::new(),
            })
            .lines
            .push(lyric.to_string());
        stored.push(lyric);
    }
    if let Some(done) = slide {
        slides.push(done);
    }
    sections.push(ReflowSection {
        kind: section_type_from_label(&label),
        label,
        header_line: current.header_line,
        lines_text: stored.join("\n"),
        slides,
    });
}

/// Parses song text into sections and slides.
pub fn parse_reflow(text: &str) -> Vec<ReflowSection> {
    let text = normalize_newlines(text);
    let mut sections = Vec::new();
    let mut current: Option<Pending<'_>> = None;
    for (index, raw) in text.split('\n').enumerate() {
        if let Some(label) = reflow_header_label(raw) {
            flush(current.take(), &mut sections);
            current = Some(Pending {
                label: Some(label),
                header_line: Some(index),
                body: Vec::new(),
            });
            continue;
        }
        current
            .get_or_insert_with(|| Pending {
                label: None,
                header_line: None,
                body: Vec::new(),
            })
            .body
            .push((raw, index));
    }
    flush(current, &mut sections);
    sections
}

/// The next free label for a new section of `kind`. Verses are always
/// numbered (`Verse 1`); the rest start bare (`Chorus`, then `Chorus 2`).
pub fn next_section_label<S: AsRef<str>>(labels: &[S], kind: LyricsSectionType) -> String {
    let taken: Vec<String> = labels.iter().map(|l| l.as_ref().to_lowercase()).collect();
    for n in 1..100 {
        let label = if kind == LyricsSectionType::Verse {
            format!("Verse {n}")
        } else {
            default_label_for_type(kind, n)
        };
        if !taken.contains(&label.to_lowercase()) {
            return label;
        }
    }
    default_label_for_type(kind, 1)
}
