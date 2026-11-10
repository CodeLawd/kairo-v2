//! Shared lyric text normalization. From `src/main/services/lyrics/normalize.ts`.
//!
//! Every online provider hands back lyrics in its own shape; all of them
//! funnel through [`structure_lyrics`] into the section-marked format the
//! lyrics parser wants.

use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;
use unicode_normalization::UnicodeNormalization;

/// Marker for a section whose content should be discarded entirely.
pub const DROP_MARKER: &str = "[__drop__]";
const UNKNOWN_SECTION: &str = "Verse";
const SECTION_MERGE_THRESHOLD: f64 = 0.6;
const STANZA_FALLBACK_LINES: usize = 4;

fn section_alias(base: &str) -> Option<&'static str> {
    Some(match base {
        "verse" | "v" | "verso" | "estrofa" | "couplet" | "ijeide" => "Verse",
        "chorus" | "c" | "refrain" | "hook" | "coro" | "estribillo" | "refrao" | "egbe" => "Chorus",
        "pre-chorus" | "prechorus" | "pre chorus" | "pre-coro" | "precoro" | "pre-refrao"
        | "prerefrao" | "pre-refrain" | "prerefrain" => "Pre-Chorus",
        "post-chorus" | "postchorus" | "tag" | "vamp" | "turnaround" | "ad-lib" | "adlib"
        | "adlibs" => "Tag",
        "bridge" | "b" | "puente" | "ponte" | "pont" => "Bridge",
        "intro" | "introduction" | "introduccion" | "introducao" => "Intro",
        "outro" => "Outro",
        "ending" | "end" | "final" | "cierre" | "encerramento" | "fin" => "Ending",
        _ => return None,
    })
}

const DROPPED_SECTIONS: [&str; 10] = [
    "instrumental",
    "instrumentals",
    "interlude",
    "interludio",
    "interlude musical",
    "solo",
    "break",
    "breakdown",
    "musical",
    "intermedio",
];

static CUE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(instruments?\s+playing|instrumental(\s+break)?|end\.?|fade\s*out)$").unwrap()
});
static TONGUES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^speaking in tongues?\.?$").unwrap());
static NON_WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^\p{L}\p{N}\p{M}\s]").unwrap());
static WS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());
static HEADER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\[([^\]]{1,60})\]$").unwrap());
static ORDINAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(.*?)\s*(\d+)\s*$").unwrap());
static EMBED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d*Embed\s*$").unwrap());
static BLANK_RUNS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n{3,}").unwrap());
static BRACKETED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[\[({].+[\])}]$").unwrap());
static SECTION_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\[.+\]$").unwrap());

/// JavaScript `String.prototype.trim`.
pub(crate) fn trim(s: &str) -> &str {
    kairo_domain::js::trim(s)
}

/// Stage directions live transcriptions insert as lines.
pub fn is_performance_cue_line(line: &str) -> bool {
    let lower = trim(line).to_lowercase();
    !lower.is_empty() && (CUE.is_match(&lower) || TONGUES.is_match(&lower))
}

/// "Refrão" → "refrao": NFD, then combining marks U+0300–U+036F dropped.
pub fn fold_accents(value: &str) -> String {
    value
        .nfd()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
        .collect()
}

/// Lowercase, unaccented, punctuation-free fingerprint of a line.
pub fn canonical_text(value: &str) -> String {
    let folded = fold_accents(&value.to_lowercase());
    let spaced = NON_WORD.replace_all(&folded, " ");
    trim(&WS.replace_all(&spaced, " ")).to_owned()
}

/// One bracketed header to a label the parser accepts, or `None`.
pub fn normalize_section_header(line: &str) -> Option<String> {
    let inner = HEADER.captures(line)?.get(1)?.as_str();
    let head = trim(inner.split(':').next().unwrap_or(""));
    let ordinal = ORDINAL.captures(head);
    let base_raw = ordinal
        .as_ref()
        .map_or(head, |c| c.get(1).map_or("", |m| m.as_str()));
    let base = WS
        .replace_all(&fold_accents(&trim(base_raw).to_lowercase()), " ")
        .into_owned();
    if DROPPED_SECTIONS.contains(&base.as_str()) {
        return Some(DROP_MARKER.into());
    }
    let canonical = section_alias(&base).unwrap_or(UNKNOWN_SECTION);
    Some(match ordinal {
        Some(c) => format!("[{canonical} {}]", &c[2]),
        None => format!("[{canonical}]"),
    })
}

/// Provider text to the section-marked format `parseText` expects.
pub fn structure_lyrics(raw: &str) -> String {
    let text = raw
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('[', "\n[")
        .replace(']', "]\n");
    let mut out: Vec<String> = Vec::new();
    let mut saw_header = false;
    for raw_line in text.split('\n') {
        // The view counter Genius appends to the final line.
        let stripped = EMBED.replace(raw_line, "");
        let line = trim(&stripped);
        if line.is_empty() {
            out.push(String::new());
            continue;
        }
        if is_performance_cue_line(line) {
            continue;
        }
        if let Some(header) = normalize_section_header(line) {
            saw_header = true;
            out.push(String::new());
            out.push(header);
            continue;
        }
        out.push(line.to_owned());
    }
    let joined = out.join("\n");
    let collapsed = trim(&BLANK_RUNS.replace_all(&joined, "\n\n")).to_owned();
    if saw_header {
        return dedupe_sections(&collapsed);
    }
    if collapsed.contains("\n\n") {
        return collapsed.replace("\n\n", "\n\n\n");
    }
    let lyric_lines: Vec<&str> = collapsed
        .split('\n')
        .filter(|l| !trim(l).is_empty())
        .collect();
    if lyric_lines.len() <= STANZA_FALLBACK_LINES {
        return collapsed;
    }
    lyric_lines
        .chunks(STANZA_FALLBACK_LINES)
        .map(|c| c.join("\n"))
        .collect::<Vec<_>>()
        .join("\n\n\n")
}

