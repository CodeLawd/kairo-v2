//! Bilingual lyric glosses: the original line, with English under it in
//! parentheses. Port of `src/lib/lyrics-translate.ts`.

use super::js_text::{
    WS, collapse_ws, has_text, is_line_terminator, is_ws, len16, re, trim, trim_end,
};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;
use unicode_normalization::UnicodeNormalization;

/// Gloss pairs harvested from lyric-site text, keyed by [`gloss_match_key`],
/// in the order they were found. TypeScript uses a `Map`, whose iteration
/// order is insertion order; lookups depend on it.
pub type GlossPairs = Vec<(String, String)>;

/// `/^<open>.+<close>$/` on the trimmed line: `.` excludes line terminators.
fn wrapped_in(line: &str, open: char, close: char) -> bool {
    let t = trim(line);
    t.strip_prefix(open)
        .and_then(|rest| rest.strip_suffix(close))
        .is_some_and(|inner| !inner.is_empty() && !inner.chars().any(is_line_terminator))
}

/// A gloss line: `(…)`.
pub fn is_gloss_line(line: &str) -> bool {
    wrapped_in(line, '(', ')')
}

/// A section marker line: `[…]`.
pub fn is_section_marker_line(line: &str) -> bool {
    wrapped_in(line, '[', ']')
}

/// Lines that should be sent to the translator.
pub fn should_translate_line(line: &str) -> bool {
    let trimmed = trim(line);
    !trimmed.is_empty() && !is_gloss_line(trimmed) && !is_section_marker_line(trimmed)
}

/// Wraps translation text as `(gloss)`, stripping outer parentheses the
/// translator added. Empty text becomes `(…)`.
pub fn format_gloss(text: &str) -> String {
    let mut body = trim(text);
    while body.starts_with('(') && body.ends_with(')') && len16(body) >= 2 {
        body = trim(&body[1..body.len() - 1]);
    }
    format!("({})", if body.is_empty() { "…" } else { body })
}

/// Inserts or replaces gloss lines under the given source-line indices. An
/// existing `(…)` under a line is replaced rather than stacked.
pub fn apply_line_glosses(lines: &[String], glosses: &HashMap<usize, String>) -> Vec<String> {
    let mut skip = HashSet::new();
    let mut out = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        if skip.contains(&i) {
            continue;
        }
        out.push(line.clone());
        let Some(gloss) = glosses.get(&i) else {
            continue;
        };
        if i + 1 < lines.len() && is_gloss_line(&lines[i + 1]) {
            skip.insert(i + 1);
        }
        out.push(format_gloss(gloss));
    }
    out
}

/// Removes gloss lines, leaving the original language.
pub fn strip_line_glosses(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .filter(|l| !is_gloss_line(l))
        .cloned()
        .collect()
}

/// Whether any section has a gloss line.
pub fn sections_have_glosses<'a>(sections: impl IntoIterator<Item = &'a [String]>) -> bool {
    sections
        .into_iter()
        .any(|lines| lines.iter().any(|l| is_gloss_line(l)))
}

/// Indices of lines that need translation: not blank, not a section marker,
/// not already a gloss.
pub fn collect_translatable_indices(lines: &[String]) -> Vec<usize> {
    (0..lines.len())
        .filter(|&i| should_translate_line(&lines[i]))
        .collect()
}

/// True when the detected source language is English.
pub fn is_english_language_code(code: Option<&str>) -> bool {
    code.is_some_and(|c| !c.is_empty() && trim(c).to_lowercase().starts_with("en"))
}

static JUNK_GLOSS: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"^(?i-u:refrain|repeat|chorus|verse|bridge|share on|opens in|email|print|languages?|download|lyrics|mp3|feat|ft\.?|x[0-9]+)(?-u:\b)",
    )
});
static OPENS_IN_NEW_WINDOW: LazyLock<Regex> = LazyLock::new(|| re(r"(?i-u:opens in new window)"));

const COMMON_ENGLISH: &[&str] = &[
    "i",
    "a",
    "an",
    "the",
    "and",
    "or",
    "to",
    "of",
    "in",
    "on",
    "for",
    "is",
    "are",
    "am",
    "be",
    "my",
    "me",
    "you",
    "he",
    "she",
    "we",
    "they",
    "it",
    "not",
    "no",
    "yes",
    "oh",
    "see",
    "jesus",
    "power",
    "belongs",
    "blood",
    "speaks",
    "god",
    "with",
    "alone",
    "christ",
    "mark",
    "cannot",
    "defeated",
    "identity",
    "hosanna",
    "hosannah",
    "amen",
    "lord",
    "holy",
    "spirit",
    "praise",
    "glory",
    "hallelujah",
    "thank",
    "your",
];

