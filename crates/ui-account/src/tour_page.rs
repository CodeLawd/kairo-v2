//! Isolated tour motion and scripted stories. Parent updates only on target changes.
use crate::{
    fluid::Pager,
    ob::Clock,
    tour::{self, TOUR_SLIDES},
};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::{
    Context, EventEmitter, InteractiveElement, IntoElement, ParentElement, Render, Role,
    SharedString, StatefulInteractiveElement, Styled, Task, Window, div, px,
};
use kairo_ui_kit::{KairoThemeExt, onboarding::colors};
use std::time::Instant;

#[derive(Clone, Copy, Debug)]
pub struct SlideChanged(pub usize);
pub struct TourPage {
    pager: Pager,
    slide: usize,
    stories: [Option<Instant>; 6],
    scroll_x: Option<f32>,
    timer: Option<Task<()>>,
    deadline: Option<Instant>,
}
impl EventEmitter<SlideChanged> for TourPage {}
impl TourPage {
    pub fn new(slide: usize, cx: &mut Context<Self>) -> Self {
        Self {
            pager: Pager::at(TOUR_SLIDES.len(), slide, cx.background_executor().now()),
            slide,
            stories: [None; 6],
            scroll_x: None,
            timer: None,
            deadline: None,
        }
    }
    pub fn go(&mut self, slide: usize, cx: &mut Context<Self>) {
        let slide = slide.min(TOUR_SLIDES.len() - 1);
        self.pager.go(slide, cx.background_executor().now());
        if self.slide != slide {
            self.slide = slide;
            cx.emit(SlideChanged(slide));
        }
        cx.notify();
    }
    fn schedule(&mut self, deadline: Option<Instant>, cx: &mut Context<Self>) {
        if self.deadline == deadline {
            return;
        }
        self.timer = None;
        self.deadline = deadline;
        if let Some(deadline) = deadline {
            let wait = deadline.saturating_duration_since(cx.background_executor().now());
            self.timer = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(wait).await;
                let _ = this.update(cx, |this, cx| {
                    this.deadline = None;
                    this.timer = None;
                    cx.notify();
                });
            }));
        }
    }
    fn release_pager(&mut self, width: f32, cx: &mut Context<Self>) {
        if let Some(index) = self.pager.release(width, cx.background_executor().now()) {
            if self.slide != index {
                self.slide = index;
                cx.emit(SlideChanged(index));
            }
            cx.notify();
        }
    }
}

