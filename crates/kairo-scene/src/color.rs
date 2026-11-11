//! Colours as `overlay-template.ts` writes them into CSS, and as Chromium
//! reads them back.
//!
//! `colorWithOpacity` only rewrites `#rgb`, `#rrggbb` and `rgb()`/`rgba()`
//! whose first three arguments are plain numbers; anything else passes
//! through unchanged and keeps whatever alpha it carries. A value CSS cannot
//! parse drops the whole declaration, which [`css_with_opacity`] reports as
//! `None`.

/// A straight (not premultiplied) colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const BLACK: Rgba = Rgba::new(0, 0, 0, 255);

    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

/// `sanitizeOverlayColor`: only `#rgb` and `#rrggbb` reach the slide, written
/// as upper-case six-digit hex. Everything else is dropped.
pub fn sanitize_overlay_color(raw: Option<&str>) -> Option<Rgba> {
    let t = kairo_domain::js::trim(raw?);
    let digits = t.strip_prefix('#')?;
    if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    match digits.len() {
        6 => parse_hex(digits),
        3 => {
            let d: Vec<char> = digits.chars().collect();
            let doubled: String = d.iter().flat_map(|c| [*c, *c]).collect();
            parse_hex(&doubled)
        }
        _ => None,
    }
}

fn parse_hex(digits: &str) -> Option<Rgba> {
    let channel = |i: usize| u8::from_str_radix(digits.get(i..i + 2)?, 16).ok();
    Some(Rgba::new(
        channel(0)?,
        channel(2)?,
        channel(4)?,
        if digits.len() == 8 { channel(6)? } else { 255 },
    ))
}

/// `colorWithOpacity(color, opacity)` followed by the browser parsing the
/// result as a CSS colour.
pub fn css_with_opacity(color: &str, opacity: f64) -> Option<Rgba> {
    match rewrite(color, opacity) {
        Rewritten::Rgb(r, g, b, a) => Some(Rgba::new(
            channel_from(r)?,
            channel_from(g)?,
            channel_from(b)?,
            alpha_from(a)?,
        )),
        Rewritten::Unchanged => parse_css(color),
    }
}

enum Rewritten {
    Rgb(f64, f64, f64, f64),
    Unchanged,
}

fn rewrite(color: &str, opacity: f64) -> Rewritten {
    let hex = kairo_domain::js::trim(color);
    // `Math.min(1, Math.max(0, opacity))`; NaN stays NaN.
    let a = if opacity.is_nan() {
        f64::NAN
    } else {
        opacity.clamp(0.0, 1.0)
    };
    if let Some(digits) = hex.strip_prefix('#')
        && digits.bytes().all(|b| b.is_ascii_hexdigit())
    {
        match digits.len() {
            3 => {
                let v: Vec<u8> = digits
                    .chars()
                    .map(|c| c.to_digit(16).unwrap_or(0) as u8 * 17)
                    .collect();
                return Rewritten::Rgb(f64::from(v[0]), f64::from(v[1]), f64::from(v[2]), a);
            }
            6 => {
                let c = parse_hex(digits).unwrap_or(Rgba::BLACK);
                return Rewritten::Rgb(f64::from(c.r), f64::from(c.g), f64::from(c.b), a);
            }
            _ => {}
        }
    }
    // /^rgba?\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)/
    let rest = hex
        .strip_prefix("rgba(")
        .or_else(|| hex.strip_prefix("rgb("));
    if let Some(rest) = rest {
        let mut numbers = [0.0f64; 3];
        let mut cursor = rest.trim_start_matches(kairo_domain::js::is_whitespace);
        for (i, slot) in numbers.iter_mut().enumerate() {
            let end = cursor
                .find(|c: char| !(c.is_ascii_digit() || c == '.'))
                .unwrap_or(cursor.len());
            if end == 0 {
                return Rewritten::Unchanged;
            }
            // `Number("1.2.3")` is NaN, which CSS then rejects.
            *slot = cursor[..end].parse::<f64>().unwrap_or(f64::NAN);
            cursor = cursor[end..].trim_start_matches(kairo_domain::js::is_whitespace);
            if i < 2 {
                let Some(next) = cursor.strip_prefix(',') else {
                    return Rewritten::Unchanged;
                };
                cursor = next.trim_start_matches(kairo_domain::js::is_whitespace);
            }
        }
        return Rewritten::Rgb(numbers[0], numbers[1], numbers[2], a);
    }
    Rewritten::Unchanged
}

fn channel_from(v: f64) -> Option<u8> {
    v.is_finite().then(|| v.clamp(0.0, 255.0).round() as u8)
}

