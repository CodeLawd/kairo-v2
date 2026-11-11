//! `SlideScene`: what `renderOverlayHTML` described, as data.
//!
//! A scene holds no markup. The reference and verse are plain text, so
//! nothing in them can change a style: `<`, `&`, quotes and `style=` mean
//! what they say. The only text-borne styling is a line colour, and that
//! passes `sanitizeOverlayColor`.

use crate::color::{Rgba, css_with_opacity};
use crate::fit;
use crate::text::paragraphs_for;
use kairo_domain::js;
use kairo_domain::overlay::{
    OverlayOutlinePosition, OverlayTextAlign, OverlayTextDecoration, OverlayTextStyle,
    OverlayTheme, OverlayVerticalAlign,
};
use std::hash::{DefaultHasher, Hash, Hasher};

/// `OverlayColoredLine`: one line of lyric body with an optional colour.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ColoredLine {
    pub text: String,
    pub color: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BoxKind {
    Verse,
    Reference,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Align {
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Vertical {
    Top,
    Middle,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Decoration {
    None,
    Underline,
    LineThrough,
}

/// `text-shadow`: offset, blur radius (sigma is half of it) and colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub x: f32,
    pub y: f32,
    pub blur: f32,
    pub color: Rgba,
}

/// `-webkit-text-stroke`: centred on the outline, painted under the fill.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stroke {
    pub width: f32,
    pub color: Rgba,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fit {
    /// The theme's size.
    Fixed(f32),
    /// The largest whole size from 12 to `cap` that fits the box.
    Auto { cap: f32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    pub family: String,
    pub weight: f32,
    pub line_height: f32,
    pub letter_spacing: f32,
    pub align: Align,
    pub decoration: Decoration,
    pub color: Rgba,
    pub shadow: Option<Shadow>,
    pub stroke: Option<Stroke>,
}

/// One paragraph: text with whitespace collapsed and `text-transform`
/// applied, and the colour of its span when it has one.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Paragraph {
    pub text: String,
    pub color: Option<Rgba>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextBox {
    pub kind: BoxKind,
    pub rect: Rect,
    pub padding: f32,
    pub vertical: Vertical,
    /// `layout.backdropBox`: a square-cornered fill behind the text.
    pub backdrop: Option<Rgba>,
    pub size: Fit,
    pub style: TextStyle,
    pub paragraphs: Vec<Paragraph>,
}

/// Everything one output's text layer draws.
#[derive(Clone, Debug, PartialEq)]
pub struct SlideScene {
    pub frame: (u32, u32),
    /// Verse first, then the reference, as in the template.
    pub boxes: Vec<TextBox>,
}

impl SlideScene {
    /// `renderOverlayHTML` for the text layer: the verse box when `text` has
    /// something to show, the reference box when it is on and non-blank.
    /// `colored` replaces the verse body when it is non-empty.
    pub fn build(
        theme: &OverlayTheme,
        reference: &str,
        text: &str,
        frame: (u32, u32),
        colored: &[ColoredLine],
    ) -> SlideScene {
        let mut boxes = Vec::new();
        if !js::trim(text).is_empty() {
            let paragraphs = if colored.is_empty() {
                paragraphs_for(text, theme.verse.text_transform, None)
            } else {
                paragraphs_for("", theme.verse.text_transform, Some(colored))
            };
            let cap = fit::smart_fill_cap(text, theme, frame);
            let size = if theme.layout.auto_fit_text {
                Fit::Auto { cap }
            } else {
                Fit::Fixed(finite_or(theme.verse.font_size_px, 1.0) as f32)
            };
            boxes.push(text_box(
                BoxKind::Verse,
                &theme.verse,
                theme,
                frame,
                size,
                paragraphs,
            ));
        }
        if theme.reference.show && !js::trim(reference).is_empty() {
            let style = &theme.reference.text;
            let paragraphs = paragraphs_for(reference, style.text_transform, None);
            let size = Fit::Fixed(finite_or(style.font_size_px, 1.0) as f32);
            boxes.push(text_box(
                BoxKind::Reference,
                style,
                theme,
                frame,
                size,
                paragraphs,
            ));
        }
        SlideScene { frame, boxes }
    }

    /// A stable key for the raster cache: equal scenes give equal keys.
    pub fn content_hash(&self) -> u64 {
        let mut h = DefaultHasher::new();
        self.frame.hash(&mut h);
        for b in &self.boxes {
            b.kind.hash(&mut h);
            for v in [b.rect.x, b.rect.y, b.rect.width, b.rect.height, b.padding] {
                v.to_bits().hash(&mut h);
            }
            b.vertical.hash(&mut h);
            b.backdrop.hash(&mut h);
            match b.size {
                Fit::Fixed(v) => (0u8, v.to_bits()).hash(&mut h),
                Fit::Auto { cap } => (1u8, cap.to_bits()).hash(&mut h),
            }
            let s = &b.style;
            s.family.hash(&mut h);
            for v in [s.weight, s.line_height, s.letter_spacing] {
                v.to_bits().hash(&mut h);
            }
            (s.align, s.decoration, s.color).hash(&mut h);
            if let Some(sh) = s.shadow {
                (
                    1u8,
                    sh.x.to_bits(),
                    sh.y.to_bits(),
                    sh.blur.to_bits(),
                    sh.color,
                )
                    .hash(&mut h);
            } else {
                0u8.hash(&mut h);
            }
            if let Some(st) = s.stroke {
                (1u8, st.width.to_bits(), st.color).hash(&mut h);
            } else {
                0u8.hash(&mut h);
            }
            b.paragraphs.hash(&mut h);
        }
        h.finish()
    }
}

fn finite_or(v: f64, fallback: f64) -> f64 {
    if v.is_finite() { v } else { fallback }
}

fn text_box(
    kind: BoxKind,
    style: &OverlayTextStyle,
    theme: &OverlayTheme,
    frame: (u32, u32),
    size: Fit,
    paragraphs: Vec<Paragraph>,
) -> TextBox {
    let (fw, fh) = (f64::from(frame.0), f64::from(frame.1));
    let b = &style.frame;
    let size = match size {
        Fit::Fixed(v) => Fit::Fixed(v.clamp(1.0, 4000.0)),
        auto => auto,
    };
    TextBox {
        kind,
        rect: Rect {
            x: (fw * finite_or(b.x_pct, 0.0) / 100.0) as f32,
            y: (fh * finite_or(b.y_pct, 0.0) / 100.0) as f32,
            width: (fw * finite_or(b.width_pct, 0.0) / 100.0) as f32,
            height: (fh * finite_or(b.height_pct, 0.0) / 100.0) as f32,
        },
        padding: finite_or(theme.layout.padding_px, 0.0).max(0.0) as f32,
        vertical: match style.vertical_align {
            OverlayVerticalAlign::Top => Vertical::Top,
            OverlayVerticalAlign::Middle => Vertical::Middle,
            OverlayVerticalAlign::Bottom => Vertical::Bottom,
        },
        // A backdrop the browser cannot parse is a dropped declaration.
        backdrop: if theme.layout.backdrop_box {
            crate::color::parse_css(&theme.layout.backdrop_color)
        } else {
            None
        },
        size,
        style: text_style(style),
        paragraphs,
    }
}

fn text_style(style: &OverlayTextStyle) -> TextStyle {
    // An unparsable colour drops `color`, and the text inherits the
    // document's black.
    let color = css_with_opacity(&style.color, style.color_opacity).unwrap_or(Rgba::BLACK);
    let shadow = &style.shadow;
    let shadow = if shadow.enabled {
        css_with_opacity(&shadow.color, shadow.opacity).map(|color| Shadow {
            x: finite_or(shadow.x_px, 0.0) as f32,
            y: finite_or(shadow.y_px, 0.0) as f32,
            blur: finite_or(shadow.blur_px, 0.0).max(0.0) as f32,
            color,
        })
    } else {
        None
    };
    let outline = &style.outline;
    // `outlineCss`: inside outlines use 0.55 of the width, at least 0.5;
    // dashed and dotted fall back to solid because text-stroke has no dash.
    let stroke = if outline.enabled && outline.width_px > 0.0 {
        let width = if outline.position == OverlayOutlinePosition::Inside {
            (outline.width_px * 0.55).max(0.5)
        } else {
            outline.width_px
        };
        css_with_opacity(&outline.color, outline.opacity).map(|color| Stroke {
            width: width as f32,
            color,
        })
    } else {
        None
    };
    TextStyle {
        family: style.font_family.clone(),
        weight: finite_or(style.font_weight, 400.0).clamp(1.0, 1000.0) as f32,
        line_height: finite_or(style.line_height, 1.0).max(0.0) as f32,
        letter_spacing: finite_or(style.letter_spacing_px, 0.0) as f32,
        align: match style.align {
            OverlayTextAlign::Left => Align::Left,
            OverlayTextAlign::Center => Align::Center,
            OverlayTextAlign::Right => Align::Right,
            OverlayTextAlign::Justify => Align::Justify,
        },
        decoration: match style.text_decoration {
            OverlayTextDecoration::None => Decoration::None,
            OverlayTextDecoration::Underline => Decoration::Underline,
            OverlayTextDecoration::LineThrough => Decoration::LineThrough,
        },
        color,
        shadow,
        stroke,
    }
}

/// Parse `OverlayColoredLine[]` as the UI sends it. `None` when any entry is
/// not an object with a string `text`: the template would throw on it.
pub fn colored_lines_from_json(value: &serde_json::Value) -> Option<Vec<ColoredLine>> {
    let items = value.as_array()?;
    items
        .iter()
        .map(|item| {
            let text = item.get("text")?.as_str()?.to_owned();
            let color = item
                .get("color")
                .and_then(|c| c.as_str())
                .map(str::to_owned);
            Some(ColoredLine { text, color })
        })
        .collect()
}
