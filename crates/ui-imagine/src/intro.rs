//! First-run introduction for Imagine and the OpenAI API key connection card.
//!
//! When an operator opens Imagine without an API key, we present a calm,
//! focused overview with real photographic examples before transitioning into
//! the OpenAI connection card.

use std::sync::Arc;
use std::time::Instant;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{Disableable, Icon, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnimationExt, AnyElement, Context, Entity, FontWeight, Hsla, Image, ImageFormat, IntoElement,
    ObjectFit, ParentElement, SpringAnimation, Styled, StyledImage, div, img, linear_color_stop,
    linear_gradient, px, relative, rems, rgba,
};
use kairo_ui_kit::motion::{OUT_EXPO, SPRING_SOFT};
use kairo_ui_kit::tokens::{Hue, TextStep, radius, text};
use kairo_ui_kit::{ControlSize, KairoIcon, KairoThemeExt};

use crate::view::{ImaginePage, KEYS_URL, ids};

const WORSHIP_JPG: &[u8] = include_bytes!("../assets/intro-worship.jpg");
const SERMON_JPG: &[u8] = include_bytes!("../assets/intro-sermon.jpg");
const YOUTH_JPG: &[u8] = include_bytes!("../assets/intro-youth.jpg");
const LOGO_SVG: &str = include_str!("../assets/imagine-logo.svg");
/// The wordmark's viewBox is 1440 × 220.
const LOGO_ASPECT: f32 = 1440. / 220.;
const LOGO_H: f32 = 2.;

/// The tour: type the prompt, "send" it, develop the result, hold, move on.
const TYPE_SECS_PER_CHAR: f32 = 0.032;
const TYPE_MAX_SECS: f32 = 1.9;
const SEND_SECS: f32 = 0.3;
const GENERATE_SECS: f32 = 1.8;
const REVEAL_SECS: f32 = 0.8;
const HOLD_SECS: f32 = 2.8;
/// The demo's result frame, 16:9.
const FRAME_W: f32 = 15.;

const STEPS: [(&str, &str); 3] = [
    ("Describe", "Say what you need, in your own words."),
    ("Generate", "Imagine makes it with your OpenAI key."),
    ("Refine and save", "Ask for changes, then save it to Media."),
];

struct Progress {
    typed: usize,
    /// 0..1 while the image is being made.
    generated: f32,
    /// 0..1 as the finished image fades in.
    reveal: f32,
}

const OPENAI_PATH: &str = "M22.282 9.821a5.985 5.985 0 0 0-.516-4.91 6.046 6.046 0 0 0-6.51-2.9A6.065 6.065 0 0 0 4.981 4.18a5.985 5.985 0 0 0-3.998 2.9 6.046 6.046 0 0 0 .743 7.097 5.98 5.98 0 0 0 .51 4.911 6.051 6.051 0 0 0 6.515 2.9A5.985 5.985 0 0 0 13.26 24a6.056 6.056 0 0 0 5.772-4.206 5.99 5.99 0 0 0 3.997-2.9 6.056 6.056 0 0 0-.747-7.073zM13.26 22.43a4.476 4.476 0 0 1-2.876-1.04l.141-.081 4.779-2.758a.795.795 0 0 0 .392-.681v-6.737l2.02 1.168a.071.071 0 0 1 .038.052v5.583a4.504 4.504 0 0 1-4.494 4.494zM3.6 18.304a4.47 4.47 0 0 1-.535-3.014l.142.085 4.783 2.759a.771.771 0 0 0 .78 0l5.843-3.369v2.332a.08.08 0 0 1-.033.062L9.74 19.95a4.5 4.5 0 0 1-6.14-1.646zM2.34 7.896a4.485 4.485 0 0 1 2.366-1.973V11.6a.766.766 0 0 0 .388.676l5.815 3.355-2.02 1.168a.076.076 0 0 1-.071 0l-4.83-2.786A4.504 4.504 0 0 1 2.34 7.872zm16.597 3.855-5.833-3.387L15.119 7.2a.076.076 0 0 1 .071 0l4.83 2.791a4.494 4.494 0 0 1-.676 8.105v-5.678a.79.79 0 0 0-.407-.667zm2.01-3.023-.141-.085-4.774-2.782a.776.776 0 0 0-.785 0L9.409 9.23V6.897a.066.066 0 0 1 .028-.061l4.83-2.787a4.5 4.5 0 0 1 6.68 4.66zm-12.64 4.135-2.02-1.164a.08.08 0 0 1-.038-.057V6.075a4.5 4.5 0 0 1 7.375-3.453l-.142.08L8.704 5.46a.795.795 0 0 0-.393.681zm1.097-2.365 2.602-1.5 2.607 1.5v2.999l-2.597 1.5-2.607-1.5Z";

