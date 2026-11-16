//! Spotlight tips over the real UI after setup (`CoachTour.tsx`). The rest
//! of the window dims, the target shows through a cutout, and a card sits
//! beside it. Clicking the highlighted control works and moves on; Escape or
//! Skip ends it; a target that never appears is skipped after 1.5 s.
//!
//! Targets are anchors: a control wraps itself with [`anchor`] under a
//! stable name, and its bounds are recorded every time it paints. Electron's
//! CSS selectors map to these names (DECISIONS.md, 073).

use gpui_kit::base::TestSupportExt;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::{
    Bounds, Context, DispatchPhase, EventEmitter, FontWeight, InteractiveElement, IntoElement,
    KeyDownEvent, MouseUpEvent, ParentElement, Pixels, Render, Role, SharedString,
    StatefulInteractiveElement, Styled, Task, Window, canvas, div, point, prelude::FluentBuilder,
    px, size,
};
use kairo_ui_kit::KairoThemeExt;
use kairo_ui_kit::tokens::{TextStep, text};
use std::time::{Duration, Instant};

pub use kairo_ui_kit::coach::{Anchors, anchors, coach_anchor};

/// One tip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoachStep {
    /// The anchor it points at.
    pub target: &'static str,
    /// Title.
    pub title: &'static str,
    /// Body.
    pub body: &'static str,
}

/// Which walk-round.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CoachTourId {
    /// The Operator, after setup or from Settings.
    Operator,
    /// One tip on Scripture.
    Scripture,
    /// One tip on Lyrics.
    Lyrics,
    /// One tip on Documents.
    Documents,
}

const fn step(target: &'static str, title: &'static str, body: &'static str) -> CoachStep {
    CoachStep {
        target,
        title,
        body,
    }
}

/// `COACH_TOURS`.
pub fn coach_tour(id: CoachTourId) -> &'static [CoachStep] {
    use anchors::*;
    const OPERATOR: &[CoachStep] = &[
        step(
            WORKSPACES,
            "Your workspaces",
            "Scripture, Lyrics, Documents and Themes live here.",
        ),
        step(
            TRANSCRIPT_START,
            "Start listening",
            "Hit Start when the message begins. Kairo transcribes the sermon live.",
        ),
        step(
            DETECTED,
            "Detected verses",
            "Verses the preacher reads land here as slides. Click one to send it live.",
        ),
        step(
            AUTOMATION,
            "Automation",
            "When it’s sure of a single verse, Kairo puts it on screen after a short countdown you can cancel.",
        ),
        step(
            LIVE_PREVIEW,
            "What the room sees",
            "This preview always shows what’s on screen right now.",
        ),
        step(
            HEADER_LIVE,
            "Your safety net",
            "Clear takes text and backgrounds off the screen instantly.",
        ),
        step(
            HEADER_SCREENS,
            "Screens",
            "Choose which display your audience sees.",
        ),
    ];
    const SCRIPTURE: &[CoachStep] = &[step(
        SCRIPTURE_QUERY,
        "Find any verse",
        "Type a reference like John 3:16, or words you remember.",
    )];
    const LYRICS: &[CoachStep] = &[step(
        LYRICS_NEW,
        "Add your first song",
        "Type it in or import lyrics — it’s saved to your Songs folder.",
    )];
    const DOCUMENTS: &[CoachStep] = &[step(
        DOCUMENTS_IMPORT,
        "Bring in a deck",
        "Import a PowerPoint or PDF to present it page by page.",
    )];
    match id {
        CoachTourId::Operator => OPERATOR,
        CoachTourId::Scripture => SCRIPTURE,
        CoachTourId::Lyrics => LYRICS,
        CoachTourId::Documents => DOCUMENTS,
    }
}

const PAD: f32 = 6.0;
const CARD_W: f32 = 280.0;
const GAP: f32 = 14.0;
const GIVE_UP: Duration = Duration::from_millis(1500);

