//! Scene to pixels: layout, then backdrop, shadow, underline, stroke, fill
//! and line-through, in the order Chromium paints them, from cached glyph
//! coverage masks.

use crate::color::Rgba;
use crate::fit;
use crate::glyphs::{GlyphMask, SUBPIXELS};
use crate::scene::{BoxKind, Decoration, Fit, Shadow, SlideScene, TextBox, Vertical};
use crate::typeset::{Shaped, ShapedLine, Typesetter, with_typesetter};
use std::sync::Arc;
use std::time::Instant;
use tiny_skia::{BlendMode, Color, Paint, Pixmap, PixmapPaint, Rect, Transform};

/// Premultiplied RGBA8 pixels of the text layer, covering `x, y, width,
/// height` of the frame. Everything outside is transparent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextRaster {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// Where the time of one raster went, in milliseconds.
#[derive(Clone, Copy, Debug, Default)]
pub struct Timings {
    /// Fit search, shaping and glyph placement.
    pub layout_ms: f64,
    pub shadow_ms: f64,
    pub stroke_ms: f64,
    pub fill_ms: f64,
    pub composite_ms: f64,
}

fn since(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

/// What laying out one box decided.
#[derive(Clone, Debug)]
pub struct BoxLayout {
    pub kind: BoxKind,
    /// The font size used: the fit result, or the theme's.
    pub size: f32,
    /// Each rendered line's text.
    pub lines: Vec<String>,
}

/// The font size and line breaks of every box, without drawing.
pub fn analyse(scene: &SlideScene) -> Vec<BoxLayout> {
    with_typesetter(|ts| {
        scene
            .boxes
            .iter()
            .map(|b| {
                let (size, shaped) = layout_box(ts, b, false);
                BoxLayout {
                    kind: b.kind,
                    size,
                    lines: shaped.lines.into_iter().map(|l| l.text).collect(),
                }
            })
            .collect()
    })
}

/// The width and height lines are laid out in: the box less its padding.
fn content_size(b: &TextBox) -> (f32, f32) {
    (
        (b.rect.width - 2.0 * b.padding).max(1.0),
        (b.rect.height - 2.0 * b.padding).max(1.0),
    )
}

fn layout_box(ts: &mut Typesetter, b: &TextBox, glyphs: bool) -> (f32, Shaped) {
    let (width, height) = content_size(b);
    let size = match b.size {
        Fit::Fixed(size) => size,
        Fit::Auto { cap } => fit::largest_that_fits(cap, |px| {
            let shaped = ts.shape(b, px, width, false);
            // `overlayTextOverflows`: one pixel of slack on both axes, on
            // the sizes the layout engine rounds to whole pixels.
            shaped.height.round() > height.round() + 1.0
                || shaped.width.round() > width.round() + 1.0
        }),
    };
    (size, ts.shape(b, size, width, glyphs))
}

/// Draw `scene` into a raster of its text layer, or `None` when it draws
/// nothing (no boxes, or no visible text and no backdrop).
pub fn rasterize(scene: &SlideScene) -> Option<TextRaster> {
    rasterize_timed(scene).0
}

/// [`rasterize`], with where the time went.
pub fn rasterize_timed(scene: &SlideScene) -> (Option<TextRaster>, Timings) {
    with_typesetter(|ts| rasterize_with(ts, scene))
}

pub fn rasterize_with(ts: &mut Typesetter, scene: &SlideScene) -> (Option<TextRaster>, Timings) {
    let mut times = Timings::default();
    let raster = rasterize_inner(ts, scene, &mut times);
    (raster, times)
}

type Bounds = (u32, u32, u32, u32);

fn rasterize_inner(
    ts: &mut Typesetter,
    scene: &SlideScene,
    times: &mut Timings,
) -> Option<TextRaster> {
    let (frame_w, frame_h) = scene.frame;
    let mut placed = Vec::new();
    for b in &scene.boxes {
        let Some(bounds) = pixel_bounds(b, frame_w, frame_h) else {
            continue;
        };
        let started = Instant::now();
        let (size, shaped) = layout_box(ts, b, true);
        times.layout_ms += since(started);
        placed.push(place(b, size, shaped, bounds));
    }
    // Tile: the union of what the boxes draw, clipped to the frame.
    let live: Vec<&Placed> = placed
        .iter()
        .filter(|p| p.region.2 > 0 && p.region.3 > 0)
        .collect();
    let left = live.iter().map(|p| p.region.0).min()?;
    let top = live.iter().map(|p| p.region.1).min()?;
    let right = live.iter().map(|p| p.region.0 + p.region.2).max()?;
    let bottom = live.iter().map(|p| p.region.1 + p.region.3).max()?;
    let tile = if let [only] = live.as_slice() {
        // One box fills its own pixmap and needs no compositing.
        draw_box(ts, only, times)?
    } else {
        let mut canvas = Pixmap::new(right - left, bottom - top)?;
        for p in &live {
            if let Some(image) = draw_box(ts, p, times) {
                let started = Instant::now();
                canvas.draw_pixmap(
                    (p.region.0 - left) as i32,
                    (p.region.1 - top) as i32,
                    image.as_ref(),
                    &PixmapPaint {
                        blend_mode: BlendMode::SourceOver,
                        ..PixmapPaint::default()
                    },
                    Transform::identity(),
                    None,
                );
                times.composite_ms += since(started);
            }
        }
        canvas
    };
    Some(TextRaster {
        x: left,
        y: top,
        width: tile.width(),
        height: tile.height(),
        pixels: tile.take(),
    })
}

/// A laid-out box and where it draws.
struct Placed<'a> {
    b: &'a TextBox,
    size: f32,
    shaped: Shaped,
    /// Top-left of the text's content area in frame coordinates, after
    /// vertical alignment.
    origin: (f32, f32),
    auto_fit: bool,
    /// The frame pixels this box can change: its whole rectangle with a
    /// backdrop, else the text and its shadow, within the rectangle. Zero
    /// size when there is nothing to draw.
    region: Bounds,
}

