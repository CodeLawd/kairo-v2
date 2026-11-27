//! The live transcript column (`Operator.tsx`, left column): the input
//! meter, the reconnect notice, highlighted speech, the
//! empty states and the session bar.

use std::hash::{Hash, Hasher};

use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_kit::component::{Disableable, Icon, Sizable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, AppContext, Context, Entity, FontWeight, Hsla, InteractiveElement,
    IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled,
    StyledText, Subscription, TextRun as GpuiRun, Window, div, prelude::FluentBuilder, px,
    relative,
};
use kairo_ui_kit::coach::coach_anchor;
use kairo_ui_kit::motion::LISTENING;
use kairo_ui_kit::tokens::{Step, TextStep, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};

use super::service::ServicePanel;
use super::text::{TextRun, build_runs};
use super::{Booth, ListenState};
use crate::scroll::{Edge, PinnedScroll};

/// Element ids, for tests.
pub mod ids {
    pub const START: &str = "transcript-start";
    pub const LISTEN: &str = "transcript-listen";
    pub const RECONNECTING: &str = "transcript-reconnecting";
    pub const LISTEN_STATE: &str = "transcript-listen-state";
    pub const LISTENING: &str = "transcript-listening";
    pub const METER: &str = "transcript-meter";
    /// Retry now, on the connection notice.
    pub const RETRY: &str = "transcript-retry";
}

const METER_BARS: usize = 12;

/// Which meter segments light, as the audio panel's level strip draws them.
pub fn meter_lit(active: bool, rms: f64) -> [bool; METER_BARS] {
    let value = kairo_ui_kit::meter_fill(rms);
    std::array::from_fn(|i| active && value >= (i as f64 + 0.5) / METER_BARS as f64)
}

pub struct TranscriptPanel {
    booth: Entity<Booth>,
    service: Entity<ServicePanel>,
    scroll: PinnedScroll,
    _subscriptions: Vec<Subscription>,
}

impl TranscriptPanel {
    pub fn new(booth: Entity<Booth>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let service = cx.new(|cx| ServicePanel::new(booth.clone(), window, cx));
        let subscriptions = vec![cx.observe(&booth, |_, _, cx| cx.notify())];
        Self {
            booth,
            service,
            scroll: PinnedScroll::new(Edge::Bottom),
            _subscriptions: subscriptions,
        }
    }

    pub fn service(&self) -> &Entity<ServicePanel> {
        &self.service
    }

    fn header(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let booth = self.booth.read(cx);
        let level = booth.level;
        let lit = meter_lit(booth.transcribing, level.rms);
        let percent = (kairo_ui_kit::meter_fill(level.rms) * 100.0).round() as u32;
        let state = booth.transcribing.then(|| booth.listen_state());
        // Five rising bars, like a signal meter: each covers two of the
        // level steps; amber at the top, red when the input clips.
        const BARS: usize = METER_BARS / 2;
        let meter = h_flex()
            .id(ids::METER)
            .test_support()
            .aria_label(if level.clipping {
                "Input level, clipping".to_owned()
            } else {
                "Input level".to_owned()
            })
            .tooltip(super::widgets::tip(if level.clipping {
                "Input is clipping".to_owned()
            } else {
                format!("Input level {percent}%")
            }))
            .h(px(12.))
            .flex_shrink_0()
            .items_end()
            .gap(px(2.))
            .children((0..BARS).map(|i| {
                let on = if level.clipping {
                    k.destructive
                } else if i == BARS - 1 {
                    k.hue(kairo_ui_kit::Hue::Amber)
                } else {
                    k.hue(kairo_ui_kit::Hue::Green)
                };
                div()
                    .w(px(3.))
                    .h(px(4. + i as f32 * 2.))
                    .rounded_full()
                    .bg(if lit[i * 2] { on } else { k.ink(0.14) })
            }));
        h_flex()
            .relative()
            .h(px(44.))
            .flex_shrink_0()
            .items_center()
            .justify_between()
            .gap_2()
            .pl_4()
            .pr_3()
            .child(
                div()
                    .truncate()
                    .text_size(text::SM.size)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(k.text_primary)
                    .child("Transcript"),
            )
            .when_some(state, |this, state| {
                // The meter and the words together in one pill, tinted by how
                // the connection is doing: never colour alone.
                let ink = if state.is_critical() {
                    k.danger_text
                } else if state.notice().is_some() {
                    k.warning_text
                } else {
                    k.success_text
                };
                this.child(
                    h_flex()
                        .id(ids::LISTEN_STATE)
                        .test_support()
                        .min_w_0()
                        .h_6()
                        .items_center()
                        .gap_2()
                        .pl_2p5()
                        .pr_3()
                        .rounded_full()
                        .bg(ink.opacity(0.12))
                        .child(meter)
                        .child(
                            div()
                                .truncate()
                                .text_size(text::LABEL.size)
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(ink)
                                .child(state.label()),
                        ),
                )
            })
            .into_any_element()
    }