fn alpha_from(v: f64) -> Option<u8> {
    v.is_finite()
        .then(|| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// The CSS colours a normalised theme can hold: `#rgb`, `#rgba`, `#rrggbb`,
/// `#rrggbbaa`, `rgb()` and `rgba()` (comma or space separated, numbers or
/// percentages), plus the three keywords the renderer writes.
pub fn parse_css(raw: &str) -> Option<Rgba> {
    let s = kairo_domain::js::trim(raw).to_ascii_lowercase();
    match s.as_str() {
        "transparent" => return Some(Rgba::new(0, 0, 0, 0)),
        "black" => return Some(Rgba::BLACK),
        "white" => return Some(Rgba::new(255, 255, 255, 255)),
        _ => {}
    }
    if let Some(digits) = s.strip_prefix('#') {
        if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        return match digits.len() {
            3 | 4 => {
                let v: Vec<u8> = digits
                    .chars()
                    .map(|c| c.to_digit(16).unwrap_or(0) as u8 * 17)
                    .collect();
                Some(Rgba::new(
                    v[0],
                    v[1],
                    v[2],
                    v.get(3).copied().unwrap_or(255),
                ))
            }
            6 | 8 => parse_hex(digits),
            _ => None,
        };
    }
    let inner = s
        .strip_prefix("rgba(")
        .or_else(|| s.strip_prefix("rgb("))?
        .strip_suffix(')')?;
    let parts: Vec<&str> = inner
        .split([',', ' ', '/'])
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    if !(parts.len() == 3 || parts.len() == 4) {
        return None;
    }
    let channel = |p: &str| -> Option<u8> {
        let v = match p.strip_suffix('%') {
            Some(pct) => pct.parse::<f64>().ok()? / 100.0 * 255.0,
            None => p.parse::<f64>().ok()?,
        };
        channel_from(v)
    };
    let alpha = match parts.get(3) {
        None => 255,
        Some(p) => {
            let v = match p.strip_suffix('%') {
                Some(pct) => pct.parse::<f64>().ok()? / 100.0,
                None => p.parse::<f64>().ok()?,
            };
            alpha_from(v)?
        }
    };
    Some(Rgba::new(
        channel(parts[0])?,
        channel(parts[1])?,
        channel(parts[2])?,
        alpha,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn css_colours_in_every_form_a_theme_writes_parse() {
        assert_eq!(parse_css("#FF0000"), Some(Rgba::new(255, 0, 0, 255)));
        assert_eq!(parse_css(" #0f0 "), Some(Rgba::new(0, 255, 0, 255)));
        assert_eq!(parse_css("#ff000080"), Some(Rgba::new(255, 0, 0, 128)));
        assert_eq!(parse_css("#f008"), Some(Rgba::new(255, 0, 0, 136)));
        assert_eq!(
            parse_css("rgba(255, 0, 0, 0.5)"),
            Some(Rgba::new(255, 0, 0, 128))
        );
        assert_eq!(
            parse_css("rgb(0 0 255 / 50%)"),
            Some(Rgba::new(0, 0, 255, 128))
        );
        assert_eq!(
            parse_css("rgb(100%, 0%, 0%)"),
            Some(Rgba::new(255, 0, 0, 255))
        );
        assert_eq!(parse_css("transparent"), Some(Rgba::new(0, 0, 0, 0)));
        assert_eq!(parse_css("Black"), Some(Rgba::BLACK));
        for bad in ["#12", "rgb(1,2)", "url(x)", ""] {
            assert_eq!(parse_css(bad), None, "{bad}");
        }
    }

    #[test]
    fn sanitize_keeps_only_hex_three_and_six() {
        assert_eq!(
            sanitize_overlay_color(Some(" #5eead4 ")),
            Some(Rgba::new(0x5E, 0xEA, 0xD4, 255))
        );
        assert_eq!(
            sanitize_overlay_color(Some("#abc")),
            Some(Rgba::new(0xAA, 0xBB, 0xCC, 255))
        );
        for bad in [
            "red",
            "#abcd",
            "#12345g",
            "#1234567",
            "rgb(1,2,3)",
            "#fff;color:red",
            "",
            "#",
            "url(x)",
        ] {
            assert_eq!(sanitize_overlay_color(Some(bad)), None, "{bad}");
        }
        assert_eq!(sanitize_overlay_color(None), None);
    }

    #[test]
    fn opacity_replaces_alpha_for_hex_and_rgb_but_not_other_forms() {
        assert_eq!(
            css_with_opacity("#ffffff", 0.5),
            Some(Rgba::new(255, 255, 255, 128))
        );
        assert_eq!(
            css_with_opacity("#f00", 1.0),
            Some(Rgba::new(255, 0, 0, 255))
        );
        assert_eq!(
            css_with_opacity("rgba(3, 10, 20, 0.75)", 0.5),
            Some(Rgba::new(3, 10, 20, 128))
        );
        // Eight digits pass through with their own alpha.
        assert_eq!(
            css_with_opacity("#11223380", 0.2),
            Some(Rgba::new(0x11, 0x22, 0x33, 0x80))
        );
        // Percentages are not matched by the rewrite either.
        assert_eq!(
            css_with_opacity("rgb(100%, 0%, 0%)", 0.25),
            Some(Rgba::new(255, 0, 0, 255))
        );
        assert_eq!(css_with_opacity("#12345", 1.0), None);
        assert_eq!(css_with_opacity("nonsense", 1.0), None);
        assert_eq!(css_with_opacity("#000", 2.0), Some(Rgba::BLACK));
        assert_eq!(css_with_opacity("#000", -1.0), Some(Rgba::new(0, 0, 0, 0)));
        assert_eq!(css_with_opacity("#000", f64::NAN), None);
        assert_eq!(css_with_opacity("rgb(1.2.3, 4, 5)", 1.0), None);
    }
}
