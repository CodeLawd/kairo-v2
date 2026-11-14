//! The Kairo lockup, animated for the launch screen
//! (\`bootstrap/AnimatedMark.tsx\`), in four beats:
//!
//! 1. Assemble: the mark starts alone in the centre; its four tiles fly in
//!    from their own corners, spun and shrunk, landing clockwise with an
//!    overshoot.
//! 2. Make room: the mark glides left to its place in the lockup while
//!    "Kairo" rises in letter by letter.
//! 3. Dot: the i's dot drops last and bounces into place.
//! 4. Alive while loading: a ripple runs clockwise round the tiles (each
//!    presses in, then springs back lit a lighter brand blue), the tiles
//!    breathe apart and back, and a light sheen sweeps across the lockup.
//!
//! The lockup is drawn as paths on a canvas from the same path data, and
//! every frame is a pure function of the time since the launch screen
//! appeared, so the owner of that clock decides when the intro is done
//! ([\`LOCKUP_INTRO_MS\`]). GPUI has no blur, so the letters rise without
//! their blur and the tile glow is drawn as stacked, widening copies; the
//! sheen is drawn in narrow clipped cells. DECISIONS.md (072) lists these.
//! A still lockup (reduced motion, or the copy that fades out) is the
//! finished lockup with nothing moving.

use crate::lockup_paths::{self, MARK_TILES, VIEWBOX_H, VIEWBOX_W};
use crate::motion::Timing;
use crate::theme::KairoThemeExt;
use crate::tokens::Step;
use gpui_kit::base::TestSupportExt;
use gpui_kit::{
    App, Bounds, ContentMask, Hsla, InteractiveElement, IntoElement, ParentElement, PathBuilder,
    Pixels, RenderOnce, Rgba, Role, SharedString, StatefulInteractiveElement, Styled, Window,
    canvas, div, point, px, size,
};
use std::sync::OnceLock;

// Timeline, ms (AnimatedMark.tsx).
const TILE_IN: f32 = 700.0;
const TILE_STAGGER: f32 = 110.0;
const SLIDE_AT: f32 = 850.0;
const SLIDE_MS: f32 = 700.0;
const LETTER_AT: f32 = 1200.0;
const LETTER_STAGGER: f32 = 85.0;
const LETTER_MS: f32 = 650.0;
const DOT_AT: f32 = 1650.0;
const DOT_MS: f32 = 900.0;
const DOT_SETTLE_MS: f32 = 1200.0;
const LOOP_AT: f32 = 2100.0;
const WAVE_MS: f32 = 2400.0;
const SHEEN_MS: f32 = 3600.0;
const SHEEN_WIDTH: f32 = 500.0;
/// \`skewX(-18deg)\`: x shifts by \`tan(-18°) · y\`.
const SHEEN_SKEW: f32 = -0.324_919_7;

/// When the intro has played out: the dot has landed.
pub const LOCKUP_INTRO_MS: u64 = 2550;

/// The mark sits at x 0–364; centred alone, it is shifted to the lockup's
/// middle.
const CENTRE_SHIFT: f32 = VIEWBOX_W / 2.0 - 182.0;

const SLIDE_EASE: Timing = Timing::Bezier(0.65, 0.0, 0.35, 1.0);
const TILE_EASE: Timing = Timing::Bezier(0.34, 1.56, 0.64, 1.0);
const LETTER_EASE: Timing = Timing::Bezier(0.16, 1.0, 0.3, 1.0);
const DROP: Timing = Timing::Bezier(0.55, 0.0, 1.0, 0.45);
const RISE: Timing = Timing::Bezier(0.0, 0.55, 0.45, 1.0);

type P = (f32, f32);

#[derive(Clone, Copy, Debug, PartialEq)]
enum Seg {
    Move(P),
    Line(P),
    Cubic(P, P, P),
    Close,
}

/// One filled piece of the lockup, in viewBox units.
#[derive(Debug)]
struct Shape {
    segs: Vec<Seg>,
    /// \`[min x, min y, max x, max y]\` of the outline.
    bbox: [f32; 4],
}

