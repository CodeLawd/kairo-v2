//! The shape catalogue as tiles (`ShapeGallery.tsx`): what the Add menu and
//! the element panel's "Change shape" offer, and the pictures on them.

use std::sync::Arc;

use gpui_kit::{Hsla, RenderImage, Rgba};
use image::{Frame, RgbaImage};
use kairo_domain::overlay::{OverlayElementKind, OverlayElementStroke};
use kairo_domain::overlay_boxes::overlay_box;
use kairo_domain::overlay_elements::create_overlay_element;
use kairo_domain::overlay_shapes::OVERLAY_SHAPE_GROUPS;
use kairo_domain::serde_util::Extra;
use kairo_scene::vector::render_vector;

/// What a gallery tile adds: a catalogue shape, or one of the box-drawn kinds.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapePick {
    pub kind: OverlayElementKind,
    pub shape: Option<String>,
    pub radius_px: f64,
}

/// One tile of the gallery.
#[derive(Clone, Debug, PartialEq)]
pub struct Tile {
    pub key: String,
    pub label: String,
    pub line_only: bool,
    pub pick: ShapePick,
}

/// The tile's pixel size in the grid, at device scale 2.
pub const THUMB_PX: u32 = 36;
/// `viewBox="-6 -6 112 112"`: the shape sits inset, so a stroke is not cut.
const INSET_PCT: f64 = 6.0 / 112.0 * 100.0;

fn tile(key: &str, label: &str, kind: OverlayElementKind, radius_px: f64) -> Tile {
    Tile {
        key: key.into(),
        label: label.into(),
        line_only: false,
        pick: ShapePick {
            kind,
            shape: None,
            radius_px,
        },
    }
}

/// Rectangle and ellipse draw from their box (radius, inset border), so they
/// lead their groups as their own kinds. `include_box_shapes` is off where a
/// box-drawn kind cannot be swapped in (changing a catalogue shape's outline).
pub fn groups(include_box_shapes: bool) -> Vec<(&'static str, Vec<Tile>)> {
    OVERLAY_SHAPE_GROUPS
        .iter()
        .map(|group| {
            let mut tiles = Vec::new();
            if include_box_shapes {
                match group.label {
                    "Rectangles" => {
                        tiles.push(tile(
                            "rectangle",
                            "Rectangle",
                            OverlayElementKind::Rectangle,
                            0.0,
                        ));
                        tiles.push(tile(
                            "rounded-rectangle",
                            "Rounded rectangle",
                            OverlayElementKind::Rectangle,
                            48.0,
                        ));
                    }
                    "Basic shapes" => {
                        tiles.push(tile("ellipse", "Ellipse", OverlayElementKind::Ellipse, 0.0));
                    }
                    _ => {}
                }
            }
            tiles.extend(group.shapes.iter().map(|shape| Tile {
                key: shape.id.into(),
                label: shape.label.into(),
                line_only: shape.line_only,
                pick: ShapePick {
                    kind: OverlayElementKind::Shape,
                    shape: Some(shape.id.into()),
                    radius_px: 0.0,
                },
            }));
            (group.label, tiles)
        })
        .collect()
}

/// The tile's outline drawn in `color` (fill at 18 %, as `ShapeThumb`), as an
/// image GPUI can show. `None` when the shape draws nothing.
pub fn thumb(tile: &Tile, color: Hsla) -> Option<Arc<RenderImage>> {
    let hex = hex(color);
    let mut el = create_overlay_element(tile.pick.kind, None, "thumb", tile.pick.shape.as_deref());
    el.frame = overlay_box(
        INSET_PCT,
        INSET_PCT,
        100.0 - 2.0 * INSET_PCT,
        100.0 - 2.0 * INSET_PCT,
    );
    el.fill = hex.clone();
    el.fill_opacity = if tile.line_only { 0.0 } else { 0.18 };
    el.stroke = OverlayElementStroke {
        enabled: true,
        color: hex,
        opacity: 1.0,
        width_px: 2.0,
        extra: Extra::new(),
    };
    // The 15 % corner of the rounded rectangle's outline, in tile pixels.
    el.radius_px =
        (tile.pick.radius_px / 48.0) * 0.15 * f64::from(THUMB_PX) * (1.0 - 2.0 * INSET_PCT / 100.0);
    let drawn = render_vector(&el, (THUMB_PX, THUMB_PX))?;
    let mut pixels = vec![0u8; (THUMB_PX * THUMB_PX * 4) as usize];
    for row in 0..drawn.height {
        for col in 0..drawn.width {
            let (x, y) = (drawn.x + col as i32, drawn.y + row as i32);
            if x < 0 || y < 0 || x >= THUMB_PX as i32 || y >= THUMB_PX as i32 {
                continue;
            }
            let from = ((row * drawn.width + col) * 4) as usize;
            let to = ((y as u32 * THUMB_PX + x as u32) * 4) as usize;
            // Premultiplied RGBA to the BGRA GPUI reads.
            pixels[to] = drawn.pixels[from + 2];
            pixels[to + 1] = drawn.pixels[from + 1];
            pixels[to + 2] = drawn.pixels[from];
            pixels[to + 3] = drawn.pixels[from + 3];
        }
    }
    let buffer = RgbaImage::from_raw(THUMB_PX, THUMB_PX, pixels)?;
    Some(Arc::new(RenderImage::new([Frame::new(buffer)])))
}

fn hex(color: Hsla) -> String {
    let c = Rgba::from(color);
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", byte(c.r), byte(c.g), byte(c.b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::hsla;
    use std::collections::HashSet;

    #[test]
    fn box_kinds_lead_their_groups_and_every_shape_appears_once() {
        let all = groups(true);
        let rectangles = &all.iter().find(|(l, _)| *l == "Rectangles").unwrap().1;
        assert_eq!(rectangles[0].pick.kind, OverlayElementKind::Rectangle);
        assert_eq!(rectangles[1].pick.radius_px, 48.0);
        let basic = &all.iter().find(|(l, _)| *l == "Basic shapes").unwrap().1;
        assert_eq!(basic[0].pick.kind, OverlayElementKind::Ellipse);
        let keys: Vec<_> = all
            .iter()
            .flat_map(|(_, t)| t.iter().map(|t| t.key.clone()))
            .collect();
        assert_eq!(keys.len(), keys.iter().collect::<HashSet<_>>().len());
        let catalogue: usize = OVERLAY_SHAPE_GROUPS.iter().map(|g| g.shapes.len()).sum();
        assert_eq!(keys.len(), catalogue + 3);
    }

    #[test]
    fn changing_a_shape_offers_only_catalogue_shapes() {
        for (_, tiles) in groups(false) {
            assert!(
                tiles
                    .iter()
                    .all(|t| t.pick.kind == OverlayElementKind::Shape)
            );
        }
    }

    #[test]
    fn every_tile_draws() {
        let color = hsla(0.0, 0.0, 0.8, 1.0);
        for (_, tiles) in groups(true) {
            for t in tiles {
                assert!(thumb(&t, color).is_some(), "{} did not draw", t.key);
            }
        }
    }
}
