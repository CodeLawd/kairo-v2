//! Stills that move: a slow camera move over the picture, transparent
//! layers in front of it that travel further (parallax), and an atmosphere
//! (dust, bokeh, light rays, haze) drawn here and looped seamlessly. All of
//! it is plain compositor motion over rasters made once, so a moving
//! background costs no more each frame than a still.

use kairo_compositor::{Content, Fill, Motion, Raster, Rect};
use kairo_domain::media::MediaMotion;
use std::sync::Arc;
use tiny_skia::{
    Color, FillRule, GradientStop, LinearGradient, Paint, PathBuilder, Pixmap, Point,
    RadialGradient, SpreadMode, Transform,
};

/// How far past the frame a moving picture is drawn, each side.
const MARGIN: f32 = 0.06;

/// The picture's box and how it moves. `travel` (0 to 1) is how much of the
/// margin it uses: nearer layers travel further, which is the parallax.
fn camera(kind: &str, (w, h): (f32, f32), travel: f32) -> (Rect, Motion) {
    let (mx, my) = (w * MARGIN, h * MARGIN);
    let breathe = |period_ms| Motion {
        period_ms,
        alternate: true,
        eased: true,
        ..Motion::default()
    };
    let wide = Rect::new(-mx - travel * mx, -my, w + 2.0 * mx, h + 2.0 * my);
    match kind {
        "zoom" => (
            Rect::new(0.0, 0.0, w, h),
            Motion {
                zoom: 0.12 * travel,
                ..breathe(26_000)
            },
        ),
        "pan" => (
            wide,
            Motion {
                distance: 2.0 * travel * mx,
                ..breathe(40_000)
            },
        ),
        // Drift: across and up together.
        _ => (
            Rect::new(wide.x, -my + travel * my, wide.width, wide.height),
            Motion {
                distance: 2.0 * travel * mx,
                rise: -2.0 * travel * my,
                ..breathe(32_000)
            },
        ),
    }
}

/// The picture (`base`, already decoded) with its motion, layers and
/// atmosphere, as one node. `layer` decodes a layer's picture.
pub fn compose(
    base: Content,
    motion: &MediaMotion,
    frame: (u32, u32),
    mut layer: impl FnMut(&str) -> Option<Fill>,
) -> Content {
    let size = (frame.0 as f32, frame.1 as f32);
    let clip = Some(Rect::new(0.0, 0.0, size.0, size.1));
    let parallax = !motion.layers.is_empty();
    // Layers need a camera to move with; drift is the gentlest.
    let kind = motion.camera.as_deref().or(parallax.then_some("drift"));
    let opacity = base.opacity;
    let look = base.look;
    let mut children = Vec::with_capacity(2 + motion.layers.len());
    let mut picture = Content {
        opacity: 1.0,
        ..base
    };
    if let Some(kind) = kind {
        let (rect, m) = camera(kind, size, if parallax { 0.35 } else { 0.9 });
        picture.rect = Some(rect);
        picture.motion = Some(m);
        picture.clip = clip;
    }
    children.push(picture);
    for (i, path) in motion.layers.iter().enumerate() {
        let (Some(kind), Some(fill)) = (kind, layer(path)) else {
            continue;
        };
        let (rect, m) = camera(kind, size, (0.75 + 0.1 * i as f32).min(1.0));
        children.push(Content {
            rect: Some(rect),
            motion: Some(m),
            clip,
            look,
            ..Content::new(fill)
        });
    }
    if let Some(air) = motion
        .atmosphere
        .as_deref()
        .and_then(|a| atmosphere(a, frame))
    {
        children.push(air);
    }
    Content {
        opacity,
        ..Content::new(Fill::Group(Arc::new(children)))
    }
}

/// A small deterministic random source: the same atmosphere every time.
struct Seed(u64);
impl Seed {
    fn next(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next()
    }
}