impl Shape {
    fn new(segs: Vec<Seg>) -> Self {
        let mut bbox = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        let mut add = |(x, y): P| {
            bbox[0] = bbox[0].min(x);
            bbox[1] = bbox[1].min(y);
            bbox[2] = bbox[2].max(x);
            bbox[3] = bbox[3].max(y);
        };
        let mut at = (0.0, 0.0);
        for seg in &segs {
            match *seg {
                Seg::Move(p) | Seg::Line(p) => {
                    add(p);
                    at = p;
                }
                Seg::Cubic(a, b, p) => {
                    // The curve's extent, not its control points (fill-box).
                    for step in 1..=16 {
                        add(cubic_point(at, a, b, p, step as f32 / 16.0));
                    }
                    at = p;
                }
                Seg::Close => {}
            }
        }
        Self { segs, bbox }
    }

    fn centre(&self) -> P {
        (
            (self.bbox[0] + self.bbox[2]) / 2.0,
            (self.bbox[1] + self.bbox[3]) / 2.0,
        )
    }

    fn bottom_centre(&self) -> P {
        ((self.bbox[0] + self.bbox[2]) / 2.0, self.bbox[3])
    }
}

fn cubic_point(p0: P, p1: P, p2: P, p3: P, t: f32) -> P {
    let u = 1.0 - t;
    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    (
        a * p0.0 + b * p1.0 + c * p2.0 + d * p3.0,
        a * p0.1 + b * p1.1 + c * p2.1 + d * p3.1,
    )
}

/// Parses absolute \`M L H V C Z\` path data, the only commands the lockup
/// uses. Other commands are skipped with their numbers.
fn parse(d: &str) -> Vec<Seg> {
    let mut segs = Vec::new();
    let mut numbers = Vec::new();
    let mut command = None;
    let mut at: P = (0.0, 0.0);
    let mut start: P = (0.0, 0.0);
    let flush = |command: Option<char>,
                 numbers: &mut Vec<f32>,
                 segs: &mut Vec<Seg>,
                 at: &mut P,
                 start: &mut P| {
        let Some(command) = command else {
            numbers.clear();
            return;
        };
        let n = numbers.as_slice();
        match command {
            'M' => {
                for (i, pair) in n.as_chunks::<2>().0.iter().enumerate() {
                    let p = (pair[0], pair[1]);
                    // Pairs after the first are implicit line-tos.
                    segs.push(if i == 0 { Seg::Move(p) } else { Seg::Line(p) });
                    if i == 0 {
                        *start = p;
                    }
                    *at = p;
                }
            }
            'L' => {
                for pair in n.as_chunks::<2>().0 {
                    *at = (pair[0], pair[1]);
                    segs.push(Seg::Line(*at));
                }
            }
            'H' => {
                for x in n {
                    *at = (*x, at.1);
                    segs.push(Seg::Line(*at));
                }
            }
            'V' => {
                for y in n {
                    *at = (at.0, *y);
                    segs.push(Seg::Line(*at));
                }
            }
            'C' => {
                for c in n.as_chunks::<6>().0 {
                    *at = (c[4], c[5]);
                    segs.push(Seg::Cubic((c[0], c[1]), (c[2], c[3]), *at));
                }
            }
            'Z' | 'z' => {
                segs.push(Seg::Close);
                *at = *start;
            }
            _ => {}
        }
        numbers.clear();
    };
    let bytes = d.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c.is_ascii_alphabetic() && c != 'e' && c != 'E' {
            flush(command, &mut numbers, &mut segs, &mut at, &mut start);
            command = Some(c);
            if c == 'Z' || c == 'z' {
                flush(command, &mut numbers, &mut segs, &mut at, &mut start);
                command = None;
            }
            i += 1;
        } else if c == '-' || c == '+' || c == '.' || c.is_ascii_digit() {
            let begin = i;
            let mut seen_dot = c == '.';
            i += 1;
            while i < bytes.len() {
                let n = bytes[i] as char;
                if n.is_ascii_digit() {
                    i += 1;
                } else if n == '.' && !seen_dot {
                    seen_dot = true;
                    i += 1;
                } else if (n == 'e' || n == 'E')
                    && bytes
                        .get(i + 1)
                        .is_some_and(|b| b.is_ascii_digit() || *b == b'-' || *b == b'+')
                {
                    i += 2;
                } else {
                    break;
                }
            }
            if let Ok(value) = d[begin..i].parse::<f32>() {
                numbers.push(value);
            }
        } else {
            i += 1;
        }
    }
    flush(command, &mut numbers, &mut segs, &mut at, &mut start);
    segs
}

