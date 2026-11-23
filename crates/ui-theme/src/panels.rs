//! The inspector's rules (prompt 087), apart from drawing: the font, weight
//! and size lists, colour text, and the edits a panel makes to a theme.
//! Sources: `ThemeTypePanel.tsx`, `ThemeLayoutPanel.tsx`,
//! `ThemeElementPanel.tsx`, `fields.tsx` and `ThemeEditor.tsx`.

use kairo_domain::js;
use kairo_domain::overlay::{OverlayElement, OverlayTextStyle, OverlayTheme, ReferencePosition};
use kairo_domain::overlay_boxes::place_reference_against_verse;
use kairo_domain::overlay_shapes::overlay_shape;

use crate::canvas::{REFERENCE, VERSE};

/// `WEIGHTS`.
pub const WEIGHTS: [(&str, f64); 9] = [
    ("Thin", 100.0),
    ("Extra Light", 200.0),
    ("Light", 300.0),
    ("Regular", 400.0),
    ("Medium", 500.0),
    ("Semibold", 600.0),
    ("Bold", 700.0),
    ("Extra Bold", 800.0),
    ("Black", 900.0),
];

/// `SIZE_PRESETS`: the sizes offered beside the size box.
pub const SIZE_PRESETS: [f64; 13] = [
    18.0, 24.0, 28.0, 32.0, 36.0, 42.0, 48.0, 54.0, 60.0, 72.0, 84.0, 96.0, 120.0,
];

/// The size box's range.
pub const FONT_SIZE_MIN: f64 = 12.0;
pub const FONT_SIZE_MAX: f64 = 200.0;

/// The colour `parseColorParts` falls back to for a backdrop it cannot read.
pub const BACKDROP_FALLBACK: (&str, f64) = ("#030a14", 0.75);

/// One choice in a list: what is shown and what is stored.
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub label: String,
    pub value: String,
}

/// The font list: `FONT_STACKS`, then the theme's own stack when it is not
/// one of them (a theme saved by an older version), so it still shows.
pub fn font_options(current: &str) -> Vec<Choice> {
    let mut options: Vec<Choice> = kairo_scene::FONT_STACKS
        .iter()
        .map(|(label, value)| Choice {
            label: (*label).to_owned(),
            value: (*value).to_owned(),
        })
        .collect();
    if !options.iter().any(|o| o.value == current) {
        options.push(Choice {
            label: stack_label(current),
            value: current.to_owned(),
        });
    }
    options
}

/// A CSS font stack's first family, unquoted: what an unlisted stack is
/// called in the list.
pub fn stack_label(stack: &str) -> String {
    let first = stack
        .split(',')
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches(['\'', '"'])
        .trim();
    if first.is_empty() {
        "Default font".to_owned()
    } else {
        first.to_owned()
    }
}

/// `nearestWeight`: the listed weight closest to `weight`; a tie goes to
/// the lighter one.
pub fn nearest_weight(weight: f64) -> f64 {
    WEIGHTS.iter().fold(WEIGHTS[0].1, |best, (_, value)| {
        if (value - weight).abs() < (best - weight).abs() {
            *value
        } else {
            best
        }
    })
}

/// `FontSizeField`'s commit on blur or Enter. Returns the text to show and
/// the new size when it changed. Empty or unreadable text puts the old
/// size back.
pub fn commit_font_size(draft: &str, current: f64) -> (String, Option<f64>) {
    let trimmed = draft.trim();
    let parsed = if trimmed.is_empty() {
        None
    } else {
        trimmed.parse::<f64>().ok().filter(|n| n.is_finite())
    };
    let Some(number) = parsed else {
        return (js::number_to_string(current), None);
    };
    let next = js::math_round(number).clamp(FONT_SIZE_MIN, FONT_SIZE_MAX);
    let changed = (next != current).then_some(next);
    (js::number_to_string(next), changed)
}

/// `expandHex`: `#rgb` and `#rrggbb`, lower case; anything else is `None`.
pub fn expand_hex(value: &str) -> Option<String> {
    let t = value.trim();
    let digits = t.strip_prefix('#')?;
    if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    match digits.len() {
        6 => Some(t.to_ascii_lowercase()),
        3 => {
            let mut out = String::from("#");
            for c in digits.chars() {
                out.push(c);
                out.push(c);
            }
            Some(out.to_ascii_lowercase())
        }
        _ => None,
    }
}