impl Render for TourPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ob = colors(cx.kairo().appearance);
        let ob = &ob;
        let width = f32::from(window.viewport_size().width);
        let still = cx.reduced_motion();
        let now = cx.background_executor().now();
        self.pager.tick(now, still);
        let pos = self.pager.position();
        let mut pages = div()
            .id("onboarding-pager")
            .test_support()
            .relative()
            .size_full()
            .overflow_hidden();
        for (i, slide) in TOUR_SLIDES.iter().enumerate() {
            let offset = i as f32 - pos;
            if if still {
                i != self.slide
            } else {
                offset.abs() >= 1.
            } {
                continue;
            }
            let since = *self.stories[i].get_or_insert(now);
            let clock = clock_since(since, 1., still, now);
            pages = pages.child(
                div()
                    .id(SharedString::from(format!("onboarding-page-{i}")))
                    .test_support()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .w(px(width))
                    .left(px(if still { 0. } else { offset * width }))
                    .opacity(if still {
                        if i == self.slide { 1. } else { 0. }
                    } else {
                        1.
                    })
                    .child(tour::render_slide(
                        slide,
                        clock,
                        width,
                        if still { 0. } else { offset },
                        ob,
                        cx,
                    )),
            );
        }
        let pages = pages
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, e: &gpui_kit::MouseDownEvent, _, cx| {
                    this.pager
                        .press(f32::from(e.position.x), cx.background_executor().now());
                    cx.notify();
                }),
            )
            .on_scroll(cx.listener(move |this, e: &gpui_kit::ScrollEvent, _, cx| {
                use gpui_kit::TouchPhase;
                let gpui_kit::ScrollDelta::Pixels(delta) = e.delta else {
                    return;
                };
                match e.touch_phase {
                    TouchPhase::Started => {
                        if f32::from(delta.x).abs() < f32::from(delta.y).abs() {
                            return;
                        }
                        this.scroll_x = Some(0.);
                        this.pager.press(0., cx.background_executor().now());
                    }
                    TouchPhase::Ended => {
                        this.scroll_x = None;
                        this.release_pager(width, cx);
                        return;
                    }
                    TouchPhase::Cancelled => {
                        this.scroll_x = None;
                        this.pager.go(this.slide, cx.background_executor().now());
                        cx.notify();
                        return;
                    }
                    _ => {}
                }
                if let Some(x) = &mut this.scroll_x {
                    *x += f32::from(delta.x);
                    this.pager.drag(*x, width, cx.background_executor().now());
                    cx.stop_propagation();
                    cx.notify();
                }
            }));
        // Window listeners keep tracking a grabbed page outside the pager/window bounds.
        let weak = cx.entity().downgrade();
        let tracking = gpui_kit::canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                let movement = weak.clone();
                window.on_mouse_event(move |e: &gpui_kit::MouseMoveEvent, phase, _, cx| {
                    if phase == gpui_kit::DispatchPhase::Bubble {
                        let _ = movement.update(cx, |this, cx| {
                            if this.pager.drag(
                                f32::from(e.position.x),
                                width,
                                cx.background_executor().now(),
                            ) {
                                cx.notify();
                            }
                        });
                    }
                });
                let release = weak.clone();
                window.on_mouse_event(move |e: &gpui_kit::MouseUpEvent, phase, _, cx| {
                    if phase == gpui_kit::DispatchPhase::Bubble
                        && e.button == gpui_kit::MouseButton::Left
                    {
                        let _ = release.update(cx, |this, cx| {
                            if this.pager.dragging() {
                                this.pager.drag(
                                    f32::from(e.position.x),
                                    width,
                                    cx.background_executor().now(),
                                );
                            }
                            this.release_pager(width, cx);
                        });
                    }
                });
            },
        )
        .absolute()
        .size_full();
        let dots = h_flex()
            .id("onboarding-dots")
            .role(Role::Group)
            .aria_label("Tour slides")
            .justify_center()
            .items_center()
            .gap_2()
            .py_4()
            .children(TOUR_SLIDES.iter().enumerate().map(|(i, slide)| {
                use gpui_kit::component::button::{Button, ButtonVariants};
                let emphasis = (1. - (i as f32 - pos).abs()).clamp(0., 1.);
                Button::new(SharedString::from(format!("onboarding-dot-{i}")))
                    .ghost()
                    .accessibility_label(SharedString::from(format!(
                        "{}, slide {} of {}",
                        slide.headline,
                        i + 1,
                        TOUR_SLIDES.len()
                    )))
                    .w(gpui_kit::rems(1.25 + emphasis))
                    .h(gpui_kit::rems(1.5))
                    .p_0()
                    .child(
                        div()
                            .w(gpui_kit::rems(0.5 + emphasis))
                            .h(gpui_kit::rems(0.5))
                            .rounded_full()
                            .bg(ob.text.opacity(0.2 + emphasis * 0.8)),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.go(i, cx)))
            }));
        let moving = self.pager.tick(now, still);
        let mut animating = moving;
        let mut wake = None;
        if !still {
            for (i, since) in self.stories.iter().enumerate() {
                if (i as f32 - pos).abs() >= 1. {
                    continue;
                }
                let Some(since) = since else {
                    continue;
                };
                let elapsed = now.saturating_duration_since(*since).as_millis() as u64;
                // Last scripted element's delay + duration, per art story.
                let entry_end = match TOUR_SLIDES[i].id {
                    "scripture" => 2350,
                    "style" => 1950,
                    "songs" => 1800,
                    "screens" => 1700,
                    _ => 1750,
                };
                if elapsed < entry_end {
                    animating = true;
                }
                let interval = match TOUR_SLIDES[i].id {
                    "scripture" => Some(500),
                    "screens" => Some(1000),
                    _ => None,
                };
                if let Some(interval) = interval {
                    let deadline = *since
                        + std::time::Duration::from_millis((elapsed / interval + 1) * interval);
                    wake = Some(wake.map_or(deadline, |old: Instant| old.min(deadline)));
                }
            }
        }
        if animating && !still {
            window.request_animation_frame();
            self.schedule(None, cx);
        } else {
            self.schedule(wake, cx);
        }
        v_flex()
            .size_full()
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(pages)
                    .child(tracking),
            )
            .child(dots)
            .into_any_element()
    }
}

/// Time since `since` as a [`Clock`].
fn clock_since(since: Instant, dir: f32, still: bool, now: Instant) -> Clock {
    if still {
        return Clock::still();
    }
    Clock {
        t: now.saturating_duration_since(since).as_secs_f32() * 1000.0,
        dir,
        still,
    }
}
