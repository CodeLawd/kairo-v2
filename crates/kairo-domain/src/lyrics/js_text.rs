//! JavaScript string semantics the lyrics ports depend on.
//!
//! Prompt 013 owns `kairo_domain::js`; these helpers are private to `lyrics`
//! until both land, then they move there (see `native-migration/DECISIONS.md`,
//! prompt 014).
//!
//! - `String.prototype.trim` and regex `\s` use the ECMAScript WhiteSpace and
//!   LineTerminator sets, which differ from Rust's `char::is_whitespace`:
//!   JavaScript includes U+FEFF and excludes U+0085.
//! - `.length` and string offsets count UTF-16 code units.
//! - Regex `\d` is ASCII, `\b` is an ASCII word boundary and `.` excludes the
//!   four line terminators. Patterns here spell those out as [`WS`], `[0-9]`,
//!   `(?-u:\b)` and [`DOT`].
//! - The `i` flag folds ASCII only for the ASCII literals used in these
//!   modules, so case-insensitive words are written `(?i-u:...)`; Rust's
//!   Unicode folding would also match `ſ` for `s` and `K` (Kelvin) for `k`.

use regex::Regex;

/// A regex class matching one JavaScript `\s` character.
pub(crate) const WS: &str = r"[\t\n\x0B\x0C\r \xA0\x{1680}\x{2000}-\x{200A}\x{2028}\x{2029}\x{202F}\x{205F}\x{3000}\x{FEFF}]";
/// A regex class matching one JavaScript `\S` character.
pub(crate) const NOT_WS: &str = r"[^\t\n\x0B\x0C\r \xA0\x{1680}\x{2000}-\x{200A}\x{2028}\x{2029}\x{202F}\x{205F}\x{3000}\x{FEFF}]";
/// A regex class matching what JavaScript `.` matches without the `s` flag.
pub(crate) const DOT: &str = r"[^\n\r\x{2028}\x{2029}]";

/// Builds a regex from a pattern known to be valid.
pub(crate) fn re(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap_or_else(|e| panic!("bad built-in pattern {pattern:?}: {e}"))
}

/// Whether `c` is a JavaScript `\s` character.
pub(crate) fn is_ws(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}

/// Whether `c` is one of JavaScript's line terminators.
pub(crate) fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// `String.prototype.trim`.
pub(crate) fn trim(s: &str) -> &str {
    s.trim_matches(is_ws)
}

/// `String.prototype.trimEnd`.
pub(crate) fn trim_end(s: &str) -> &str {
    s.trim_end_matches(is_ws)
}

/// `String.prototype.trimStart`.
pub(crate) fn trim_start(s: &str) -> &str {
    s.trim_start_matches(is_ws)
}

/// Whether the trimmed string is non-empty: JavaScript's `line.trim()` used
/// as a condition.
pub(crate) fn has_text(s: &str) -> bool {
    !trim(s).is_empty()
}

/// `.length`: UTF-16 code units.
pub(crate) fn len16(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

/// `s.split(/\s+/)`, keeping the empty strings JavaScript keeps at the ends.
pub(crate) fn split_ws(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut in_ws = false;
    let mut ws_start = 0;
    for (i, c) in s.char_indices() {
        if is_ws(c) {
            if !in_ws {
                in_ws = true;
                ws_start = i;
            }
        } else if in_ws {
            out.push(&s[start..ws_start]);
            start = i;
            in_ws = false;
        }
    }
    if in_ws {
        out.push(&s[start..ws_start]);
        out.push("");
    } else {
        out.push(&s[start..]);
    }
    out
}

/// Replaces every run of JavaScript whitespace with one space.
pub(crate) fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_ws = false;
    for c in s.chars() {
        if is_ws(c) {
            if !in_ws {
                out.push(' ');
            }
            in_ws = true;
        } else {
            out.push(c);
            in_ws = false;
        }
    }
    out
}

/// `text.replace(/\r\n?/g, '\n')`.
pub(crate) fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// `Math.max(a, b)`, which returns `NaN` when either side is `NaN`.
pub(crate) fn max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}

/// `Math.min(a, b)`, which returns `NaN` when either side is `NaN`.
pub(crate) fn min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.min(b)
    }
}

/// `Number.prototype.toString()` for a finite or non-finite number.
pub(crate) fn number_to_string(value: f64) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if value == 0.0 {
        return "0".into();
    }
    let sign = if value < 0.0 { "-" } else { "" };
    // `{:e}` writes the shortest digits that round trip, as JavaScript does.
    let sci = format!("{:e}", value.abs());
    let (mantissa, exponent) = sci.split_once('e').unwrap_or((&sci, "0"));
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n = exponent.parse::<i32>().unwrap_or(0) + 1;
    let body = if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let e = n - 1;
        let e = if e >= 0 {
            format!("+{e}")
        } else {
            e.to_string()
        };
        if k == 1 {
            format!("{digits}e{e}")
        } else {
            format!("{}.{}e{e}", &digits[..1], &digits[1..])
        }
    };
    format!("{sign}{body}")
}

/// Converts a UTF-16 offset into a byte offset, rounding down to a character
/// boundary when it falls inside a surrogate pair.
pub(crate) fn byte_offset(s: &str, units: usize) -> usize {
    let mut seen = 0;
    for (i, c) in s.char_indices() {
        if seen >= units {
            return i;
        }
        seen += c.len_utf16();
        if seen > units {
            return i;
        }
    }
    s.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitespace_follows_ecmascript() {
        assert!(is_ws('\u{FEFF}'));
        assert!(!is_ws('\u{85}'));
        assert_eq!(trim("\u{FEFF} a\u{3000}"), "a");
        assert_eq!(trim("\u{85}a"), "\u{85}a");
    }

    #[test]
    fn split_keeps_end_empties() {
        assert_eq!(split_ws(" a  b "), vec!["", "a", "b", ""]);
        assert_eq!(split_ws(""), vec![""]);
        assert_eq!(split_ws("a"), vec!["a"]);
    }

    #[test]
    fn lengths_are_utf16() {
        assert_eq!(len16("😀"), 2);
        assert_eq!(len16("ọ́"), 2);
        assert_eq!(byte_offset("a😀b", 2), 1);
        assert_eq!(byte_offset("a😀b", 3), 5);
    }

    #[test]
    fn numbers_print_like_javascript() {
        for (value, text) in [
            (1.0, "1"),
            (2.5, "2.5"),
            (-0.0, "0"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (1e-7, "1e-7"),
            (0.000001, "0.000001"),
            (123.456, "123.456"),
            (1.5e-10, "1.5e-10"),
            (f64::NAN, "NaN"),
            (f64::NEG_INFINITY, "-Infinity"),
        ] {
            assert_eq!(number_to_string(value), text);
        }
    }
}
