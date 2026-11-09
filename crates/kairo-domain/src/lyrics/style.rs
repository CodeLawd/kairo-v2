//! Lyric line colours. Port of `src/lib/lyrics-style.ts`.

use super::js_text::trim;
use super::translate::is_gloss_line;

/// The translated line's colour by default: a light palette blue.
pub const DEFAULT_GLOSS_COLOR: &str = "#AABED7";

/// The paint colour for one lyric line. A manual override wins; otherwise
/// gloss lines `(…)` use the settings gloss colour, or the default when that
/// is blank.
pub fn resolve_lyric_line_color(
    line: &str,
    gloss_color: &str,
    override_color: Option<&str>,
) -> Option<String> {
    if let Some(color) = override_color.map(trim).filter(|c| !c.is_empty()) {
        return Some(color.to_string());
    }
    if is_gloss_line(line) {
        let color = trim(gloss_color);
        return Some(
            if color.is_empty() {
                DEFAULT_GLOSS_COLOR
            } else {
                color
            }
            .to_string(),
        );
    }
    None
}

/// A `#RRGGBB` colour in upper case; `#RGB` is expanded. `None` (a value that
/// is not a string) and anything else give [`DEFAULT_GLOSS_COLOR`].
pub fn normalize_gloss_color(raw: Option<&str>) -> String {
    let Some(raw) = raw else {
        return DEFAULT_GLOSS_COLOR.into();
    };
    let t = trim(raw);
    let hex = |s: &str| s.chars().all(|c| c.is_ascii_hexdigit());
    match t.strip_prefix('#') {
        Some(body) if body.len() == 6 && hex(body) => t.to_ascii_uppercase(),
        Some(body) if body.len() == 3 && hex(body) => {
            let mut out = String::from("#");
            for c in body.chars() {
                out.push(c.to_ascii_uppercase());
                out.push(c.to_ascii_uppercase());
            }
            out
        }
        _ => DEFAULT_GLOSS_COLOR.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // tests/lyrics-style.test.ts
    #[test]
    fn override_wins_then_gloss_then_none() {
        assert_eq!(
            resolve_lyric_line_color("(Hi)", "#111111", Some(" #FF0000 ")).as_deref(),
            Some("#FF0000")
        );
        assert_eq!(
            resolve_lyric_line_color("(Hi)", "", None).as_deref(),
            Some(DEFAULT_GLOSS_COLOR)
        );
        assert_eq!(
            resolve_lyric_line_color("(Hi)", "#123456", Some("  ")).as_deref(),
            Some("#123456")
        );
        assert_eq!(resolve_lyric_line_color("Hello", "#123456", None), None);
    }

    #[test]
    fn gloss_colors_normalise() {
        assert_eq!(normalize_gloss_color(Some("#abc")), "#AABBCC");
        assert_eq!(normalize_gloss_color(Some(" #a1b2c3 ")), "#A1B2C3");
        assert_eq!(normalize_gloss_color(Some("red")), DEFAULT_GLOSS_COLOR);
        assert_eq!(normalize_gloss_color(None), DEFAULT_GLOSS_COLOR);
    }
}