/// One-shot entrance durations; once settled, idle CPU drops to zero.
const WELCOME_ENTER_SECS: f32 = 0.55;
const CONNECT_ENTER_SECS: f32 = 0.45;

struct ShowcaseSample {
    /// What it is, under the title in the story column.
    kind: &'static str,
    prompt: &'static str,
}

const SAMPLES: [ShowcaseSample; 3] = [
    ShowcaseSample {
        kind: "Worship background",
        prompt: "Golden-hour mountain clouds for a warm worship background",
    },
    ShowcaseSample {
        kind: "Sermon title slide",
        prompt: "Easter Sunday title slide, warm light beam, editorial serif",
    },
    ShowcaseSample {
        kind: "Announcement deck · 4 slides",
        prompt: "4-slide Youth Night announcement deck with concert lighting",
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntroStep {
    Welcome,
    Connect,
}

pub struct IntroState {
    step: IntroStep,
    /// The example on screen, and the one it is fading in over.
    selected: usize,
    previous: Option<usize>,
    /// When the selected example's prompt started typing.
    shown_at: Instant,
    /// Plays each example once on arrival; any pick stops it.
    touring: bool,
    entered: Instant,
    step_changed: Instant,
    photos: [Arc<Image>; 3],
    openai_dark: Arc<Image>,
    openai_light: Arc<Image>,
    logo_dark: Arc<Image>,
    logo_light: Arc<Image>,
}

fn openai_svg(fill: &str) -> Arc<Image> {
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="24" height="24"><path fill="{fill}" d="{OPENAI_PATH}"/></svg>"##
    );
    svg_image(svg)
}

fn svg_image(svg: String) -> Arc<Image> {
    Arc::new(Image::from_bytes(ImageFormat::Svg, svg.into_bytes()))
}

/// Seconds to type one example's prompt.
fn type_secs(sample: &ShowcaseSample) -> f32 {
    (sample.prompt.chars().count() as f32 * TYPE_SECS_PER_CHAR).min(TYPE_MAX_SECS)
}

/// Seconds one example stays on screen during the tour.
fn tour_secs(sample: &ShowcaseSample) -> f32 {
    type_secs(sample) + SEND_SECS + GENERATE_SECS + REVEAL_SECS + HOLD_SECS
}

impl Default for IntroState {
    fn default() -> Self {
        Self::new()
    }
}

impl IntroState {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            step: IntroStep::Welcome,
            selected: 0,
            previous: None,
            shown_at: now,
            touring: true,
            entered: now,
            step_changed: now,
            photos: [
                Arc::new(Image::from_bytes(ImageFormat::Jpeg, WORSHIP_JPG.to_vec())),
                Arc::new(Image::from_bytes(ImageFormat::Jpeg, SERMON_JPG.to_vec())),
                Arc::new(Image::from_bytes(ImageFormat::Jpeg, YOUTH_JPG.to_vec())),
            ],
            openai_dark: openai_svg("#ffffff"),
            openai_light: openai_svg("#14161c"),
            logo_dark: svg_image(LOGO_SVG.to_owned()),
            // The ink gradient is white for dark surfaces; light ones get the same near-black as the OpenAI mark.
            logo_light: svg_image(
                LOGO_SVG
                    .replace(r##"stop-color="#fff""##, r##"stop-color="#14161c""##)
                    .replace(r##"stop-color="#f7f3ed""##, r##"stop-color="#2a2d36""##),
            ),
        }
    }

    pub fn step(&self) -> IntroStep {
        self.step
    }

    /// The operator picked an example: show it now and stop the tour.
    pub fn select(&mut self, index: usize) {
        if index < SAMPLES.len() {
            self.show(index);
            self.touring = false;
        }
    }

    fn show(&mut self, index: usize) {
        if index != self.selected {
            self.previous = Some(self.selected);
        }
        self.selected = index;
        self.shown_at = Instant::now();
    }

    /// Moves the tour on once an example has had its time. Call each frame
    /// unless reduced motion is on.
    pub fn tick(&mut self) {
        if self.step != IntroStep::Welcome || !self.touring {
            return;
        }
        if self.shown_at.elapsed().as_secs_f32() < tour_secs(&SAMPLES[self.selected]) {
            return;
        }
        if self.selected + 1 < SAMPLES.len() {
            self.show(self.selected + 1);
        } else {
            // Rest on the last example; idle CPU drops to zero.
            self.touring = false;
        }
    }

    /// Plays the tour again from the first example.
    pub fn replay(&mut self) {
        self.show(0);
        self.touring = true;
    }

    pub fn show_welcome(&mut self) {
        let now = Instant::now();
        self.step = IntroStep::Welcome;
        self.entered = now;
        self.step_changed = now;
    }

    pub fn show_connect(&mut self) {
        self.step = IntroStep::Connect;
        self.step_changed = Instant::now();
    }

    /// True while something on screen moves, so idle CPU stays at zero.
    pub fn animating(&self) -> bool {
        match self.step {
            IntroStep::Welcome => {
                let sample = &SAMPLES[self.selected];
                self.touring
                    || self.entered.elapsed().as_secs_f32() < WELCOME_ENTER_SECS
                    || self.shown_at.elapsed().as_secs_f32()
                        < type_secs(sample) + SEND_SECS + GENERATE_SECS + REVEAL_SECS
            }
            IntroStep::Connect => self.step_changed.elapsed().as_secs_f32() < CONNECT_ENTER_SECS,
        }
    }

    pub fn render(
        &self,
        key: &Entity<InputState>,
        connecting: bool,
        notice: Option<&str>,
        cx: &mut Context<ImaginePage>,
    ) -> AnyElement {
        let reduced = cx.reduced_motion();
        let (backdrop, content) = match self.step {
            IntroStep::Welcome => (
                self.render_backdrop(reduced, cx),
                self.render_welcome(reduced, cx),
            ),
            IntroStep::Connect => (
                render_top_sheen(cx),
                self.render_connect_card(key, connecting, notice, reduced, cx),
            ),
        };

        div()
            .relative()
            .size_full()
            .overflow_hidden()
            .child(backdrop)
            .child(
                v_flex()
                    .relative()
                    .size_full()
                    .items_center()
                    .justify_center()
                    .px_8()
                    .py_6()
                    .child(content),
            )
            .into_any_element()
    }

    /// How far the selected example has got through the tour.
    fn progress(&self, reduced: bool) -> Progress {
        let sample = &SAMPLES[self.selected];
        let chars = sample.prompt.chars().count();
        if reduced {
            return Progress {
                typed: chars,
                generated: 1.0,
                reveal: 1.0,
            };
        }
        let t = self.shown_at.elapsed().as_secs_f32();
        let typing = type_secs(sample);
        let typed = ((t / typing) * chars as f32).floor() as usize;
        let started = typing + SEND_SECS;
        Progress {
            typed: typed.min(chars),
            generated: eased(t, started, GENERATE_SECS),
            reveal: eased(t, started + GENERATE_SECS, REVEAL_SECS),
        }
    }

    /// The selected result fills the page behind everything, developing out of
    /// the previous one once its prompt has been "sent".
    fn render_backdrop(&self, reduced: bool, cx: &mut Context<ImaginePage>) -> AnyElement {
        let k = *cx.kairo();
        let reveal = self.progress(reduced).reveal;
        let photo = |index: usize| {
            img(self.photos[index].clone())
                .absolute()
                .inset_0()
                .size_full()
                .object_fit(ObjectFit::Cover)
        };
        let veil = if k.appearance.is_dark() { 0.5 } else { 0.62 };

        div()
            .absolute()
            .inset_0()
            .bg(k.surface)
            .children(self.previous.map(photo))
            .child(photo(self.selected).opacity(reveal))
            // The workspace colour over the photograph: dark themes get a dim
            // cinema, light ones a soft wash; text stays on theme tokens.
            .child(div().absolute().inset_0().bg(with_alpha(k.surface, veil)))
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(rems(8.))
                    .bg(linear_gradient(
                        180.0,
                        linear_color_stop(k.surface, 0.0),
                        linear_color_stop(with_alpha(k.surface, 0.0), 1.0),
                    )),
            )
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .h_1_2()
                    .bg(linear_gradient(
                        180.0,
                        linear_color_stop(with_alpha(k.surface, 0.0), 0.0),
                        linear_color_stop(k.surface, 1.0),
                    )),
            )
            .into_any_element()
    }

    fn render_welcome(&self, reduced: bool, cx: &mut Context<ImaginePage>) -> AnyElement {
        let k = *cx.kairo();
        let elapsed = if reduced {
            10.0
        } else {
            self.entered.elapsed().as_secs_f32()
        };
        let header_t = if reduced {
            1.0
        } else {
            eased(elapsed, 0.0, 0.4)
        };
        let demo_t = if reduced {
            1.0
        } else {
            eased(elapsed, 0.12, 0.45)
        };
        let cta_t = if reduced {
            1.0
        } else {
            eased(elapsed, 0.24, 0.45)
        };
        let logo = if k.appearance.is_dark() {
            self.logo_dark.clone()
        } else {
            self.logo_light.clone()
        };

        v_flex()
            .w_full()
            .max_w(rems(40.))
            .items_center()
            .child(
                v_flex()
                    .items_center()
                    .opacity(header_t)
                    .relative()
                    .top(px((1.0 - header_t) * 10.0))
                    .child(
                        img(logo)
                            .h(rems(LOGO_H))
                            .w(rems(LOGO_H * LOGO_ASPECT))
                            .object_fit(ObjectFit::Contain),
                    )
                    .child(
                        div()
                            .mt_4()
                            .text_center()
                            .text_size(text::XL3.size)
                            .line_height(text::XL3.line_height)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(k.text_primary)
                            .child("Describe it. See it on screen."),
                    )
                    .child(
                        div()
                            .mt_1p5()
                            .text_center()
                            .text_size(text::SM.size)
                            .line_height(text::SM.line_height)
                            .text_color(k.text(TextStep::T500))
                            .child("Backgrounds, sermon graphics and slide decks from a sentence."),
                    ),
            )
            .child(
                // The demo: the prompt bar types, then the result is made below it.
                v_flex()
                    .w_full()
                    .mt_6()
                    .gap_3()
                    .opacity(demo_t)
                    .relative()
                    .top(px((1.0 - demo_t) * 14.0))
                    .child(self.composer(reduced, cx))
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .gap_5()
                            .p_3()
                            .rounded(radius::xl2())
                            .bg(k.glass_modal())
                            .border_1()
                            .border_color(k.ink(0.1))
                            .child(self.result_frame(reduced, cx))
                            .child(self.steps(reduced, cx)),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .mt_6()
                    .opacity(cta_t)
                    .child(self.connect_bar(cx)),
            )
            .into_any_element()
    }

    /// A look-alike of the real prompt bar, typing the selected example's
    /// prompt; its send button lights while the result is made.
    fn composer(&self, reduced: bool, cx: &mut Context<ImaginePage>) -> AnyElement {
        let k = *cx.kairo();
        let sample = &SAMPLES[self.selected];
        let p = self.progress(reduced);
        let done = p.typed == sample.prompt.chars().count();
        let working = done && p.reveal < 1.0 && !reduced;
        let text: String = sample.prompt.chars().take(p.typed).collect();

        h_flex()
            .w_full()
            .items_center()
            .gap_3()
            .pl_5()
            .pr_2()
            .py_2()
            .rounded_full()
            .bg(k.glass_modal())
            .border_1()
            .border_color(k.ink(0.12))
            .child(
                Icon::new(KairoIcon::Plus)
                    .size_4()
                    .text_color(k.text(TextStep::T400)),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(text::BASE.size)
                            .text_color(k.text_primary)
                            .child(text),
                    )
                    // The caret only while typing; a resting page shows none.
                    .when(!done, |s| {
                        s.child(
                            div()
                                .flex_none()
                                .ml_0p5()
                                .w(px(2.))
                                .h(rems(1.125))
                                .bg(k.control_accent),
                        )
                    }),
            )
            .child(
                div()
                    .flex_none()
                    .size_9()
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(if done { k.control_accent } else { k.ink(0.08) })
                    // Breathes while the image is being made.
                    .opacity(if working {
                        0.7 + 0.3 * (p.generated * std::f32::consts::TAU * 2.0).cos()
                    } else {
                        1.0
                    })
                    .child(Icon::new(KairoIcon::ArrowUp).size_4().text_color(if done {
                        k.on_accent
                    } else {
                        k.text(TextStep::T400)
                    })),
            )
            .into_any_element()
    }

    /// The result as the conversation shows it: an empty frame, then
    /// "Creating image" with progress, then the image and its version.
    fn result_frame(&self, reduced: bool, cx: &mut Context<ImaginePage>) -> AnyElement {
        let k = *cx.kairo();
        let sample = &SAMPLES[self.selected];
        let p = self.progress(reduced);
        let generating = p.generated > 0.0 && p.reveal < 1.0;
        let page = cx.entity();

        let status = if p.generated <= 0.0 {
            v_flex()
                .items_center()
                .gap_1p5()
                .child(
                    Icon::new(KairoIcon::Image)
                        .size_5()
                        .text_color(k.text(TextStep::T400)),
                )
                .child(
                    div()
                        .text_size(text::LABEL.size)
                        .text_color(k.text(TextStep::T400))
                        .child("Your image appears here"),
                )
                .into_any_element()
        } else {
            v_flex()
                .items_center()
                .gap_1p5()
                .child(
                    Icon::new(KairoIcon::Loader)
                        .size_5()
                        .text_color(k.text(TextStep::T500)),
                )
                .child(
                    div()
                        .text_size(text::LABEL.size)
                        .text_color(k.text(TextStep::T500))
                        .child("Creating image…"),
                )
                .into_any_element()
        };

        div()
            .flex_none()
            .w(rems(FRAME_W))
            .h(rems(FRAME_W * 9. / 16.))
            .relative()
            .rounded(radius::lg())
            .overflow_hidden()
            .bg(k.ink(0.05))
            .border_1()
            .border_color(k.ink(0.1))
            .child(
                v_flex()
                    .absolute()
                    .inset_0()
                    .items_center()
                    .justify_center()
                    .child(status),
            )
            .when(generating, |s| {
                s.child(
                    div()
                        .absolute()
                        .left_0()
                        .bottom_0()
                        .h(px(3.))
                        .w(relative(p.generated))
                        .bg(k.control_accent),
                )
            })
            .when(p.reveal > 0.0, |s| {
                s.child(
                    img(self.photos[self.selected].clone())
                        .absolute()
                        .inset_0()
                        .size_full()
                        .object_fit(ObjectFit::Cover)
                        .opacity(p.reveal),
                )
            })
            .when(p.reveal >= 1.0, |s| {
                // Caption over the photograph: fixed light-on-dark, like a slide.
                s.child(
                    h_flex()
                        .absolute()
                        .left_2()
                        .bottom_2()
                        .items_center()
                        .gap_1()
                        .px_2()
                        .py_0p5()
                        .rounded(radius::md())
                        .bg(rgba(0x0b0d12c0))
                        .text_size(text::MICRO.size)
                        .text_color(rgba(0xffffffe6))
                        .child(Icon::new(KairoIcon::Check).size_3())
                        .child(format!("Version 1 · {}", sample.kind)),
                )
            })
            .when(!self.touring && p.reveal >= 1.0 && !reduced, |s| {
                s.child(
                    div().absolute().top_2().right_2().child(
                        Button::new(ids::INTRO_REPLAY)
                            .xsmall()
                            .ghost()
                            .icon(Icon::new(KairoIcon::RotateCcw))
                            .tooltip("Play the demo again")
                            .on_click(move |_, _, cx| page.update(cx, |p, cx| p.replay_intro(cx))),
                    ),
                )
            })
            .into_any_element()
    }

    /// Describe, generate, refine: the step the demo is on is lit.
    fn steps(&self, reduced: bool, cx: &mut Context<ImaginePage>) -> AnyElement {
        let k = *cx.kairo();
        let p = self.progress(reduced);
        let now = if p.generated <= 0.0 {
            0
        } else if p.reveal < 1.0 {
            1
        } else {
            2
        };

        v_flex()
            .flex_1()
            .min_w_0()
            .gap_3()
            .children(STEPS.iter().enumerate().map(|(i, (title, body))| {
                let (dot_bg, dot_fg, title_fg) = match i.cmp(&now) {
                    std::cmp::Ordering::Less => {
                        (k.ink(0.1), k.text_primary, k.text(TextStep::T500))
                    }
                    // At rest; the spring below lights it.
                    std::cmp::Ordering::Equal => (k.ink(0.1), k.on_accent, k.text_primary),
                    std::cmp::Ordering::Greater => {
                        (k.ink(0.06), k.text(TextStep::T400), k.text(TextStep::T400))
                    }
                };
                h_flex()
                    .items_start()
                    .gap_3()
                    .child(
                        div()
                            .flex_none()
                            .size_5()
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(dot_fg)
                            .text_size(text::MICRO.size)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(if i < now {
                                Icon::new(KairoIcon::Check).size_3().into_any_element()
                            } else {
                                (i + 1).to_string().into_any_element()
                            })
                            // The lit step glides on and off rather than switching.
                            .with_spring(
                                ("imagine-intro-step", i),
                                SpringAnimation::new(SPRING_SOFT).to(i == now),
                                move |dot, phase| {
                                    dot.bg(phase.interpolate_clamped(dot_bg, k.control_accent))
                                },
                            ),
                    )
                    .child(
                        v_flex()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(text::SM.size)
                                    .line_height(text::SM.line_height)
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(title_fg)
                                    .child(*title),
                            )
                            .child(
                                div()
                                    .text_size(text::LABEL.size)
                                    .text_color(k.text(TextStep::T400))
                                    .child(*body),
                            ),
                    )
            }))
            .into_any_element()
    }

    /// The one action: what connecting means, and the button.
    fn connect_bar(&self, cx: &mut Context<ImaginePage>) -> AnyElement {
        let k = *cx.kairo();
        let page = cx.entity();
        let logo = if k.appearance.is_dark() {
            self.openai_dark.clone()
        } else {
            self.openai_light.clone()
        };

        h_flex()
            .w_full()
            .items_center()
            .gap_4()
            .p_3()
            .pl_3()
            .rounded(radius::xl2())
            .bg(k.glass_modal())
            .border_1()
            .border_color(k.ink(0.1))
            .child(
                div()
                    .flex_none()
                    .size_10()
                    .rounded(radius::lg())
                    .bg(k.surface_tertiary)
                    .border_1()
                    .border_color(k.surface_border)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(img(logo).size_5()),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(text::SM.size)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(k.text_primary)
                            .child("Start with your OpenAI API key"),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap_1()
                            .child(
                                Icon::new(KairoIcon::Lock)
                                    .size_3()
                                    .text_color(k.text(TextStep::T400)),
                            )
                            .child(
                                div()
                                    .text_size(text::LABEL.size)
                                    .text_color(k.text(TextStep::T400))
                                    .child("Stored in your keychain. You pay OpenAI only for what you make."),
                            ),
                    ),
            )
            .child(
                Button::new(ids::INTRO_CONTINUE)
                    .primary()
                    .label("Connect OpenAI")
                    .on_click(move |_, _, cx| page.update(cx, |p, cx| p.open_connect(cx))),
            )
            .into_any_element()
    }

    fn render_connect_card(
        &self,
        key: &Entity<InputState>,
        connecting: bool,
        notice: Option<&str>,
        reduced: bool,
        cx: &mut Context<ImaginePage>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let (connect, back) = (cx.entity(), cx.entity());
        let t = if reduced {
            1.0
        } else {
            eased(self.step_changed.elapsed().as_secs_f32(), 0.0, 0.38)
        };
        let logo = if k.appearance.is_dark() {
            self.openai_dark.clone()
        } else {
            self.openai_light.clone()
        };

        v_flex()
            .w(px(408.))
            .items_center()
            .opacity(t)
            .relative()
            .top(px((1.0 - t) * 12.0))
            .child(
                v_flex()
                    .w_full()
                    .items_center()
                    .gap_3p5()
                    .p_7()
                    .rounded(radius::xl())
                    .bg(k.surface_secondary)
                    .border_1()
                    .border_color(k.surface_border)
                    .child(
                        div()
                            .size(px(44.))
                            .rounded(radius::lg())
                            .bg(k.surface_tertiary)
                            .border_1()
                            .border_color(k.surface_border)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(img(logo).size(px(22.))),
                    )
                    .child(
                        v_flex()
                            .items_center()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(text::LG.size)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(k.text_primary)
                                    .child("Connect OpenAI"),
                            )
                            .child(
                                div()
                                    .text_center()
                                    .text_size(text::XS.size)
                                    .text_color(k.text(TextStep::T500))
                                    .child("Use your own API key — pay-as-you-go via OpenAI."),
                            ),
                    )
                    .child(
                        div().w_full().mt_1().child(
                            Input::new(key)
                                .kairo_size()
                                .id(ids::KEY)
                                .aria_label("OpenAI API key")
                                .mask_toggle()
                                .disabled(connecting),
                        ),
                    )
                    .children(notice.map(|n| {
                        div()
                            .w_full()
                            .text_center()
                            .text_size(text::LABEL.size)
                            .text_color(k.warning_text)
                            .child(n.to_owned())
                    }))
                    .child(
                        Button::new(ids::CONNECT)
                            .w_full()
                            .primary()
                            .label(if connecting { "Checking…" } else { "Connect" })
                            .disabled(connecting)
                            .on_click(move |_, window, cx| {
                                connect.update(cx, |p, cx| p.connect(window, cx))
                            }),
                    )
                    .child(
                        Button::new(ids::GET_KEY)
                            .small()
                            .ghost()
                            .icon(Icon::new(KairoIcon::ExternalLink))
                            .label("Get an OpenAI API key")
                            .on_click(move |_, _, cx| cx.open_url(KEYS_URL)),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .pt_3p5()
                            .mt_0p5()
                            .border_t_1()
                            .border_color(k.surface_border)
                            .items_center()
                            .justify_center()
                            .gap_1p5()
                            .child(
                                Icon::new(KairoIcon::Lock)
                                    .size_3()
                                    .text_color(k.text(TextStep::T400)),
                            )
                            .child(
                                div()
                                    .text_size(text::LABEL.size)
                                    .text_color(k.text(TextStep::T400))
                                    .child("Stored in your keychain · Only sent to OpenAI"),
                            ),
                    ),
            )
            .child(
                div().mt_3().child(
                    Button::new(ids::INTRO_BACK)
                        .small()
                        .ghost()
                        .icon(Icon::new(KairoIcon::ChevronLeft))
                        .label("Back")
                        .on_click(move |_, _, cx| back.update(cx, |p, cx| p.open_welcome(cx))),
                ),
            )
            .into_any_element()
    }
}