fn place(b: &TextBox, size: f32, shaped: Shaped, bounds: Bounds) -> Placed<'_> {
    let (_, content_h) = content_size(b);
    let free = content_h - shaped.height;
    let auto_fit = matches!(b.size, Fit::Auto { .. });
    // A fixed-size verse overflows from the top padding (`.pa-verse` shrinks
    // to the box and its text spills downward). The reference is a plain
    // flex item and a fitted verse is a full-size flex column, so both keep
    // aligning when they overflow.
    let free = if auto_fit || b.kind != BoxKind::Verse {
        free
    } else {
        free.max(0.0)
    };
    let shift = match b.vertical {
        Vertical::Top => 0.0,
        Vertical::Bottom => free,
        Vertical::Middle => free / 2.0,
    };
    let origin = (b.rect.x + b.padding, b.rect.y + b.padding + shift);
    let mut region = bounds;
    if b.backdrop.is_none_or(|c| c.a == 0) {
        region = (bounds.0, bounds.1, 0, 0);
        // Bounds from the line boxes, generous: glyphs ascend and descend
        // past the font size and overshoot their advance a little.
        let (mut l, mut t, mut r, mut bt) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for line in shaped.lines.iter().filter(|l| !l.glyphs.is_empty()) {
            l = l.min(line.x);
            r = r.max(line.x + line.advance);
            t = t.min(line.baseline - size * 1.3);
            bt = bt.max(line.baseline + size * 0.6);
        }
        if l <= r {
            let stroke = b.style.stroke.map_or(0.0, |s| s.width);
            let reach = b.style.shadow.map_or(0.0, |s| {
                s.x.abs().max(s.y.abs()) + (s.blur / 2.0 * 3.0).ceil() + 3.0
            });
            let pad = size * 0.5 + stroke + reach + 2.0;
            let x0 = (origin.0 + l - pad).floor().max(bounds.0 as f32);
            let y0 = (origin.1 + t - stroke - reach - 2.0)
                .floor()
                .max(bounds.1 as f32);
            let x1 = (origin.0 + r + pad)
                .ceil()
                .min((bounds.0 + bounds.2) as f32);
            let y1 = (origin.1 + bt + stroke + reach + 2.0)
                .ceil()
                .min((bounds.1 + bounds.3) as f32);
            if x1 > x0 && y1 > y0 {
                region = (x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32);
            }
        }
    }
    Placed {
        b,
        size,
        shaped,
        origin,
        auto_fit,
        region,
    }
}

