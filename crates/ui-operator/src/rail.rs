//! The live rail (`LiveOutputRail.tsx`, `LiveOutputPreview.tsx`,
//! `LiveLayerStrip.tsx`, `LiveVideoControls.tsx`): the monitor of what one
//! output shows, the clear-layer strip under it, the background video
//! transport, the output picker, and the booth tools below.
//!
//! The monitor is the engine's own frame (prompt 066), so the HTML preview
//! renderers Electron needed (`PreviewBody`, `ProgramLayersPreview`) have no
//! counterpart: whatever the output draws, the monitor shows.

use kairo_ui_kit::components::PopoverTriggerExt;
use std::sync::Arc;

use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::resizable::resize_handle;
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::{Disableable, Icon, Sizable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, AnyView, App, AppContext, Axis, Context, DragMoveEvent, Entity, EventEmitter,
    FocusHandle, Global, InteractiveElement, IntoElement, KeyDownEvent, MouseButton,
    MouseDownEvent, ParentElement, Pixels, Render, SharedString, StatefulInteractiveElement,
    Styled, Subscription, Window, div, prelude::FluentBuilder, px,
};
use kairo_core_gpui::preview::LivePreview;
use kairo_domain::{
    media::MediaKind,
    overlay::OverlayOutput,
    overlay_outputs::{is_rendered_kind, preview_outputs},
};
use kairo_engine_proto::VideoTime;
use kairo_ui_kit::tokens::{Step, TextStep, radius, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};

use crate::backend::{LiveMedia, LiveSnapshot, OperatorBackend, OperatorCommand};
use crate::layers::{self, Layer};
use crate::layout::{self, KEYBOARD_STEP, PanelSide, UiState};
use crate::transport::{self, EMPTY_TIME, TransportKey};

/// The rail's width, shared by every page that shows it (`liveRailWidth` in
/// the app store), so a resize on one page lands on all of them.
pub struct RailWidth(pub f32);
impl Global for RailWidth {}

/// The rail width: loaded from `ui-state` on first use.
pub fn rail_width(ui_state: &dyn UiState, cx: &mut App) -> f32 {
    if !cx.has_global::<RailWidth>() {
        let width = layout::load_panel_width(ui_state, PanelSide::Right);
        cx.set_global(RailWidth(width));
    }
    cx.global::<RailWidth>().0
}

/// Sets and stores the shared width; every rail repaints.
pub fn set_rail_width(ui_state: &dyn UiState, width: f32, cx: &mut App) {
    if cx.has_global::<RailWidth>() && cx.global::<RailWidth>().0 == width {
        return;
    }
    cx.set_global(RailWidth(width));
    layout::save_panel_width(ui_state, PanelSide::Right, width);
    cx.refresh_windows();
}

/// What the rail asks its owner to do.
#[derive(Clone, Debug, PartialEq)]
pub enum RailEvent {
    /// "Turn on a screen": open the Screens window.
    OpenScreens,
}

