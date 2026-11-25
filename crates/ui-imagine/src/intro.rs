//! First-run introduction for Imagine and the OpenAI API key connection card.
//!
//! When an operator opens Imagine without an API key, we bridge the gap with a
//! short, calm entrance ("Meet Imagine") showing real photographic examples of
//! what Imagine creates before transitioning into the OpenAI connection card.

use std::sync::Arc;
use std::time::Instant;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{Disableable, Icon, Sizable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, Bounds, Context, Entity, FontWeight, Hsla, Image, ImageFormat, IntoElement,
    ObjectFit, ParentElement, PathBuilder, Pixels, Styled, StyledImage, Window, canvas, div, img,
    linear_color_stop, linear_gradient, point, px, rgb, rgba,
};
use kairo_ui_kit::motion::OUT_EXPO;
use kairo_ui_kit::tokens::{Hue, TextStep, radius, text};
use kairo_ui_kit::{ControlSize, KairoIcon, KairoThemeExt};

use crate::view::{ImaginePage, KEYS_URL, ids};

const WORSHIP_JPG: &[u8] = include_bytes!("../assets/intro-worship.jpg");
const SERMON_JPG: &[u8] = include_bytes!("../assets/intro-sermon.jpg");
const YOUTH_JPG: &[u8] = include_bytes!("../assets/intro-youth.jpg");

const OPENAI_PATH: &str = "M22.282 9.821a5.985 5.985 0 0 0-.516-4.91 6.046 6.046 0 0 0-6.51-2.9A6.065 6.065 0 0 0 4.981 4.18a5.985 5.985 0 0 0-3.998 2.9 6.046 6.046 0 0 0 .743 7.097 5.98 5.98 0 0 0 .51 4.911 6.051 6.051 0 0 0 6.515 2.9A5.985 5.985 0 0 0 13.26 24a6.056 6.056 0 0 0 5.772-4.206 5.99 5.99 0 0 0 3.997-2.9 6.056 6.056 0 0 0-.747-7.073zM13.26 22.43a4.476 4.476 0 0 1-2.876-1.04l.141-.081 4.779-2.758a.795.795 0 0 0 .392-.681v-6.737l2.02 1.168a.071.071 0 0 1 .038.052v5.583a4.504 4.504 0 0 1-4.494 4.494zM3.6 18.304a4.47 4.47 0 0 1-.535-3.014l.142.085 4.783 2.759a.771.771 0 0 0 .78 0l5.843-3.369v2.332a.08.08 0 0 1-.033.062L9.74 19.95a4.5 4.5 0 0 1-6.14-1.646zM2.34 7.896a4.485 4.485 0 0 1 2.366-1.973V11.6a.766.766 0 0 0 .388.676l5.815 3.355-2.02 1.168a.076.076 0 0 1-.071 0l-4.83-2.786A4.504 4.504 0 0 1 2.34 7.872zm16.597 3.855-5.833-3.387L15.119 7.2a.076.076 0 0 1 .071 0l4.83 2.791a4.494 4.494 0 0 1-.676 8.105v-5.678a.79.79 0 0 0-.407-.667zm2.01-3.023-.141-.085-4.774-2.782a.776.776 0 0 0-.785 0L9.409 9.23V6.897a.066.066 0 0 1 .028-.061l4.83-2.787a4.5 4.5 0 0 1 6.68 4.66zm-12.64 4.135-2.02-1.164a.08.08 0 0 1-.038-.057V6.075a4.5 4.5 0 0 1 7.375-3.453l-.142.08L8.704 5.46a.795.795 0 0 0-.393.681zm1.097-2.365 2.602-1.5 2.607 1.5v2.999l-2.597 1.5-2.607-1.5Z";

/// Duration of one prompt beat before cycling to the next showcase card.
const PROMPT_CYCLE_SECS: f32 = 3.2;
/// How long the Connect step's entrance animates before going idle.
const CONNECT_ENTER_SECS: f32 = 0.65;
/// Concentric rings used to paint the soft top canvas wash.
const WASH_RINGS: usize = 14;

struct PromptBeat {
    prompt: &'static str,
    card_index: usize,
}

