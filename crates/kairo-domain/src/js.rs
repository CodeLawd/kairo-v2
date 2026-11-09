//! JavaScript semantics that change results when ported naively to Rust.
//!
//! Every helper names the JavaScript it replaces and is checked against
//! Node through the oracle (`oracle:js` in `tools/ts-oracle/modules`).
//! Reuse these rather than writing local copies:
//!
//! | JavaScript                     | Rust                    | Why Rust's own differs                     |
//! |--------------------------------|-------------------------|--------------------------------------------|
//! | `Math.round(x)`                | [`math_round`]          | `f64::round` rounds halves away from zero  |
//! | `Math.min(a, b)`, `Math.max`   | [`math_min`], [`math_max`] | `f64::min` ignores `NaN`                |
//! | `String(n)`, `` `${n}` ``      | [`number_to_string`]    | exponent rules, `-0`, `NaN`                |
//! | `String(v)` for JSON values    | [`value_to_string`]     | arrays join, objects print `[object Object]` |
//! | `Number(s)`                    | [`string_to_number`]    | `''` is 0, hex/octal/binary, `Infinity`    |
//! | `parseInt(s, radix)`           | [`parse_int`]           | parses a prefix and ignores the rest       |
//! | `parseFloat(s)`                | [`parse_float`]         | parses a prefix and ignores the rest       |
//! | `s.trim()`                     | [`trim`]                | `char::is_whitespace` has a different set  |
//! | `/\s/`                         | [`is_whitespace`]       | as above                                   |
//! | `Boolean(v)`, `!!v`            | [`truthy`]              | no Rust equivalent                         |
//! | `Number.isSafeInteger(n)`      | [`is_safe_integer`]     | no Rust equivalent                         |
//! | `s.replaceAll(a, b)`           | [`replace_all`]         | `$&`, `$$`, `` $` ``, `$'` in `b` are patterns |
//!
//! JSON values never hold `NaN` or infinities, so helpers that read a
//! `serde_json::Value` see only finite numbers.

use serde_json::Value;

/// `Math.round`: halves round towards positive infinity, so `-2.5` gives `-2`.
/// `NaN` and infinities pass through.
pub fn math_round(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let floor = x.floor();
    // `x - floor` is exact for every finite double.
    let rounded = if x - floor >= 0.5 { floor + 1.0 } else { floor };
    // JavaScript keeps the sign of a zero result: Math.round(-0.4) is -0.
    if rounded == 0.0 && x.is_sign_negative() {
        -0.0
    } else {
        rounded
    }
}

/// `Math.min(a, b)`: `NaN` if either is `NaN`; `-0` is less than `0`.
pub fn math_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == b {
        if a.is_sign_negative() { a } else { b }
    } else {
        a.min(b)
    }
}

/// `Math.max(a, b)`: `NaN` if either is `NaN`; `0` is greater than `-0`.
pub fn math_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == b {
        if a.is_sign_negative() { b } else { a }
    } else {
        a.max(b)
    }
}

/// `Math.min(max, Math.max(min, n))`, the clamp every TypeScript module writes.
pub fn clamp(n: f64, min: f64, max: f64) -> f64 {
    math_min(max, math_max(min, n))
}

/// `Number.isSafeInteger(n)`.
pub fn is_safe_integer(n: f64) -> bool {
    n.is_finite() && n.fract() == 0.0 && n.abs() <= 9_007_199_254_740_991.0
}