/// Site chrome and section labels that must never become glosses.
pub fn is_junk_gloss(gloss: &str) -> bool {
    let t = trim(gloss);
    t.is_empty() || JUNK_GLOSS.is_match(t) || OPENS_IN_NEW_WINDOW.is_match(t)
}

/// Heuristic: the line is already English worship text. Short Latin lines
/// made mostly (75%) of common English words count.
pub fn is_likely_english_lyric(line: &str) -> bool {
    let key = gloss_match_key(line);
    if key.is_empty() {
        return false;
    }
    if line.chars().any(|c| c > '\u{024F}') {
        return false;
    }
    let words: Vec<&str> = key.split(' ').filter(|w| !w.is_empty()).collect();
    if words.is_empty() {
        return false;
    }
    let hits = words.iter().filter(|w| COMMON_ENGLISH.contains(w)).count();
    hits as f64 / words.len() as f64 >= 0.75
}

/// Skip glossing when the source was English, the gloss is junk, or the
/// translation is the same text.
pub fn should_apply_gloss(source: &str, translated: &str, detected_language: Option<&str>) -> bool {
    !is_english_language_code(detected_language)
        && !is_likely_english_lyric(source)
        && !is_junk_gloss(translated)
        && gloss_match_key(source) != gloss_match_key(translated)
}

/// Loose key for matching lyric lines across sites and punctuation:
/// unaccented, lower case, ASCII letters and digits separated by one space.
pub fn gloss_match_key(line: &str) -> String {
    let stripped: String = line
        .nfkd()
        .filter(|c| !('\u{0300}'..='\u{036F}').contains(c))
        .collect();
    let mapped: String = stripped
        .to_lowercase()
        .chars()
        .filter(|c| !matches!(c, '\'' | '`' | '´'))
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || is_ws(c) {
                c
            } else {
                ' '
            }
        })
        .collect();
    trim(&collapse_ws(&mapped)).to_string()
}

/// Collapses doubled characters in each word (`oguun` → `ogun`). TypeScript
/// uses `/(.)\1+/g`; keys are ASCII, so this needs no backreference engine.
fn normalize_gloss_tokens(key: &str) -> Vec<String> {
    key.split(' ')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut out = String::with_capacity(w.len());
            let mut last = None;
            for c in w.chars() {
                if last != Some(c) {
                    out.push(c);
                }
                last = Some(c);
            }
            out
        })
        .collect()
}

/// How well a harvested key matches a slide line's key (1 = exact).
fn gloss_key_score(key: &str, candidate: &str) -> f64 {
    if key.is_empty() || candidate.is_empty() {
        return 0.0;
    }
    if key == candidate {
        return 1.0;
    }
    let (shorter, longer) = if key.len() <= candidate.len() {
        (key, candidate)
    } else {
        (candidate, key)
    };
    if longer.starts_with(&format!("{shorter} ")) {
        return shorter.len() as f64 / longer.len() as f64;
    }
    if format!(" {longer} ").contains(&format!(" {shorter} ")) {
        return shorter.len() as f64 / longer.len() as f64;
    }
    let a = normalize_gloss_tokens(key);
    let b = normalize_gloss_tokens(candidate);
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    if a.join(" ") == b.join(" ") {
        return 0.95;
    }
    if a.concat() == b.concat() {
        return 0.92;
    }
    let set_a: HashSet<&String> = a.iter().collect();
    let set_b: HashSet<&String> = b.iter().collect();
    let inter = set_a.iter().filter(|t| set_b.contains(*t)).count();
    let union = set_a.len() + set_b.len() - inter;
    if union == 0 {
        return 0.0;
    }
    let jaccard = inter as f64 / union as f64;
    if jaccard >= 0.7 && inter >= 3 {
        jaccard * 0.9
    } else {
        0.0
    }
}