/// The drag payload of the rail's edge.
#[derive(Clone, Copy)]
struct RailResize;
impl Render for RailResize {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// Which video the time belongs to: a new item or cue starts at zero.
type TimeKey = (String, u64);

pub struct LiveRail {
    backend: Arc<dyn OperatorBackend>,
    ui_state: Arc<dyn UiState>,
    live: LiveSnapshot,
    /// `operatorPreviewOutputId`; not persisted, as in Electron.
    selected: Option<String>,
    preview: Option<Entity<LivePreview>>,
    toolbox: Option<AnyView>,
    busy: bool,
    error: Option<SharedString>,
    time: VideoTime,
    time_key: Option<TimeKey>,
    /// The value under the pointer while scrubbing.
    scrub: Option<f64>,
    seek: Entity<SliderState>,
    /// The press point and width at the start of an edge drag.
    press: Option<(Pixels, f32)>,
    edge_focus: FocusHandle,
    transport_focus: FocusHandle,
    _seek_events: Subscription,
}

/// The scrubber's resolution: the slider runs 0 to this, the duration maps on.
const SEEK_STEPS: f32 = 1000.0;

impl EventEmitter<RailEvent> for LiveRail {}

impl LiveRail {
    pub fn new(
        backend: Arc<dyn OperatorBackend>,
        ui_state: Arc<dyn UiState>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        rail_width(&*ui_state, cx);
        let preview = backend
            .preview_hub()
            .map(|hub| cx.new(|cx| LivePreview::new(hub, cx)));
        let seek = cx.new(|_| SliderState::new().min(0.0).max(SEEK_STEPS).step(1.0));
        let seek_events =
            cx.subscribe(
                &seek,
                |this: &mut Self, _, event: &SliderEvent, cx| match event {
                    SliderEvent::Change(value) => {
                        let seconds = value.start() as f64 / SEEK_STEPS as f64 * this.time.duration;
                        this.scrub = Some(seconds);
                        this.seek_to(seconds, cx);
                    }
                    SliderEvent::Release(_) => {
                        this.scrub = None;
                        cx.notify();
                    }
                },
            );
        Self {
            backend,
            ui_state,
            live: LiveSnapshot::default(),
            selected: None,
            preview,
            toolbox: None,
            busy: false,
            error: None,
            time: EMPTY_TIME,
            time_key: None,
            scrub: None,
            seek,
            press: None,
            edge_focus: cx.focus_handle(),
            transport_focus: cx.focus_handle(),
            _seek_events: seek_events,
        }
    }

    /// The booth tools under the monitor (the toolbox, prompt 077 onward).
    pub fn set_toolbox(&mut self, toolbox: Option<AnyView>, cx: &mut Context<Self>) {
        self.toolbox = toolbox;
        cx.notify();
    }

    pub fn live(&self) -> &LiveSnapshot {
        &self.live
    }

    pub fn busy(&self) -> bool {
        self.busy
    }

    pub fn time(&self) -> VideoTime {
        self.time
    }

    /// The output the monitor shows, `None` for "Preview only".
    pub fn selected_output(&self) -> Option<&OverlayOutput> {
        let outputs = preview_outputs(&self.live.outputs);
        self.selected
            .as_deref()
            .and_then(|id| outputs.iter().find(|o| o.id == id))
            .or_else(|| outputs.iter().find(|o| o.enabled))
            .or_else(|| outputs.first())
            .copied()
    }

    pub fn select_output(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = Some(id);
        self.sync_preview(window, cx);
        cx.notify();
    }

    /// A new read of the live state.
    pub fn set_live(&mut self, live: LiveSnapshot, window: &mut Window, cx: &mut Context<Self>) {
        if self.live == live {
            return;
        }
        self.live = live;
        let key = self
            .live
            .live_media()
            .map(|m| (m.item.id.clone(), m.cue.to_bits()));
        if key != self.time_key {
            // `setMediaTime(EMPTY_TIME); setSeekTo(null)` on a new item or cue.
            self.time_key = key;
            self.time = EMPTY_TIME;
            self.scrub = None;
            self.sync_seek(window, cx);
        }
        self.sync_preview(window, cx);
        cx.notify();
    }

    /// The output whose video clock the transport follows, while it shows.
    pub fn transport_output(&self) -> Option<String> {
        let view = self.selected_output()?;
        (self.transport_media().is_some() && view.enabled).then(|| view.id.clone())
    }

    /// The engine's clock for the live video.
    pub fn set_time(
        &mut self,
        time: Option<VideoTime>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let time = time.unwrap_or(EMPTY_TIME);
        if self.time == time {
            return;
        }
        self.time = time;
        self.sync_seek(window, cx);
        cx.notify();
    }

    /// The live video, when the monitor's output shows backgrounds.
    fn transport_media(&self) -> Option<LiveMedia> {
        let view = self.selected_output()?;
        if !is_rendered_kind(view.kind) || !view.show.backgrounds || self.live.document_live {
            return None;
        }
        self.live
            .live_media()
            .filter(|m| m.item.kind == MediaKind::Video)
    }

    /// The engine surface the monitor reads: an enabled output only.
    fn preview_surface(&self) -> Option<String> {
        self.selected_output()
            .filter(|o| o.enabled)
            .map(|o| o.id.clone())
    }