    fn segment(&self, last: bool, segment: &super::Segment, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let base = if last {
            k.text_primary
        } else {
            k.text(TextStep::T500)
        };
        let (string, runs) = styled_runs(&build_runs(&segment.text, &segment.highlights), base, &k);
        div()
            .id(SharedString::from(format!("segment-{}", segment.id)))
            .rounded_md()
            .py_1()
            .child(StyledText::new(string).with_runs(runs))
            .into_any_element()
    }

    fn empty(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let k = *cx.kairo();
        let booth = self.booth.read(cx);
        if !booth.segments.is_empty() || !booth.interim.is_empty() {
            return None;
        }
        if booth.transcript_idle() {
            let busy = booth.pipeline_busy;
            return Some(
                v_flex()
                    .mx_auto()
                    .w_full()
                    .max_w(px(240.))
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .text_center()
                    .child(
                        div()
                            .size(px(44.))
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(k.surface_tertiary)
                            .text_color(k.text(TextStep::T400))
                            .child(Icon::new(KairoIcon::Mic).size(px(17.))),
                    )
                    .child(
                        div()
                            .mt_4()
                            .text_size(text::SM.size)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(k.text(TextStep::T300))
                            .child("Ready when you are"),
                    )
                    .child(
                        div()
                            .mt_1()
                            .text_size(text::XS.size)
                            .text_color(k.text(TextStep::T500))
                            .child("Start when the message begins."),
                    )
                    .child(
                        div()
                            .relative()
                            .mt_5()
                            .child(coach_anchor("transcript-start"))
                            .child(
                                super::widgets::primary_button(ids::START, "Start listening", cx)
                                    .icon(Icon::new(KairoIcon::Play).size(px(12.)))
                                    .tooltip("Start transcription")
                                    .disabled(busy)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.booth.update(cx, |b, cx| b.press_listen(cx))
                                    })),
                            ),
                    )
                    .into_any_element(),
            );
        }
        if !booth.transcribing {
            return None;
        }
        let bars = h_flex()
            .h_4()
            .items_end()
            .gap(px(3.))
            .children((0..4).map(|bar| {
                let el = div()
                    .w(px(3.))
                    .h(relative(0.4))
                    .rounded_full()
                    .bg(k.text(TextStep::T500));
                listening_bar(el, bar, cx)
            }));
        Some(
            v_flex()
                .id(ids::LISTENING)
                .test_support()
                .flex_1()
                .items_center()
                .justify_center()
                .text_center()
                .child(bars)
                .child(
                    div()
                        .mt_3()
                        .text_size(text::SM.size)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(k.text(TextStep::T300))
                        .child("Listening…"),
                )
                .child(
                    div()
                        .mt_1()
                        .text_size(text::XS.size)
                        .text_color(k.text(TextStep::T500))
                        .child("Words appear here as they\u{2019}re spoken."),
                )
                .into_any_element(),
        )
    }

    fn session_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let booth = self.booth.read(cx);
        let transcribing = booth.transcribing;
        let label = if transcribing { "Pause" } else { "Resume" };
        // The header carries the listening state; this says only paused.
        let status = (booth.active_service().is_none() && !transcribing).then_some("Paused");
        let busy = booth.pipeline_busy;
        h_flex()
            .h_12()
            .flex_shrink_0()
            .items_center()
            .gap_2()
            .px_2p5()
            .border_t_1()
            .border_color(k.surface_line)
            .child(
                div()
                    .relative()
                    .child(coach_anchor("transcript-start"))
                    .child(
                        Button::new(ids::LISTEN)
                            .cursor_pointer()
                            .custom(if transcribing {
                                ButtonCustomVariant::new(cx)
                                    .foreground(k.text(TextStep::T200))
                                    .hover(k.surface_tertiary)
                                    .active(k.surface_tertiary)
                            } else {
                                ButtonCustomVariant::new(cx)
                                    .color(k.accent(Step::S500))
                                    .foreground(k.on_accent)
                                    .hover(k.accent(Step::S600))
                                    .active(k.accent(Step::S600))
                            })
                            // Solid fill: the custom variant paints its colour at 20%.
                            .when(!transcribing, |b| b.bg(k.accent(Step::S500)))
                            .small()
                            .icon(
                                Icon::new(if transcribing {
                                    KairoIcon::Pause.fill()
                                } else {
                                    KairoIcon::Play.fill()
                                })
                                .size_3p5(),
                            )
                            .label(label)
                            .tooltip(if transcribing {
                                "Pause transcription"
                            } else {
                                "Resume transcription"
                            })
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.booth.update(cx, |b, cx| b.press_listen(cx))
                            })),
                    ),
            )
            .when_some(status, |this, status| {
                this.child(
                    h_flex()
                        .min_w_0()
                        .flex_1()
                        .justify_end()
                        .px_2()
                        .text_size(text::LABEL.size)
                        .text_color(k.text(TextStep::T500))
                        .child(status),
                )
            })
            .child(self.service.clone())
            .into_any_element()
    }
}