/// The box's pixel rectangle, clipped to the frame; `None` when empty.
fn pixel_bounds(b: &TextBox, frame_w: u32, frame_h: u32) -> Option<Bounds> {
    let r = b.rect;
    let x0 = r.x.floor().max(0.0);
    let y0 = r.y.floor().max(0.0);
    let x1 = (r.x + r.width).ceil().min(frame_w as f32);
    let y1 = (r.y + r.height).ceil().min(frame_h as f32);
    (x1 > x0 && y1 > y0).then_some((x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32))
}

fn paint(c: Rgba) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(c.r, c.g, c.b, c.a));
    paint.anti_alias = true;
    paint
}

/// An integer pixel rectangle `left, top, right, bottom` in the box pixmap.
type Clip = (i32, i32, i32, i32);

/// One glyph ready to blit: its masks and whole-pixel position in the box
/// pixmap.
struct DrawGlyph {
    fill: Arc<GlyphMask>,
    stroke: Option<Arc<GlyphMask>>,
    x: i32,
    y: i32,
    line: usize,
}

fn draw_box(ts: &mut Typesetter, placed: &Placed<'_>, times: &mut Timings) -> Option<Pixmap> {
    let Placed {
        b,
        size,
        shaped,
        auto_fit,
        region: (px, py, pw, ph),
        ..
    } = placed;
    let (b, size, auto_fit) = (*b, *size, *auto_fit);
    let (px, py, pw, ph) = (*px, *py, *pw, *ph);
    let mut image = Pixmap::new(pw, ph)?;
    // The box's own origin inside the pixmap.
    let box_x = b.rect.x - px as f32;
    let box_y = b.rect.y - py as f32;
    if let Some(color) = b.backdrop
        && color.a > 0
    {
        // Backgrounds are pixel-snapped.
        let rect = Rect::from_ltrb(
            box_x.round(),
            box_y.round(),
            (box_x + b.rect.width).round(),
            (box_y + b.rect.height).round(),
        );
        if let Some(rect) = rect {
            let mut p = paint(color);
            p.anti_alias = false;
            image.fill_rect(rect, &p, Transform::identity(), None);
        }
    }
    let origin = (placed.origin.0 - px as f32, placed.origin.1 - py as f32);
    // A fitted verse clips to its own content box, rounded to pixels; a
    // fixed one to the box, which the pixmap already is.
    let (cw, ch) = content_size(b);
    let clip: Clip = if auto_fit {
        (
            (box_x + b.padding).round() as i32,
            (box_y + b.padding).round() as i32,
            (box_x + b.padding + cw).round() as i32,
            (box_y + b.padding + ch).round() as i32,
        )
    } else {
        (0, 0, pw as i32, ph as i32)
    };
    let style = &b.style;
    let started = Instant::now();
    let glyphs = collect_glyphs(ts, shaped, origin, style.stroke.map(|s| s.width));
    times.layout_ms += since(started);
    let bars: Vec<Bar> = if style.decoration == Decoration::None {
        Vec::new()
    } else {
        shaped
            .lines
            .iter()
            .enumerate()
            .filter(|(_, l)| !l.glyphs.is_empty())
            .flat_map(|(i, line)| decoration_bars(line, i, &glyphs, style.decoration, size, origin))
            .collect()
    };
    // Chromium paints a decoration in the stroke colour when there is a
    // stroke, and casts the text shadow from it too.
    let decoration_color = style.stroke.map_or(style.color, |s| s.color);
    let started = Instant::now();
    if let Some(shadow) = style.shadow
        && shadow.color.a > 0
    {
        draw_shadow(&mut image, &glyphs, &bars, shadow, clip);
    }
    times.shadow_ms += since(started);
    let draw_bars = |image: &mut Pixmap| {
        for bar in &bars {
            fill_bar(image, bar, decoration_color, clip);
        }
    };
    if style.decoration == Decoration::Underline {
        draw_bars(&mut image);
    }
    let started = Instant::now();
    if let Some(stroke) = style.stroke
        && stroke.color.a > 0
    {
        for g in &glyphs {
            if let Some(mask) = &g.stroke {
                blit(&mut image, mask, g.x, g.y, stroke.color, clip);
            }
        }
    }
    times.stroke_ms += since(started);
    let started = Instant::now();
    for g in &glyphs {
        let color = shaped.lines[g.line].color.unwrap_or(style.color);
        blit(&mut image, &g.fill, g.x, g.y, color, clip);
    }
    if style.decoration == Decoration::LineThrough {
        draw_bars(&mut image);
    }
    times.fill_ms += since(started);
    Some(image)
}