    fn sync_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let surface = self.preview_surface();
        if let Some(preview) = self.preview.clone() {
            preview.update(cx, |preview, cx| preview.set_output(surface, window, cx));
        }
    }

    fn sync_seek(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.scrub.is_some() {
            return;
        }
        let value = transport::progress(self.time.current_time, &self.time) * SEEK_STEPS;
        self.seek
            .update(cx, |seek, cx| seek.set_value(value, window, cx));
    }

    // ── Requests ───────────────────────────────────────────────────────────

    /// Runs one request; the strip is disabled until it finishes. Failures
    /// are logged and shown under the monitor (`console.error` in Electron).
    fn run(&mut self, command: OperatorCommand, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        cx.notify();
        self.send(command, true, cx);
    }

    /// Sends without the busy gate (the transport is never gated).
    fn send(&mut self, command: OperatorCommand, gated: bool, cx: &mut Context<Self>) {
        let (tx, rx) = futures_channel::oneshot::channel();
        let label = format!("{command:?}");
        self.backend
            .run(command, Box::new(move |result| drop(tx.send(result))));
        cx.spawn(async move |this, cx| {
            let result = rx
                .await
                .unwrap_or_else(|_| Err("The request was dropped.".into()));
            let _ = this.update(cx, |this, cx| {
                if gated {
                    this.busy = false;
                }
                if let Err(message) = result {
                    tracing::error!(command = label, %message, "[Operator] request failed");
                    this.error = Some(message.into());
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn clear_layer(&mut self, layer: Layer, cx: &mut Context<Self>) {
        if layer.is_on(&self.live) {
            self.run(OperatorCommand::Clear(layer), cx);
        }
    }

    pub fn clear_all(&mut self, cx: &mut Context<Self>) {
        if let Some(others) = layers::clear_all_others(&self.live) {
            self.run(OperatorCommand::ClearAll { others }, cx);
        }
    }

    fn seek_to(&mut self, seconds: f64, cx: &mut Context<Self>) {
        let Some(media) = self.transport_media() else {
            return;
        };
        let next = transport::clamp_seek(seconds, &self.time, media.playback.start_seconds);
        self.time.current_time = next;
        self.time.ended = false;
        self.send(OperatorCommand::Seek(next), false, cx);
        cx.notify();
    }

    pub fn toggle_pause(&mut self, cx: &mut Context<Self>) {
        if let Some(media) = self.transport_media() {
            let paused = transport::toggle_request(media.paused, &self.time);
            self.send(OperatorCommand::SetPaused(paused), false, cx);
        }
    }

    pub fn skip(&mut self, delta: f64, cx: &mut Context<Self>) {
        self.seek_to(self.time.current_time + delta, cx);
    }

    pub fn restart(&mut self, cx: &mut Context<Self>) {
        self.seek_to(0.0, cx);
    }

    pub fn toggle_loop(&mut self, cx: &mut Context<Self>) {
        if let Some(media) = self.transport_media() {
            self.send(
                OperatorCommand::SetLoop {
                    item_id: media.item.id.clone(),
                    looped: !media.playback.looped,
                },
                false,
                cx,
            );
        }
    }

    fn resize_by(&mut self, delta: f32, cx: &mut Context<Self>) {
        let width = layout::resize_panel(PanelSide::Right, rail_width(&*self.ui_state, cx), delta);
        set_rail_width(&*self.ui_state, width, cx);
    }

    // ── Rendering ──────────────────────────────────────────────────────────

    fn strip(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let anything = !layers::lit(&self.live).is_empty();
        let mut row = h_flex()
            .id("clear-layers")
            .test_support()
            .aria_label("Clear layers")
            .h_9()
            .flex_shrink_0()
            .bg(k.neutral(Step::S900))
            .border_1()
            .border_color(k.white.opacity(0.08))
            .rounded_b(radius::md())
            .child(self.strip_button(
                "clear-all".into(),
                "Clear all".into(),
                KairoIcon::X,
                anything,
                true,
                false,
                cx.listener(|this, _, _, cx| this.clear_all(cx)),
                cx,
            ))
            .child(
                div()
                    .w(px(1.))
                    .h_full()
                    .flex_shrink_0()
                    .bg(k.neutral(Step::S700)),
            );
        for (index, layer) in Layer::ALL.into_iter().enumerate() {
            let on = layer.is_on(&self.live);
            row = row.child(self.strip_button(
                SharedString::from(format!("clear-{}", layer.id())),
                layer.button_label(on).into(),
                layer.icon(),
                on,
                false,
                index > 0,
                cx.listener(move |this, _, _, cx| this.clear_layer(layer, cx)),
                cx,
            ));
        }
        row.into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn strip_button(
        &self,
        id: SharedString,
        label: SharedString,
        icon: KairoIcon,
        on: bool,
        danger: bool,
        divided: bool,
        click: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static,
        cx: &App,
    ) -> AnyElement {
        let k = *cx.kairo();
        // A lit layer lifts a step with an accent icon; hover turns it red,
        // since clicking takes it down. Empty layers stay dim.
        let variant = if on {
            if danger {
                ButtonCustomVariant::new(cx)
                    .color(k.tints.red)
                    .foreground(k.danger_text_soft)
                    .hover(k.tints.rose)
                    .active(k.tints.rose)
            } else {
                ButtonCustomVariant::new(cx)
                    .color(k.accent(Step::S500).opacity(0.22))
                    .foreground(k.accent(Step::S200))
                    .hover(k.tints.red)
                    .active(k.tints.red)
            }
        } else {
            ButtonCustomVariant::new(cx)
                .foreground(k.neutral(Step::S600))
                .hover(k.neutral(Step::S900))
                .active(k.neutral(Step::S900))
        };
        // Kit paints a custom variant at 20%; a lit layer needs a solid fill.
        let fill = on.then(|| {
            if danger {
                k.tints.red
            } else {
                k.accent(Step::S500).opacity(0.22)
            }
        });
        div()
            .relative()
            .flex()
            .h_full()
            .min_w_0()
            .flex_1()
            .when(divided, |this| {
                this.border_l_1().border_color(k.neutral(Step::S800))
            })
            .child(
                Button::new(id)
                    .cursor_pointer()
                    .custom(variant)
                    .when_some(fill, |b, fill| b.bg(fill))
                    .icon(Icon::new(icon).size_4())
                    .rounded(px(0.))
                    .w_full()
                    .h_full()
                    .accessibility_label(label.clone())
                    .tooltip(label)
                    .disabled(!on || self.busy)
                    .on_click(click),
            )
            // A lit layer also carries an accent bar along its foot.
            .when(on && !danger, |this| {
                this.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .h(px(2.))
                        .bg(k.accent(Step::S400)),
                )
            })
            .into_any_element()
    }

    fn empty_frame(icon: KairoIcon, title: &str, body: &str, cx: &App) -> AnyElement {
        let k = *cx.kairo();
        let label = format!("{title}. {body}");
        // Drawn on the monitor, so the greys are fixed whites: the app's quiet
        // greys turn dark in light mode and vanish into the black frame.
        v_flex()
            .id("live-monitor-empty")
            .test_support()
            .aria_label(label)
            .size_full()
            .items_center()
            .justify_center()
            .px_3()
            .bg(k.monitor())
            .child(
                Icon::new(icon)
                    .size_5()
                    .mb_2()
                    .text_color(k.white.opacity(0.35)),
            )
            .child(
                div()
                    .text_size(text::XS.size)
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .text_color(k.white.opacity(0.75))
                    .child(title.to_string()),
            )
            .child(
                div()
                    .mt_0p5()
                    .text_size(text::MICRO.size)
                    .text_color(k.white.opacity(0.45))
                    .child(body.to_string()),
            )
            .into_any_element()
    }

    /// Something is being drawn on the program, beyond text and background:
    /// a countdown, clock, stage message, message, prop, logo, poll, question
    /// or web page (`drawn` plus `web` in Electron).
    fn program_drawn(&self) -> bool {
        let p = &self.live.program;
        p.message.is_some()
            || p.stage_message.is_some()
            || p.poll.is_some()
            || p.question.is_some()
            || !p.active_prop_ids.is_empty()
            || p.logo
            || p.web.is_some()
            || p.countdown_on_screens
            || p.clock_on_screens
    }

    fn monitor(&self, width: f32, height: f32, cx: &App) -> AnyElement {
        let body = match (&self.preview, self.preview_surface()) {
            (_, None) => Self::empty_frame(
                KairoIcon::MonitorOff,
                "No screens on",
                "Turn one on in Theme → Output",
                cx,
            ),
            (None, Some(_)) => Self::empty_frame(
                KairoIcon::MonitorOff,
                "Preview unavailable",
                "The screen engine is not running",
                cx,
            ),
            (Some(_), Some(_))
                if !self.live.has_text()
                    && !self.live.has_background()
                    && !self.program_drawn() =>
            {
                Self::empty_frame(
                    KairoIcon::BookOpenCheck,
                    "Nothing is live",
                    "Send a verse or click a background",
                    cx,
                )
            }
            (Some(preview), Some(_)) => preview.clone().into_any_element(),
        };
        div()
            .id("live-monitor")
            .test_support()
            .relative()
            .flex_shrink_0()
            .w(px(width))
            .h(px(height))
            .overflow_hidden()
            .child(body)
            .into_any_element()
    }

    fn transport(&self, media: &LiveMedia, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let shown = self.scrub.unwrap_or(self.time.current_time);
        let ready = self.time.duration > 0.0;
        let (now, total) = transport::clock_text(shown, &self.time);
        let playing = transport::playing(media.paused, &self.time);
        let looped = media.playback.looped;
        let transport_btn = |id: &'static str, label: String, icon: KairoIcon| {
            Button::new(id)
                .cursor_pointer()
                .ghost()
                .small()
                .icon(Icon::new(icon).size_3p5())
                .accessibility_label(label.clone())
                .tooltip(label)
                .text_color(k.text(TextStep::T300))
        };
        let loop_btn = Button::new("video-loop")
            .cursor_pointer()
            .xsmall()
            .icon(Icon::new(KairoIcon::Repeat).size_3p5())
            .toggled(looped)
            .accessibility_label(if looped {
                "Turn loop off"
            } else {
                "Turn loop on"
            })
            .tooltip(if looped { "Loop on" } else { "Loop off" })
            .when(looped, |b| {
                let tint = k.accent(Step::S500).opacity(0.18);
                b.custom(
                    ButtonCustomVariant::new(cx)
                        .color(tint)
                        .foreground(k.accent(Step::S400))
                        .hover(k.accent(Step::S500).opacity(0.26))
                        .active(k.accent(Step::S500).opacity(0.32)),
                )
                .bg(tint)
            })
            .when(!looped, |b| b.ghost().text_color(k.text(TextStep::T400)))
            .on_click(cx.listener(|this, _, _, cx| this.toggle_loop(cx)));
        let play_label = if playing { "Pause" } else { "Play" };
        v_flex()
            .id("live-video-controls")
            .test_support()
            .track_focus(&self.transport_focus)
            // A press anywhere in the bar gives it the keys (Space, k, arrows, Home).
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, window, cx| {
                    this.transport_focus.focus(window, cx)
                }),
            )
            .mx_3()
            .mt_2()
            .rounded(radius::md())
            .bg(k.surface_secondary)
            .border_1()
            .border_color(k.panel_border)
            .px_2p5()
            .py_2()
            .gap_1p5()
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                match transport::key_action(event.keystroke.key.as_str()) {
                    Some(TransportKey::TogglePause) => this.toggle_pause(cx),
                    Some(TransportKey::Skip(delta)) => this.skip(delta, cx),
                    Some(TransportKey::Restart) => this.restart(cx),
                    None => return,
                }
                cx.stop_propagation();
            }))
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_shrink_0()
                            .min_w(px(32.))
                            .text_size(text::MICRO.size)
                            .font_family(text::MONO_FAMILY)
                            .text_color(k.text(TextStep::T300))
                            .child(now),
                    )
                    .child(
                        div().flex_1().min_w_0().px_1().child(
                            Slider::new(&self.seek)
                                .disabled(!ready)
                                .accessibility_label("Seek"),
                        ),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .min_w(px(32.))
                            .text_right()
                            .text_size(text::MICRO.size)
                            .font_family(text::MONO_FAMILY)
                            .text_color(k.text(TextStep::T500))
                            .child(total),
                    ),
            )
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(
                        Button::new("video-restart")
                            .cursor_pointer()
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(KairoIcon::RotateCcw).size_3p5())
                            .accessibility_label("Restart")
                            .tooltip("Restart")
                            .text_color(k.text(TextStep::T400))
                            .on_click(cx.listener(|this, _, _, cx| this.restart(cx))),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap_1()
                            .child(
                                transport_btn(
                                    "video-back",
                                    format!("Skip back {} seconds", transport::SKIP_SECONDS),
                                    KairoIcon::SkipBack,
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| this.skip(-transport::SKIP_SECONDS, cx),
                                )),
                            )
                            .child(
                                Button::new("video-play")
                                    .cursor_pointer()
                                    .secondary()
                                    .small()
                                    .icon(
                                        Icon::new(if playing {
                                            KairoIcon::Pause
                                        } else {
                                            KairoIcon::Play
                                        })
                                        .size_3p5(),
                                    )
                                    .accessibility_label(play_label)
                                    .tooltip(play_label)
                                    .on_click(cx.listener(|this, _, _, cx| this.toggle_pause(cx))),
                            )
                            .child(
                                transport_btn(
                                    "video-forward",
                                    format!("Skip forward {} seconds", transport::SKIP_SECONDS),
                                    KairoIcon::SkipForward,
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| this.skip(transport::SKIP_SECONDS, cx),
                                )),
                            ),
                    )
                    .child(loop_btn),
            )
            .into_any_element()
    }

    fn picker(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let Some(selected) = self.selected_output().cloned() else {
            // Nothing is going to a screen yet: say so plainly, and offer the fix.
            return h_flex()
                .gap_2p5()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .text_size(text::XS.size)
                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                        .text_color(k.text(TextStep::T300))
                        .child("Preview only"),
                )
                .child(
                    Button::new("turn-on-screen")
                        .cursor_pointer()
                        .ghost()
                        .xsmall()
                        .label("Turn on a screen")
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(RailEvent::OpenScreens))),
                )
                .into_any_element();
        };
        let outputs: Vec<(String, String)> = preview_outputs(&self.live.outputs)
            .into_iter()
            .map(|o| {
                let off = if o.enabled { "" } else { " (off)" };
                (o.id.clone(), format!("{}{off}", o.name))
            })
            .collect();
        let rail = cx.entity();
        let selected_id = selected.id.clone();
        Button::new("preview-output")
            .cursor_pointer()
            .outline()
            .small()
            .w_full()
            .label(format!(
                "{}{}",
                selected.name,
                if selected.enabled { "" } else { " (off)" }
            ))
            .dropdown_caret(true)
            .accessibility_label("Preview output")
            .popover_trigger()
            .dropdown_menu(move |mut menu, _, _| {
                for (id, name) in &outputs {
                    let pick = rail.clone();
                    let id = id.clone();
                    menu = menu.item(
                        PopupMenuItem::new(name.clone())
                            .checked(id == selected_id)
                            .on_click(move |_, window, cx| {
                                let id = id.clone();
                                pick.update(cx, |rail, cx| rail.select_output(id, window, cx));
                            }),
                    );
                }
                menu
            })
            .into_any_element()
    }
}

