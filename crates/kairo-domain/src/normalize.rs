//! Validation primitives for settings normalisers, from `src/lib/normalize.ts`.
//!
//! Each reads one raw JSON value, as found in a store file, and returns a
//! safe value or the fallback. `None` stands for an absent key (`undefined`).

use crate::js;
use serde_json::{Map, Value};

/// `COLOR_RE`: `/^#[0-9a-fA-F]{3,8}$|^rgba?\([\d.,\s%]+\)$/`.
pub fn is_color(s: &str) -> bool {
    if let Some(hex) = s.strip_prefix('#') {
        return (3..=8).contains(&hex.len()) && hex.bytes().all(|b| b.is_ascii_hexdigit());
    }
    let inner = s
        .strip_prefix("rgba(")
        .or_else(|| s.strip_prefix("rgb("))
        .and_then(|rest| rest.strip_suffix(')'));
    inner.is_some_and(|inner| {
        !inner.is_empty()
            && inner
                .chars()
                .all(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | '%') || js::is_whitespace(c))
    })
}

/// `isPlainObject`: an object, not an array or `null`.
pub fn is_plain_object(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Object(_)))
}

static EMPTY: std::sync::LazyLock<Map<String, Value>> = std::sync::LazyLock::new(Map::new);

/// `asObject`: the object, or an empty one for anything else.
pub fn as_object(v: Option<&Value>) -> &Map<String, Value> {
    match v {
        Some(Value::Object(map)) => map,
        _ => &EMPTY,
    }
}

/// `safeString`.
pub fn safe_string(v: Option<&Value>, fallback: &str) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        _ => fallback.to_owned(),
    }
}

/// `safeBool`.
pub fn safe_bool(v: Option<&Value>, fallback: bool) -> bool {
    match v {
        Some(Value::Bool(b)) => *b,
        _ => fallback,
    }
}

/// The number in `v`, if it is one. JSON numbers are always finite.
pub fn number(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64).filter(|n| n.is_finite())
}

/// `safeNumber`: a finite number, else the fallback.
pub fn safe_number(v: Option<&Value>, fallback: f64) -> f64 {
    number(v).unwrap_or(fallback)
}

/// `clampNum`: [`safe_number`] clamped to `min..=max`. The fallback is clamped too.
pub fn clamp_num(v: Option<&Value>, min: f64, max: f64, fallback: f64) -> f64 {
    js::clamp(safe_number(v, fallback), min, max)
}

/// `nonNegNumber`: a finite number that is not negative, else the fallback.
pub fn non_neg_number(v: Option<&Value>, fallback: f64) -> f64 {
    let n = safe_number(v, fallback);
    if n < 0.0 { fallback } else { n }
}

/// `safeColor`: a trimmed colour matching [`is_color`], else the fallback.
pub fn safe_color(v: Option<&Value>, fallback: &str) -> String {
    match v {
        Some(Value::String(s)) if is_color(js::trim(s)) => js::trim(s).to_owned(),
        _ => fallback.to_owned(),
    }
}

/// `safeEnum` for the string enums in this crate: the parsed value when `v`
/// is one of its names, else the fallback.
pub fn safe_enum<T: StringEnum>(v: Option<&Value>, fallback: T) -> T {
    match v {
        Some(Value::String(s)) => T::from_name(s).unwrap_or(fallback),
        _ => fallback,
    }
}

/// `safeEnum` with an explicit allowed list, for TypeScript call sites that
/// accept only part of an enum.
pub fn safe_enum_in<T: StringEnum + Copy>(v: Option<&Value>, allowed: &[T], fallback: T) -> T {
    match v {
        Some(Value::String(s)) => allowed
            .iter()
            .copied()
            .find(|candidate| candidate.name() == s)
            .unwrap_or(fallback),
        _ => fallback,
    }
}

/// A string-literal union from `serde_util::string_enum!`.
pub trait StringEnum: Sized {
    fn from_name(name: &str) -> Option<Self>;
    fn name(&self) -> &'static str;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours() {
        for ok in [
            "#fff",
            "#12345678",
            "rgb(1, 2, 3)",
            "rgba(3, 10, 20, 0.75)",
            "rgb( % )",
        ] {
            assert!(is_color(ok), "{ok}");
        }
        for bad in [
            "#ff",
            "#123456789",
            "red",
            "rgb()",
            "rgb(1,2,3",
            "rgba(1,2,3) ",
            "#ggg",
            "url(x)",
        ] {
            assert!(!is_color(bad), "{bad}");
        }
    }
}
