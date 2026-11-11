//! Rectangles, ellipses and catalogue shapes: the elements of a theme that
//! are drawn rather than loaded.
//!
//! `elementHTML` made these as a `div` (rectangle, ellipse) or an `svg` with
//! one stretched path. Both are drawn here the way Chromium paints them:
//!
//! - the box is the element's percentages of the frame, snapped to whole
//!   pixels (a box background is pixel-snapped);
//! - a rectangle or ellipse has its stroke as `box-shadow: inset`, a ring
//!   inside the box that never changes its footprint, over the fill;
//! - a shape is its 100 × 100 path stretched to the box
//!   (`preserveAspectRatio="none"`), filled even-odd, with a round-joined
//!   stroke that keeps its pixel width however far the path stretches
//!   (`vector-effect: non-scaling-stroke`) and may spill past the box;
//! - rotation and flips turn the finished geometry about the box centre, so
//!   edges stay sharp instead of being resampled.

use crate::color::{Rgba, css_with_opacity};
use kairo_domain::overlay::{OverlayBox, OverlayElement, OverlayElementKind};
use kairo_domain::overlay_shapes::overlay_shape;
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};

/// Drawn pixels and where they sit in the frame.
#[derive(Clone, Debug)]
pub struct Tile {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    /// Premultiplied RGBA.
    pub pixels: Vec<u8>,
}

/// The element's box in frame pixels, snapped as Chromium snaps a box.
pub fn snapped_box(frame: &OverlayBox, size: (u32, u32)) -> (f64, f64, f64, f64) {
    let left = frame.x_pct / 100.0 * f64::from(size.0);
    let top = frame.y_pct / 100.0 * f64::from(size.1);
    let right = left + frame.width_pct / 100.0 * f64::from(size.0);
    let bottom = top + frame.height_pct / 100.0 * f64::from(size.1);
    let (x0, y0) = (left.round(), top.round());
    (x0, y0, right.round() - x0, bottom.round() - y0)
}

fn paint(color: Rgba) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(color.r, color.g, color.b, color.a);
    paint.anti_alias = true;
    paint
}

/// An element's CSS opacity, as the property clamps it.
pub fn element_opacity(opacity: f64) -> f32 {
    if opacity.is_nan() {
        return 1.0;
    }
    opacity.clamp(0.0, 1.0) as f32
}

/// Control distance of a cubic quarter circle.
const KAPPA: f32 = 0.552_285;

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.max(0.0).min(w / 2.0).min(h / 2.0);
    let mut b = PathBuilder::new();
    if r <= 0.0 {
        b.push_rect(tiny_skia::Rect::from_xywh(x, y, w, h)?);
        return b.finish();
    }
    // Quarter arcs as cubics: the usual 0.5523 control distance.
    let k = r * KAPPA;
    b.move_to(x + r, y);
    b.line_to(x + w - r, y);
    b.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    b.line_to(x + w, y + h - r);
    b.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    b.line_to(x + r, y + h);
    b.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    b.line_to(x, y + r);
    b.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    b.close();
    b.finish()
}

fn ellipse(x: f32, y: f32, w: f32, h: f32) -> Option<Path> {
    let (rx, ry) = (w / 2.0, h / 2.0);
    let (cx, cy) = (x + rx, y + ry);
    let (kx, ky) = (rx * KAPPA, ry * KAPPA);
    let mut b = PathBuilder::new();
    b.move_to(cx + rx, cy);
    b.cubic_to(cx + rx, cy + ky, cx + kx, cy + ry, cx, cy + ry);
    b.cubic_to(cx - kx, cy + ry, cx - rx, cy + ky, cx - rx, cy);
    b.cubic_to(cx - rx, cy - ky, cx - kx, cy - ry, cx, cy - ry);
    b.cubic_to(cx + kx, cy - ry, cx + rx, cy - ky, cx + rx, cy);
    b.close();
    b.finish()
}

/// `border-radius` for `radiusPx` on a box: the radius is cut back so two
/// corners never overlap.
fn corner_radius(radius: f64, w: f64, h: f64) -> f32 {
    if !radius.is_finite() || radius <= 0.0 {
        return 0.0;
    }
    radius.min(w / 2.0).min(h / 2.0) as f32
}