/// Place every glyph on the pixel grid the way Skia does: the baseline
/// snaps to a whole pixel and x to a quarter pixel.
fn collect_glyphs(
    ts: &mut Typesetter,
    shaped: &Shaped,
    origin: (f32, f32),
    stroke: Option<f32>,
) -> Vec<DrawGlyph> {
    let mut out = Vec::new();
    for (line_index, line) in shaped.lines.iter().enumerate() {
        for g in &line.glyphs {
            let run = &line.runs[g.run as usize];
            let steps = ((origin.0 + g.x) * f32::from(SUBPIXELS) + 0.5).floor() as i64;
            let whole = steps.div_euclid(i64::from(SUBPIXELS)) as i32;
            let sub = steps.rem_euclid(i64::from(SUBPIXELS)) as u8;
            let y = (origin.1 + g.y).round() as i32;
            let Some(fill) = ts.glyph_cache.mask(&mut ts.scale, run, g.id, sub, 0.0) else {
                continue;
            };
            let stroke = stroke
                .filter(|w| *w > 0.0)
                .and_then(|w| ts.glyph_cache.mask(&mut ts.scale, run, g.id, sub, w));
            out.push(DrawGlyph {
                fill,
                stroke,
                x: whole,
                y,
                line: line_index,
            });
        }
    }
    out
}

/// Source-over of a coverage mask in `color` onto premultiplied pixels.
fn blit(image: &mut Pixmap, mask: &GlyphMask, x: i32, y: i32, color: Rgba, clip: Clip) {
    let (w, h) = (image.width() as i32, image.height() as i32);
    let (cl, ct, cr, cb) = (clip.0.max(0), clip.1.max(0), clip.2.min(w), clip.3.min(h));
    let x0 = (x + mask.left).max(cl);
    let x1 = (x + mask.left + mask.width as i32).min(cr);
    let y0 = (y + mask.top).max(ct);
    let y1 = (y + mask.top + mask.height as i32).min(cb);
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let ca = u32::from(color.a);
    let (cr8, cg8, cb8) = (u32::from(color.r), u32::from(color.g), u32::from(color.b));
    let stride = w as usize * 4;
    let data = image.data_mut();
    for row in y0..y1 {
        let mstart =
            ((row - y - mask.top) as usize) * mask.width as usize + (x0 - x - mask.left) as usize;
        let src = &mask.data[mstart..mstart + (x1 - x0) as usize];
        let dst = &mut data[row as usize * stride + x0 as usize * 4..][..(x1 - x0) as usize * 4];
        for (&cov, px) in src.iter().zip(dst.as_chunks_mut::<4>().0.iter_mut()) {
            if cov == 0 {
                continue;
            }
            // Alpha of this sample, and the premultiplied colour at it.
            let a = (u32::from(cov) * ca + 127) / 255;
            let inv = 255 - a;
            let mul = |c: u32, d: u8| -> u8 {
                (((c * a + 127) / 255) + (u32::from(d) * inv + 127) / 255).min(255) as u8
            };
            px[0] = mul(cr8, px[0]);
            px[1] = mul(cg8, px[1]);
            px[2] = mul(cb8, px[2]);
            px[3] = (a + (u32::from(px[3]) * inv + 127) / 255).min(255) as u8;
        }
    }
}

/// A decoration bar in box-pixmap coordinates.
struct Bar {
    left: f32,
    right: f32,
    top: f32,
    thickness: f32,
}

