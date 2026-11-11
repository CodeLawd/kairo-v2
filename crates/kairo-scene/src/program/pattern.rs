//! `outputTestPatternHTML`: colour bars, fine text and the motion sweep,
//! over everything on the output, with the output's name in a corner.

use super::*;
use crate::LINE_HEIGHT_NORMAL;
use crate::color::Rgba;
use crate::scene::{Rect, Vertical};
use tiny_skia::{Color, GradientStop, LinearGradient, Point, SpreadMode};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pattern {
    Bars,
    Text,
    Motion,
}

const WHITE: Rgba = Rgba::new(255, 255, 255, 255);
const BLACK: Rgba = Rgba::new(0, 0, 0, 255);

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

/// A box filled by `paint`, snapped to whole pixels.
fn painted(r: Rect, frame: (u32, u32), paint: impl Fn(&mut Pixmap, f32, f32)) -> Option<Tile> {
    let x0 = r.x.round().max(0.0);
    let y0 = r.y.round().max(0.0);
    let x1 = (r.x + r.width).round().min(frame.0 as f32);
    let y1 = (r.y + r.height).round().min(frame.1 as f32);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let mut pixmap = Pixmap::new((x1 - x0) as u32, (y1 - y0) as u32)?;
    paint(&mut pixmap, x0, y0);
    Some(Tile {
        x: x0 as i32,
        y: y0 as i32,
        width: pixmap.width(),
        height: pixmap.height(),
        pixels: pixmap.take(),
    })
}

fn rgb(hex: u32) -> Rgba {
    Rgba::new((hex >> 16) as u8, (hex >> 8) as u8, hex as u8, 255)
}

/// The test pattern `name` · `pattern` on a black frame.
pub fn plan_pattern(
    ts: &mut Typesetter,
    pattern: Pattern,
    name: &str,
    frame: (u32, u32),
) -> LayerPlan {
    let mut plan = LayerPlan::default();
    let (fw, fh) = (frame.0 as f32, frame.1 as f32);
    plan.push(box_tile(
        rect(0.0, 0.0, fw, fh),
        Corners::Px(0.0),
        BLACK,
        frame,
    ));
    match pattern {
        Pattern::Bars => {
            // Seven flex columns over 72 % of the height, then a black to
            // white ramp over the rest.
            let colors = [
                0xffffff, 0xffff00, 0x00ffff, 0x00ff00, 0xff00ff, 0xff0000, 0x0000ff,
            ];
            let top = fh * 0.72;
            let w = fw / colors.len() as f32;
            for (i, c) in colors.into_iter().enumerate() {
                plan.push(box_tile(
                    rect(w * i as f32, 0.0, w, top),
                    Corners::Px(0.0),
                    rgb(c),
                    frame,
                ));
            }
            plan.push(painted(
                rect(0.0, top, fw, fh * 0.28),
                frame,
                |pixmap, x0, _| {
                    let shader = LinearGradient::new(
                        Point::from_xy(-x0, 0.0),
                        Point::from_xy(fw - x0, 0.0),
                        vec![
                            GradientStop::new(0.0, Color::BLACK),
                            GradientStop::new(1.0, Color::WHITE),
                        ],
                        SpreadMode::Pad,
                        Transform::identity(),
                    );
                    if let Some(shader) = shader {
                        let paint = Paint {
                            shader,
                            ..Paint::default()
                        };
                        let r = tiny_skia::Rect::from_xywh(
                            0.0,
                            0.0,
                            pixmap.width() as f32,
                            pixmap.height() as f32,
                        );
                        if let Some(r) = r {
                            pixmap.fill_rect(r, &paint, Transform::identity(), None);
                        }
                    }
                },
            ));
        }
        Pattern::Text => {
            // `padding: 6%` (of the width) and `line-height: 1.5`.
            let pad = fw * 0.06;
            let inner = fw - 2.0 * pad;
            let mut y = pad;
            for (size, line) in [
                (64.0, "Kairo · Fine text and alignment"),
                (36.0, "ABCDEFGHIJKLMNOPQRSTUVWXYZ · 0123456789"),
                (24.0, "White text · sharp edges · even horizontal lines"),
            ] {
                let look = TextLook::new(ARIAL, size, 400.0, 1.5, WHITE);
                let (_, h) = measure(ts, &look, line, Space::Normal, inner);
                plan.push(text_tile(
                    ts,
                    &look,
                    line,
                    Space::Normal,
                    rect(pad, y, inner, h),
                    Vertical::Top,
                    false,
                    frame,
                ));
                y += h;
            }
            // 1 px white lines every 4 px, 180 px tall, 40 px below.
            plan.push(painted(
                rect(pad, y + 40.0, inner, 180.0),
                frame,
                |pixmap, _, y0| {
                    let mut paint = Paint::default();
                    paint.set_color(Color::WHITE);
                    let w = pixmap.width() as f32;
                    let first = y + 40.0;
                    for row in 0..pixmap.height() {
                        let frame_y = y0 + row as f32;
                        if ((frame_y - first).round() as i64).rem_euclid(4) == 0
                            && let Some(r) = tiny_skia::Rect::from_xywh(0.0, row as f32, w, 1.0)
                        {
                            pixmap.fill_rect(r, &paint, Transform::identity(), None);
                        }
                    }
                },
            ));
        }
        Pattern::Motion => {
            // A white bar 120 px wide sweeping 1800 px and back every 3 s.
            if let Some(tile) = box_tile(rect(0.0, 0.0, 120.0, fh), Corners::Px(0.0), WHITE, frame)
            {
                plan.pieces.push(Piece::Moving {
                    tile,
                    clip: rect(0.0, 0.0, fw, fh),
                    distance: 1800.0,
                    period_ms: 3000,
                    alternate: true,
                });
            }
            // Over it, a 1 px slate line closing every 120 px column.
            let mut x = 119.0;
            while x < fw {
                plan.push(box_tile(
                    rect(x, 0.0, 1.0, fh),
                    Corners::Px(0.0),
                    rgb(0x475569),
                    frame,
                ));
                x += 120.0;
            }
        }
    }
    let label = match pattern {
        Pattern::Bars => "Color bars",
        Pattern::Text => "Fine text",
        Pattern::Motion => "Motion",
    };
    let text = format!("{name} · {label} test");
    let look = TextLook::new(ARIAL, 24.0, 400.0, LINE_HEIGHT_NORMAL, WHITE);
    let (w, h) = measure(ts, &look, &text, Space::Normal, fw - 48.0);
    // `bottom: 24px; left: 24px; padding: 12px 18px` on black.
    let card = rect(24.0, fh - 24.0 - (h + 24.0), w + 36.0, h + 24.0);
    plan.push(box_tile(card, Corners::Px(0.0), BLACK, frame));
    plan.push(text_tile(
        ts,
        &look,
        &text,
        Space::Normal,
        rect(42.0, card.y + 12.0, w, h),
        Vertical::Top,
        false,
        frame,
    ));
    plan
}