static LEADING_PUNCT: LazyLock<Regex> = LazyLock::new(|| re(&format!(r"^(?:{WS}|[\-–—:|])+")));
/// `GLUED_SECTION_LABEL` without its `(?=[A-Z0-9])` lookahead, which
/// [`clean_gloss_source`] checks by hand.
static GLUED_SECTION_LABEL: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"^(?i-u:lyrics|uncategorized|chorus|verse|bridge|refrain|spoken|outro|intro|tag|hook|pre[- ]?chorus)",
    )
});
static LEADING_LABEL: LazyLock<Regex> = LazyLock::new(|| {
    re(&format!(
        r"^(?i-u:chorus|verse|bridge|refrain|spoken){WS}*[:.\-]?{WS}*"
    ))
});
static TRAILING_TIMES: LazyLock<Regex> =
    LazyLock::new(|| re(&format!(r"{WS}*(?i-u:x)[0-9]+{WS}*$")));
static TRAILING_REPEAT: LazyLock<Regex> =
    LazyLock::new(|| re(&format!(r"{WS}*\({WS}*(?i-u:repeat){WS}*\){WS}*$")));

/// Peels site chrome glued onto lyric lines (`ChorusOdudu…`, `Refrain: Ide…`,
/// `… x4`).
pub fn clean_gloss_source(source: &str) -> String {
    let mut s = trim(&LEADING_PUNCT.replace(source, "")).to_string();
    for _ in 0..5 {
        let Some(m) = GLUED_SECTION_LABEL.find(&s) else {
            break;
        };
        // The lookahead: the label must be followed by an ASCII letter or
        // digit (the `i` flag widens `[A-Z]` to both cases).
        if !s[m.end()..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
        {
            break;
        }
        s = s[m.end()..].to_string();
    }
    let s = LEADING_LABEL.replace(&s, "");
    let s = TRAILING_TIMES.replace(&s, "");
    let s = TRAILING_REPEAT.replace(&s, "");
    trim(&s).to_string()
}

static URL_LIKE: LazyLock<Regex> = LazyLock::new(|| re(r"(?i-u:https?:|www\.)"));

/// One match of `/([^\n(]{2,120}?)\s*\(([^)\n]{2,120})\)/g` starting at
/// UTF-16 offset `start`, as `(group 1, group 2, end offset)`. Limits count
/// UTF-16 code units, as JavaScript does.
fn inline_gloss_at(units: &[u16], start: usize) -> Option<(usize, usize, usize, usize, usize)> {
    const NL: u16 = b'\n' as u16;
    const OPEN: u16 = b'(' as u16;
    const CLOSE: u16 = b')' as u16;
    let ws = |u: u16| char::from_u32(u32::from(u)).is_some_and(is_ws);
    // Group 1 may hold anything but a newline or `(`.
    let mut run_end = start;
    while run_end < units.len() && units[run_end] != NL && units[run_end] != OPEN {
        run_end += 1;
    }
    // Whitespace at the end of the run belongs to `\s*` when group 1 is lazy.
    let mut trailing = run_end;
    while trailing > start && ws(units[trailing - 1]) {
        trailing -= 1;
    }
    let group1_len = (trailing - start).max(2);
    if group1_len > 120 || start + group1_len > run_end {
        return None;
    }
    let mut open = run_end;
    while open < units.len() && units[open] != OPEN && ws(units[open]) {
        open += 1;
    }
    if open >= units.len() || units[open] != OPEN {
        return None;
    }
    let gloss_start = open + 1;
    let mut gloss_end = gloss_start;
    while gloss_end < units.len()
        && gloss_end - gloss_start < 120
        && units[gloss_end] != CLOSE
        && units[gloss_end] != NL
    {
        gloss_end += 1;
    }
    let len = gloss_end - gloss_start;
    if len < 2 || gloss_end >= units.len() || units[gloss_end] != CLOSE {
        return None;
    }
    Some((
        start,
        start + group1_len,
        gloss_start,
        gloss_end,
        gloss_end + 1,
    ))
}

/// Pulls `singable (English gloss)` pairs out of lyric-site text, keyed by
/// [`gloss_match_key`]; the first gloss for a key wins.
pub fn extract_inline_gloss_pairs(raw: &str) -> GlossPairs {
    let units: Vec<u16> = raw.encode_utf16().collect();
    let mut pairs: GlossPairs = Vec::new();
    let mut at = 0;
    while at < units.len() {
        let Some((g1s, g1e, g2s, g2e, end)) = inline_gloss_at(&units, at) else {
            at += 1;
            continue;
        };
        at = end;
        let source = String::from_utf16_lossy(&units[g1s..g1e]);
        let gloss_raw = String::from_utf16_lossy(&units[g2s..g2e]);
        let cleaned = clean_gloss_source(&source);
        let mut gloss = trim(&gloss_raw);
        if cleaned.is_empty()
            || gloss.is_empty()
            || is_junk_gloss(gloss)
            || URL_LIKE.is_match(gloss)
        {
            continue;
        }
        let key = gloss_match_key(&cleaned);
        if key.len() < 3 || is_likely_english_lyric(&cleaned) {
            continue;
        }
        while gloss.starts_with('(') && gloss.ends_with(')') && !gloss.is_empty() {
            gloss = if gloss.len() == 1 {
                ""
            } else {
                trim(&gloss[1..gloss.len() - 1])
            };
        }
        if is_junk_gloss(gloss) {
            continue;
        }
        if !pairs.iter().any(|(k, _)| *k == key) {
            pairs.push((key, gloss.to_string()));
        }
    }
    pairs
}

/// `line.match(/^(.*?)\s*\(([^)]{2,120})\)\s*$/)` as `(group 1, group 2)`.
fn trailing_inline_gloss(line: &str) -> Option<(&str, &str)> {
    let last = line.char_indices().rev().find(|(_, c)| !is_ws(*c))?;
    if last.1 != ')' {
        return None;
    }
    let close = last.0;
    for (open, c) in line.char_indices() {
        if c != '(' {
            continue;
        }
        // Group 1 (`.*?`) cannot hold a line terminator; the whitespace
        // before `(` is `\s*`.
        let prefix = trim_end(&line[..open]);
        if prefix.chars().any(is_line_terminator) {
            return None;
        }
        let gloss = &line[open + 1..close];
        let len = len16(gloss);
        if !gloss.contains(')') && (2..=120).contains(&len) {
            return Some((&line[..open], gloss));
        }
    }
    None
}

/// Turns inline `line (gloss)` into a line with its gloss under it.
pub fn expand_inline_glosses_to_lines(raw: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for line in raw.split('\n') {
        let trimmed = trim_end(line);
        if !has_text(trimmed) || is_section_marker_line(trim(trimmed)) {
            out.push(trimmed.to_string());
            continue;
        }
        match trailing_inline_gloss(trimmed) {
            Some((source, gloss)) => {
                let (source, gloss) = (trim(source), trim(gloss));
                if source.is_empty() || gloss.is_empty() || is_junk_gloss(gloss) {
                    out.push(trimmed.to_string());
                } else {
                    out.push(source.to_string());
                    out.push(format_gloss(gloss));
                }
            }
            None => out.push(trimmed.to_string()),
        }
    }
    out.join("\n")
}

/// Looks up a gloss by fuzzy line key. Requires a close match (score of at
/// least 0.5) so a loose line does not pick up an unrelated gloss.
pub fn lookup_online_gloss(line: &str, pairs: &[(String, String)]) -> Option<String> {
    let cleaned = clean_gloss_source(line);
    if cleaned.is_empty() || is_likely_english_lyric(&cleaned) {
        return None;
    }
    let mut keys = vec![gloss_match_key(&cleaned)];
    for part in cleaned.split(['!', '?', ';', '|', '/']) {
        let k = gloss_match_key(part);
        if k.len() >= 3 && !keys.contains(&k) {
            keys.push(k);
        }
    }
    let mut best: Option<(&str, f64)> = None;
    for key in &keys {
        if key.is_empty() || is_likely_english_lyric(key) {
            continue;
        }
        if let Some((_, exact)) = pairs.iter().find(|(k, _)| k == key)
            && !exact.is_empty()
            && !is_junk_gloss(exact)
        {
            return Some(exact.clone());
        }
        for (candidate, gloss) in pairs {
            if is_junk_gloss(gloss) {
                continue;
            }
            let score = gloss_key_score(key, candidate);
            if score < 0.5 {
                continue;
            }
            if best.is_none_or(|(_, s)| score > s) {
                best = Some((gloss, score));
            }
            if score == 1.0 {
                return Some(gloss.clone());
            }
        }
    }
    best.map(|(g, _)| g.to_string())
}