/// `String(n)` for a number (`Number.prototype.toString()` with radix 10).
///
/// Shortest round-trip digits, as both languages use, laid out by the
/// ECMAScript rules: plain decimals for exponents from -7 to 20, otherwise
/// `1.5e+21` / `1e-7` form. `-0` prints `0`.
pub fn number_to_string(n: f64) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if n == 0.0 {
        return "0".into();
    }
    let sign = if n < 0.0 { "-" } else { "" };
    // `{:e}` gives the shortest round-trip digits, as `d.ddde±x`.
    let sci = format!("{:e}", n.abs());
    let (mantissa, exponent) = sci.split_once('e').expect("{:e} always has an exponent");
    let mut digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let mut exponent = exponent
        .parse::<i32>()
        .expect("{:e} exponent is an integer");
    even_tie(n.abs(), &mut digits, &mut exponent);
    let k = digits.len() as i32;
    // ECMAScript's `n`: the decimal point sits after `n` digits.
    let point = exponent + 1;
    let body = if k <= point && point <= 21 {
        format!("{digits}{}", "0".repeat((point - k) as usize))
    } else if 0 < point && point <= 21 {
        format!(
            "{}.{}",
            &digits[..point as usize],
            &digits[point as usize..]
        )
    } else if -6 < point && point <= 0 {
        format!("0.{}{digits}", "0".repeat((-point) as usize))
    } else {
        let e = point - 1;
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

/// When the double lies exactly halfway between two shortest digit strings,
/// ECMAScript picks the even one; Rust's formatter picks the upper. Ties need
/// an exact decimal expansion only one digit longer than the shortest, which
/// only happens at 15 or more significant digits, so shorter ones are skipped.
fn even_tie(n: f64, digits: &mut String, exponent: &mut i32) {
    let k = digits.len();
    if k < 15 {
        return;
    }
    // Rust prints the exact expansion when given enough precision.
    let exact = format!("{n:.800e}");
    let (mantissa, exact_exponent) = exact.split_once('e').expect("{:e} always has an exponent");
    let exact_digits: Vec<u8> = mantissa.bytes().filter(|b| *b != b'.').collect();
    let exact_exponent: i32 = exact_exponent.parse().expect("integer exponent");
    let tie =
        exact_digits.get(k) == Some(&b'5') && exact_digits[k + 1..].iter().all(|b| *b == b'0');
    if !tie || exact_exponent != *exponent {
        return;
    }
    let lower = &exact_digits[..k];
    if lower[k - 1].is_multiple_of(2) {
        // The lower candidate is even.
        *digits = String::from_utf8(lower.to_vec()).expect("ASCII digits");
    } else {
        // The upper candidate is even: lower + 1 in the last place.
        let mut upper = lower.to_vec();
        let mut i = k;
        loop {
            i -= 1;
            if upper[i] == b'9' {
                upper[i] = b'0';
                if i == 0 {
                    upper.insert(0, b'1');
                    *exponent += 1;
                    break;
                }
            } else {
                upper[i] += 1;
                break;
            }
        }
        while upper.len() > 1 && upper.last() == Some(&b'0') {
            upper.pop();
        }
        *digits = String::from_utf8(upper).expect("ASCII digits");
    }
}

/// `String(v)` for a JSON value: `null` → `"null"`, arrays join their items
/// with `,` (a `null` item prints as empty), objects print `[object Object]`.
pub fn value_to_string(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => number_to_string(n.as_f64().unwrap_or(f64::NAN)),
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .map(|item| match item {
                Value::Null => String::new(),
                other => value_to_string(other),
            })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

/// `Boolean(v)` for a JSON value. Absent (`undefined`) is `None` → false.
pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(_) | Value::Object(_)) => true,
    }
}

/// The characters JavaScript's `trim` and regex `\s` remove: WhiteSpace and
/// LineTerminator in ECMAScript. Unlike `char::is_whitespace`, this includes
/// U+FEFF and excludes U+0085.
pub fn is_whitespace(c: char) -> bool {
    matches!(
        c,
        '\u{0009}'..='\u{000D}'
            | '\u{0020}'
            | '\u{00A0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200A}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202F}'
            | '\u{205F}'
            | '\u{3000}'
            | '\u{FEFF}'
    )
}

/// `s.trim()`.
pub fn trim(s: &str) -> &str {
    s.trim_matches(is_whitespace)
}