const BEATS: [PromptBeat; 3] = [
    PromptBeat {
        prompt: "Easter Sunday title slide, editorial serif",
        card_index: 1,
    },
    PromptBeat {
        prompt: "Golden-hour worship background",
        card_index: 0,
    },
    PromptBeat {
        prompt: "4-slide Youth Night announcement deck",
        card_index: 2,
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntroStep {
    Welcome,
    Connect,
}

pub struct IntroState {
    step: IntroStep,
    entered: Instant,
    step_changed: Instant,
    photos: [Arc<Image>; 3],
    openai_dark: Arc<Image>,
    openai_light: Arc<Image>,
}

fn openai_svg(fill: &str) -> Arc<Image> {
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="24" height="24"><path fill="{fill}" d="{OPENAI_PATH}"/></svg>"##
    );
    Arc::new(Image::from_bytes(ImageFormat::Svg, svg.into_bytes()))
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
            entered: now,
            step_changed: now,
            photos: [
                Arc::new(Image::from_bytes(ImageFormat::Jpeg, WORSHIP_JPG.to_vec())),
                Arc::new(Image::from_bytes(ImageFormat::Jpeg, SERMON_JPG.to_vec())),
                Arc::new(Image::from_bytes(ImageFormat::Jpeg, YOUTH_JPG.to_vec())),
            ],
            openai_dark: openai_svg("#ffffff"),
            openai_light: openai_svg("#14161c"),
        }
    }

    pub fn step(&self) -> IntroStep {
        self.step
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

    /// True while the intro needs animation frames. Stops requesting frames
    /// once the Connect card has settled so idle CPU stays at zero.
    pub fn animating(&self) -> bool {
        match self.step {
            IntroStep::Welcome => true,
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
        let wash_alpha = match self.step {
            IntroStep::Welcome => {
                if reduced {
                    1.0
                } else {
                    eased(self.entered.elapsed().as_secs_f32(), 0.0, 0.7)
                }
            }
            IntroStep::Connect => {
                if reduced {
                    0.32
                } else {
                    let t = eased(self.step_changed.elapsed().as_secs_f32(), 0.0, 0.5);
                    1.0 - 0.68 * t
                }
            }
        };

        let content = match self.step {
            IntroStep::Welcome => self.render_welcome(reduced, cx),
            IntroStep::Connect => self.render_connect_card(key, connecting, notice, reduced, cx),
        };

        div()
            .relative()
            .size_full()
            .overflow_hidden()
            .child(render_wash(wash_alpha, cx))
            .child(
                v_flex()
                    .relative()
                    .size_full()
                    .items_center()
                    .justify_center()
                    .px_6()
                    .child(content),
            )
            .into_any_element()
    }

    fn render_welcome(&self, reduced: bool, cx: &mut Context<ImaginePage>) -> AnyElement {
        let k = *cx.kairo();
        let page = cx.entity();
        let elapsed = if reduced {
            10.0
        } else {
            self.entered.elapsed().as_secs_f32()
        };

        // Staggered Dot-style beats: hero cards first, copy next, floating pill last.
        let hero_t = if reduced {
            1.0
        } else {
            eased(elapsed, 0.05, 0.56)
        };
        let copy_t = if reduced {
            1.0
        } else {
            eased(elapsed, 0.22, 0.56)
        };
        let pill_in = if reduced {
            1.0
        } else {
            eased(elapsed, 0.52, 0.42)
        };
        let bob = if reduced {
            0.0
        } else {
            (elapsed * 1.35).sin() * 3.0
        };

        // Cycle smoothly through the three church prompt examples.
        let (beat_idx, pill_swap_alpha) = if reduced {
            (0, 1.0)
        } else {
            let cycle_t = (elapsed - 0.52).max(0.0);
            let idx = ((cycle_t / PROMPT_CYCLE_SECS).floor() as usize) % BEATS.len();
            let phase = (cycle_t % PROMPT_CYCLE_SECS) / PROMPT_CYCLE_SECS;
            // Soft crossfade at the edges of each beat.
            let fade = if phase < 0.07 {
                phase / 0.07
            } else if phase > 0.93 {
                (1.0 - phase) / 0.07
            } else {
                1.0
            };
            (idx, fade.clamp(0.0, 1.0))
        };
        let beat = &BEATS[beat_idx];
        let pill_alpha = (pill_in * pill_swap_alpha).clamp(0.0, 1.0);
        let dark = k.appearance.is_dark();

        let pill_bg = if dark {
            rgba(0xffffff17).into()
        } else {
            k.surface_secondary
        };
        let pill_border = if dark {
            rgba(0xffffff24).into()
        } else {
            k.surface_border
        };

        v_flex()
            .w(px(520.))
            .items_center()
            .child(
                // Floating morphing prompt pill above the showcase cards.
                v_flex()
                    .items_center()
                    .mb_5()
                    .opacity(pill_alpha)
                    .relative()
                    .top(px((1.0 - pill_in) * 8.0))
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .px_3p5()
                            .py_1p5()
                            .rounded_full()
                            .bg(pill_bg)
                            .border_1()
                            .border_color(pill_border)
                            .child(div().size(px(7.)).rounded_full().bg(k.hue(Hue::Pink)))
                            .child(
                                div()
                                    .text_size(text::XS.size)
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(k.text_primary)
                                    .child(beat.prompt),
                            ),
                    )
                    // Subtle speech-dot trail beneath the pill.
                    .child(div().mt_1p5().size(px(5.)).rounded_full().bg(if dark {
                        rgba(0xffffff30).into()
                    } else {
                        k.surface_border
                    })),
            )
            .child(
                // 3-card photographic showcase (left, center hero, right).
                h_flex()
                    .items_end()
                    .justify_center()
                    .gap_3()
                    .h(px(144.))
                    .opacity(hero_t)
                    .relative()
                    .top(px((1.0 - hero_t) * 20.0 + bob))
                    .child(self.showcase_card(
                        0,
                        beat.card_index == 0,
                        false,
                        "WORSHIP",
                        "Great Is Thy Faithfulness",
                        None,
                    ))
                    .child(self.showcase_card(
                        1,
                        beat.card_index == 1,
                        true,
                        "EASTER SUNDAY",
                        "He Is Risen",
                        Some("9:00 & 11:00 AM"),
                    ))
                    .child(self.showcase_card(
                        2,
                        beat.card_index == 2,
                        false,
                        "4-SLIDE DECK",
                        "Youth Night",
                        None,
                    )),
            )
            .child(
                // Staggered copy + single Continue CTA.
                v_flex()
                    .items_center()
                    .mt_7()
                    .opacity(copy_t)
                    .relative()
                    .top(px((1.0 - copy_t) * 12.0))
                    .child(
                        div()
                            .text_size(text::XL.size)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(k.text_primary)
                            .child("Meet Imagine"),
                    )
                    .child(
                        div()
                            .mt_2()
                            .max_w(px(380.))
                            .text_center()
                            .text_size(text::SM.size)
                            .line_height(text::SM.line_height)
                            .text_color(k.text(TextStep::T500))
                            .child(
                                "Generate worship backgrounds, sermon graphics, and slide decks \
                                 from a prompt — then refine them right in Kairo.",
                            ),
                    )
                    .child(
                        div().mt_6().w(px(220.)).child(
                            Button::new(ids::INTRO_CONTINUE)
                                .cursor_pointer()
                                .w_full()
                                .primary()
                                .label("Continue")
                                .on_click(move |_, _, cx| {
                                    page.update(cx, |p, cx| p.open_connect(cx))
                                }),
                        ),
                    ),
            )
            .into_any_element()
    }

    fn showcase_card(
        &self,
        index: usize,
        active: bool,
        featured: bool,
        eyebrow: &'static str,
        title: &'static str,
        subtitle: Option<&'static str>,
    ) -> AnyElement {
        let (w, h) = if featured {
            if active { (194., 116.) } else { (182., 108.) }
        } else if active {
            (164., 98.)
        } else {
            (150., 90.)
        };
        let lift = if active {
            -8.0
        } else if featured {
            -4.0
        } else {
            0.0
        };
        let border = if active {
            rgba(0xffffff66)
        } else {
            rgba(0xffffff24)
        };
        let opacity = if active {
            1.0
        } else if featured {
            0.88
        } else {
            0.72
        };

        div()
            .relative()
            .top(px(lift))
            .w(px(w))
            .h(px(h))
            .rounded(radius::lg())
            .overflow_hidden()
            .border_1()
            .border_color(border)
            .opacity(opacity)
            .child(
                img(self.photos[index].clone())
                    .size_full()
                    .object_fit(ObjectFit::Cover),
            )
            .child(
                // Dark scrim so overlaid slide typography always reads crisply.
                div().absolute().inset_0().bg(linear_gradient(
                    180.0,
                    linear_color_stop(rgba(0x090b1026), 0.0),
                    linear_color_stop(rgba(0x090b10c8), 1.0),
                )),
            )
            .child(if featured {
                v_flex()
                    .absolute()
                    .inset_0()
                    .items_center()
                    .justify_center()
                    .px_3()
                    .child(
                        div()
                            .text_size(px(9.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(0xe8cc90))
                            .child(eyebrow),
                    )
                    .child(
                        div()
                            .mt_0p5()
                            .text_size(text::BASE.size)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(0xffffff))
                            .child(title),
                    )
                    .children(subtitle.map(|sub| {
                        div()
                            .mt_1()
                            .text_size(px(9.))
                            .text_color(rgba(0xffffffcc))
                            .child(sub)
                    }))
                    .into_any_element()
            } else {
                v_flex()
                    .absolute()
                    .left_2p5()
                    .right_2p5()
                    .bottom_2()
                    .child(
                        div()
                            .text_size(px(8.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgba(0xffffffb8))
                            .child(eyebrow),
                    )
                    .child(
                        div()
                            .text_size(text::XS.size)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(0xffffff))
                            .truncate()
                            .child(title),
                    )
                    .into_any_element()
            })
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
            eased(self.step_changed.elapsed().as_secs_f32(), 0.0, 0.42)
        };
        let logo = if k.appearance.is_dark() {
            self.openai_dark.clone()
        } else {
            self.openai_light.clone()
        };

        v_flex()
            .w(px(388.))
            .items_center()
            .opacity(t)
            .relative()
            .top(px((1.0 - t) * 14.0))
            .child(
                v_flex()
                    .w_full()
                    .items_center()
                    .gap_3()
                    .p_6()
                    .rounded(radius::xl())
                    .bg(k.surface_secondary)
                    .border_1()
                    .border_color(k.surface_border)
                    // OpenAI blossom mark in a clean squircle badge.
                    .child(
                        div()
                            .size(px(46.))
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
                            .cursor_pointer()
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
                            .cursor_pointer()
                            .small()
                            .ghost()
                            .icon(Icon::new(KairoIcon::ExternalLink))
                            .label("Get an OpenAI API key")
                            .on_click(move |_, _, cx| cx.open_url(KEYS_URL)),
                    )
                    // Quiet keychain trust footer.
                    .child(
                        h_flex()
                            .w_full()
                            .pt_3()
                            .mt_1()
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
                        .cursor_pointer()
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

/// Soft top-centre atmospheric wash painted as concentric translucent ellipses
/// (same technique as `kairo_ui_kit::onboarding::StageGlow`).
fn render_wash(alpha: f32, cx: &mut Context<ImaginePage>) -> AnyElement {
    let k = *cx.kairo();
    let dark = k.appearance.is_dark();
    let pink = with_alpha(k.hue(Hue::Pink), if dark { 0.24 } else { 0.14 } * alpha);
    let blue = with_alpha(k.hue(Hue::Blue), if dark { 0.16 } else { 0.10 } * alpha);

    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            window.with_content_mask(Some(gpui_kit::ContentMask { bounds }), |window| {
                paint_ellipse_glow(bounds, (0.50, 0.02), (0.62, 0.56), pink, window);
                paint_ellipse_glow(bounds, (0.54, 0.24), (0.50, 0.44), blue, window);
            });
        },
    )
    .absolute()
    .inset_0()
    .size_full()
    .into_any_element()
}

fn with_alpha(mut color: Hsla, a: f32) -> Hsla {
    color.a = a.clamp(0.0, 1.0);
    color
}

fn ring_alpha(alpha: f32) -> f32 {
    if alpha <= 0.0 {
        return 0.0;
    }
    1.0 - (1.0 - alpha.clamp(0.0, 0.99)).powf(1.0 / WASH_RINGS as f32)
}

fn paint_ellipse_glow(
    bounds: Bounds<Pixels>,
    at: (f32, f32),
    radius: (f32, f32),
    color: Hsla,
    window: &mut Window,
) {
    if color.a <= 0.001 {
        return;
    }
    let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    let cx = f32::from(bounds.origin.x) + at.0 * w;
    let cy = f32::from(bounds.origin.y) + at.1 * h;
    let (rx, ry) = (radius.0 * w * 0.72, radius.1 * h * 0.72);
    let ring_color = with_alpha(color, ring_alpha(color.a));
    for ring in 0..WASH_RINGS {
        let scale = 1.0 - ring as f32 / WASH_RINGS as f32;
        let (r_x, r_y) = (rx * scale, ry * scale);
        if r_x <= 0.0 || r_y <= 0.0 {
            continue;
        }
        let mut b = PathBuilder::fill();
        b.move_to(point(px(cx + r_x), px(cy)));
        b.arc_to(
            point(px(r_x), px(r_y)),
            px(0.),
            false,
            true,
            point(px(cx - r_x), px(cy)),
        );
        b.arc_to(
            point(px(r_x), px(r_y)),
            px(0.),
            false,
            true,
            point(px(cx + r_x), px(cy)),
        );
        b.close();
        if let Ok(path) = b.build() {
            window.paint_path(path, ring_color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intro_starts_on_welcome_and_switches_steps() {
        let mut intro = IntroState::new();
        assert_eq!(intro.step(), IntroStep::Welcome);
        assert!(intro.animating());

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