/// Fill a bar with exact edge coverage, inside `clip`.
fn fill_bar(image: &mut Pixmap, bar: &Bar, color: Rgba, clip: Clip) {
    let (w, h) = (image.width() as i32, image.height() as i32);
    let (cl, ct, cr, cb) = (clip.0.max(0), clip.1.max(0), clip.2.min(w), clip.3.min(h));
    let (y0, y1) = (bar.top, bar.top + bar.thickness);
    let stride = w as usize * 4;
    let data = image.data_mut();
    for y in (y0.floor() as i32).max(ct)..(y1.ceil() as i32).min(cb) {
        let cy = (y1.min(y as f32 + 1.0) - y0.max(y as f32)).max(0.0);
        for x in (bar.left.floor() as i32).max(cl)..(bar.right.ceil() as i32).min(cr) {
            let cx = (bar.right.min(x as f32 + 1.0) - bar.left.max(x as f32)).max(0.0);
            let a = (cx * cy * f32::from(color.a)).round().clamp(0.0, 255.0) as u32;
            if a == 0 {
                continue;
            }
            let px = &mut data[y as usize * stride + x as usize * 4..][..4];
            let inv = 255 - a;
            let mul = |c: u8, d: u8| -> u8 {
                (((u32::from(c) * a + 127) / 255) + (u32::from(d) * inv + 127) / 255).min(255) as u8
            };
            px[0] = mul(color.r, px[0]);
            px[1] = mul(color.g, px[1]);
            px[2] = mul(color.b, px[2]);
            px[3] = (a + (u32::from(px[3]) * inv + 127) / 255).min(255) as u8;
        }
    }
}

/// Chromium's `text-decoration-thickness: auto`: a tenth of the font size,
/// at least one pixel.
fn decoration_thickness(size: f32) -> f32 {
    (size / 10.0).max(1.0)
}

/// The decoration of one line as bars. The bar's centre is a whole pixel. An
/// underline hangs `ceil(thickness / 2)` below the baseline and skips the
/// ink it crosses (`skip-ink: auto`); a line-through sits a third of the
/// ascent above it.
fn decoration_bars(
    line: &ShapedLine,
    index: usize,
    glyphs: &[DrawGlyph],
    which: Decoration,
    size: f32,
    origin: (f32, f32),
) -> Vec<Bar> {
    let t = decoration_thickness(size);
    let baseline = origin.1 + line.baseline;
    let centre = match which {
        Decoration::Underline => baseline + (t / 2.0).ceil().max(1.0) + t / 2.0,
        Decoration::LineThrough => baseline - line.decoration.ascent / 3.0,
        Decoration::None => return Vec::new(),
    }
    .round();
    let (left, right) = (origin.0 + line.x, origin.0 + line.x + line.advance);
    let top = centre - t / 2.0;
    let spans = if which == Decoration::Underline {
        let own: Vec<&DrawGlyph> = glyphs.iter().filter(|g| g.line == index).collect();
        skip_ink(&own, left, right, top, top + t, t)
    } else {
        vec![(left, right)]
    };
    spans
        .into_iter()
        .map(|(a, b)| Bar {
            left: a,
            right: b,
            top,
            thickness: t,
        })
        .collect()
}

/// The parts of `[left, right]` not within `dilate` of glyph ink inside the
/// band `top..bottom`.
fn skip_ink(
    glyphs: &[&DrawGlyph],
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
    dilate: f32,
) -> Vec<(f32, f32)> {
    let (band_top, band_bottom) = (top.floor() as i32, bottom.ceil() as i32);
    let mut skips: Vec<(f32, f32)> = Vec::new();
    for g in glyphs {
        let m = &g.fill;
        let (gx, gy) = (g.x + m.left, g.y + m.top);
        let rows = band_top.max(gy)..band_bottom.min(gy + m.height as i32);
        let (mut first, mut last) = (i32::MAX, i32::MIN);
        for row in rows {
            let line = &m.data[((row - gy) as usize) * m.width as usize..][..m.width as usize];
            if let Some(i) = line.iter().position(|&v| v > 0) {
                first = first.min(i as i32);
                last = last.max(line.iter().rposition(|&v| v > 0).unwrap_or(0) as i32);
            }
        }
        if first <= last {
            skips.push((
                (gx + first) as f32 - dilate,
                (gx + last + 1) as f32 + dilate,
            ));
        }
    }
    skips.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut spans = Vec::new();
    let mut cursor = left;
    for (s, e) in skips {
        if e <= cursor {
            continue;
        }
        if s >= right {
            break;
        }
        if s > cursor {
            spans.push((cursor, s));
        }
        cursor = cursor.max(e);
    }
    if cursor < right {
        spans.push((cursor, right));
    }
    spans
}