/// The hex box's text: six digits, upper case, without `#`; a colour that
/// is not hex shows as written.
pub fn hex_display(color: &str) -> String {
    expand_hex(color)
        .unwrap_or_else(|| color.to_owned())
        .trim_start_matches('#')
        .to_ascii_uppercase()
}

/// What typing in the hex box stores: up to six hex digits as `#…`.
/// Anything else is refused (`None`) and the box keeps its colour.
pub fn typed_hex(raw: &str) -> Option<String> {
    let digits = raw.trim();
    let digits = digits.strip_prefix('#').unwrap_or(digits);
    (digits.len() <= 6 && digits.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| format!("#{digits}"))
}

/// The hex box on blur: a short or partial colour written out in full,
/// `None` when it is not a colour yet.
pub fn settled_hex(color: &str) -> Option<String> {
    if color.starts_with('#') {
        expand_hex(color)
    } else {
        expand_hex(&format!("#{color}"))
    }
}

/// `parseColorParts`: a backdrop colour as hex and opacity.
pub fn parse_color_parts(value: &str) -> (String, f64) {
    if let Some(hex) = expand_hex(value) {
        return (hex, 1.0);
    }
    let t = value.trim();
    let inner = t
        .strip_prefix("rgba(")
        .or_else(|| t.strip_prefix("rgb("))
        .and_then(|r| r.strip_suffix(')'));
    if let Some(inner) = inner {
        let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
        let number = |s: &str| -> Option<f64> {
            (!s.is_empty() && s.chars().all(|c| c.is_ascii_digit() || c == '.'))
                .then(|| s.parse::<f64>().ok())
                .flatten()
        };
        if (parts.len() == 3 || parts.len() == 4)
            && let (Some(r), Some(g), Some(b)) =
                (number(parts[0]), number(parts[1]), number(parts[2]))
        {
            let alpha = match parts.get(3) {
                Some(a) => match number(a) {
                    Some(a) => a.clamp(0.0, 1.0),
                    None => return fallback(),
                },
                None => 1.0,
            };
            let channel = |v: f64| format!("{:02x}", js::math_round(v) as i64);
            return (
                format!("#{}{}{}", channel(r), channel(g), channel(b)),
                alpha,
            );
        }
    }
    fallback()
}

fn fallback() -> (String, f64) {
    (BACKDROP_FALLBACK.0.to_owned(), BACKDROP_FALLBACK.1)
}

/// `colorWithOpacity` in `overlay-template.ts`: a hex colour as `rgba()`;
/// anything else unchanged.
pub fn color_with_opacity(color: &str, opacity: f64) -> String {
    let Some(hex) = expand_hex(color) else {
        return color.to_owned();
    };
    let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0);
    format!(
        "rgba({}, {}, {}, {})",
        channel(1),
        channel(3),
        channel(5),
        js::number_to_string(opacity.clamp(0.0, 1.0))
    )
}

/// The text style of a text layer.
pub fn layer_style<'a>(theme: &'a OverlayTheme, layer: &str) -> &'a OverlayTextStyle {
    if layer == REFERENCE {
        &theme.reference.text
    } else {
        &theme.verse
    }
}

/// Changes one text layer's style.
pub fn edit_layer(
    theme: &OverlayTheme,
    layer: &str,
    edit: impl FnOnce(&mut OverlayTextStyle),
) -> OverlayTheme {
    let mut next = theme.clone();
    if layer == REFERENCE {
        edit(&mut next.reference.text);
    } else {
        edit(&mut next.verse);
    }
    next
}

/// Size follows the box: the verse with Fit to box on.
pub fn automatic_size(theme: &OverlayTheme, layer: &str) -> bool {
    layer != REFERENCE && theme.layout.auto_fit_text
}

/// A typed size. On the verse with Fit to box on, the size turns fitting
/// off, so it takes effect (`theme-text-size.integration.cjs`).
pub fn set_font_size(theme: &OverlayTheme, layer: &str, size: f64) -> OverlayTheme {
    let mut next = edit_layer(theme, layer, |s| s.font_size_px = size);
    if automatic_size(theme, layer) {
        next.layout.auto_fit_text = false;
    }
    next
}