/// A shape's path in a 100 × 100 box, stretched to `w` × `h`.
fn catalogue_path(d: &str, w: f32, h: f32) -> Option<Path> {
    let bez = kurbo::BezPath::from_svg(d).ok()?;
    let sx = w / 100.0;
    let sy = h / 100.0;
    let mut b = PathBuilder::new();
    for el in bez.elements() {
        use kurbo::PathEl::*;
        let p = |p: kurbo::Point| (p.x as f32 * sx, p.y as f32 * sy);
        match el {
            MoveTo(a) => {
                let (x, y) = p(*a);
                b.move_to(x, y);
            }
            LineTo(a) => {
                let (x, y) = p(*a);
                b.line_to(x, y);
            }
            QuadTo(a, c) => {
                let (x1, y1) = p(*a);
                let (x, y) = p(*c);
                b.quad_to(x1, y1, x, y);
            }
            CurveTo(a, c, d) => {
                let (x1, y1) = p(*a);
                let (x2, y2) = p(*c);
                let (x, y) = p(*d);
                b.cubic_to(x1, y1, x2, y2, x, y);
            }
            ClosePath => b.close(),
        }
    }
    b.finish()
}

/// Frame bounds a rotated or flipped box covers, grown by `grow`.
fn bounds(
    origin: (f32, f32),
    size: (f32, f32),
    turn: Transform,
    grow: f32,
) -> (f32, f32, f32, f32) {
    let corners = [
        (-grow, -grow),
        (size.0 + grow, -grow),
        (-grow, size.1 + grow),
        (size.0 + grow, size.1 + grow),
    ];
    let mut min = (f32::MAX, f32::MAX);
    let mut max = (f32::MIN, f32::MIN);
    for (x, y) in corners {
        let tx = turn.sx * x + turn.kx * y + turn.tx + origin.0;
        let ty = turn.ky * x + turn.sy * y + turn.ty + origin.1;
        min = (min.0.min(tx), min.1.min(ty));
        max = (max.0.max(tx), max.1.max(ty));
    }
    (min.0, min.1, max.0, max.1)
}

