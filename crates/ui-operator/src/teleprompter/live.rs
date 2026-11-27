//! The live panel: the screen the prompter goes to, a preview of what the
//! presenter reads, and the controls. The preview runs on its own, so the
//! operator can rehearse speed, pause and play before going live; once
//! live, the same controls drive the screen.

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::slider::Slider;
use gpui_kit::component::{Disableable, Icon, Selectable, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use kairo_engine_proto::StageOperation;
use kairo_ui_kit::tokens::{TextStep, metrics, radius, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};

use super::{GUIDE, LINE_HEIGHT, Teleprompter, paragraph_at, paragraph_starts, speed_level};

impl Teleprompter {
    pub(super) fn live(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        v_flex()
            .w_96()
            .flex_none()
            .h_full()
            .min_h_0()
            .bg(k.surface_rail)
            .border_l(metrics::panel_border_width())
            .border_color(k.panel_border)
            .child(self.live_header(cx))
            .child(self.preview(cx))
            .child(self.transport_bar(cx))
            .child(self.display_options(cx))
            .into_any_element()
    }

    fn target_name(&self, id: Option<&str>) -> Option<String> {
        let id = id?;
        self.targets
            .iter()
            .find(|target| target.id == id)
            .map(|target| target.name.clone())
    }

    fn live_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let live_name = self.target_name(self.live.as_deref());
        let (dot, state) = match &live_name {
            Some(name) => (k.live, format!("Live on {name}")),
            None => (k.text(TextStep::T400), "Rehearsal".to_owned()),
        };
        // The screen it is live on wins over a choice that no longer exists.
        let chosen = self
            .live
            .as_deref()
            .or(self.target.as_deref())
            .and_then(|id| self.targets.iter().find(|target| target.id == id))
            .or_else(|| {
                self.target
                    .as_deref()
                    .and_then(|id| self.targets.iter().find(|target| target.id == id))
            });
        let ready = chosen.is_some_and(|target| target.ready);
        let reason = chosen
            .filter(|target| !target.ready)
            .map(|target| target.reason.clone())
            .filter(|reason| !reason.is_empty());
        let behind = self.live.is_some()
            && self.selected.is_some()
            && self.selected.as_deref() == self.on_display.as_deref()
            && self
                .published
                .as_ref()
                .is_some_and(|published| published.body != self.body.read(cx).value().as_ref());
        let entity = cx.entity();
        let targets = self.targets.clone();
        let current = chosen.map(|target| target.id.clone());
        let picker_label = chosen.map_or_else(
            || {
                if self.targets.is_empty() {
                    "No screens".to_owned()
                } else {
                    "Choose a screen".to_owned()
                }
            },
            |target| target.name.clone(),
        );
        let live = self.live.is_some();
        let picker = Button::new("teleprompter-target")
            .w_full()
            .small()
            .label(picker_label)
            .dropdown_caret(true)
            .disabled(self.targets.is_empty())
            .accessibility_label("Screen for the prompter")
            .dropdown_menu(move |mut menu, _, _| {
                for target in &targets {
                    let view = entity.clone();
                    let id = target.id.clone();
                    let label = if target.ready || target.reason.is_empty() {
                        target.name.clone()
                    } else {
                        format!("{} \u{2014} {}", target.name, target.reason)
                    };
                    menu = menu.item(
                        PopupMenuItem::new(label)
                            .checked(current.as_deref() == Some(id.as_str()))
                            .disabled(!target.ready)
                            .on_click(move |_, window, cx| {
                                let id = id.clone();
                                view.update(cx, |this, cx| this.choose_target(id, window, cx));
                            }),
                    );
                }
                menu
            });
        // Go live and Take off sit beside the screen they act on.
        let action = if live {
            Button::new("teleprompter-take-off")
                .danger()
                .small()
                .icon(Icon::new(KairoIcon::MonitorOff))
                .label("Take off")
                .tooltip("Take the prompter off the screen")
                .disabled(self.busy)
                .on_click(cx.listener(|this, _, window, cx| this.take_off(window, cx)))
        } else {
            Button::new("teleprompter-go-live")
                .primary()
                .small()
                .icon(Icon::new(KairoIcon::MonitorPlay))
                .label("Go live")
                .disabled(self.busy || !ready)
                .on_click(cx.listener(|this, _, window, cx| this.go_live(window, cx)))
        };
        let pill_ink = if live { k.live } else { k.text(TextStep::T400) };
        v_flex()
            .px_4()
            .pt_3()
            .pb_3()
            .gap_2p5()
            .border_b(metrics::panel_border_width())
            .border_color(k.panel_border)
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .text_size(text::SM.size)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Prompter"),
                    )
                    .child(
                        h_flex()
                            .id("teleprompter-state")
                            .flex_none()
                            .min_w_0()
                            .gap_1p5()
                            .items_center()
                            .px_2()
                            .py_0p5()
                            .rounded_full()
                            .bg(pill_ink.opacity(0.14))
                            .text_size(text::LABEL.size)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(pill_ink)
                            .aria_label(state.clone())
                            .child(div().size_1p5().flex_none().rounded_full().bg(dot))
                            .child(div().truncate().child(state)),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(div().flex_1().min_w_0().child(picker))
                    .child(div().flex_none().child(action)),
            )
            .when_some(
                reason.or_else(|| {
                    self.targets
                        .is_empty()
                        .then(|| "No screen available. Turn one on in Screens.".to_owned())
                }),
                |this, reason| {
                    this.child(
                        div()
                            .text_size(text::LABEL.size)
                            .text_color(k.text(TextStep::T400))
                            .child(reason),
                    )
                },
            )
            .when(live && behind, |this| {
                this.child(
                    Button::new("teleprompter-update")
                        .small()
                        .w_full()
                        .icon(Icon::new(KairoIcon::RefreshCw))
                        .label("Update screen with your edits")
                        .disabled(self.busy)
                        .on_click(cx.listener(|this, _, window, cx| this.publish(window, cx))),
                )
            })
            .into_any_element()
    }

    /// The presenter's view at the preview's size: the same text column,
    /// text size and reading line, scrolling at the prompter's speed.
    fn preview(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let (title, body) = self.preview_text(cx);
        let scale = self.preview_scale();
        let font = self.layout.font_px * scale;
        let line = font * LINE_HEIGHT;
        let height = f32::from(self.preview_scroll.bounds().size.height);
        let guide = height * GUIDE;
        let margin = f32::from(self.preview_scroll.bounds().size.width) * 0.08;
        let current = if self.live.is_some() {
            self.position
                .as_ref()
                .map(|position| paragraph_at(&body, position.anchor))
        } else {
            None
        };
        let max = f32::from(self.preview_scroll.max_offset().y);
        let percent = if self.live.is_some() {
            self.position.as_ref().map_or(0, |position| {
                if body.is_empty() {
                    0
                } else {
                    (position.anchor.min(body.len()) * 100 / body.len()) as u32
                }
            })
        } else if max > 0.0 {
            // The real position, so a wheel scroll shows at once.
            let offset = f32::from(-self.preview_scroll.offset().y).max(0.0);
            (offset / max * 100.0).round().clamp(0.0, 100.0) as u32
        } else {
            0
        };
        let starts = paragraph_starts(&body);
        // Display geometry scaled to the preview: these lengths must match
        // the screen's layout, so they come from the frame, not the UI scale.
        let mut script = div()
            .id("teleprompter-preview")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.preview_scroll)
            .px(px(margin))
            .child(div().h(px(guide)));
        for (index, paragraph) in body.split('\n').enumerate() {
            let at = starts[index];
            script = script.child(
                div()
                    .id(("teleprompter-paragraph", index))
                    .min_h(px(line))
                    .text_size(px(font))
                    .line_height(px(line))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(if current.is_none_or(|current| current == index) {
                        k.white
                    } else {
                        k.white.opacity(0.7)
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.transport(StageOperation::Seek { anchor: at }, window, cx)
                    }))
                    .child(paragraph.to_owned()),
            );
        }
        script = script.child(div().h(px((height - guide).max(0.0))));
        v_flex()
            .flex_1()
            .min_h_0()
            .m_4()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .text_size(text::XS.size)
                    .text_color(k.text(TextStep::T400))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(if title.trim().is_empty() {
                                "Untitled script".to_owned()
                            } else {
                                title
                            }),
                    )
                    .child(div().flex_none().child(format!("{percent}%"))),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .rounded(radius::md())
                    .bg(k.monitor())
                    .child(script)
                    .child(
                        // The reading line, fixed while the script moves.
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .top(px(guide))
                            .h(px(2.0))
                            .bg(k.accent(kairo_ui_kit::tokens::Step::S400).opacity(0.8)),
                    )
                    .when(body.trim().is_empty(), |this| {
                        this.child(
                            div()
                                .absolute()
                                .inset_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(text::SM.size)
                                .text_color(k.white.opacity(0.6))
                                .child("Write or open a script to preview it"),
                        )
                    })
                    .when(self.layout.mirror, |this| {
                        this.child(
                            div()
                                .absolute()
                                .top_2()
                                .right_3()
                                .text_size(text::LABEL.size)
                                .text_color(k.white.opacity(0.6))
                                .child("Mirrored on the screen"),
                        )
                    }),
            )
            .into_any_element()
    }

    fn transport_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let usable = !self.preview_text(cx).1.trim().is_empty();
        let control =
            |id: &'static str, icon: KairoIcon, label: &'static str, tip: &'static str| {
                Button::new(id)
                    .ghost()
                    .icon(Icon::new(icon))
                    .accessibility_label(label)
                    .tooltip(tip)
                    .disabled(!usable)
            };
        h_flex()
            .px_4()
            .gap_2()
            .items_center()
            .justify_center()
            .child(
                control(
                    "teleprompter-restart",
                    KairoIcon::SkipBack,
                    "Restart",
                    "Restart (Home)",
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.transport(StageOperation::Seek { anchor: 0 }, window, cx)
                })),
            )
            .child(
                control(
                    "teleprompter-back",
                    KairoIcon::ChevronUp,
                    "Back one line",
                    "Back one line (\u{2190})",
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.transport(StageOperation::Scroll { lines: -1.0 }, window, cx)
                })),
            )
            .child(
                Button::new("teleprompter-play")
                    .large()
                    .icon(Icon::new(if self.playing {
                        KairoIcon::Pause
                    } else {
                        KairoIcon::Play
                    }))
                    .label(if self.playing { "Pause" } else { "Play" })
                    .tooltip("Play or pause (Space)")
                    .disabled(!usable)
                    .on_click(cx.listener(|this, _, window, cx| this.toggle_play(window, cx))),
            )
            .child(
                control(
                    "teleprompter-forward",
                    KairoIcon::ChevronDown,
                    "Forward one line",
                    "Forward one line (\u{2192})",
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.transport(StageOperation::Scroll { lines: 1.0 }, window, cx)
                })),
            )
            .child(div().w(px(1.)).h_5().mx_1().bg(cx.kairo().panel_border))
            .child(
                // Mirror for a beam-splitter rig: an option set once.
                Button::new("teleprompter-mirror")
                    .ghost()
                    .icon(Icon::new(KairoIcon::FlipHorizontal))
                    .toggled(self.layout.mirror)
                    .when(self.layout.mirror, |b| b.selected(true))
                    .accessibility_label("Mirror text")
                    .tooltip(if self.layout.mirror {
                        "Mirrored for a beam-splitter rig. Click to turn off"
                    } else {
                        "Mirror the text for a beam-splitter rig"
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        let next = !this.layout.mirror;
                        this.set_mirror(next, window, cx)
                    })),
            )
            .child(
                Button::new("teleprompter-keys")
                    .ghost()
                    .icon(Icon::new(KairoIcon::Keyboard))
                    .accessibility_label("Keyboard shortcuts")
                    .tooltip("Space play/pause \u{b7} \u{2191}\u{2193} speed \u{b7} \u{2190}\u{2192} one line \u{b7} Page Up/Down jump \u{b7} Home restart"),
            )
            .into_any_element()
    }

    fn display_options(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        // One line each: name, slider, value.
        let row = |name: &'static str, slider: Slider, value: String| {
            h_flex()
                .gap_3()
                .items_center()
                .child(
                    div()
                        .flex_none()
                        .w(rems(4.))
                        .text_size(text::XS.size)
                        .text_color(k.text(TextStep::T300))
                        .child(name),
                )
                .child(div().flex_1().min_w_0().child(slider))
                .child(
                    div()
                        .flex_none()
                        .w(rems(2.))
                        .text_right()
                        .text_size(text::XS.size)
                        .text_color(k.text(TextStep::T400))
                        .child(value),
                )
        };
        v_flex()
            .px_4()
            .pt_3()
            .pb_4()
            .gap_3()
            .child(row(
                "Speed",
                Slider::new(&self.speed_slider).accessibility_label("Scroll speed"),
                format!("{:.0}", speed_level(self.layout.speed)),
            ))
            .child(row(
                "Text size",
                Slider::new(&self.font_slider).accessibility_label("Text size"),
                format!("{:.0}", self.layout.font_px),
            ))
            .into_any_element()
    }
}
