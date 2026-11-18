//! The live transcript column (`Operator.tsx`, left column): the input
//! meter, saved nuggets, the reconnect notice, highlighted speech, the
//! empty states and the session bar.

use std::hash::{Hash, Hasher};

use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_kit::component::{Disableable, Icon, Sizable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, AppContext, ClipboardItem, Context, Entity, FontWeight, Hsla,
    InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, StyledText, Subscription, TextRun as GpuiRun, Window, div,
    prelude::FluentBuilder, px, relative,
};
use kairo_ui_kit::coach::coach_anchor;
use kairo_ui_kit::motion::LISTENING;
use kairo_ui_kit::tokens::{Step, TextStep, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};

use super::service::ServicePanel;
use super::text::{TextRun, build_runs};
use super::{Booth, format_local_time};
use crate::scroll::{Edge, PinnedScroll};

/// Element ids, for tests.
pub mod ids {
    pub const START: &str = "transcript-start";
    pub const LISTEN: &str = "transcript-listen";
    pub const NUGGETS: &str = "transcript-nuggets";
    pub const NUGGETS_EXPORT: &str = "nuggets-export";
    pub const RECONNECTING: &str = "transcript-reconnecting";
    pub const LISTENING: &str = "transcript-listening";
    pub const METER: &str = "transcript-meter";
}

const METER_BARS: usize = 12;

