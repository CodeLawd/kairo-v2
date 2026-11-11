//! Glyph coverage masks, cached.
//!
//! Skia (and so Chromium) draws text from small coverage bitmaps, one per
//! glyph, size and quarter-pixel horizontal position, and blits them at a
//! baseline snapped to a whole pixel. This does the same, which makes a
//! slide's text a few hundred small blits instead of filling thousands of
//! outline contours, and lets a repeated glyph cost nothing.

use crate::typeset::RunFont;
use std::collections::HashMap;
use std::sync::Arc;
use swash::scale::ScaleContext;
use swash::zeno::Verb;
use tiny_skia::{FillRule, Mask, PathBuilder, Stroke, Transform};

/// Coverage (0 to 255) of one glyph, positioned relative to the pixel the
/// glyph origin falls in.
pub struct GlyphMask {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    font: u64,
    index: u32,
    glyph: u16,
    size: u32,
    coords: u64,
    sub: u8,
    stroke: u32,
}

/// Quarter-pixel steps across a pixel.
pub const SUBPIXELS: u8 = 4;
const BUDGET_BYTES: usize = 24 * 1024 * 1024;

#[derive(Default)]
pub struct GlyphCache {
    masks: HashMap<Key, Option<Arc<GlyphMask>>>,
    bytes: usize,
}

impl GlyphCache {
    /// The mask of `glyph` in `font` at `sub` quarter pixels right of its
    /// pixel, or of its outline stroked `stroke` wide when that is non-zero.
    /// `None` for a glyph with no outline (a space).
    pub fn mask(
        &mut self,
        scale: &mut ScaleContext,
        font: &RunFont,
        glyph: u16,
        sub: u8,
        stroke: f32,
    ) -> Option<Arc<GlyphMask>> {
        let key = Key {
            font: font.data.id(),
            index: font.index,
            glyph,
            size: font.size.to_bits(),
            coords: font.coords_hash,
            sub,
            stroke: stroke.to_bits(),
        };
        if let Some(found) = self.masks.get(&key) {
            return found.clone();
        }
        let made = make(scale, font, glyph, sub, stroke).map(Arc::new);
        if self.bytes > BUDGET_BYTES {
            self.masks.clear();
            self.bytes = 0;
        }
        self.bytes += made.as_ref().map_or(0, |m| m.data.len());
        self.masks.insert(key, made.clone());
        made
    }
}

fn make(
    scale: &mut ScaleContext,
    font: &RunFont,
    glyph: u16,
    sub: u8,
    stroke: f32,
) -> Option<GlyphMask> {
    let font_ref = swash::FontRef::from_index(font.data.data(), font.index as usize)?;
    let mut scaler = scale
        .builder(font_ref)
        .size(font.size)
        .normalized_coords(&font.coords)
        .build();
    let outline = scaler.scale_outline(glyph)?;
    let dx = f32::from(sub) / f32::from(SUBPIXELS);
    let mut builder = PathBuilder::new();
    let mut points = outline.points().iter().map(|p| (p.x + dx, -p.y));
    for verb in outline.verbs() {
        match verb {
            Verb::MoveTo => {
                let (x, y) = points.next()?;
                builder.move_to(x, y);
            }
            Verb::LineTo => {
                let (x, y) = points.next()?;
                builder.line_to(x, y);
            }
            Verb::QuadTo => {
                let (cx, cy) = points.next()?;
                let (x, y) = points.next()?;
                builder.quad_to(cx, cy, x, y);
            }
            Verb::CurveTo => {
                let (x1, y1) = points.next()?;
                let (x2, y2) = points.next()?;
                let (x, y) = points.next()?;
                builder.cubic_to(x1, y1, x2, y2, x, y);
            }
            Verb::Close => builder.close(),
        }
    }
    let mut path = builder.finish()?;
    if stroke > 0.0 {
        path = path.stroke(
            &Stroke {
                width: stroke,
                ..Stroke::default()
            },
            1.0,
        )?;
    }
    let b = path.bounds();
    let left = b.left().floor() as i32 - 1;
    let top = b.top().floor() as i32 - 1;
    let right = b.right().ceil() as i32 + 1;
    let bottom = b.bottom().ceil() as i32 + 1;
    let (width, height) = ((right - left) as u32, (bottom - top) as u32);
    let mut mask = Mask::new(width, height)?;
    mask.fill_path(
        &path,
        FillRule::Winding,
        true,
        Transform::from_translate(-left as f32, -top as f32),
    );
    Some(GlyphMask {
        left,
        top,
        width,
        height,
        data: mask.data().to_vec(),
    })
}