/// The reference's Position: the box moves to the verse's other side.
pub fn set_reference_position(theme: &OverlayTheme, position: ReferencePosition) -> OverlayTheme {
    let mut next = theme.clone();
    next.reference.position = position;
    next.reference.text.frame =
        place_reference_against_verse(&theme.verse.frame, &theme.reference.text.frame, position);
    next
}

/// The Stroke checkbox: turning it on with no width gives it 2 px.
pub fn set_outline_enabled(theme: &OverlayTheme, layer: &str, enabled: bool) -> OverlayTheme {
    edit_layer(theme, layer, |s| {
        s.outline.enabled = enabled;
        if enabled && s.outline.width_px == 0.0 {
            s.outline.width_px = 2.0;
        }
    })
}

/// The element's Shape list: a line needs a visible stroke. `false` when
/// the shape is unknown.
pub fn set_shape(el: &mut OverlayElement, shape: &str) -> bool {
    let Some(def) = overlay_shape(Some(shape)) else {
        return false;
    };
    el.shape = Some(def.id.to_owned());
    if def.line_only && !el.stroke.enabled {
        el.stroke.enabled = true;
    }
    true
}

/// The adjustments an image or video carries; `None` is neutral.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Adjustments {
    pub hue: f64,
    pub saturation: f64,
    pub brightness: f64,
    pub contrast: f64,
    pub blur_px: f64,
}

/// `NEUTRAL_ADJUSTMENTS`.
pub const NEUTRAL: Adjustments = Adjustments {
    hue: 0.0,
    saturation: 1.0,
    brightness: 1.0,
    contrast: 1.0,
    blur_px: 0.0,
};

impl Adjustments {
    /// Reset shows only when something is not neutral.
    pub fn adjusted(&self) -> bool {
        *self != NEUTRAL
    }

    pub fn of_element(el: &OverlayElement) -> Self {
        Self {
            hue: el.hue,
            saturation: el.saturation,
            brightness: el.brightness,
            contrast: el.contrast,
            blur_px: el.blur_px,
        }
    }

    pub fn of_background(theme: &OverlayTheme) -> Self {
        let bg = &theme.background;
        Self {
            hue: bg.hue.unwrap_or(NEUTRAL.hue),
            saturation: bg.saturation.unwrap_or(NEUTRAL.saturation),
            brightness: bg.brightness.unwrap_or(NEUTRAL.brightness),
            contrast: bg.contrast.unwrap_or(NEUTRAL.contrast),
            blur_px: bg.blur_px.unwrap_or(NEUTRAL.blur_px),
        }
    }
}

/// `pct`: a 0–1 value as a whole percentage.
pub fn pct(v: f64) -> String {
    format!("{}%", js::math_round(v * 100.0))
}

/// The selected text layer for the Text and Layout tabs: anything but the
/// reference is the verse.
pub fn text_layer(selected: Option<&str>) -> &'static str {
    if selected == Some(REFERENCE) {
        REFERENCE
    } else {
        VERSE
    }
}