/// The listening bars breathe in turn, 0.15 s apart; still under reduced motion.
fn listening_bar(el: gpui_kit::Div, index: usize, cx: &App) -> AnyElement {
    use gpui_kit::AnimationExt;
    if cx.reduced_motion() {
        return el.into_any_element();
    }
    let delay = index as f32 * 150.0 / LISTENING.duration_ms as f32;
    el.with_animation(
        SharedString::from(format!("listening-{index}")),
        LISTENING.animation(),
        move |el, t| {
            let phase = (t + 1.0 - delay).fract();
            el.h(relative(LISTENING.sample(phase).height))
        },
    )
    .into_any_element()
}

/// The transcript runs as one string with coloured spans: triggers bold
/// teal, matched quotes in the scripture tint.
fn styled_runs(
    runs: &[TextRun],
    base: Hsla,
    k: &kairo_ui_kit::KairoColors,
) -> (SharedString, Vec<GpuiRun>) {
    let mut string = String::new();
    let mut out = Vec::new();
    let font = |weight: FontWeight| gpui_kit::Font {
        family: text::FAMILY.into(),
        weight,
        ..gpui_kit::font(text::FAMILY)
    };
    for run in runs {
        let (body, color, weight, background) = match run {
            TextRun::Plain(t) => (t, base, FontWeight::NORMAL, None),
            TextRun::Trigger(t) => (t, k.accent(Step::S400), FontWeight::SEMIBOLD, None),
            TextRun::Scripture { text, .. } => (
                text,
                k.accent(Step::S200),
                FontWeight::NORMAL,
                Some(k.tints.teal),
            ),
        };
        string.push_str(body);
        out.push(GpuiRun {
            len: body.len(),
            font: font(weight),
            color,
            background_color: background,
            underline: None,
            strikethrough: None,
        });
    }
    (string.into(), out)
}