/// Gaussian text shadow: the glyphs (and decorations, not the stroke)
/// blurred at sigma = blur / 2, offset, tinted, under the text.
fn draw_shadow(image: &mut Pixmap, glyphs: &[DrawGlyph], bars: &[Bar], shadow: Shadow, clip: Clip) {
    let (w, h) = (image.width() as i32, image.height() as i32);
    let sigma = shadow.blur / 2.0;
    let (dx, dy) = (shadow.x.round() as i32, shadow.y.round() as i32);
    // Ink bounds in the pixmap.
    let (mut l, mut t, mut r, mut b) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for g in glyphs {
        l = l.min(g.x + g.fill.left);
        t = t.min(g.y + g.fill.top);
        r = r.max(g.x + g.fill.left + g.fill.width as i32);
        b = b.max(g.y + g.fill.top + g.fill.height as i32);
    }
    for bar in bars {
        l = l.min(bar.left.floor() as i32);
        t = t.min(bar.top.floor() as i32);
        r = r.max(bar.right.ceil() as i32);
        b = b.max((bar.top + bar.thickness).ceil() as i32);
    }
    if l >= r || t >= b {
        return;
    }
    let margin = (sigma * 3.0).ceil() as i32 + 2;
    let left = (l + dx - margin).max(0);
    let top = (t + dy - margin).max(0);
    let right = (r + dx + margin).min(w);
    let bottom = (b + dy + margin).min(h);
    if right <= left || bottom <= top {
        return;
    }
    let (cw, chh) = ((right - left) as usize, (bottom - top) as usize);
    // A wide blur varies slowly, so it is computed at half size: each source
    // pixel adds a quarter of its coverage to its cell.
    let half = sigma >= 2.0;
    let (bw, bh) = if half {
        (cw.div_ceil(2), chh.div_ceil(2))
    } else {
        (cw, chh)
    };
    let (weight, shift) = if half { (0.25f32, 1) } else { (1.0, 0) };
    let mut blurred = vec![0f32; bw * bh];
    let mut add = |x: i32, y: i32, v: f32| {
        if x >= left && x < right && y >= top && y < bottom {
            let cell = (((y - top) as usize) >> shift) * bw + (((x - left) as usize) >> shift);
            blurred[cell] += v * weight;
        }
    };
    for g in glyphs {
        let m = &g.fill;
        let (gx, gy) = (g.x + m.left + dx, g.y + m.top + dy);
        for row in 0..m.height as i32 {
            let line = &m.data[(row as usize) * m.width as usize..][..m.width as usize];
            for (col, &v) in line.iter().enumerate() {
                if v != 0 {
                    add(gx + col as i32, gy + row, f32::from(v));
                }
            }
        }
    }
    for bar in bars {
        let (y0, y1) = (bar.top + dy as f32, bar.top + bar.thickness + dy as f32);
        let (x0, x1) = (bar.left + dx as f32, bar.right + dx as f32);
        for y in (y0.floor() as i32)..(y1.ceil() as i32) {
            let cy = (y1.min(y as f32 + 1.0) - y0.max(y as f32)).max(0.0);
            for x in (x0.floor() as i32)..(x1.ceil() as i32) {
                let cx = (x1.min(x as f32 + 1.0) - x0.max(x as f32)).max(0.0);
                add(x, y, cx * cy * 255.0);
            }
        }
    }
    // Overlapping glyphs add up; coverage cannot pass full.
    for v in &mut blurred {
        *v = v.min(255.0);
    }
    let scale = if half { 0.5 } else { 1.0 };
    if sigma > 0.0 {
        gaussian_boxes(&mut blurred, bw, bh, sigma * scale);
    }
    let opacity = f32::from(shadow.color.a) / 255.0;
    let (cl, ct, cr, cb) = (clip.0.max(0), clip.1.max(0), clip.2.min(w), clip.3.min(h));
    let columns: Vec<_> = (0..cw).map(|c| sample_at(c, bw, scale)).collect();
    let stride = w as usize * 4;
    let data = image.data_mut();
    let (r8, g8, b8) = (shadow.color.r, shadow.color.g, shadow.color.b);
    let x_from = (cl - left).max(0) as usize;
    let x_to = ((cr - left).max(0) as usize).min(cw);
    let mut mixed = vec![0f32; cw];
    for row in 0..chh {
        let y = top + row as i32;
        if y < ct || y >= cb || x_from >= x_to {
            continue;
        }
        let (a_row, b_row, fy) = sample_at(row, bh, scale);
        let upper = &blurred[a_row * bw..][..bw];
        let lower = &blurred[b_row * bw..][..bw];
        for (m, &(lo, hi, fx)) in mixed[x_from..x_to].iter_mut().zip(&columns[x_from..x_to]) {
            let above = upper[lo] + (upper[hi] - upper[lo]) * fx;
            let below = lower[lo] + (lower[hi] - lower[lo]) * fx;
            *m = (above + (below - above) * fy) * opacity;
        }
        let start = y as usize * stride + (left as usize + x_from) * 4;
        let dst = data[start..start + (x_to - x_from) * 4]
            .as_chunks_mut::<4>()
            .0;
        for (&value, px) in mixed[x_from..x_to].iter().zip(dst.iter_mut()) {
            let alpha = (value + 0.5).clamp(0.0, 255.0) as u8;
            if alpha == 0 {
                continue;
            }
            let pre = |c: u8| ((u16::from(c) * u16::from(alpha) + 127) / 255) as u8;
            if px[3] == 0 {
                *px = [pre(r8), pre(g8), pre(b8), alpha];
                continue;
            }
            let inverse = u16::from(255 - alpha);
            let over = |src: u8, dst: u8| {
                src.saturating_add(((u16::from(dst) * inverse + 127) / 255) as u8)
            };
            *px = [
                over(pre(r8), px[0]),
                over(pre(g8), px[1]),
                over(pre(b8), px[2]),
                over(alpha, px[3]),
            ];
        }
    }
}