/// A rounded rectangle as path segments (SVG \`<rect rx>\`).
fn rounded_rect([x, y, w, h, r]: [f32; 5]) -> Vec<Seg> {
    // Circle-quadrant cubic constant.
    let k = r * 0.552_284_8;
    let (right, bottom) = (x + w, y + h);
    vec![
        Seg::Move((x + r, y)),
        Seg::Line((right - r, y)),
        Seg::Cubic((right - r + k, y), (right, y + r - k), (right, y + r)),
        Seg::Line((right, bottom - r)),
        Seg::Cubic(
            (right, bottom - r + k),
            (right - r + k, bottom),
            (right - r, bottom),
        ),
        Seg::Line((x + r, bottom)),
        Seg::Cubic((x + r - k, bottom), (x, bottom - r + k), (x, bottom - r)),
        Seg::Line((x, y + r)),
        Seg::Cubic((x, y + r - k), (x + r - k, y), (x + r, y)),
        Seg::Close,
    ]
}

struct Lockup {
    tiles: [Shape; 4],
    /// k, a, the i's stem, r, o: the order they rise in.
    letters: [Shape; 5],
    dot: Shape,
}

fn lockup() -> &'static Lockup {
    static LOCKUP: OnceLock<Lockup> = OnceLock::new();
    LOCKUP.get_or_init(|| Lockup {
        tiles: MARK_TILES.each_ref().map(|t| Shape::new(parse(t.d))),
        letters: [
            Shape::new(parse(lockup_paths::K)),
            Shape::new(parse(lockup_paths::A)),
            Shape::new(rounded_rect(lockup_paths::I_STEM)),
            Shape::new(parse(lockup_paths::R)),
            Shape::new(parse(lockup_paths::O)),
        ],
        dot: Shape::new(parse(lockup_paths::I_DOT)),
    })
}

/// A 2D affine map \`(x, y) → (a·x + c·y + e, b·x + d·y + f)\`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Affine([f32; 6]);

