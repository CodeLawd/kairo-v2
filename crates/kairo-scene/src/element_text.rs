//! A theme's own text boxes (`OverlayElementKind::Text`), drawn by the same
//! typesetter and raster as the verse, so they look the same on every screen.

use crate::color::{Rgba, css_with_opacity};
use crate::scene::{
    Align, BoxKind, Decoration, Fit, LINE_HEIGHT_NORMAL, Paragraph, Rect, SlideScene, Stroke,
    TextBox, TextStyle, Vertical,
};
use crate::text::collapse;
use crate::typeset::FONT_STACKS;
use crate::vector::{Tile, snapped_box};
use kairo_domain::overlay::{OverlayElement, OverlayElementKind, OverlayTextAlign};
use kairo_domain::overlay_elements::{TEXT_DEFAULT_SIZE_PX, TEXT_DEFAULT_WEIGHT};

/// The family a text element draws in when it names none.
pub fn default_family() -> &'static str {
    // Source Sans 3 is bundled with every screen, so it looks the same everywhere.
    FONT_STACKS
        .iter()
        .find(|(label, _)| *label == "Source Sans 3")
        .map_or("sans-serif", |(_, stack)| stack)
}

/// The scene a text element draws: one box, its lines as paragraphs.
pub fn text_scene(el: &OverlayElement, size: (u32, u32)) -> Option<SlideScene> {
    if el.kind != OverlayElementKind::Text {
        return None;
    }
    let text = el.text.as_deref()?;
    let paragraphs: Vec<Paragraph> = text
        .split('\n')
        .map(|line| Paragraph {
            text: collapse(line),
            color: None,
        })
        .collect();
    if paragraphs.iter().all(|p| p.text.trim().is_empty()) {
        return None;
    }
    let (x, y, w, h) = snapped_box(&el.frame, size);
    if !(w > 0.0 && h > 0.0) {
        return None;
    }
    let color = css_with_opacity(&el.fill, el.fill_opacity).unwrap_or(Rgba::BLACK);
    let stroke = (el.stroke.enabled && el.stroke.width_px > 0.0)
        .then(|| css_with_opacity(&el.stroke.color, el.stroke.opacity))
        .flatten()
        .map(|color| Stroke {
            width: el.stroke.width_px as f32,
            color,
        });
    let style = TextStyle {
        family: el
            .font_family
            .clone()
            .unwrap_or_else(|| default_family().to_owned()),
        weight: el.font_weight.unwrap_or(TEXT_DEFAULT_WEIGHT) as f32,
        line_height: LINE_HEIGHT_NORMAL,
        tabular: false,
        letter_spacing: 0.0,
        align: match el.text_align.unwrap_or(OverlayTextAlign::Center) {
            OverlayTextAlign::Left => Align::Left,
            OverlayTextAlign::Center => Align::Center,
            OverlayTextAlign::Right => Align::Right,
            OverlayTextAlign::Justify => Align::Justify,
        },
        decoration: Decoration::None,
        color,
        shadow: None,
        stroke,
    };
    Some(SlideScene {
        frame: size,
        boxes: vec![TextBox {
            kind: BoxKind::Verse,
            rect: Rect {
                x: x as f32,
                y: y as f32,
                width: w as f32,
                height: h as f32,
            },
            padding: 0.0,
            vertical: Vertical::Middle,
            backdrop: None,
            size: Fit::Fixed(
                el.font_size_px
                    .unwrap_or(TEXT_DEFAULT_SIZE_PX)
                    .clamp(1.0, 4000.0) as f32,
            ),
            style,
            paragraphs,
        }],
    })
}

/// Draws a text element. `None` when it shows nothing. Rotation and flips
/// don't apply to text yet.
pub fn render_text_element(el: &OverlayElement, size: (u32, u32)) -> Option<Tile> {
    let scene = text_scene(el, size)?;
    let raster = crate::raster::rasterize(&scene)?;
    Some(Tile {
        x: raster.x as i32,
        y: raster.y as i32,
        width: raster.width,
        height: raster.height,
        pixels: raster.pixels,
    })
}

/// Draws any drawn element: text through the typesetter, the rest as vectors.
pub fn render_element(el: &OverlayElement, size: (u32, u32)) -> Option<Tile> {
    if el.kind == OverlayElementKind::Text {
        render_text_element(el, size)
    } else {
        crate::vector::render_vector(el, size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairo_domain::overlay_elements::create_overlay_element;

    #[test]
    fn a_text_element_draws_inside_its_box() {
        let el = create_overlay_element(OverlayElementKind::Text, None, "t", None);
        let tile = render_element(&el, (1920, 1080)).expect("drawn");
        let (x, y, w, h) = snapped_box(&el.frame, (1920, 1080));
        assert!(tile.width > 0 && tile.height > 0);
        assert!(f64::from(tile.x) >= x - 1.0 && f64::from(tile.y) >= y - 1.0);
        assert!(f64::from(tile.width) <= w + 2.0 && f64::from(tile.height) <= h + 2.0);
        assert!(tile.pixels.chunks(4).any(|p| p[3] > 0), "some ink");
    }

    #[test]
    fn blank_text_draws_nothing() {
        let mut el = create_overlay_element(OverlayElementKind::Text, None, "t", None);
        el.text = Some("  \n ".into());
        assert!(render_element(&el, (1920, 1080)).is_none());
    }

    #[test]
    fn a_pen_path_draws_open_as_a_line_and_closed_filled() {
        let mut el = create_overlay_element(OverlayElementKind::Path, None, "p", None);
        el.stroke.enabled = true;
        el.stroke.width_px = 6.0;
        el.path = Some("M0 0 L100 100".into());
        assert!(render_element(&el, (1920, 1080)).is_some());
        el.path = Some("M0 0 L100 0 L50 100 Z".into());
        el.stroke.enabled = false;
        let tile = render_element(&el, (1920, 1080)).expect("filled");
        assert!(tile.pixels.chunks(4).any(|p| p[3] > 0));
    }
}