impl Render for LiveRail {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = *cx.kairo();
        let width = rail_width(&*self.ui_state, cx);
        // The frame sits inside 12 px insets and takes the rail's full width.
        let preview_width = (width - 24.0).max(160.0);
        let preview_height = (preview_width * 9.0 / 16.0).round();
        let off = self.selected_output().is_some_and(|o| !o.enabled);
        let media = self.transport_media();

        h_flex()
            .id("live-rail")
            .relative()
            .h_full()
            .min_h_0()
            .flex_shrink_0()
            .border_l(kairo_ui_kit::tokens::metrics::panel_border_width())
            .border_color(k.panel_border)
            .child(
                // Drag or arrow keys resize; every page with the rail follows.
                div()
                    .id("live-rail-edge")
                    .test_support()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(-4.))
                    .w_2()
                    .track_focus(&self.edge_focus)
                    .tab_index(0)
                    .aria_label("Resize live output preview panel")
                    .tooltip(|window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(
                            "Drag to resize live output preview",
                        )
                        .build(window, cx)
                    })
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                        let delta = match event.keystroke.key.as_str() {
                            "left" => -KEYBOARD_STEP,
                            "right" => KEYBOARD_STEP,
                            _ => return,
                        };
                        this.resize_by(delta, cx);
                        cx.stop_propagation();
                    }))
                    .child(
                        resize_handle::<RailResize, RailResize>(
                            "live-rail-handle",
                            Axis::Horizontal,
                        )
                        .on_drag(RailResize, |drag, _, _, cx| cx.new(|_| *drag))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, event: &MouseDownEvent, window, cx| {
                                this.edge_focus.focus(window, cx);
                                this.press =
                                    Some((event.position.x, rail_width(&*this.ui_state, cx)));
                            }),
                        )
                        .on_drag_move(cx.listener(
                            |this, event: &DragMoveEvent<RailResize>, _, cx| {
                                let Some((origin, start)) = this.press else {
                                    return;
                                };
                                let delta = f32::from(event.event.position.x - origin);
                                let width = layout::resize_panel(PanelSide::Right, start, delta);
                                set_rail_width(&*this.ui_state, width, cx);
                            },
                        )),
                    ),
            )
            .child(
                v_flex()
                    .id("live-rail-body")
                    .w(px(width))
                    .h_full()
                    .min_h_0()
                    .overflow_hidden()
                    .bg(k.surface_rail)
                    .child(
                        v_flex()
                            .relative()
                            .child(kairo_ui_kit::coach::coach_anchor(
                                kairo_ui_kit::coach::anchors::LIVE_PREVIEW,
                            ))
                            .flex_shrink_0()
                            .border_b(kairo_ui_kit::tokens::metrics::panel_border_width())
                            .border_color(k.panel_border)
                            .child(
                                div()
                                    .px_3()
                                    .pb_1p5()
                                    .pt_2()
                                    .text_size(text::LABEL.size)
                                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                                    .text_color(k.text(TextStep::T400))
                                    .child("Live"),
                            )
                            .child(
                                div().px_3().child(
                                    v_flex()
                                        .overflow_hidden()
                                        .rounded(radius::md())
                                        .child(self.monitor(preview_width, preview_height, cx))
                                        .child(self.strip(cx)),
                                ),
                            )
                            .when_some(media.as_ref(), |this, media| {
                                this.child(self.transport(media, cx))
                            })
                            .child(
                                // A column, so the picker stretches to the
                                // monitor's width (Electron's `w-full`).
                                v_flex().px_3().pb_2().pt_2().child(self.picker(cx)),
                            )
                            .when(off, |this| {
                                this.child(
                                    div()
                                        .id("preview-off")
                                        .test_support()
                                        .aria_label("Preview only — this screen is off.")
                                        .px_3()
                                        .pb_2()
                                        .text_size(text::LABEL.size)
                                        .text_color(k.text(TextStep::T400))
                                        .child("Preview only — this screen is off."),
                                )
                            })
                            .when_some(self.error.clone(), |this, error| {
                                this.child(
                                    div()
                                        .id("live-rail-error")
                                        .test_support()
                                        .aria_label(error.clone())
                                        .px_3()
                                        .pb_2()
                                        .text_size(text::LABEL.size)
                                        .text_color(k.danger_text)
                                        .child(error),
                                )
                            }),
                    )
                    .child(
                        v_flex()
                            .min_h_0()
                            .flex_1()
                            .when_some(self.toolbox.clone(), |this, toolbox| this.child(toolbox)),
                    ),
            )
    }
}