/// `subject.replaceAll(pattern, replacement)` with a string pattern.
///
/// `replacement` is a template as in JavaScript: `$$` is `$`, `$&` the match,
/// `` $` `` the text before it and `$'` the text after it. Anything else,
/// including `$1`, is literal. An empty pattern matches between every UTF-16
/// unit; that case is handled per character here, which matches JavaScript
/// for every string without astral characters.
pub fn replace_all(subject: &str, pattern: &str, replacement: &str) -> String {
    let positions: Vec<usize> = if pattern.is_empty() {
        subject
            .char_indices()
            .map(|(i, _)| i)
            .chain(std::iter::once(subject.len()))
            .collect()
    } else {
        subject.match_indices(pattern).map(|(i, _)| i).collect()
    };
    let mut out = String::with_capacity(subject.len());
    let mut last = 0;
    for position in positions {
        out.push_str(&subject[last..position]);
        let end = position + pattern.len();
        substitute(
            &mut out,
            replacement,
            &subject[position..end],
            subject,
            position,
            end,
        );
        last = end;
    }
    out.push_str(&subject[last..]);
    out
}

/// ECMAScript `GetSubstitution` for a string pattern (no capture groups).
fn substitute(
    out: &mut String,
    template: &str,
    matched: &str,
    subject: &str,
    start: usize,
    end: usize,
) {
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('$') => {
                out.push('$');
                chars.next();
            }
            Some('&') => {
                out.push_str(matched);
                chars.next();
            }
            Some('`') => {
                out.push_str(&subject[..start]);
                chars.next();
            }
            Some('\'') => {
                out.push_str(&subject[end..]);
                chars.next();
            }
            _ => out.push('$'),
        }
    }
}

/// `Number(s)` for a string: surrounding whitespace is ignored, `''` is `0`,
/// `0x`/`0o`/`0b` prefixes are read, `Infinity` with an optional sign is
/// accepted, and anything else that is not a full decimal literal is `NaN`.
pub fn string_to_number(s: &str) -> f64 {
    let t = trim(s);
    if t.is_empty() {
        return 0.0;
    }
    for (prefix, radix) in [
        ("0x", 16),
        ("0X", 16),
        ("0o", 8),
        ("0O", 8),
        ("0b", 2),
        ("0B", 2),
    ] {
        if let Some(rest) = t.strip_prefix(prefix) {
            return digits_in_radix(rest, radix).unwrap_or(f64::NAN);
        }
    }
    let (negative, unsigned) = match t.as_bytes()[0] {
        b'+' => (false, &t[1..]),
        b'-' => (true, &t[1..]),
        _ => (false, t),
    };
    let magnitude = if unsigned == "Infinity" {
        f64::INFINITY
    } else if is_decimal_literal(unsigned) {
        unsigned.parse::<f64>().unwrap_or(f64::NAN)
    } else {
        f64::NAN
    };
    if negative { -magnitude } else { magnitude }
}