/// The picked file's name, as `MediaPicker` shows it.
pub fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairo_domain::overlay_defaults::DEFAULT_OVERLAY_THEME;

    #[test]
    fn the_font_list_is_the_stacks_and_keeps_an_older_one() {
        let base = kairo_scene::FONT_STACKS.len();
        let listed = kairo_scene::FONT_STACKS[2].1;
        assert_eq!(font_options(listed).len(), base);
        let old = "'Source Sans Pro', Arial, sans-serif";
        let options = font_options(old);
        assert_eq!(options.len(), base + 1);
        assert_eq!(options[base].label, "Source Sans Pro");
        assert_eq!(options[base].value, old);
        assert_eq!(stack_label(""), "Default font");
    }

    #[test]
    fn weights_round_to_the_nearest_listed_one() {
        assert_eq!(nearest_weight(650.0), 600.0);
        assert_eq!(nearest_weight(660.0), 700.0);
        assert_eq!(nearest_weight(1000.0), 900.0);
        assert_eq!(nearest_weight(0.0), 100.0);
    }

    #[test]
    fn a_typed_size_is_rounded_clamped_or_put_back() {
        assert_eq!(commit_font_size("73", 48.0), ("73".into(), Some(73.0)));
        assert_eq!(commit_font_size(" 72.6 ", 48.0), ("73".into(), Some(73.0)));
        assert_eq!(commit_font_size("5", 48.0), ("12".into(), Some(12.0)));
        assert_eq!(commit_font_size("900", 48.0), ("200".into(), Some(200.0)));
        assert_eq!(commit_font_size("", 48.0), ("48".into(), None));
        assert_eq!(commit_font_size("big", 48.0), ("48".into(), None));
        assert_eq!(commit_font_size("48", 48.0), ("48".into(), None));
    }

    #[test]
    fn hex_text_expands_filters_and_settles() {
        assert_eq!(expand_hex("#ABC"), Some("#aabbcc".into()));
        assert_eq!(expand_hex(" #A0B1C2 "), Some("#a0b1c2".into()));
        assert_eq!(expand_hex("#abcd"), None);
        assert_eq!(hex_display("#abc"), "AABBCC");
        assert_eq!(hex_display("rgba(0,0,0,0.5)"), "RGBA(0,0,0,0.5)");
        assert_eq!(typed_hex("ff0"), Some("#ff0".into()));
        assert_eq!(typed_hex("#ff00"), Some("#ff00".into()));
        assert_eq!(typed_hex("ff00zz"), None);
        assert_eq!(typed_hex("1234567"), None);
        assert_eq!(settled_hex("#f00"), Some("#ff0000".into()));
        assert_eq!(settled_hex("#ff00"), None);
    }

    #[test]
    fn backdrop_colours_split_and_join() {
        assert_eq!(parse_color_parts("#FFF"), ("#ffffff".into(), 1.0));
        assert_eq!(
            parse_color_parts("rgba(3, 10, 20, 0.5)"),
            ("#030a14".into(), 0.5)
        );
        assert_eq!(parse_color_parts("rgb(255,0,0)"), ("#ff0000".into(), 1.0));
        assert_eq!(parse_color_parts("rgba(1,2,3,7)").1, 1.0);
        assert_eq!(parse_color_parts("hsl(1,2%,3%)"), ("#030a14".into(), 0.75));
        assert_eq!(color_with_opacity("#030a14", 0.75), "rgba(3, 10, 20, 0.75)");
        assert_eq!(color_with_opacity("#fff", 2.0), "rgba(255, 255, 255, 1)");
        assert_eq!(color_with_opacity("red", 0.5), "red");
    }

    #[test]
    fn a_size_on_a_fitted_verse_turns_fitting_off_but_not_on_the_reference() {
        let mut theme = DEFAULT_OVERLAY_THEME.clone();
        theme.layout.auto_fit_text = true;
        let next = set_font_size(&theme, VERSE, 73.0);
        assert_eq!(next.verse.font_size_px, 73.0);
        assert!(!next.layout.auto_fit_text);
        let next = set_font_size(&theme, REFERENCE, 40.0);
        assert_eq!(next.reference.text.font_size_px, 40.0);
        assert!(next.layout.auto_fit_text);
    }

    #[test]
    fn stroke_gets_a_width_and_reference_moves_with_its_position() {
        let mut theme = DEFAULT_OVERLAY_THEME.clone();
        theme.verse.outline.width_px = 0.0;
        let on = set_outline_enabled(&theme, VERSE, true);
        assert!(on.verse.outline.enabled);
        assert_eq!(on.verse.outline.width_px, 2.0);
        let off = set_outline_enabled(&on, VERSE, false);
        assert_eq!(off.verse.outline.width_px, 2.0);

        let other = match theme.reference.position {
            ReferencePosition::Above => ReferencePosition::Below,
            ReferencePosition::Below => ReferencePosition::Above,
        };
        let moved = set_reference_position(&theme, other);
        assert_eq!(moved.reference.position, other);
        assert_eq!(
            moved.reference.text.frame,
            place_reference_against_verse(&theme.verse.frame, &theme.reference.text.frame, other)
        );
    }

    #[test]
    fn adjustments_and_names() {
        let theme = DEFAULT_OVERLAY_THEME.clone();
        assert!(!Adjustments::of_background(&theme).adjusted());
        assert!(
            Adjustments {
                blur_px: 2.0,
                ..NEUTRAL
            }
            .adjusted()
        );
        assert_eq!(pct(0.754), "75%");
        assert_eq!(file_name("/a/b/c.png"), "c.png");
        assert_eq!(file_name("C:\\x\\y.mp4"), "y.mp4");
        assert_eq!(text_layer(Some("el-1")), VERSE);
        assert_eq!(text_layer(Some(REFERENCE)), REFERENCE);
    }
}