/// `placeCard`: right of the target where it fits, then below, left, above;
/// kept inside the window.
pub fn place_card(rect: Bounds<Pixels>, height: f32, viewport: (f32, f32)) -> (f32, f32) {
    let (vw, vh) = viewport;
    let (left, top) = (f32::from(rect.origin.x), f32::from(rect.origin.y));
    let (right, bottom) = (
        left + f32::from(rect.size.width),
        top + f32::from(rect.size.height),
    );
    let clamp_x = |x: f32| x.max(12.0).min(vw - CARD_W - 12.0);
    let clamp_y = |y: f32| y.max(12.0).min(vh - height - 12.0);
    if right + GAP + CARD_W < vw {
        return (right + GAP + PAD, clamp_y(top));
    }
    if bottom + GAP + height < vh {
        return (clamp_x(left), bottom + GAP + PAD);
    }
    if left - GAP - CARD_W > 0.0 {
        return (left - GAP - CARD_W - PAD, clamp_y(top));
    }
    (clamp_x(left), clamp_y(top - height - GAP - PAD))
}

/// The tour ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoachDone;

/// Element ids, for tests.
pub mod ids {
    /// The card.
    pub const CARD: &str = "coach-card";
    /// Next or Got it.
    pub const NEXT: &str = "coach-next";
    /// Skip.
    pub const SKIP: &str = "coach-skip";
}

/// The running tour.
pub struct CoachTour {
    steps: &'static [CoachStep],
    index: usize,
    step_at: Instant,
    card_height: f32,
    /// Wakes the tour to skip a target that has not appeared.
    give_up: Option<Task<()>>,
}

impl EventEmitter<CoachDone> for CoachTour {}

impl CoachTour {
    /// A tour of `steps`.
    pub fn new(steps: &'static [CoachStep]) -> Self {
        Self::at(steps, Instant::now())
    }

    /// Creates a tour using the caller’s clock.
    pub fn at(steps: &'static [CoachStep], now: Instant) -> Self {
        Self {
            steps,
            index: 0,
            step_at: now,
            card_height: 140.0,
            give_up: None,
        }
    }

    /// The tip showing.
    pub fn index(&self) -> usize {
        self.index
    }

    /// Moves on, or ends after the last tip.
    pub fn next(&mut self, cx: &mut Context<Self>) {
        if self.index + 1 >= self.steps.len() {
            cx.emit(CoachDone);
        } else {
            self.index += 1;
            self.step_at = cx.background_executor().now();
            self.give_up = None;
            cx.notify();
        }
    }
}

