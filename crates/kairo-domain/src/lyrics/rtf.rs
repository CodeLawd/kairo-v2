//! Plain text out of an RTF document. Port of `src/lib/rtf-text.ts`.
//!
//! Works on UTF-16 code units, as the TypeScript does: `\uN` writes one code
//! unit and surrogate halves arrive as two of them. A half left unpaired
//! becomes U+FFFD, since a Rust string cannot hold it.

use super::js_text::{trim, trim_start};

/// Groups whose contents are formatting metadata, never body text.
const SKIPPED_DESTINATIONS: [&str; 18] = [
    "fonttbl",
    "colortbl",
    "stylesheet",
    "listtable",
    "listoverridetable",
    "revtbl",
    "rsidtbl",
    "generator",
    "info",
    "pict",
    "object",
    "themedata",
    "colorschememapping",
    "latentstyles",
    "datastore",
    "xmlnstbl",
    "fldinst",
    "expandedcolortbl",
];

/// Control words that produce a line break.
const BREAK_WORDS: [&str; 6] = ["par", "line", "sect", "page", "column", "softline"];

fn literal_word(word: &str) -> Option<u16> {
    Some(match word {
        "tab" => 0x09,
        "emdash" => 0x2014,
        "endash" => 0x2013,
        "emspace" | "enspace" | "qmspace" => 0x20,
        "bullet" => 0x2022,
        "lquote" => 0x2018,
        "rquote" => 0x2019,
        "ldblquote" => 0x201C,
        "rdblquote" => 0x201D,
        "nbsp" => 0xA0,
        _ => return None,
    })
}

/// Windows-1252 characters where Latin-1 has control codes.
fn cp1252_high(code: u32) -> Option<u16> {
    Some(match code {
        0x80 => 0x20AC,
        0x82 => 0x201A,
        0x83 => 0x0192,
        0x84 => 0x201E,
        0x85 => 0x2026,
        0x86 => 0x2020,
        0x87 => 0x2021,
        0x88 => 0x02C6,
        0x89 => 0x2030,
        0x8A => 0x0160,
        0x8B => 0x2039,
        0x8C => 0x0152,
        0x8E => 0x017D,
        0x91 => 0x2018,
        0x92 => 0x2019,
        0x93 => 0x201C,
        0x94 => 0x201D,
        0x95 => 0x2022,
        0x96 => 0x2013,
        0x97 => 0x2014,
        0x98 => 0x02DC,
        0x99 => 0x2122,
        0x9A => 0x0161,
        0x9B => 0x203A,
        0x9C => 0x0153,
        0x9E => 0x017E,
        0x9F => 0x0178,
        _ => return None,
    })
}

#[derive(Clone, Copy)]
struct GroupState {
    /// Characters to skip after a `\uN`, set by `\ucN`.
    unicode_skip: f64,
    /// True when this group's text is metadata.
    ignoring: bool,
}

const ROOT: GroupState = GroupState {
    unicode_skip: 1.0,
    ignoring: false,
};

/// Cheap check for RTF content.
pub fn looks_like_rtf(raw: &str) -> bool {
    trim_start(raw).starts_with("{\\rtf")
}

/// JavaScript's `ToUint16`, as `String.fromCharCode` applies it.
fn to_uint16(value: f64) -> u16 {
    if !value.is_finite() {
        return 0;
    }
    value.trunc().rem_euclid(65536.0) as u16
}

/// `parseInt(text, 16)` on the two code units after `\'`.
fn parse_hex(units: &[u16]) -> Option<f64> {
    let text = String::from_utf16_lossy(units);
    let text = trim_start(&text);
    let (negative, body) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let body = body
        .strip_prefix("0x")
        .or_else(|| body.strip_prefix("0X"))
        .unwrap_or(body);
    let digits: String = body.chars().take_while(char::is_ascii_hexdigit).collect();
    if digits.is_empty() {
        return None;
    }
    let value = u32::from_str_radix(&digits, 16).ok()? as f64;
    Some(if negative { -value } else { value })
}

fn is_alpha(u: u16) -> bool {
    u < 128 && (u as u8).is_ascii_alphabetic()
}