impl Affine {
    const IDENTITY: Affine = Affine([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    fn translate(x: f32, y: f32) -> Self {
        Affine([1.0, 0.0, 0.0, 1.0, x, y])
    }

    fn scale(x: f32, y: f32) -> Self {
        Affine([x, 0.0, 0.0, y, 0.0, 0.0])
    }

    fn rotate(degrees: f32) -> Self {
        let (s, c) = degrees.to_radians().sin_cos();
        Affine([c, s, -s, c, 0.0, 0.0])
    }

    /// \`self\` after \`other\`: \`other\` is applied first.
    fn then(self, other: Affine) -> Affine {
        let [a, b, c, d, e, f] = self.0;
        let [a2, b2, c2, d2, e2, f2] = other.0;
        Affine([
            a * a2 + c * b2,
            b * a2 + d * b2,
            a * c2 + c * d2,
            b * c2 + d * d2,
            a * e2 + c * f2 + e,
            b * e2 + d * f2 + f,
        ])
    }

    fn apply(self, (x, y): P) -> P {
        let [a, b, c, d, e, f] = self.0;
        (a * x + c * y + e, b * x + d * y + f)
    }

    /// CSS individual transforms about \`origin\`: translate, then rotate,
    /// then scale.
    fn element(origin: P, translate: P, rotate: f32, scale: P) -> Affine {
        Affine::translate(origin.0 + translate.0, origin.1 + translate.1)
            .then(Affine::rotate(rotate))
            .then(Affine::scale(scale.0, scale.1))
            .then(Affine::translate(-origin.0, -origin.1))
    }
}

/// A keyframe property: \`(offset, value, timing to the next stop)\`.
fn track(u: f32, stops: &[(f32, f32, Timing)]) -> f32 {
    let u = u.clamp(0.0, 1.0);
    for pair in stops.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if u <= b.0 {
            let span = (b.0 - a.0).max(f32::EPSILON);
            let local = a.2.sample(((u - a.0) / span).clamp(0.0, 1.0));
            return a.1 + (b.1 - a.1) * local;
        }
    }
    stops.last().map_or(0.0, |s| s.1)
}

/// Progress through a one-shot animation with \`fill-mode: both\`.
fn once(t: f32, delay: f32, duration: f32) -> f32 {
    ((t - delay) / duration).clamp(0.0, 1.0)
}

/// Progress through an infinite animation, or \`None\` before its delay.
fn looping(t: f32, delay: f32, duration: f32) -> Option<f32> {
    (t >= delay).then(|| ((t - delay) % duration) / duration)
}

fn mix(a: Hsla, b: Hsla, t: f32) -> Hsla {
    let (a, b) = (Rgba::from(a), Rgba::from(b));
    let l = |x: f32, y: f32| x + (y - x) * t;
    Hsla::from(Rgba {
        r: l(a.r, b.r),
        g: l(a.g, b.g),
        b: l(a.b, b.b),
        a: l(a.a, b.a),
    })
}

/// The colours the lockup is drawn in.
#[derive(Clone, Copy, Debug)]
struct Palette {
    /// \`--accent-500\`: the tiles, and the dot as it lands.
    tile: Hsla,
    /// \`--accent-400\`: the glow.
    glow: Hsla,
    /// \`--accent-300\`: a tile lit by the ripple.
    lit: Hsla,
    /// \`text-zinc-100\`: the wordmark.
    word: Hsla,
    /// The sheen's white.
    sheen: Hsla,
}

/// One piece to paint: shape, placement in viewBox units, colour.
struct Piece {
    shape: &'static Shape,
    transform: Affine,
    color: Hsla,
}

/// Everything drawn at \`t\` ms. \`still\` is the finished lockup.
fn frame(t: f32, still: bool, palette: Palette) -> (Vec<Piece>, Option<f32>) {
    let lockup = lockup();
    let mut pieces = Vec::new();
    if still {
        for shape in &lockup.tiles {
            pieces.push(Piece {
                shape,
                transform: Affine::IDENTITY,
                color: palette.tile,
            });
        }
        for shape in lockup.letters.iter().chain([&lockup.dot]) {
            pieces.push(Piece {
                shape,
                transform: Affine::IDENTITY,
                color: palette.word,
            });
        }
        return (pieces, None);
    }

    // The mark's glide from the centre.
    let slide = CENTRE_SHIFT * (1.0 - SLIDE_EASE.sample(once(t, SLIDE_AT, SLIDE_MS)));
    let mark = Affine::translate(slide, 0.0);

    for (i, (shape, tile)) in lockup.tiles.iter().zip(MARK_TILES.iter()).enumerate() {
        let (dx, dy) = (tile.dx, tile.dy);
        let p = once(t, i as f32 * TILE_STAGGER, TILE_IN);
        let e = TILE_EASE.sample(p);
        let mut translate = (dx * 520.0 * (1.0 - e), dy * 520.0 * (1.0 - e));
        let mut rotate = dx * dy * 90.0 * (1.0 - e);
        let mut scale = 0.3 + 0.7 * e;
        let opacity = track(
            p,
            &[
                (0.0, 0.0, TILE_EASE),
                (0.6, 1.0, TILE_EASE),
                (1.0, 1.0, TILE_EASE),
            ],
        );
        let mut color = palette.tile;
        let mut glow = 0.0;
        let ease = Timing::EaseInOut;
        if let Some(u) = looping(t, LOOP_AT + i as f32 * WAVE_MS / 4.0, WAVE_MS) {
            let k = dx * dy;
            scale = track(
                u,
                &[
                    (0.0, 1.0, ease),
                    (0.06, 0.88, ease),
                    (0.15, 1.05, ease),
                    (0.34, 1.0, ease),
                    (1.0, 1.0, ease),
                ],
            );
            rotate = track(
                u,
                &[
                    (0.0, 0.0, ease),
                    (0.06, -4.0 * k, ease),
                    (0.15, 2.0 * k, ease),
                    (0.34, 0.0, ease),
                    (1.0, 0.0, ease),
                ],
            );
            let lit = track(
                u,
                &[
                    (0.0, 0.0, ease),
                    (0.06, 0.0, ease),
                    (0.15, 1.0, ease),
                    (0.34, 0.0, ease),
                    (1.0, 0.0, ease),
                ],
            );
            color = mix(palette.tile, palette.lit, lit);
            glow = track(
                u,
                &[
                    (0.0, 0.0, ease),
                    (0.06, 0.0, ease),
                    (0.15, 0.45, ease),
                    (0.34, 0.0, ease),
                    (1.0, 0.0, ease),
                ],
            );
        }
        if let Some(u) = looping(t, LOOP_AT, WAVE_MS * 2.0) {
            let apart = track(u, &[(0.0, 0.0, ease), (0.5, 10.0, ease), (1.0, 0.0, ease)]);
            translate = (dx * apart, dy * apart);
        }
        let origin = shape.centre();
        if glow > 0.0 {
            // A 36px blur, approximated by widening copies.
            for (spread, weight) in [(1.04, 0.45), (1.1, 0.3), (1.18, 0.2), (1.28, 0.12)] {
                pieces.push(Piece {
                    shape,
                    transform: mark.then(Affine::element(origin, translate, 0.0, (spread, spread))),
                    color: palette.glow.opacity(glow * weight),
                });
            }
        }
        pieces.push(Piece {
            shape,
            transform: mark.then(Affine::element(origin, translate, rotate, (scale, scale))),
            color: color.opacity(opacity),
        });
    }

    for (i, shape) in lockup.letters.iter().enumerate() {
        let e = LETTER_EASE.sample(once(t, LETTER_AT + i as f32 * LETTER_STAGGER, LETTER_MS));
        let scale = 0.85 + 0.15 * e;
        pieces.push(Piece {
            shape,
            transform: Affine::element(
                shape.bottom_centre(),
                (0.0, 140.0 * (1.0 - e)),
                0.0,
                (scale, scale),
            ),
            color: palette.word.opacity(e),
        });
    }

    // The drop: two shrinking bounces, squashing on each landing.
    let u = once(t, DOT_AT, DOT_MS);
    let y = track(
        u,
        &[
            (0.0, -420.0, DROP),
            (0.45, 0.0, RISE),
            (0.65, -110.0, DROP),
            (0.82, 0.0, RISE),
            (0.91, -30.0, DROP),
            (1.0, 0.0, DROP),
        ],
    );
    let sx = track(
        u,
        &[
            (0.0, 1.0, DROP),
            (0.45, 1.25, RISE),
            (0.65, 0.92, DROP),
            (0.82, 1.12, RISE),
            (0.91, 1.0, DROP),
            (1.0, 1.0, DROP),
        ],
    );
    let sy = track(
        u,
        &[
            (0.0, 1.0, DROP),
            (0.45, 0.7, RISE),
            (0.65, 1.1, DROP),
            (0.82, 0.85, RISE),
            (0.91, 1.0, DROP),
            (1.0, 1.0, DROP),
        ],
    );
    let opacity = track(
        u,
        &[
            (0.0, 0.0, DROP),
            (0.1, 1.0, Timing::Linear),
            (1.0, 1.0, Timing::Linear),
        ],
    );
    // Lands in the accent, then cools to the wordmark colour.
    let cool = Timing::EaseOut.sample(once(t, DOT_AT + DOT_MS, DOT_SETTLE_MS));
    pieces.push(Piece {
        shape: &lockup.dot,
        transform: Affine::element(lockup.dot.bottom_centre(), (0.0, y), 0.0, (sx, sy)),
        color: mix(palette.tile, palette.word, cool).opacity(opacity),
    });

    let sheen = match looping(t, LOOP_AT + 300.0, SHEEN_MS) {
        Some(u) => track(
            u,
            &[
                (0.0, -700.0, Timing::EaseInOut),
                (0.45, VIEWBOX_W + 200.0, Timing::EaseInOut),
                (1.0, VIEWBOX_W + 200.0, Timing::EaseInOut),
            ],
        ),
        None => -700.0,
    };
    (pieces, Some(sheen))
}

fn build(shape: &Shape, transform: Affine) -> Option<gpui_kit::Path<Pixels>> {
    let mut builder = PathBuilder::fill();
    let at = |p: P| {
        let (x, y) = transform.apply(p);
        point(px(x), px(y))
    };
    for seg in &shape.segs {
        match *seg {
            Seg::Move(p) => builder.move_to(at(p)),
            Seg::Line(p) => builder.line_to(at(p)),
            Seg::Cubic(a, b, p) => builder.cubic_bezier_to(at(p), at(a), at(b)),
            Seg::Close => builder.close(),
        }
    }
    builder.build().ok()
}

/// The sheen's opacity at viewBox point \`(x, y)\` with the band at \`left\`:
/// the gradient \`transparent → white 0.55 → transparent\` across the skewed
/// 500-wide band.
fn sheen_alpha(left: f32, (x, y): P) -> f32 {
    let u = (x - left - SHEEN_SKEW * y) / SHEEN_WIDTH;
    if !(0.0..=1.0).contains(&u) {
        return 0.0;
    }
    0.55 * (1.0 - (2.0 * u - 1.0).abs())
}

fn paint(bounds: Bounds<Pixels>, t: f32, still: bool, palette: Palette, window: &mut Window) {
    let s = f32::from(bounds.size.width) / VIEWBOX_W;
    let screen = Affine::translate(f32::from(bounds.origin.x), f32::from(bounds.origin.y))
        .then(Affine::scale(s, s));
    let (pieces, sheen) = frame(t, still, palette);
    for piece in &pieces {
        if piece.color.a <= 0.0 {
            continue;
        }
        if let Some(path) = build(piece.shape, screen.then(piece.transform)) {
            window.paint_path(path, piece.color);
        }
    }
    let Some(left) = sheen else {
        return;
    };
    // One sheen across mark and word together, masked to the lockup at rest.
    let x_min = (left + SHEEN_SKEW * VIEWBOX_H).max(0.0);
    let x_max = (left + SHEEN_WIDTH).min(VIEWBOX_W);
    if x_min >= x_max {
        return;
    }
    let lockup = lockup();
    let shapes: Vec<(&Shape, gpui_kit::Path<Pixels>)> = lockup
        .tiles
        .iter()
        .chain(lockup.letters.iter())
        .chain([&lockup.dot])
        .filter(|shape| shape.bbox[2] >= x_min && shape.bbox[0] <= x_max)
        .filter_map(|shape| build(shape, screen).map(|path| (shape, path)))
        .collect();
    const CELL_PX: f32 = 4.0;
    const ROWS: usize = 6;
    let cell_w = CELL_PX / s;
    let cell_h = VIEWBOX_H / ROWS as f32;
    let mut x = x_min;
    while x < x_max {
        for row in 0..ROWS {
            let y = row as f32 * cell_h;
            let alpha = sheen_alpha(left, (x + cell_w / 2.0, y + cell_h / 2.0));
            if alpha <= 0.0 {
                continue;
            }
            let (x0, y0) = screen.apply((x, y));
            let cell = Bounds::new(point(px(x0), px(y0)), size(px(CELL_PX), px(cell_h * s)));
            window.with_content_mask(Some(ContentMask { bounds: cell }), |window| {
                for (shape, path) in &shapes {
                    if shape.bbox[2] >= x && shape.bbox[0] <= x + cell_w {
                        window.paint_path(path.clone(), palette.sheen.opacity(alpha));
                    }
                }
            });
        }
        x += cell_w;
    }
}

/// The animated lockup at \`width\`, drawn as it is \`elapsed_ms\` after the
/// launch screen appeared. With \`still\`, or under reduced motion, the
/// finished lockup with nothing moving.
#[derive(IntoElement)]
pub struct AnimatedLockup {
    width: Pixels,
    elapsed_ms: f32,
    still: bool,
}

impl AnimatedLockup {
    /// A lockup \`width\` wide at \`elapsed_ms\` into its timeline.
    pub fn new(width: Pixels, elapsed_ms: f32) -> Self {
        Self {
            width,
            elapsed_ms,
            still: false,
        }
    }