impl Render for CoachTour {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // No per-frame redraw: anchors ask for a frame when they move.
        let Some(step) = self.steps.get(self.index).copied() else {
            return div().into_any_element();
        };
        let found = cx.try_global::<Anchors>().and_then(|a| a.get(step.target));
        let Some(rect) = found else {
            if cx
                .background_executor()
                .now()
                .saturating_duration_since(self.step_at)
                > GIVE_UP
            {
                let entity = cx.entity();
                window.defer(cx, move |_, cx| entity.update(cx, |this, cx| this.next(cx)));
            } else if self.give_up.is_none() {
                let wait = GIVE_UP.saturating_sub(
                    cx.background_executor()
                        .now()
                        .saturating_duration_since(self.step_at),
                ) + Duration::from_millis(16);
                self.give_up = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(wait).await;
                    let _ = this.update(cx, |_, cx| cx.notify());
                }));
            }
            return div().into_any_element();
        };
        let k = *cx.kairo();
        let last = self.index + 1 == self.steps.len();
        let viewport = window.viewport_size();
        let (vw, vh) = (f32::from(viewport.width), f32::from(viewport.height));
        let spot = Bounds::new(
            point(rect.origin.x - px(PAD), rect.origin.y - px(PAD)),
            size(
                rect.size.width + px(PAD * 2.0),
                rect.size.height + px(PAD * 2.0),
            ),
        );
        let (card_left, card_top) = place_card(rect, self.card_height, (vw, vh));
        let dim = gpui_kit::black().opacity(0.62);
        let (sx, sy) = (f32::from(spot.origin.x), f32::from(spot.origin.y));
        let (sw, sh) = (f32::from(spot.size.width), f32::from(spot.size.height));
        let shade = |l: f32, t: f32, w: f32, h: f32| {
            div()
                .absolute()
                .left(px(l))
                .top(px(t))
                .w(px(w.max(0.0)))
                .h(px(h.max(0.0)))
                .bg(dim)
        };
        let entity = cx.entity();
        let watcher = canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                let click = entity.clone();
                window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture && spot.contains(&event.position) {
                        click.update(cx, |this, cx| this.next(cx));
                    }
                });
                let keys = entity.clone();
                window.on_key_event(move |event: &KeyDownEvent, phase, _, cx| {
                    if phase != DispatchPhase::Capture {
                        return;
                    }
                    match event.keystroke.key.as_str() {
                        "escape" => {
                            cx.stop_propagation();
                            keys.update(cx, |_, cx| cx.emit(CoachDone));
                        }
                        "right" | "enter" => {
                            cx.stop_propagation();
                            keys.update(cx, |this, cx| this.next(cx));
                        }
                        _ => {}
                    }
                });
            },
        )
        .absolute()
        .size_0();
        let card = v_flex()
            .id(ids::CARD)
            .test_support()
            .role(Role::Dialog)
            .aria_label(SharedString::new_static(step.title))
            .absolute()
            .left(px(card_left))
            .top(px(card_top))
            .w(px(CARD_W))
            .p_4()
            .rounded(px(10.))
            .bg(k.surface_elevated)
            .occlude()
            .when(self.steps.len() > 1, |el| {
                el.child(
                    div()
                        .text_size(px(11.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(k.text(TextStep::T500))
                        .child(format!("{} of {}", self.index + 1, self.steps.len())),
                )
            })
            .child(
                div()
                    .mt_1()
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(k.text_primary)
                    .child(step.title),
            )
            .child(
                div()
                    .mt_1()
                    .text_size(text::SM.size)
                    .text_color(k.text(TextStep::T400))
                    .child(step.body),
            )
            .child(
                h_flex()
                    .mt_4()
                    .items_center()
                    .justify_between()
                    .child(if self.steps.len() > 1 && !last {
                        crate::style::text_button(
                            ids::SKIP,
                            "Skip",
                            k.text(TextStep::T500),
                            k.text_primary,
                            false,
                            cx.listener(|_, _, _, cx| cx.emit(CoachDone)),
                        )
                        .text_size(px(12.))
                        .into_any_element()
                    } else {
                        div().into_any_element()
                    })
                    .child(
                        gpui_kit::component::button::Button::new(ids::NEXT)
                            .cursor_pointer()
                            .primary()
                            .xsmall()
                            .label(if last { "Got it" } else { "Next" })
                            .on_click(cx.listener(|this, _, _, cx| this.next(cx))),
                    ),
            );
        div()
            .id("coach-root")
            .absolute()
            .inset_0()
            .child(shade(0.0, 0.0, vw, sy))
            .child(shade(0.0, sy + sh, vw, vh - sy - sh))
            .child(shade(0.0, sy, sx, sh))
            .child(shade(sx + sw, sy, vw - sx - sw, sh))
            .child(
                div()
                    .absolute()
                    .left(px(sx))
                    .top(px(sy))
                    .w(px(sw))
                    .h(px(sh))
                    .rounded(px(10.))
                    .border(px(1.5))
                    .border_color(k.text(TextStep::T500)),
            )
            // Each tip fades in (`spring-in`), as the card moves on.
            .child(kairo_ui_kit::motion::enter(
                card,
                SharedString::from(format!("coach-card-{}", self.index)),
                cx,
            ))
            .child(watcher)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(w), px(h)))
    }

    #[test]
    fn card_goes_right_then_below_then_left_then_above() {
        let vp = (1280.0, 800.0);
        assert_eq!(
            place_card(rect(10.0, 100.0, 60.0, 400.0), 140.0, vp),
            (10.0 + 60.0 + GAP + PAD, 100.0)
        );
        assert_eq!(
            place_card(rect(1100.0, 10.0, 170.0, 40.0), 140.0, vp),
            (1280.0 - CARD_W - 12.0, 10.0 + 40.0 + GAP + PAD)
        );
        assert_eq!(
            place_card(rect(1100.0, 700.0, 170.0, 90.0), 140.0, vp),
            (1100.0 - GAP - CARD_W - PAD, 800.0 - 140.0 - 12.0)
        );
    }

    #[test]
    fn tours_match_the_source() {
        assert_eq!(coach_tour(CoachTourId::Operator).len(), 7);
        assert_eq!(
            coach_tour(CoachTourId::Scripture)[0].title,
            "Find any verse"
        );
        assert_eq!(
            coach_tour(CoachTourId::Operator)[5].target,
            anchors::HEADER_LIVE
        );
    }
}