fn eased(elapsed: f32, delay: f32, duration: f32) -> f32 {
    if duration <= 0.0 {
        return 1.0;
    }
    let t = ((elapsed - delay) / duration).clamp(0.0, 1.0);
    OUT_EXPO.sample(t)
}

/// Smooth GPU linear sheen at the top edge — no concentric ring banding.
fn render_top_sheen(cx: &mut Context<ImaginePage>) -> AnyElement {
    let k = *cx.kairo();
    let dark = k.appearance.is_dark();
    let tint = with_alpha(k.hue(Hue::Blue), if dark { 0.07 } else { 0.04 });
    let clear = with_alpha(tint, 0.0);

    div()
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .h(px(320.))
        .bg(linear_gradient(
            180.0,
            linear_color_stop(tint, 0.0),
            linear_color_stop(clear, 1.0),
        ))
        .into_any_element()
}

fn with_alpha(mut color: Hsla, a: f32) -> Hsla {
    color.a = a.clamp(0.0, 1.0);
    color
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intro_starts_on_welcome_and_switches_steps() {
        let mut intro = IntroState::new();
        assert_eq!(intro.step(), IntroStep::Welcome);
        assert!(intro.animating());

        assert!(intro.touring);
        intro.select(2);
        assert_eq!(intro.selected, 2);
        assert_eq!(intro.previous, Some(0));
        assert!(!intro.touring, "a pick stops the tour");

        intro.show_connect();
        assert_eq!(intro.step(), IntroStep::Connect);

        intro.show_welcome();
        assert_eq!(intro.step(), IntroStep::Welcome);
    }

    #[test]
    fn easing_clamps_before_delay_and_after_duration() {
        assert_eq!(eased(0.0, 0.2, 0.5), 0.0);
        assert_eq!(eased(1.0, 0.2, 0.5), 1.0);
        assert!(eased(0.35, 0.2, 0.5) > 0.5);
    }
}