    /// Draws the finished lockup with no motion.
    pub fn still(mut self, still: bool) -> Self {
        self.still = still;
        self
    }
}

impl RenderOnce for AnimatedLockup {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = *cx.kairo();
        let palette = Palette {
            tile: k.accent(Step::S500),
            glow: k.accent(Step::S400),
            lit: k.accent(Step::S300),
            word: k.text_primary,
            sheen: k.white,
        };
        let still = self.still || cx.reduced_motion();
        let t = self.elapsed_ms;
        let height = f32::from(self.width) * VIEWBOX_H / VIEWBOX_W;
        div()
            .id("kairo-launch-lockup")
            .test_support()
            .role(Role::Image)
            .aria_label(SharedString::new_static(crate::brand::PRODUCT_NAME))
            .w(self.width)
            .h(px(height))
            .flex_shrink_0()
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, _| paint(bounds, t, still, palette, window),
                )
                .size_full(),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette() -> Palette {
        let c = |l: f32| Hsla {
            h: 0.6,
            s: 0.8,
            l,
            a: 1.0,
        };
        Palette {
            tile: c(0.5),
            glow: c(0.6),
            lit: c(0.7),
            word: c(0.95),
            sheen: c(1.0),
        }
    }

    #[test]
    fn parses_every_piece_of_the_lockup() {
        let lockup = lockup();
        for shape in lockup
            .tiles
            .iter()
            .chain(lockup.letters.iter())
            .chain([&lockup.dot])
        {
            assert!(matches!(shape.segs.first(), Some(Seg::Move(_))));
            assert!(shape.segs.contains(&Seg::Close));
            assert!(shape.bbox[0] >= -0.5 && shape.bbox[2] <= VIEWBOX_W + 0.5);
            assert!(shape.bbox[1] >= -0.5 && shape.bbox[3] <= VIEWBOX_H + 0.5);
        }
        // The mark spans x 0–364, as CENTRE_SHIFT assumes.
        let right = lockup.tiles.iter().map(|s| s.bbox[2]).fold(0.0, f32::max);
        assert!((right - 364.0).abs() < 0.5, "{right}");
    }