fn sample_at(position: usize, size: usize, scale: f32) -> (usize, usize, f32) {
    let coordinate = ((position as f32 + 0.5) * scale - 0.5).clamp(0.0, (size - 1) as f32);
    let low = coordinate.floor() as usize;
    (low, (low + 1).min(size - 1), coordinate - low as f32)
}

/// Three separable box filters approximate a Gaussian of `sigma`. Running
/// sums keep the cost linear in the area.
fn gaussian_boxes(data: &mut [f32], width: usize, height: usize, sigma: f32) {
    let ideal = (4.0 * sigma * sigma + 1.0).sqrt();
    let lower = (ideal.floor() as usize).max(1) | 1;
    let lower = if lower as f32 > ideal {
        lower.saturating_sub(2).max(1)
    } else {
        lower
    };
    let upper = lower + 2;
    let low = lower as f32;
    let small = ((12.0 * sigma * sigma - 3.0 * low * low - 12.0 * low - 9.0) / (-4.0 * low - 4.0))
        .round()
        .clamp(0.0, 3.0) as usize;
    let mut scratch = vec![0.0; data.len()];
    for pass in 0..3 {
        let radius = if pass < small { lower / 2 } else { upper / 2 };
        blur_axis(data, &mut scratch, height, width, width, 1, radius);
        blur_axis(&scratch, data, width, height, 1, width, radius);
    }
}

fn blur_axis(
    input: &[f32],
    output: &mut [f32],
    lines: usize,
    length: usize,
    line_stride: usize,
    step: usize,
    radius: usize,
) {
    let divisor = (radius * 2 + 1) as f32;
    for line in 0..lines {
        let start = line * line_stride;
        let mut sum: f32 = (0..=radius.min(length - 1))
            .map(|i| input[start + i * step])
            .sum();
        for i in 0..length {
            output[start + i * step] = sum / divisor;
            if i >= radius {
                sum -= input[start + (i - radius) * step];
            }
            if i + radius + 1 < length {
                sum += input[start + (i + radius + 1) * step];
            }
        }
    }
}
