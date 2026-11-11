//! `#pa-info-layer`: the clock bar and the stage message, sized from
//! `confidenceSizes`, at the top or bottom of the frame.

use super::*;
use crate::LINE_HEIGHT_NORMAL;
use crate::color::Rgba;
use crate::scene::{Align, Rect, Shadow, Vertical};
use kairo_domain::program::{ConfidenceInfo, TopOrBottom};

const WHITE: Rgba = Rgba::new(255, 255, 255, 255);
/// `#pa-info-message { background: #b91c1c }`.
pub(super) const STAGE_RED: Rgba = Rgba::new(0xb9, 0x1c, 0x1c, 255);

/// `__setInfo(next)`: nothing when `info` is `None`.
pub fn plan_info(ts: &mut Typesetter, info: &ConfidenceInfo, frame: (u32, u32)) -> LayerPlan {
    let mut plan = LayerPlan::default();
    let (fw, fh) = (frame.0 as f32, frame.1 as f32);
    let bottom = info.layout.position == TopOrBottom::Bottom;
    let bar_px = info.sizes.bar_px as f32;
    let message_px = info.sizes.message_px as f32;
    if info.clock {
        let backdrop = info.style.backdrop;
        let mut look = TextLook::new(SANS, bar_px, 700.0, LINE_HEIGHT_NORMAL, WHITE);
        look.tabular = true;
        // `text-shadow: 0 2px 8px rgba(0,0,0,.8)`, off over the backdrop.
        look.shadow = (!backdrop).then_some(Shadow {
            x: 0.0,
            y: 2.0,
            blur: 8.0,
            color: Rgba::new(0, 0, 0, 204),
        });
        let (_, line) = measure(ts, &look, "0", Space::NoWrap, fw);
        // `padding: 20px 48px`.
        let bar_h = line + 40.0;
        let y = if bottom { fh - bar_h } else { 0.0 };
        if backdrop {
            let bar = Rect {
                x: 0.0,
                y,
                width: fw,
                height: bar_h,
            };
            plan.push(box_tile(
                bar,
                Corners::Px(0.0),
                Rgba::new(0, 0, 0, 184),
                frame,
            ));
        }
        look.align = Align::Left;
        plan.pieces.push(Piece::Live(Live {
            kind: LiveKind::Clock { color: WHITE },
            text: look,
            rect: Rect {
                x: 48.0,
                y: y + 20.0,
                width: fw - 96.0,
                height: line,
            },
            vertical: Vertical::Top,
            clipped: false,
            opacity: 1.0,
            frame,
        }));
    }
    // `#pa-info-message:empty { display: none }`: only the empty string hides it.
    if let Some(text) = info.stage_message.as_deref().filter(|t| !t.is_empty()) {
        // `calc(var(--info-bar) * 1.25 + 40px)` below the bar, or 40 px in.
        let offset = if info.clock {
            bar_px * 1.25 + 40.0
        } else {
            40.0
        };
        stage_message(ts, &mut plan, text, message_px, offset, bottom, frame);
    }
    plan
}

/// The red stage message box: `left: 40px; right: 40px; padding: 18px
/// 28px; border-radius: 14px`, bold centred words at `size`, line height
/// 1.15, `offset` from the top (or the bottom).
pub(super) fn stage_message(
    ts: &mut Typesetter,
    plan: &mut LayerPlan,
    text: &str,
    size: f32,
    offset: f32,
    from_bottom: bool,
    frame: (u32, u32),
) {
    let width = frame.0 as f32 - 80.0;
    let inner = width - 56.0;
    let mut look = TextLook::new(SANS, size, 700.0, 1.15, WHITE);
    look.align = Align::Center;
    let (_, h) = measure(ts, &look, text, Space::Normal, inner);
    let box_h = h + 36.0;
    let y = if from_bottom {
        frame.1 as f32 - offset - box_h
    } else {
        offset
    };
    let card = Rect {
        x: 40.0,
        y,
        width,
        height: box_h,
    };
    plan.push(box_tile(card, Corners::Px(14.0), STAGE_RED, frame));
    let words = Rect {
        x: 68.0,
        y: y + 18.0,
        width: inner,
        height: h,
    };
    plan.push(text_tile(
        ts,
        &look,
        text,
        Space::Normal,
        words,
        Vertical::Top,
        false,
        frame,
    ));
}