/// Projectable lines out of structured or raw text.
pub fn extract_lyric_lines(text: &str) -> Vec<String> {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .split('\n')
        .map(|l| {
            l.trim_end_matches(kairo_domain::js::is_whitespace)
                .to_owned()
        })
        .filter(|line| {
            let t = trim(line);
            !(t.is_empty()
                || (BRACKETED.is_match(t) && t.encode_utf16().count() <= 40)
                || t == DROP_MARKER
                || is_performance_cue_line(t))
        })
        .collect()
}

struct Block {
    header: String,
    lines: Vec<String>,
}

/// Collapses repeats of the same section; drops empty and dropped sections.
pub fn dedupe_sections(text: &str) -> String {
    let mut blocks: Vec<Block> = Vec::new();
    let mut current: Option<Block> = None;
    for raw in text.split('\n') {
        let line = trim(raw);
        if SECTION_LINE.is_match(line) {
            if let Some(b) = current.take() {
                blocks.push(b);
            }
            current = Some(Block {
                header: line.into(),
                lines: Vec::new(),
            });
        } else if !line.is_empty() {
            current
                .get_or_insert_with(|| Block {
                    header: String::new(),
                    lines: Vec::new(),
                })
                .lines
                .push(line.into());
        }
    }
    if let Some(b) = current {
        blocks.push(b);
    }
    let mut kept: Vec<(Block, HashSet<String>)> = Vec::new();
    for block in blocks {
        if block.header == DROP_MARKER || block.lines.is_empty() {
            continue;
        }
        let canonical = canonical_line_set(&block.lines);
        if canonical.is_empty() {
            continue;
        }
        let twin = kept.iter_mut().find(|(k, set)| {
            k.header == block.header && overlap(set, &canonical) >= SECTION_MERGE_THRESHOLD
        });
        match twin {
            None => kept.push((block, canonical)),
            Some((k, set)) => {
                if block.lines.len() > k.lines.len() {
                    k.lines = block.lines;
                    *set = canonical;
                }
            }
        }
    }
    kept.iter()
        .map(|(b, _)| {
            if b.header.is_empty() {
                b.lines.join("\n")
            } else {
                format!("{}\n{}", b.header, b.lines.join("\n"))
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Share of the larger set's lines also in the smaller one.
fn overlap(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    let (large, small) = if a.len() >= b.len() { (a, b) } else { (b, a) };
    if large.is_empty() {
        return 0.0;
    }
    small.iter().filter(|l| large.contains(*l)).count() as f64 / large.len() as f64
}

fn canonical_line_set(lines: &[String]) -> HashSet<String> {
    lines
        .iter()
        .map(|l| canonical_text(l))
        .filter(|c| !c.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_text_folds_case_accents_and_punctuation() {
        assert_eq!(canonical_text("Èsé! èsé, ÈSÉ o"), "ese ese ese o");
        assert_eq!(canonical_text("  Refrão — 2 "), "refrao 2");
    }

    #[test]
    fn headers_map_to_supported_types() {
        assert_eq!(
            normalize_section_header("[Chorus: Some Artist]").as_deref(),
            Some("[Chorus]")
        );
        assert_eq!(
            normalize_section_header("[Verse 2: A & B]").as_deref(),
            Some("[Verse 2]")
        );
        assert_eq!(
            normalize_section_header("[Refrão]").as_deref(),
            Some("[Chorus]")
        );
        assert_eq!(
            normalize_section_header("[Instrumental]").as_deref(),
            Some(DROP_MARKER)
        );
        assert_eq!(
            normalize_section_header("[Spoken]").as_deref(),
            Some("[Verse]")
        );
        assert_eq!(normalize_section_header("Chorus"), None);
    }

    #[test]
    fn flat_text_is_chunked_into_stanzas() {
        let flat = (1..=9)
            .map(|n| format!("line {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(structure_lyrics(&flat).matches("\n\n\n").count(), 2);
        assert_eq!(structure_lyrics("a\nb"), "a\nb");
        assert_eq!(structure_lyrics("a\n\nb"), "a\n\n\nb");
    }

    #[test]
    fn repeated_choruses_collapse_and_the_fuller_take_wins() {
        let text =
            "[Chorus]\nEse o\nThank you\n\n[Verse 1]\nOne\n\n[Chorus]\nEse o\nThank you\nAmen";
        assert_eq!(
            structure_lyrics(text),
            "[Chorus]\nEse o\nThank you\nAmen\n\n[Verse 1]\nOne"
        );
    }
}