/// `StrUnsignedDecimalLiteral` without `Infinity`: `1`, `1.`, `.5`, `1e3`, `1.5E-2`.
fn is_decimal_literal(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut i = 0;
    let int_digits = count_digits(&bytes[i..]);
    i += int_digits;
    let mut frac_digits = 0;
    if bytes.get(i) == Some(&b'.') {
        i += 1;
        frac_digits = count_digits(&bytes[i..]);
        i += frac_digits;
    }
    if int_digits + frac_digits == 0 {
        return false;
    }
    if matches!(bytes.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(bytes.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let exp_digits = count_digits(&bytes[i..]);
        if exp_digits == 0 {
            return false;
        }
        i += exp_digits;
    }
    i == bytes.len()
}

fn count_digits(bytes: &[u8]) -> usize {
    bytes.iter().take_while(|b| b.is_ascii_digit()).count()
}

/// The whole of `s` as digits in `radix` (2, 8 or 16 here), or `None` if
/// empty or any is invalid. Rounded exactly, as V8 does for these radices.
fn digits_in_radix(s: &str, radix: u32) -> Option<f64> {
    if s.is_empty() || s.chars().any(|c| c.to_digit(radix).is_none()) {
        return None;
    }
    Some(power_of_two_digits(s, radix))
}

/// Digits already checked valid in a power-of-two radix, correctly rounded
/// to the nearest double (ties to even), as V8's `InternalStringToIntDouble`.
fn power_of_two_digits(digits: &str, radix: u32) -> f64 {
    let bits_per_digit = radix.trailing_zeros();
    let mut bits: Vec<bool> = Vec::with_capacity(digits.len() * bits_per_digit as usize);
    for c in digits.chars() {
        let d = c.to_digit(radix).expect("checked by the caller");
        for shift in (0..bits_per_digit).rev() {
            bits.push((d >> shift) & 1 == 1);
        }
    }
    let first = bits.iter().position(|b| *b).unwrap_or(bits.len());
    let bits = &bits[first..];
    if bits.len() <= 53 {
        return bits
            .iter()
            .fold(0.0, |acc, b| acc * 2.0 + f64::from(u8::from(*b)));
    }
    let mut mantissa = bits[..53]
        .iter()
        .fold(0u64, |acc, b| (acc << 1) | u64::from(*b));
    let round = bits[53];
    let sticky = bits[54..].iter().any(|b| *b);
    if round && (sticky || mantissa & 1 == 1) {
        mantissa += 1;
    }
    let exponent = (bits.len() - 53) as i32;
    // 2^53 after rounding up is still exact; powi overflows to infinity past f64::MAX.
    mantissa as f64 * 2f64.powi(exponent)
}

/// V8's `parseInt` for a radix that is neither 10 nor a power of two: 32-bit
/// chunks multiplied into a double, which loses precision past about 2^56
/// exactly as V8 does (the specification allows an approximation here).
fn generic_radix_digits(digits: &str, radix: u32) -> f64 {
    const MAXIMUM_MULTIPLIER: u32 = u32::MAX / 36;
    let values: Vec<u32> = digits
        .chars()
        .map(|c| c.to_digit(radix).expect("checked by the caller"))
        .collect();
    let mut result = 0.0_f64;
    let mut i = 0;
    while i < values.len() {
        let (mut part, mut multiplier) = (0u32, 1u32);
        while i < values.len() {
            let m = multiplier * radix;
            if m > MAXIMUM_MULTIPLIER {
                break;
            }
            part = part * radix + values[i];
            multiplier = m;
            i += 1;
        }
        // V8's `result * multiplier + part` is compiled to a fused
        // multiply-add on arm64 (one rounding), so it is fused here too.
        result = result.mul_add(f64::from(multiplier), f64::from(part));
    }
    result
}

/// `parseInt(s, radix)`. `radix` of `None` or `0` means 10, or 16 after a
/// `0x` prefix. A radix outside 2–36 gives `NaN`. Reads the longest valid
/// prefix after optional whitespace and sign; no digits gives `NaN`.
///
/// Large values round as V8 rounds them: exactly for radix 10 and powers of
/// two, and by V8's 32-bit chunk method for other radices.
pub fn parse_int(s: &str, radix: Option<f64>) -> f64 {
    let mut t = s.trim_start_matches(is_whitespace);
    let mut negative = false;
    if let Some(rest) = t.strip_prefix('-') {
        negative = true;
        t = rest;
    } else if let Some(rest) = t.strip_prefix('+') {
        t = rest;
    }
    // ToInt32 of the radix, as the specification says.
    let mut r = radix.map_or(0, to_int32);
    let mut strip_prefix = true;
    if r != 0 {
        if !(2..=36).contains(&r) {
            return f64::NAN;
        }
        if r != 16 {
            strip_prefix = false;
        }
    } else {
        r = 10;
    }
    if strip_prefix && (t.starts_with("0x") || t.starts_with("0X")) {
        t = &t[2..];
        r = 16;
    }
    let radix = r as u32;
    let end = t
        .char_indices()
        .find(|(_, c)| c.to_digit(radix).is_none())
        .map_or(t.len(), |(i, _)| i);
    let digits = &t[..end];
    if digits.is_empty() {
        return f64::NAN;
    }
    let value = if radix == 10 {
        // Decimal text parses exactly, as V8 does for radix 10.
        digits.parse::<f64>().unwrap_or(f64::NAN)
    } else if radix.is_power_of_two() {
        power_of_two_digits(digits, radix)
    } else {
        generic_radix_digits(digits, radix)
    };
    if negative { -value } else { value }
}

/// ECMAScript `ToInt32` of a number.
fn to_int32(n: f64) -> i32 {
    if !n.is_finite() {
        return 0;
    }
    let int = n.trunc();
    let modulo = int.rem_euclid(4_294_967_296.0);
    if modulo >= 2_147_483_648.0 {
        (modulo - 4_294_967_296.0) as i32
    } else {
        modulo as i32
    }
}

/// `parseFloat(s)`: the longest decimal-literal prefix after whitespace, or
/// `Infinity` with an optional sign; otherwise `NaN`. No hex.
pub fn parse_float(s: &str) -> f64 {
    let t = s.trim_start_matches(is_whitespace);
    let (negative, unsigned) = match t.as_bytes().first() {
        Some(b'+') => (false, &t[1..]),
        Some(b'-') => (true, &t[1..]),
        _ => (false, t),
    };
    let magnitude = if unsigned.starts_with("Infinity") {
        f64::INFINITY
    } else {
        let bytes = unsigned.as_bytes();
        let mut i = count_digits(bytes);
        let int_digits = i;
        let mut frac_digits = 0;
        if bytes.get(i) == Some(&b'.') {
            frac_digits = count_digits(&bytes[i + 1..]);
            if int_digits + frac_digits > 0 {
                i += 1 + frac_digits;
            }
        }
        if int_digits + frac_digits == 0 {
            return f64::NAN;
        }
        if matches!(bytes.get(i), Some(b'e' | b'E')) {
            let mut j = i + 1;
            if matches!(bytes.get(j), Some(b'+' | b'-')) {
                j += 1;
            }
            let exp = count_digits(&bytes[j..]);
            if exp > 0 {
                i = j + exp;
            }
        }
        unsigned[..i].parse::<f64>().unwrap_or(f64::NAN)
    };
    if negative { -magnitude } else { magnitude }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_matches_math_round() {
        assert_eq!(math_round(2.5), 3.0);
        assert_eq!(math_round(-2.5), -2.0);
        assert_eq!(math_round(0.49999999999999994), 0.0);
        assert!(math_round(-0.4).is_sign_negative());
        assert!(math_round(f64::NAN).is_nan());
    }

    #[test]
    fn numbers_print_like_javascript() {
        for (n, s) in [
            (0.0, "0"),
            (-0.0, "0"),
            (1.0, "1"),
            (-1.5, "-1.5"),
            (0.1 + 0.2, "0.30000000000000004"),
            (1e21, "1e+21"),
            (1.5e21, "1.5e+21"),
            (1e20, "100000000000000000000"),
            (1e-6, "0.000001"),
            (1e-7, "1e-7"),
            (1.25e-7, "1.25e-7"),
            (123456.789, "123456.789"),
            (f64::MAX, "1.7976931348623157e+308"),
            (5e-324, "5e-324"),
            (1524785257910610.2, "1524785257910610.2"),
        ] {
            assert_eq!(number_to_string(n), s, "{n:e}");
        }
    }

    #[test]
    fn number_coercion() {
        assert_eq!(string_to_number(""), 0.0);
        assert_eq!(string_to_number("  12  "), 12.0);
        assert_eq!(string_to_number("0x1f"), 31.0);
        assert!(string_to_number("12px").is_nan());
        assert!(string_to_number("-0x1").is_nan());
        assert_eq!(string_to_number("-Infinity"), f64::NEG_INFINITY);
        assert_eq!(string_to_number(".5"), 0.5);
        assert_eq!(string_to_number("5."), 5.0);
        assert!(string_to_number(".").is_nan());
        assert_eq!(parse_int("12px", None), 12.0);
        assert_eq!(parse_int(" -0x1f", None), -31.0);
        assert!(parse_int("px", None).is_nan());
        assert_eq!(parse_int("0x10", Some(10.0)), 0.0);
        assert_eq!(parse_float("1.5e3x"), 1500.0);
        assert_eq!(parse_float("1e"), 1.0);
        assert_eq!(parse_float(".5."), 0.5);
    }

    #[test]
    fn replacement_patterns() {
        assert_eq!(replace_all("a{R}b{R}", "{R}", "$&!"), "a{R}!b{R}!");
        assert_eq!(replace_all("x", "x", "$$"), "$");
        assert_eq!(replace_all("axb", "x", "[$`|$']"), "a[a|b]b");
        assert_eq!(replace_all("x", "x", "$1$"), "$1$");
    }
}
