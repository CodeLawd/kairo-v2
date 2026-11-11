//! `stage.html`: the confidence monitor. Clock and countdown along the top,
//! the stage message, the slide that is live, and what comes next.

use super::info::stage_message;
use super::*;
use crate::LINE_HEIGHT_NORMAL;
use crate::color::Rgba;
use crate::scene::{Align, Rect, Vertical};
use kairo_domain::program::StageView;

/// `stage.html` is always laid out at 1920 × 1080 and letterboxed.
pub const STAGE_FRAME: (u32, u32) = (1920, 1080);

const BLACK: Rgba = Rgba::new(0, 0, 0, 255);
const RULE: Rgba = Rgba::new(0x36, 0x36, 0x2d, 255);
const MUTED: Rgba = Rgba::new(0xa2, 0x9f, 0x96, 255);
const TEXT: Rgba = Rgba::new(0xf9, 0xfa, 0xfb, 255);

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

/// `__setStage(view)`; the clock and countdown tick by themselves.
pub fn plan_stage(ts: &mut Typesetter, view: &StageView) -> LayerPlan {
    let frame = STAGE_FRAME;
    let mut plan = LayerPlan::default();
    plan.push(box_tile(
        rect(0.0, 0.0, 1920.0, 1080.0),
        Corners::Px(0.0),
        BLACK,
        frame,
    ));

    // #top: 120 px with a 2 px rule inside its bottom edge; the items are
    // centred in the 118 px above it, `space-between` across 56 px padding.
    plan.push(box_tile(
        rect(0.0, 118.0, 1920.0, 2.0),
        Corners::Px(0.0),
        RULE,
        frame,
    ));
    let mut clock_look = TextLook::new(
        SANS,
        76.0,
        600.0,
        LINE_HEIGHT_NORMAL,
        Rgba::new(0xe7, 0xe4, 0xdc, 255),
    );
    clock_look.tabular = true;
    let (_, line) = measure(ts, &clock_look, "0", Space::NoWrap, 1808.0);
    let lane = rect(56.0, (118.0 - line) / 2.0, 1808.0, line);
    if view.show_clock {
        plan.pieces.push(Piece::Live(Live {
            kind: LiveKind::Clock {
                color: clock_look.color,
            },
            text: clock_look.clone(),
            rect: lane,
            vertical: Vertical::Top,
            clipped: false,
            opacity: 1.0,
            frame,
        }));
    }
    if view.show_timer {
        let mut look = TextLook::new(SANS, 76.0, 700.0, LINE_HEIGHT_NORMAL, TEXT);
        look.tabular = true;
        // Alone, `space-between` puts it at the start.
        look.align = if view.show_clock {
            Align::Right
        } else {
            Align::Left
        };
        plan.pieces.push(Piece::Live(Live {
            kind: LiveKind::Timer {
                run: TimerRun {
                    duration_sec: view.timer.duration_sec,
                    ends_at: view.timer.ends_at,
                    remaining_sec: view.timer.remaining_sec,
                    rollover: view.timer_style.rollover,
                },
                color: css(&view.timer_style.color).unwrap_or(TEXT),
                overrun: css(&view.timer_style.overrun_color).unwrap_or(TEXT),
            },
            text: look,
            rect: lane,
            vertical: Vertical::Top,
            clipped: false,
            opacity: 1.0,
            frame,
        }));
    }

    // #current: a centred column cut to its box; taller when NEXT is hidden.
    let bottom = if view.show_next { 300.0 } else { 40.0 };
    let current = rect(56.0, 140.0, 1808.0, 1080.0 - bottom - 140.0);
    let reference = view.current.as_ref().map_or("", |c| c.reference.as_str());
    let words = view.current.as_ref().map_or("", |c| c.text.as_str());
    let ref_look = TextLook::new(SANS, 40.0, 400.0, LINE_HEIGHT_NORMAL, MUTED);
    let text_look = TextLook::new(SANS, 84.0, 600.0, 1.18, TEXT);
    let (_, ref_h) = measure(ts, &ref_look, reference, Space::Normal, current.width);
    let (_, text_h) = measure(ts, &text_look, words, Space::Pre, current.width);
    // The empty reference still has its 12 px margin.
    let total = ref_h + 12.0 + text_h;
    let top = current.y + (current.height - total) / 2.0;
    for tile in [
        text_tile(
            ts,
            &ref_look,
            reference,
            Space::Normal,
            rect(current.x, top, current.width, ref_h),
            Vertical::Top,
            false,
            frame,
        ),
        text_tile(
            ts,
            &text_look,
            words,
            Space::Pre,
            rect(current.x, top + ref_h + 12.0, current.width, text_h),
            Vertical::Top,
            false,
            frame,
        ),
    ]
    .into_iter()
    .flatten()
    {
        plan.push(clip_tile(tile, current));
    }

    // #next: 280 px at the bottom, a 2 px rule on top, 24 px 56 px padding.
    if view.show_next {
        let next = rect(0.0, 800.0, 1920.0, 280.0);
        plan.push(box_tile(
            next,
            Corners::Px(0.0),
            Rgba::new(0x0c, 0x11, 0x1d, 255),
            frame,
        ));
        plan.push(box_tile(
            rect(0.0, 800.0, 1920.0, 2.0),
            Corners::Px(0.0),
            RULE,
            frame,
        ));
        let mut label = TextLook::new(SANS, 32.0, 700.0, LINE_HEIGHT_NORMAL, MUTED);
        // `letter-spacing: 0.06em`.
        label.letter_spacing = 32.0 * 0.06;
        let (_, label_h) = measure(ts, &label, "NEXT", Space::NoWrap, 1808.0);
        let x = 56.0;
        let y = 826.0;
        let following = match &view.next {
            Some(n) if !n.text.is_empty() => n.text.as_str(),
            Some(n) => n.reference.as_str(),
            None => "—",
        };
        let next_look = TextLook::new(SANS, 52.0, 400.0, 1.2, Rgba::new(0xd5, 0xd1, 0xc9, 255));
        let (_, next_h) = measure(ts, &next_look, following, Space::Pre, 1808.0);
        for tile in [
            text_tile(
                ts,
                &label,
                "NEXT",
                Space::NoWrap,
                rect(x, y, 1808.0, label_h),
                Vertical::Top,
                false,
                frame,
            ),
            text_tile(
                ts,
                &next_look,
                following,
                Space::Pre,
                rect(x, y + label_h + 8.0, 1808.0, next_h),
                Vertical::Top,
                false,
                frame,
            ),
        ]
        .into_iter()
        .flatten()
        {
            plan.push(clip_tile(tile, next));
        }
    }

    // #message sits above the slide (`z-index: 2`), 132 px down.
    if let Some(text) = view.stage_message.as_deref().filter(|t| !t.is_empty()) {
        stage_message(ts, &mut plan, text, 60.0, 132.0, false, frame);
    }
    plan
}