/// Draws a rectangle, ellipse or shape. `None` when nothing shows: a media
/// element, an unknown shape, an empty or off-frame box.
pub fn render_vector(el: &OverlayElement, size: (u32, u32)) -> Option<Tile> {
    let (bx, by, bw, bh) = snapped_box(&el.frame, size);
    if !(bw > 0.0 && bh > 0.0) || !bx.is_finite() || !by.is_finite() {
        return None;
    }
    let (w, h) = (bw as f32, bh as f32);
    let (cx, cy) = (w / 2.0, h / 2.0);
    // flips first, then the turn, about the centre: p' = R S (p - c) + c.
    let theta = if el.rotation_deg.is_finite() {
        el.rotation_deg.to_radians() as f32
    } else {
        0.0
    };
    let (sin, cos) = theta.sin_cos();
    let fx = if el.flip_x { -1.0 } else { 1.0 };
    let fy = if el.flip_y { -1.0 } else { 1.0 };
    let (m11, m12, m21, m22) = (cos * fx, -sin * fy, sin * fx, cos * fy);
    let turn = Transform::from_row(
        m11,
        m21,
        m12,
        m22,
        cx - (m11 * cx + m12 * cy),
        cy - (m21 * cx + m22 * cy),
    );

    let stroke_on = el.stroke.enabled && el.stroke.width_px > 0.0 && el.stroke.width_px.is_finite();
    let stroke_color = if stroke_on {
        css_with_opacity(&el.stroke.color, el.stroke.opacity)
    } else {
        None
    };
    let fill_color = css_with_opacity(&el.fill, el.fill_opacity);

    let shape = if el.kind == OverlayElementKind::Shape {
        Some(overlay_shape(el.shape.as_deref())?)
    } else {
        None
    };
    // A shape's stroke may spill past its box; nothing else does.
    let spill = match (shape, stroke_color) {
        (Some(_), Some(_)) => (el.stroke.width_px as f32) / 2.0 + 2.0,
        _ => 1.0,
    };
    let (x0, y0, x1, y1) = bounds((bx as f32, by as f32), (w, h), turn, spill);
    let tx0 = x0.floor().max(0.0) as i32;
    let ty0 = y0.floor().max(0.0) as i32;
    let tx1 = x1.ceil().min(size.0 as f32) as i32;
    let ty1 = y1.ceil().min(size.1 as f32) as i32;
    if tx1 <= tx0 || ty1 <= ty0 {
        return None;
    }
    let (tw, th) = ((tx1 - tx0) as u32, (ty1 - ty0) as u32);
    let mut pixmap = Pixmap::new(tw, th)?;
    // Box space to tile space: turn about the box centre, then place.
    let to_tile =
        Transform::from_translate(bx as f32 - tx0 as f32, by as f32 - ty0 as f32).pre_concat(turn);

    match shape {
        Some(def) => {
            let path = catalogue_path(&def.path, w, h)?;
            if !def.line_only {
                // An unreadable colour is an invalid attribute: the default, black.
                let color = fill_color.unwrap_or(Rgba::BLACK);
                pixmap.fill_path(&path, &paint(color), FillRule::EvenOdd, to_tile, None);
            }
            if let Some(color) = stroke_color {
                let stroke = Stroke {
                    width: el.stroke.width_px as f32,
                    line_cap: LineCap::Round,
                    line_join: LineJoin::Round,
                    ..Stroke::default()
                };
                pixmap.stroke_path(&path, &paint(color), &stroke, to_tile, None);
            }
        }
        None => {
            let is_ellipse = el.kind == OverlayElementKind::Ellipse;
            let radius = corner_radius(el.radius_px, bw, bh);
            let outer = if is_ellipse {
                ellipse(0.0, 0.0, w, h)?
            } else {
                rounded_rect(0.0, 0.0, w, h, radius)?
            };
            if let Some(color) = fill_color {
                pixmap.fill_path(&outer, &paint(color), FillRule::Winding, to_tile, None);
            }
            if let Some(color) = stroke_color {
                let sw = el.stroke.width_px as f32;
                // `box-shadow: inset 0 0 0 W`: the border box minus the box
                // pulled in by W, whose corners are W tighter.
                let (iw, ih) = (w - 2.0 * sw, h - 2.0 * sw);
                let ring = if iw <= 0.0 || ih <= 0.0 {
                    outer.clone()
                } else {
                    let inner = if is_ellipse {
                        ellipse(sw, sw, iw, ih)?
                    } else {
                        rounded_rect(sw, sw, iw, ih, radius - sw)?
                    };
                    let mut b = PathBuilder::new();
                    b.push_path(&outer);
                    b.push_path(&inner);
                    b.finish()?
                };
                pixmap.fill_path(&ring, &paint(color), FillRule::EvenOdd, to_tile, None);
            }
        }
    }
    Some(Tile {
        x: tx0,
        y: ty0,
        width: tw,
        height: th,
        pixels: pixmap.take(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairo_domain::overlay_elements::create_overlay_element;

    fn rect(patch: impl FnOnce(&mut OverlayElement)) -> OverlayElement {
        let mut el = create_overlay_element(OverlayElementKind::Rectangle, None, "r", None);
        el.frame = OverlayBox {
            x_pct: 10.0,
            y_pct: 10.0,
            width_pct: 20.0,
            height_pct: 20.0,
            extra: Default::default(),
        };
        el.fill = "#ff0000".into();
        el.fill_opacity = 1.0;
        patch(&mut el);
        el
    }

    fn pixel(tile: &Tile, x: i32, y: i32) -> [u8; 4] {
        let i = (((y - tile.y) as u32 * tile.width + (x - tile.x) as u32) * 4) as usize;
        tile.pixels[i..i + 4].try_into().unwrap()
    }

    #[test]
    fn a_box_snaps_to_whole_pixels() {
        let b = OverlayBox {
            x_pct: 10.04,
            y_pct: 10.0,
            width_pct: 20.0,
            height_pct: 20.0,
            extra: Default::default(),
        };
        assert_eq!(snapped_box(&b, (1920, 1080)), (193.0, 108.0, 384.0, 216.0));
    }

    #[test]
    fn a_rectangle_fills_its_box() {
        let tile = render_vector(&rect(|_| {}), (1920, 1080)).unwrap();
        assert_eq!(
            (tile.x, tile.y),
            (191, 107),
            "a pixel of margin for the edge"
        );
        assert_eq!(pixel(&tile, 200, 120), [255, 0, 0, 255]);
        assert_eq!(pixel(&tile, 192, 108), [255, 0, 0, 255]);
        assert_eq!(pixel(&tile, 191, 108)[3], 0);
    }

    #[test]
    fn the_stroke_is_a_ring_inside_the_box_over_the_fill() {
        let el = rect(|el| {
            el.stroke.enabled = true;
            el.stroke.width_px = 10.0;
            el.stroke.color = "#0000ff".into();
            el.stroke.opacity = 1.0;
        });
        let tile = render_vector(&el, (1920, 1080)).unwrap();
        assert_eq!((tile.width, tile.height), (386, 218));
        assert_eq!(pixel(&tile, 192 + 4, 108 + 100), [0, 0, 255, 255]);
        assert_eq!(pixel(&tile, 192 + 40, 108 + 100), [255, 0, 0, 255]);
    }

    #[test]
    fn an_ellipse_leaves_its_corners_empty() {
        let el = rect(|el| el.kind = OverlayElementKind::Ellipse);
        let tile = render_vector(&el, (1920, 1080)).unwrap();
        assert_eq!(pixel(&tile, 193, 109)[3], 0);
        assert_eq!(pixel(&tile, 192 + 192, 108 + 108), [255, 0, 0, 255]);
    }

    #[test]
    fn a_shape_is_stretched_and_a_line_is_stroke_only() {
        let mut tri =
            create_overlay_element(OverlayElementKind::Shape, None, "t", Some("triangle"));
        tri.fill = "#00ff00".into();
        tri.fill_opacity = 1.0;
        tri.stroke.enabled = false;
        let tile = render_vector(&tri, (1920, 1080)).unwrap();
        assert!(tile.pixels.chunks(4).any(|p| p == [0, 255, 0, 255]));
        let mut line = create_overlay_element(OverlayElementKind::Shape, None, "l", Some("line"));
        line.fill = "#00ff00".into();
        line.stroke.enabled = true;
        line.stroke.color = "#ffffff".into();
        line.stroke.opacity = 1.0;
        line.stroke.width_px = 6.0;
        let tile = render_vector(&line, (1920, 1080)).unwrap();
        assert!(
            !tile.pixels.chunks(4).any(|p| p == [0, 255, 0, 255]),
            "lines are never filled"
        );
        assert!(tile.pixels.chunks(4).any(|p| p == [255, 255, 255, 255]));
    }

    #[test]
    fn unknown_shapes_and_empty_boxes_draw_nothing() {
        let mut unknown =
            create_overlay_element(OverlayElementKind::Shape, None, "u", Some("triangle"));
        unknown.shape = Some("no-such-shape".into());
        assert!(render_vector(&unknown, (1920, 1080)).is_none());
        assert!(render_vector(&rect(|el| el.frame.width_pct = 0.0), (1920, 1080)).is_none());
        assert!(render_vector(&rect(|el| el.frame.x_pct = 150.0), (1920, 1080)).is_none());
    }

    #[test]
    fn turning_and_flipping_move_the_geometry_not_the_box() {
        let el = rect(|el| {
            el.rotation_deg = 90.0;
            el.frame.width_pct = 40.0;
            el.frame.height_pct = 10.0;
            el.frame.y_pct = 45.0;
        });
        let tile = render_vector(&el, (1920, 1080)).unwrap();
        // 768 × 108 turned a quarter: 108 wide, 768 tall about the centre.
        assert!((i64::from(tile.width) - 110).abs() <= 3, "{}", tile.width);
        assert!((i64::from(tile.height) - 770).abs() <= 3, "{}", tile.height);
    }
}