/// Which meter segments light, as the audio panel's level strip draws them.
pub fn meter_lit(active: bool, rms: f64) -> [bool; METER_BARS] {
    let value = rms.clamp(0.0, 1.0);
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
        let percent = (level.rms.clamp(0.0, 1.0) * 100.0).round() as u32;
        let count = booth.nuggets().len();
        let open = booth.nuggets_open;
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
            .w(px(78.))
            .h(px(6.))
            .flex_shrink_0()
            .gap(px(2.))
            .children((0..METER_BARS).map(|i| {
                // Green, amber near the top, red when the input clips.
                let on = if level.clipping {
                    k.destructive
                } else if i >= METER_BARS - 2 {
                    k.hue(kairo_ui_kit::Hue::Amber)
                } else {
                    k.hue(kairo_ui_kit::Hue::Green)
                };
                div()
                    .min_w_0()
                    .flex_1()
                    .h_full()
                    .rounded(px(1.))
                    .bg(if lit[i] { on } else { k.surface_elevated })
            }));
        h_flex()
            .relative()
            .h(px(44.))
            .flex_shrink_0()
            .items_center()
            .justify_between()
            .gap_2()
            .pl_4()
            .pr_2p5()
            .child(
                h_flex()
                    .min_w_0()
                    .items_center()
                    .gap_2p5()
                    .child(
                        div()
                            .truncate()
                            .text_size(text::XS.size)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(k.text(TextStep::T200))
                            .child("Transcript"),
                    )
                    .child(meter),
            )
            .child(
                Button::new(ids::NUGGETS)
                    .cursor_pointer()
                    .custom(if open {
                        ButtonCustomVariant::new(cx)
                            .color(k.tints.amber)
                            .foreground(k.warning_text)
                            .hover(k.tints.amber)
                            .active(k.tints.amber)
                    } else {
                        ButtonCustomVariant::new(cx)
                            .foreground(k.text(TextStep::T500))
                            .hover(k.surface_tertiary)
                            .active(k.surface_tertiary)
                    })
                    // Solid fill: the custom variant paints its colour at 20%.
                    .when(open, |b| b.bg(k.tints.amber))
                    .xsmall()
                    .icon(Icon::new(KairoIcon::BookmarkSimple).size(px(11.)))
                    .when(count > 0, |b| b.label(count.to_string()))
                    .accessibility_label(format!("Saved message nuggets: {count}"))
                    .tooltip(format!("Saved quotes ({count})"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.booth.update(cx, |b, cx| b.toggle_nuggets(cx))
                    })),
            )
            .into_any_element()
    }

    fn nuggets_popover(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let booth = self.booth.read(cx);
        let nuggets = booth.nuggets();
        let title = if booth.active_service().is_some() {
            "This service\u{2019}s nuggets"
        } else {
            "Legacy nuggets"
        };
        let mut list = v_flex()
            .id("nuggets-list")
            .max_h(px(288.))
            .overflow_y_scroll()
            .p_2();
        if nuggets.is_empty() {
            list = list.child(
                div()
                    .px_2()
                    .py_5()
                    .text_center()
                    .text_size(text::LABEL.size)
                    .text_color(k.text(TextStep::T500))
                    .child("Bookmark a transcript line to save a quote for after service."),
            );
        }
        for (id, body, at) in nuggets.iter().cloned() {
            let copy = body.clone();
            let remove = id.clone();
            list = list.child(
                h_flex()
                    .id(SharedString::from(format!("nugget-{id}")))
                    .group("nugget")
                    .gap_2()
                    .rounded_md()
                    .px_2()
                    .py_2()
                    .hover(|s| s.bg(k.surface_tertiary))
                    .child(
                        v_flex()
                            .min_w_0()
                            .flex_1()
                            .child(
                                div()
                                    .text_size(text::LABEL.size)
                                    .text_color(k.text(TextStep::T300))
                                    .child(body),
                            )
                            .child(
                                div()
                                    .mt_1()
                                    .text_size(px(9.))
                                    .text_color(k.text(TextStep::T600))
                                    .child(format_local_time(at, false)),
                            ),
                    )
                    .child(
                        h_flex()
                            .self_start()
                            .child(
                                Button::new(SharedString::from(format!("nugget-copy-{id}")))
                                    .cursor_pointer()
                                    .ghost()
                                    .xsmall()
                                    .icon(Icon::new(KairoIcon::Copy).size(px(11.)))
                                    .accessibility_label("Copy nugget")
                                    .tooltip("Copy nugget")
                                    .on_click(move |_, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            copy.clone(),
                                        ))
                                    }),
                            )
                            .child(
                                Button::new(SharedString::from(format!("nugget-remove-{id}")))
                                    .cursor_pointer()
                                    .ghost()
                                    .xsmall()
                                    .icon(Icon::new(KairoIcon::X).size(px(11.)))
                                    .accessibility_label("Remove nugget")
                                    .tooltip("Remove nugget")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        let id = remove.clone();
                                        this.booth.update(cx, |b, cx| b.remove_nugget(&id, cx))
                                    })),
                            ),
                    ),
            );
        }
        v_flex()
            .id("nuggets-popover")
            .test_support()
            .aria_label(title)
            .absolute()
            .left_2()
            .right_2()
            .top(px(50.))
            .rounded_lg()
            .bg(k.neutral(Step::S950))
            .shadow_2xl()
            .overflow_hidden()
            .occlude()
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .py_2()
                    .child(
                        div()
                            .text_size(text::LABEL.size)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(k.text(TextStep::T200))
                            .child(title),
                    )
                    .child(
                        Button::new(ids::NUGGETS_EXPORT)
                            .cursor_pointer()
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(KairoIcon::Download).size(px(11.)))
                            .label("Export")
                            .disabled(nuggets.is_empty())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.booth.update(cx, |b, cx| b.export_nuggets(cx))
                            })),
                    ),
            )
            .child(list)
            .into_any_element()
    }

    fn segment(
        &self,
        index: usize,
        last: bool,
        saved: bool,
        can_save: bool,
        segment: &super::Segment,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let base = if last {
            k.text_primary
        } else {
            k.text(TextStep::T500)
        };
        let (string, runs) = styled_runs(&build_runs(&segment.text, &segment.highlights), base, &k);
        let id = segment.id.clone();
        div()
            .id(SharedString::from(format!("segment-{}", segment.id)))
            .group("segment")
            .relative()
            .rounded_md()
            .py_1()
            .pr_5()
            .when(saved, |s| s.bg(k.tints.amber))
            .child(StyledText::new(string).with_runs(runs))
            .child(
                div()
                    .absolute()
                    .right(px(-4.))
                    .top(px(6.))
                    .when(!saved, |s| {
                        s.opacity(0.).group_hover("segment", |s| s.opacity(1.))
                    })
                    .child(
                        Button::new(SharedString::from(format!(
                            "segment-save-{index}-{}",
                            segment.id
                        )))
                        .cursor_pointer()
                        .custom(
                            ButtonCustomVariant::new(cx)
                                .foreground(if saved {
                                    k.warning_text
                                } else {
                                    k.text(TextStep::T500)
                                })
                                .hover(k.tints.amber)
                                .active(k.tints.amber),
                        )
                        .xsmall()
                        .icon(Icon::new(KairoIcon::BookmarkSimple).size(px(13.)))
                        .disabled(saved || !can_save)
                        .accessibility_label(if saved {
                            "Nugget saved"
                        } else {
                            "Save as message nugget"
                        })
                        .tooltip(if saved {
                            "Nugget saved"
                        } else {
                            "Save this message nugget"
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let id = id.clone();
                            this.booth.update(cx, |b, cx| b.save_nugget(&id, cx))
                        })),
                    ),
            )
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
        let status = (booth.active_service().is_none()).then(|| {
            if transcribing {
                if booth.stt_reconnecting() {
                    "Reconnecting"
                } else {
                    "Listening"
                }
            } else {
                "Paused"
            }
        });
        let busy = booth.pipeline_busy;
        h_flex()
            .h_12()
            .flex_shrink_0()
            .items_center()
            .gap_1()
            .px_2()
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
                                    KairoIcon::Pause
                                } else {
                                    KairoIcon::Play
                                })
                                .size(px(12.)),
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
        let nuggets: Vec<String> = booth.nuggets().into_iter().map(|n| n.1).collect();
        let can_save = booth.active_service().is_some();
        let idle = booth.transcript_idle();
        // Only while listening: stopping also reports "disconnected".
        let reconnecting =
            booth.resilience.is_some() && booth.transcribing && booth.stt_reconnecting();
        let (notice, detail) = if booth.stt_weak() {
            ("Weak connection", "Captions may lag")
        } else {
            ("Reconnecting…", "Audio is buffered")
        };
        let nuggets_open = booth.nuggets_open;
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
            .map(|(i, s)| {
                self.segment(
                    i,
                    i + 1 == count,
                    nuggets.contains(&s.text),
                    can_save,
                    s,
                    cx,
                )
            })
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
            .when(reconnecting, |this| {
                this.child(
                    h_flex()
                        .id(ids::RECONNECTING)
                        .test_support()
                        .flex_shrink_0()
                        .flex_wrap()
                        .items_center()
                        .gap_1p5()
                        .px_3()
                        .py_2()
                        .bg(k.tints.amber)
                        .text_size(text::LABEL.size)
                        .text_color(k.warning_text)
                        .child(notice)
                        .child(
                            div()
                                .rounded_sm()
                                .px_1p5()
                                .py_0p5()
                                .bg(k.tints.amber)
                                .text_size(px(9.))
                                .child(detail),
                        ),
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
            .when(nuggets_open, |this| this.child(self.nuggets_popover(cx)))
    }
}