/// Converts an RTF document to plain text, keeping line and paragraph
/// breaks. Input that is not RTF gives an empty string.
///
/// One deliberate difference: TypeScript looks literal control words up in a
/// plain object, so `\toString`, `\valueOf` and the other `Object.prototype`
/// names emit JavaScript function source. Here they are formatting, like any
/// other unknown word (see `native-migration/DECISIONS.md`, prompt 014).
pub fn rtf_to_plain_text(raw: &str) -> String {
    if raw.is_empty() || !looks_like_rtf(raw) {
        return String::new();
    }
    let units: Vec<u16> = raw.encode_utf16().collect();
    let mut out: Vec<u16> = Vec::with_capacity(units.len() / 2);
    let mut stack: Vec<GroupState> = Vec::new();
    let mut state = ROOT;
    let mut skip_chars = 0.0_f64;

    let emit = |text: &[u16], state: &GroupState, skip_chars: &mut f64, out: &mut Vec<u16>| {
        if state.ignoring {
            return;
        }
        if *skip_chars > 0.0 {
            let drop = skip_chars.min(text.len() as f64);
            *skip_chars -= drop;
            out.extend_from_slice(&text[drop as usize..]);
            return;
        }
        out.extend_from_slice(text);
    };

    let mut i = 0;
    while i < units.len() {
        let c = units[i];
        match c {
            0x7B => {
                stack.push(state);
                i += 1;
                continue;
            }
            0x7D => {
                state = stack.pop().unwrap_or(ROOT);
                skip_chars = 0.0;
                i += 1;
                continue;
            }
            0x0D | 0x0A => {
                i += 1;
                continue;
            }
            0x5C => {}
            _ => {
                emit(&[c], &state, &mut skip_chars, &mut out);
                i += 1;
                continue;
            }
        }
        let Some(&next) = units.get(i + 1) else {
            break;
        };
        if next == 0x5C || next == 0x7B || next == 0x7D {
            emit(&[next], &state, &mut skip_chars, &mut out);
            i += 2;
            continue;
        }
        if next == 0x0A || next == 0x0D {
            if !state.ignoring {
                out.push(0x0A);
            }
            i += 2;
            continue;
        }
        if next == u16::from(b'\'') {
            let hex = &units[(i + 2).min(units.len())..(i + 4).min(units.len())];
            i += 4;
            if let Some(code) = parse_hex(hex) {
                let unit = if code >= 0.0 {
                    cp1252_high(code as u32)
                } else {
                    None
                };
                emit(
                    &[unit.unwrap_or_else(|| to_uint16(code))],
                    &state,
                    &mut skip_chars,
                    &mut out,
                );
            }
            continue;
        }
        if next == u16::from(b'*') {
            state.ignoring = true;
            i += 2;
            continue;
        }
        if !is_alpha(next) {
            if next == u16::from(b'~') {
                emit(&[0xA0], &state, &mut skip_chars, &mut out);
            }
            i += 2;
            continue;
        }
        // \word, an optional numeric parameter and one optional space.
        let word_start = i + 1;
        let mut j = word_start;
        while j < units.len() && is_alpha(units[j]) {
            j += 1;
        }
        let word = String::from_utf16_lossy(&units[word_start..j]);
        let mut k = j;
        if k < units.len() && units[k] == u16::from(b'-') {
            k += 1;
        }
        let digits_start = k;
        while k < units.len() && (0x30..=0x39).contains(&units[k]) {
            k += 1;
        }
        let param = if k > digits_start {
            let text = String::from_utf16_lossy(&units[j..k]);
            text.parse::<f64>().ok()
        } else {
            k = j;
            None
        };
        if k < units.len() && units[k] == 0x20 {
            k += 1;
        }
        i = k;

        if word == "u"
            && let Some(param) = param
        {
            if !state.ignoring {
                out.push(to_uint16(if param < 0.0 { param + 65536.0 } else { param }));
            }
            skip_chars = state.unicode_skip;
            continue;
        }
        if word == "uc"
            && let Some(param) = param
        {
            state.unicode_skip = param.max(0.0);
            continue;
        }
        if SKIPPED_DESTINATIONS.contains(&word.to_ascii_lowercase().as_str()) {
            state.ignoring = true;
            continue;
        }
        if BREAK_WORDS.contains(&word.as_str()) {
            if !state.ignoring {
                out.push(0x0A);
            }
            skip_chars = 0.0;
            continue;
        }
        if let Some(unit) = literal_word(&word) {
            emit(&[unit], &state, &mut skip_chars, &mut out);
        }
        // Any other control word is formatting.
    }
    tidy(&String::from_utf16_lossy(&out))
}

/// Collapses the whitespace RTF writers scatter, keeping stanza breaks.
fn tidy(text: &str) -> String {
    let lines: Vec<String> = text
        .replace('\0', "")
        .split('\n')
        .map(|line| {
            let mut collapsed = String::with_capacity(line.len());
            let mut in_run = false;
            for c in line.chars() {
                if c == ' ' || c == '\t' {
                    if !in_run {
                        collapsed.push(' ');
                    }
                    in_run = true;
                } else {
                    collapsed.push(c);
                    in_run = false;
                }
            }
            trim(&collapsed).to_string()
        })
        .collect();
    let mut joined = lines.join("\n");
    while joined.contains("\n\n\n") {
        joined = joined.replace("\n\n\n", "\n\n");
    }
    trim(&joined).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_parsing_follows_parse_int() {
        let u = |s: &str| s.encode_utf16().collect::<Vec<_>>();
        assert_eq!(parse_hex(&u("e9")), Some(233.0));
        assert_eq!(parse_hex(&u("4g")), Some(4.0));
        assert_eq!(parse_hex(&u("g4")), None);
        assert_eq!(parse_hex(&u(" a")), Some(10.0));
        assert_eq!(parse_hex(&u("-a")), Some(-10.0));
        assert_eq!(parse_hex(&u("0x")), None);
        assert_eq!(parse_hex(&u("")), None);
    }
}