    #[test]
    fn parses_exponents_and_implicit_repeats() {
        let segs = parse("M0 0L1 2 3 4H5V6C1 2 3 4 5e-1 -2.5Z");
        assert_eq!(
            segs,
            vec![
                Seg::Move((0.0, 0.0)),
                Seg::Line((1.0, 2.0)),
                Seg::Line((3.0, 4.0)),
                Seg::Line((5.0, 4.0)),
                Seg::Line((5.0, 6.0)),
                Seg::Cubic((1.0, 2.0), (3.0, 4.0), (0.5, -2.5)),
                Seg::Close,
            ]
        );
    }

    #[test]
    fn starts_centred_and_hidden_then_settles() {
        let (start, sheen) = frame(0.0, false, palette());
        // The first tile has not appeared; the letters and dot are hidden.
        assert_eq!(start[0].color.a, 0.0);
        assert!(start[4..].iter().all(|p| p.color.a == 0.0));
        assert_eq!(sheen, Some(-700.0));
        // Mark centred: its transform carries the full shift.
        assert!((start[0].transform.apply((0.0, 0.0)).0 - CENTRE_SHIFT).abs() > 1.0);

        // Before the loop starts the tiles are at rest.
        let (assembled, _) = frame(LOOP_AT - 1.0, false, palette());
        for tile in &assembled[..4] {
            let rest = tile.transform.apply((10.0, 10.0));
            assert!(
                (rest.0 - 10.0).abs() < 0.01 && (rest.1 - 10.0).abs() < 0.01,
                "{rest:?}"
            );
        }
        // Once the dot lands the whole lockup is showing.
        let (done, _) = frame(LOCKUP_INTRO_MS as f32, false, palette());
        assert!(done[done.len() - 6..].iter().all(|p| p.color.a > 0.99));
    }