impl Render for TranscriptPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = *cx.kairo();
        let booth = self.booth.read(cx);
        let segments = booth.segments.clone();
        let interim = booth.interim.clone();
        let idle = booth.transcript_idle();
        // Only while listening: stopping also reports "disconnected".
        let state = booth.listen_state();
        let notice = booth.transcribing.then(|| state.notice()).flatten();
        let (tint, ink) = if state.is_critical() {
            (k.tints.red, k.danger_text)
        } else {
            (k.tints.amber, k.warning_text)
        };
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for s in &segments {
            s.id.hash(&mut hasher);
        }
        interim.hash(&mut hasher);
        self.scroll.follow(hasher.finish());
        let count = segments.len();
        let rows: Vec<AnyElement> = segments
            .iter()
            .enumerate()
            .map(|(i, s)| self.segment(i + 1 == count, s, cx))
            .collect();
        let empty = self.empty(cx);
        v_flex()
            .id("transcript-panel")
            .relative()
            .size_full()
            .bg(k.surface_secondary)
            .border_r_1()
            .border_color(k.surface_line)
            .child(self.header(cx))
            .when_some(notice, |this, (notice, detail)| {
                // A status, not a button: the words say what is happening and
                // what Kairo is doing about it. Retry now is the one action,
                // where retrying can help.
                let retry = matches!(
                    state,
                    ListenState::Stopped | ListenState::Reconnecting | ListenState::MicInterrupted
                );
                let busy = self.booth.read(cx).pipeline_busy;
                this.child(
                    h_flex()
                        .id(ids::RECONNECTING)
                        .test_support()
                        .role(gpui_kit::Role::Status)
                        .flex_shrink_0()
                        .items_center()
                        .gap_2p5()
                        .mx_3()
                        .mb_2()
                        .px_3()
                        .py_2()
                        .rounded(kairo_ui_kit::tokens::radius::lg())
                        .bg(tint)
                        .child(
                            Icon::new(if state.is_critical() {
                                KairoIcon::AlertCircle
                            } else {
                                KairoIcon::AlertTriangle
                            })
                            .size_4()
                            .flex_none()
                            .text_color(ink),
                        )
                        .child(
                            v_flex()
                                .min_w_0()
                                .flex_1()
                                .child(
                                    div()
                                        .text_size(text::XS.size)
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(ink)
                                        .child(notice),
                                )
                                .child(
                                    div()
                                        .text_size(text::LABEL.size)
                                        .text_color(k.text(TextStep::T400))
                                        .child(detail),
                                ),
                        )
                        .when(retry, |this| {
                            this.child(
                                Button::new(ids::RETRY)
                                    .cursor_pointer()
                                    .ghost()
                                    .xsmall()
                                    .label(if busy { "Retrying…" } else { "Retry now" })
                                    .disabled(busy)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.booth.update(cx, |b, cx| b.retry_listening(cx))
                                    })),
                            )
                        }),
                )
            })
            .child(
                v_flex()
                    .id("transcript-scroll")
                    .track_scroll(self.scroll.handle())
                    .min_h_0()
                    .flex_1()
                    .overflow_y_scroll()
                    .px_4()
                    .py_4()
                    .text_size(px(14.))
                    .line_height(relative(1.75))
                    .text_color(k.text(TextStep::T500))
                    .when(count > 0 || !interim.is_empty(), |s| s.gap_2())
                    .children(rows)
                    .when(!interim.is_empty(), |this| {
                        this.child(
                            div()
                                .id("transcript-interim")
                                .test_support()
                                .border_l_2()
                                .border_color(k.accent(Step::S500).opacity(0.4))
                                .pl_3()
                                .text_color(k.text(TextStep::T400))
                                .child(interim.clone()),
                        )
                    })
                    .children(empty),
            )
            .when(!idle, |this| this.child(self.session_bar(cx)))
    }
}
