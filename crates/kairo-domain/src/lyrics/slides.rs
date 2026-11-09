//! Splitting songs into on-screen slides. Port of `src/lib/lyrics-slides.ts`.
//!
//! Character limits count UTF-16 code units, as JavaScript's `.length` does.

use super::js_text::{self, has_text, len16, split_ws, trim_end};
use super::style::resolve_lyric_line_color;
use super::{LyricsSong, LyricsSongSection, SongPresentOptions};
use serde::{Deserialize, Serialize};

/// Default couplet size for a freshly imported song with no slide breaks.
pub const DEFAULT_IMPORT_SLIDE_LINES: f64 = 2.0;
pub const DEFAULT_MAX_CHARS_PER_LINE: f64 = 40.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricSlide {
    pub section_label: String,
    pub lines: Vec<String>,
    /// Parallel to `lines`: the resolved paint colour when set.
    pub line_colors: Vec<Option<String>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColoredLyricLine {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Index into the section's `lines` (before soft-wrap).
    pub source_line_index: usize,
}

/// Soft-wraps one lyric line at word boundaries, never splitting a word.
/// `max_chars` is a JavaScript number, so `NaN` puts every word on its own row.
pub fn wrap_line(line: &str, max_chars: f64) -> Vec<String> {
    if len16(line) as f64 <= max_chars {
        return vec![line.to_string()];
    }
    let mut wrapped = Vec::new();
    let mut current = String::new();
    let mut current_len = 0;
    for word in split_ws(line) {
        let word_len = len16(word);
        let combined_len = current_len + 1 + word_len;
        if current.is_empty() {
            current.push_str(word);
            current_len = word_len;
        } else if combined_len as f64 <= max_chars {
            current.push(' ');
            current.push_str(word);
            current_len = combined_len;
        } else {
            wrapped.push(std::mem::replace(&mut current, word.to_string()));
            current_len = word_len;
        }
    }
    if !current.is_empty() {
        wrapped.push(current);
    }
    if wrapped.is_empty() {
        vec![line.to_string()]
    } else {
        wrapped
    }
}

/// Keeps a single blank line as a slide break, drops leading and trailing
/// blanks, and collapses runs of blanks.
pub fn preserve_slide_breaks<S: AsRef<str>>(raw_lines: &[S]) -> Vec<String> {
    let mut out = Vec::new();
    let mut pending_break = false;
    for raw in raw_lines {
        let line = trim_end(raw.as_ref());
        if !has_text(line) {
            if !out.is_empty() {
                pending_break = true;
            }
            continue;
        }
        if pending_break {
            out.push(String::new());
            pending_break = false;
        }
        out.push(line.to_string());
    }
    out
}

/// Inserts a blank line every `every` lyric lines (at least 1, floored).
pub fn insert_slide_breaks(text: &str, every: f64) -> String {
    let size = js_text::max(1.0, every.floor());
    let lines: Vec<&str> = text
        .split('\n')
        .map(trim_end)
        .filter(|l| has_text(l))
        .collect();
    let mut out: Vec<&str> = Vec::with_capacity(lines.len() * 2);
    for (i, line) in lines.iter().enumerate() {
        if i > 0 && (i as f64) % size == 0.0 {
            out.push("");
        }
        out.push(line);
    }
    out.join("\n")
}

/// Import-time helper: keeps a section's own blank-line breaks, otherwise
/// breaks it into groups of `every` lines (default 2).
pub fn apply_import_slide_breaks<S: AsRef<str>>(lines: &[S], every: Option<f64>) -> Vec<String> {
    if lines.iter().any(|l| !has_text(l.as_ref())) {
        return preserve_slide_breaks(lines);
    }
    let joined = lines
        .iter()
        .map(AsRef::as_ref)
        .collect::<Vec<_>>()
        .join("\n");
    let broken = insert_slide_breaks(&joined, every.unwrap_or(DEFAULT_IMPORT_SLIDE_LINES));
    preserve_slide_breaks(&broken.split('\n').collect::<Vec<_>>())
}

/// Splits a section into slides. Blank lines are the only slide boundaries;
/// long lines soft-wrap within a slide. Never returns an empty list: no
/// lyrics give one empty slide.
pub fn split_into_slide_chunks(lines: &[String], max_chars: Option<f64>) -> Vec<Vec<String>> {
    split_into_colored_slide_chunks(lines, None, "", max_chars)
        .into_iter()
        .map(|chunk| chunk.into_iter().map(|l| l.text).collect())
        .collect()
}

/// Like [`split_into_slide_chunks`], keeping gloss and override colours
/// through soft-wrap.
pub fn split_into_colored_slide_chunks(
    lines: &[String],
    line_colors: Option<&[Option<String>]>,
    gloss_color: &str,
    max_chars: Option<f64>,
) -> Vec<Vec<ColoredLyricLine>> {
    let max_chars = max_chars.unwrap_or(DEFAULT_MAX_CHARS_PER_LINE);
    let mut chunks = Vec::new();
    let mut current = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if !has_text(line) {
            if !current.is_empty() {
                chunks.push(std::mem::take(&mut current));
            }
            continue;
        }
        let override_color = line_colors
            .and_then(|c| c.get(i))
            .and_then(|c| c.as_deref());
        let color = resolve_lyric_line_color(line, gloss_color, override_color);
        for part in wrap_line(line, max_chars) {
            current.push(ColoredLyricLine {
                text: part,
                color: color.clone(),
                source_line_index: i,
            });
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    if chunks.is_empty() {
        chunks.push(Vec::new());
    }
    chunks
}

/// Every slide of a song, labelled `Label (n/total)` when a section has more
/// than one.
pub fn build_slides(
    song: &LyricsSong,
    opts: &SongPresentOptions,
    gloss_color: Option<&str>,
) -> Vec<LyricSlide> {
    let gloss_color = gloss_color.unwrap_or("");
    let mut slides = Vec::new();
    for section in &song.sections {
        let chunks = split_into_colored_slide_chunks(
            &section.lines,
            section.line_colors.as_deref(),
            gloss_color,
            opts.max_chars_per_line,
        );
        let total = chunks.len();
        for (i, chunk) in chunks.into_iter().enumerate() {
            let section_label = if total == 1 {
                section.label.clone()
            } else {
                format!("{} ({}/{})", section.label, i + 1, total)
            };
            let (lines, line_colors) = chunk.into_iter().map(|l| (l.text, l.color)).unzip();
            slides.push(LyricSlide {
                section_label,
                lines,
                line_colors,
            });
        }
    }
    slides
}

pub fn section_slide_chunks(
    section: &LyricsSongSection,
    opts: &SongPresentOptions,
) -> Vec<Vec<String>> {
    split_into_slide_chunks(&section.lines, opts.max_chars_per_line)
}

pub fn section_colored_slide_chunks(
    section: &LyricsSongSection,
    gloss_color: &str,
    opts: &SongPresentOptions,
) -> Vec<Vec<ColoredLyricLine>> {
    split_into_colored_slide_chunks(
        &section.lines,
        section.line_colors.as_deref(),
        gloss_color,
        opts.max_chars_per_line,
    )
}