/// A soft round light: full `alpha` at the centre, `hold` of the radius
/// solid, fading to nothing at the edge.
fn glow(pixmap: &mut Pixmap, x: f32, y: f32, r: f32, rgb: (u8, u8, u8), alpha: f32, hold: f32) {
    let c = |a: f32| Color::from_rgba8(rgb.0, rgb.1, rgb.2, (a.clamp(0.0, 1.0) * 255.0) as u8);
    let Some(shader) = RadialGradient::new(
        Point::from_xy(x, y),
        Point::from_xy(x, y),
        r,
        vec![
            GradientStop::new(0.0, c(alpha)),
            GradientStop::new(hold, c(alpha * 0.85)),
            GradientStop::new(1.0, c(0.0)),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    ) else {
        return;
    };
    let Some(path) = PathBuilder::from_circle(x, y, r) else {
        return;
    };
    let paint = Paint {
        shader,
        anti_alias: true,
        ..Paint::default()
    };
    pixmap.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

/// Lights scattered over one `tile`, drawn into a pixmap that repeats it
/// twice along y (or x), so sliding by one tile loops without a seam.
fn tiled(
    tile: (u32, u32),
    vertical: bool,
    count: usize,
    seed: u64,
    mut light: impl FnMut(&mut Seed) -> (f32, (u8, u8, u8), f32, f32),
) -> Option<Pixmap> {
    let (tw, th) = tile;
    let (w, h) = if vertical { (tw, th * 2) } else { (tw * 2, th) };
    let mut pixmap = Pixmap::new(w, h)?;
    let mut rng = Seed(seed);
    for _ in 0..count {
        let (x, y) = (rng.range(0.0, tw as f32), rng.range(0.0, th as f32));
        let (r, rgb, alpha, hold) = light(&mut rng);
        for k in -1..=2 {
            let k = k as f32;
            let (dx, dy) = if vertical {
                (0.0, k * th as f32)
            } else {
                (k * tw as f32, 0.0)
            };
            glow(&mut pixmap, x + dx, y + dy, r, rgb, alpha, hold);
        }
    }
    Some(pixmap)
}

fn image(pixmap: Pixmap) -> Fill {
    let (w, h) = (pixmap.width(), pixmap.height());
    Fill::Image(Raster::new(w, h, pixmap.take()))
}

/// An atmosphere over a `frame`-sized picture, drawn at half size.
pub fn atmosphere(kind: &str, frame: (u32, u32)) -> Option<Content> {
    let (fw, fh) = (frame.0 as f32, frame.1 as f32);
    let tile = ((frame.0 / 2).max(2), (frame.1 / 2).max(2));
    let unit = tile.0 as f32 / 960.0;
    let clip = Some(Rect::new(0.0, 0.0, fw, fh));
    let warm = (255, 242, 214);
    let looping = |rise: f32, distance: f32, period_ms| Motion {
        rise,
        distance,
        period_ms,
        ..Motion::default()
    };
    let (pixmap, rect, motion) = match kind {
        "dust" => (
            tiled(tile, true, 140, 7, |r| {
                (r.range(0.8, 2.6) * unit, warm, r.range(0.35, 0.9), 0.35)
            })?,
            Rect::new(0.0, 0.0, fw, fh * 2.0),
            looping(-fh, 0.0, 60_000),
        ),
        "bokeh" => (
            tiled(tile, true, 26, 11, |r| {
                let gold = r.next() < 0.6;
                let rgb = if gold {
                    (255, 214, 150)
                } else {
                    (200, 220, 255)
                };
                (r.range(18.0, 70.0) * unit, rgb, r.range(0.06, 0.16), 0.7)
            })?,
            Rect::new(0.0, 0.0, fw, fh * 2.0),
            looping(-fh, 0.0, 110_000),
        ),
        "haze" => (
            tiled(tile, false, 14, 23, |r| {
                (
                    r.range(140.0, 320.0) * unit,
                    (235, 238, 245),
                    r.range(0.05, 0.1),
                    0.2,
                )
            })?,
            Rect::new(0.0, 0.0, fw * 2.0, fh),
            looping(0.0, -fw, 140_000),
        ),
        "rays" => {
            let (w, h) = ((tile.0 as f32 * 1.3) as u32, tile.1);
            let mut pixmap = Pixmap::new(w, h)?;
            let (wf, hf) = (w as f32, h as f32);
            let source = Point::from_xy(wf * 0.2, -hf * 0.15);
            let mut rng = Seed(31);
            for i in 0..7 {
                let reach = hf * 1.5;
                let angle = 0.35 + i as f32 * 0.16 + rng.range(-0.04, 0.04);
                let spread = rng.range(0.025, 0.06);
                let at =
                    |a: f32| Point::from_xy(source.x + reach * a.cos(), source.y + reach * a.sin());
                let (a, b) = (at(angle - spread), at(angle + spread));
                let mut pb = PathBuilder::new();
                pb.move_to(source.x, source.y);
                pb.line_to(a.x, a.y);
                pb.line_to(b.x, b.y);
                pb.close();
                let alpha = rng.range(0.07, 0.14);
                let c = |a: f32| Color::from_rgba8(warm.0, warm.1, warm.2, (a * 255.0) as u8);
                let shader = LinearGradient::new(
                    source,
                    at(angle),
                    vec![
                        GradientStop::new(0.0, c(alpha)),
                        GradientStop::new(1.0, c(0.0)),
                    ],
                    SpreadMode::Pad,
                    Transform::identity(),
                )?;
                let paint = Paint {
                    shader,
                    anti_alias: true,
                    ..Paint::default()
                };
                pixmap.fill_path(
                    &pb.finish()?,
                    &paint,
                    FillRule::Winding,
                    Transform::identity(),
                    None,
                );
            }
            glow(
                &mut pixmap,
                source.x,
                source.y + hf * 0.1,
                hf * 0.45,
                warm,
                0.18,
                0.1,
            );
            (
                pixmap,
                Rect::new(-fw * 0.28, 0.0, fw * 1.3, fh),
                Motion {
                    distance: fw * 0.22,
                    period_ms: 18_000,
                    alternate: true,
                    eased: true,
                    ..Motion::default()
                },
            )
        }
        _ => return None,
    };
    Some(Content {
        rect: Some(rect),
        clip,
        motion: Some(motion),
        ..Content::new(image(pixmap))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn never_shows_an_edge(rect: Rect, m: Motion, (w, h): (f32, f32)) {
        for ms in (0..80_000).step_by(500) {
            let r = m.apply(rect, Duration::from_millis(ms), 1.0);
            assert!(r.x <= 0.01 && r.y <= 0.01, "{ms}: {r:?}");
            assert!(
                r.x + r.width >= w - 0.01 && r.y + r.height >= h - 0.01,
                "{ms}: {r:?}"
            );
        }
    }

    #[test]
    fn camera_moves_keep_the_frame_covered() {
        let size = (1920.0, 1080.0);
        for kind in ["zoom", "drift", "pan"] {
            for travel in [0.35, 0.75, 0.9, 1.0] {
                let (rect, m) = camera(kind, size, travel);
                never_shows_an_edge(rect, m, size);
            }
        }
    }

    #[test]
    fn atmospheres_loop_and_cover_the_frame() {
        for kind in ["dust", "bokeh", "haze", "rays"] {
            let c = atmosphere(kind, (640, 360)).unwrap();
            never_shows_an_edge(c.rect.unwrap(), c.motion.unwrap(), (640.0, 360.0));
        }
        // A looping atmosphere's tile repeats exactly.
        let Some(Fill::Image(r)) = atmosphere("dust", (640, 360)).map(|c| c.fill) else {
            panic!()
        };
        let half = r.pixels.len() / 2;
        let row = r.width as usize * 4;
        assert_eq!(
            &r.pixels[row * 10..row * 11],
            &r.pixels[half + row * 10..half + row * 11]
        );
        assert!(atmosphere("fireworks", (640, 360)).is_none());
    }
}
