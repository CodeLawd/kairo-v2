//! JavaScript string and number behaviour the scripture ports depend on.
//!
//! Private to `scripture` until prompt 013's `kairo_domain::js` lands; then
//! these should move there (recorded in DECISIONS.md, prompt 015).

/// JavaScript `\s` and the characters `String.prototype.trim` removes:
/// WhiteSpace plus LineTerminator. Differs from `char::is_whitespace`, which
/// includes U+0085 and excludes U+FEFF.
pub fn is_space(c: char) -> bool {
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

/// The `\s` class for the `regex` crate, matching JavaScript's set.
pub const SPACE_CLASS: &str = r"\t\n\x0B\x0C\r \u{A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}";

/// A line terminator, which a JavaScript `.` does not match.
pub fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// `String.prototype.trim`.
pub fn trim(text: &str) -> &str {
    text.trim_matches(is_space)
}

/// `text.replace(/\s+/g, ' ')`.
pub fn collapse_spaces(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_space = false;
    for c in text.chars() {
        if is_space(c) {
            if !in_space {
                out.push(' ');
            }
            in_space = true;
        } else {
            out.push(c);
            in_space = false;
        }
    }
    out
}

/// `String.prototype.length`: UTF-16 code units.
pub fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// `Math.max(a, b)`: `NaN` wins, unlike `f64::max`.
pub fn max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == b {
        // max(-0, 0) is 0.
        if a.is_sign_negative() { b } else { a }
    } else if a > b {
        a
    } else {
        b
    }
}

/// `Math.min(a, b)`: `NaN` wins, unlike `f64::min`.
pub fn min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == b {
        if a.is_sign_negative() { a } else { b }
    } else if a < b {
        a
    } else {
        b
    }
}

/// `Math.round`: halves round towards positive infinity.
pub fn round(value: f64) -> f64 {
    if !value.is_finite() {
        return value;
    }
    let floor = value.floor();
    if value - floor >= 0.5 {
        floor + 1.0
    } else {
        floor
    }
}

/// `Number(digits)` for a string of ASCII digits.
pub fn number_from_digits(digits: &str) -> f64 {
    digits.parse::<f64>().unwrap_or(f64::NAN)
}

/// `String(value)` for a number (ECMAScript `Number::toString`, radix 10).
pub fn number_to_string(value: f64) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value == 0.0 {
        return "0".into();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    let sign = if value < 0.0 { "-" } else { "" };
    // `{:e}` gives the shortest round-trip digits, as JavaScript requires.
    let exp_form = format!("{:e}", value.abs());
    let (mantissa, exponent) = exp_form.split_once('e').unwrap_or((&exp_form, "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let k = digits.len() as i32;
    let n = exponent + 1;
    let body = if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let e = n - 1;
        let e_sign = if e < 0 { '-' } else { '+' };
        if k == 1 {
            format!("{digits}e{e_sign}{}", e.abs())
        } else {
            format!("{}.{}e{e_sign}{}", &digits[..1], &digits[1..], e.abs())
        }
    };
    format!("{sign}{body}")
}

/// Stable sort by a JavaScript comparator's sign; `NaN` counts as equal, as
/// `Array.prototype.sort` treats it.
pub fn sort_by_number<T>(items: &mut [T], mut key: impl FnMut(&T, &T) -> f64) {
    items.sort_by(|a, b| {
        let d = key(a, b);
        if d < 0.0 {
            std::cmp::Ordering::Less
        } else if d > 0.0 {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
}

/// Converts a byte offset in `text` to a UTF-16 offset.
pub fn utf16_offset(text: &str, byte: usize) -> usize {
    utf16_len(&text[..byte])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_to_string_matches_javascript() {
        let cases = [
            (1.0, "1"),
            (-0.0, "0"),
            (1.5, "1.5"),
            (0.000001, "0.000001"),
            (0.0000001, "1e-7"),
            (1e21, "1e+21"),
            (123456789012345680000.0, "123456789012345680000"),
            (1.2345678901234568e23, "1.2345678901234569e+23"),
            (-42.25, "-42.25"),
            (f64::NAN, "NaN"),
            (f64::INFINITY, "Infinity"),
            (100.0, "100"),
        ];
        for (value, text) in cases {
            assert_eq!(number_to_string(value), text, "{value}");
        }
    }

    #[test]
    fn math_helpers_propagate_nan() {
        assert!(max(f64::NAN, 1.0).is_nan());
        assert!(min(1.0, f64::NAN).is_nan());
        assert_eq!(round(2.5), 3.0);
        assert_eq!(round(-2.5), -2.0);
        assert_eq!(round(0.49999999999999994), 0.0);
    }

    #[test]
    fn spaces_follow_javascript() {
        assert!(is_space('\u{FEFF}'));
        assert!(!is_space('\u{85}'));
        assert_eq!(trim("\u{FEFF} a \u{3000}"), "a");
        assert_eq!(collapse_spaces("a \t\n b"), "a b");
    }
}