    #[test]
    fn dot_lands_in_the_accent_and_cools_to_the_word() {
        let p = palette();
        let dot = |t: f32| frame(t, false, p).0.pop().expect("dot").color;
        let landed = Rgba::from(dot(DOT_AT + DOT_MS));
        let tile = Rgba::from(p.tile);
        assert!((landed.r - tile.r).abs() < 0.01 && (landed.b - tile.b).abs() < 0.01);
        let cooled = Rgba::from(dot(DOT_AT + DOT_MS + DOT_SETTLE_MS + 1.0));
        let word = Rgba::from(p.word);
        assert!((cooled.r - word.r).abs() < 0.01 && (cooled.b - word.b).abs() < 0.01);
    }

    #[test]
    fn ripple_lights_tiles_clockwise_and_glows() {
        let p = palette();
        // 15% into the first tile's wave: lit and glowing; the second waits.
        let t = LOOP_AT + 0.15 * WAVE_MS;
        let (pieces, _) = frame(t, false, p);
        let glows = pieces
            .iter()
            .filter(|piece| piece.color.h == p.glow.h && piece.color.a < 0.3 && piece.color.a > 0.0)
            .count();
        assert!(glows >= 4, "glow copies drawn: {glows}");
    }

    #[test]
    fn still_lockup_has_no_motion_glow_or_sheen() {
        let p = palette();
        let (pieces, sheen) = frame(500.0, true, p);
        assert_eq!(sheen, None);
        assert_eq!(pieces.len(), 10);
        assert!(
            pieces
                .iter()
                .all(|piece| piece.transform == Affine::IDENTITY)
        );
        // The dot is in the wordmark colour.
        assert_eq!(pieces[9].color, p.word);
    }

    #[test]
    fn sheen_is_a_skewed_triangular_band() {
        assert_eq!(sheen_alpha(0.0, (-1.0, 0.0)), 0.0);
        assert!((sheen_alpha(0.0, (250.0, 0.0)) - 0.55).abs() < 1e-4);
        // Lower rows are shifted left by the skew.
        let y = 300.0;
        assert!((sheen_alpha(0.0, (250.0 + SHEEN_SKEW * y, y)) - 0.55).abs() < 1e-3);
    }

    #[test]
    fn element_transform_scales_about_its_origin() {
        let m = Affine::element((10.0, 10.0), (5.0, 0.0), 0.0, (2.0, 2.0));
        assert_eq!(m.apply((10.0, 10.0)), (15.0, 10.0));
        assert_eq!(m.apply((11.0, 10.0)), (17.0, 10.0));
        let r = Affine::rotate(90.0).apply((1.0, 0.0));
        assert!(r.0.abs() < 1e-6 && (r.1 - 1.0).abs() < 1e-6);
    }
}
