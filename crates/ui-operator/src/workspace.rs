//! The Operator workspace frame (`Operator.tsx` layout, `BoothWorkspace`):
//! the transcript panel on the left, the page content and media dock in the
//! middle, and the live rail on the right at full height.
//!
//! The panels' contents belong to later prompts (077 transcript, queue and
//! program panels; 083 media dock); they arrive through the `set_*` slots.

use std::{sync::Arc, time::Duration};

use gpui_kit::base::TestSupportExt;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::{
    AnyView, App, AppContext, Context, DragMoveEvent, Entity, EventEmitter, FocusHandle,
    InteractiveElement, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, ParentElement,
    Pixels, Render, StatefulInteractiveElement, Styled, Subscription, Task, Window, div,
    prelude::FluentBuilder, px,
};
use kairo_ui_kit::KairoThemeExt;

use crate::backend::OperatorBackend;
use crate::layout::{self, KEYBOARD_STEP, PanelSide, UiState};
use crate::rail::{LiveRail, RailEvent};

/// The one clock for the workspace: live state, the video readout and any
/// later countdowns all move on this tick (no per-control timers).
pub const TICK: Duration = Duration::from_millis(250);

#[derive(Clone, Debug, PartialEq)]
pub enum OperatorEvent {
    /// Open the Screens window ("Turn on a screen").
    OpenScreens,
}

#[derive(Clone, Copy)]
struct TranscriptResize;
impl Render for TranscriptResize {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

pub struct OperatorWorkspace {
    backend: Arc<dyn OperatorBackend>,
    ui_state: Arc<dyn UiState>,
    rail: Entity<LiveRail>,
    transcript_width: f32,
    transcript: Option<AnyView>,
    content: Option<AnyView>,
    dock: Option<AnyView>,
    /// A dialog over the whole window (the activation prompt).
    dialog: Option<AnyView>,
    press: Option<(Pixels, f32)>,
    edge_focus: FocusHandle,
    ticks: u64,
    _tick: Task<()>,
    _rail_events: Subscription,
}

impl EventEmitter<OperatorEvent> for OperatorWorkspace {}

impl OperatorWorkspace {
    pub fn new(
        backend: Arc<dyn OperatorBackend>,
        ui_state: Arc<dyn UiState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let rail = cx.new(|cx| LiveRail::new(backend.clone(), ui_state.clone(), window, cx));
        let rail_events = cx.subscribe(&rail, |_, _, event: &RailEvent, cx| match event {
            RailEvent::OpenScreens => cx.emit(OperatorEvent::OpenScreens),
        });
        let transcript_width = layout::load_panel_width(&*ui_state, PanelSide::Left);
        let tick = cx.spawn_in(window, async move |this, cx| {
            loop {
                let alive = this
                    .update_in(cx, |this, window, cx| this.tick(window, cx))
                    .is_ok();
                if !alive {
                    break;
                }
                cx.background_executor().timer(TICK).await;
            }
        });
        Self {
            backend,
            ui_state,
            rail,
            transcript_width,
            transcript: None,
            content: None,
            dock: None,
            dialog: None,
            press: None,
            edge_focus: cx.focus_handle(),
            ticks: 0,
            _tick: tick,
            _rail_events: rail_events,
        }
    }

    pub fn rail(&self) -> &Entity<LiveRail> {
        &self.rail
    }

    pub fn transcript_width(&self) -> f32 {
        self.transcript_width
    }

    /// How many times the clock has run.
    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    pub fn set_transcript(&mut self, view: Option<AnyView>, cx: &mut Context<Self>) {
        self.transcript = view;
        cx.notify();
    }

    pub fn set_content(&mut self, view: Option<AnyView>, cx: &mut Context<Self>) {
        self.content = view;
        cx.notify();
    }

    pub fn set_dialog(&mut self, view: Option<AnyView>, cx: &mut Context<Self>) {
        self.dialog = view;
        cx.notify();
    }

    pub fn dialog(&self) -> Option<&AnyView> {
        self.dialog.as_ref()
    }

    pub fn set_dock(&mut self, view: Option<AnyView>, cx: &mut Context<Self>) {
        self.dock = view;
        cx.notify();
    }

    /// Reads the live state now (after an action, so the strip follows at
    /// once instead of on the next tick).
    pub fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let live = self.backend.snapshot();
        self.rail
            .update(cx, |rail, cx| rail.set_live(live, window, cx));
    }

    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ticks += 1;
        self.refresh(window, cx);
        let Some(output) = self.rail.read(cx).transport_output() else {
            return;
        };
        let (tx, rx) = futures_channel::oneshot::channel();
        self.backend.video_time(
            &output,
            Box::new(move |time| {
                let _ = tx.send(time);
            }),
        );
        let rail = self.rail.downgrade();
        cx.spawn_in(window, async move |_, cx| {
            let time = rx.await.ok().flatten();
            let _ = rail.update_in(cx, |rail, window, cx| rail.set_time(time, window, cx));
        })
        .detach();
    }

    fn set_transcript_width(&mut self, width: f32, cx: &mut Context<Self>) {
        if self.transcript_width == width {
            return;
        }
        self.transcript_width = width;
        layout::save_panel_width(&*self.ui_state, PanelSide::Left, width);
        cx.notify();
    }

    /// The left panel's resize, by pointer delta from the press.
    pub fn resize_transcript(&mut self, delta: f32, cx: &mut Context<Self>) {
        let width = layout::resize_panel(PanelSide::Left, self.transcript_width, delta);
        self.set_transcript_width(width, cx);
    }
}

impl Render for OperatorWorkspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = *cx.kairo();
        let dialog = self
            .dialog
            .clone()
            .map(|view| crate::booth::widgets::window_layer(view, window));
        h_flex()
            .id("workspace-operator")
            .relative()
            .items_stretch()
            .size_full()
            .min_h_0()
            .overflow_hidden()
            .bg(k.surface)
            .child(
                v_flex()
                    .min_h_0()
                    .min_w_0()
                    .flex_1()
                    .overflow_hidden()
                    .child(
                        h_flex()
                            .items_stretch()
                            .min_h_0()
                            .flex_1()
                            .overflow_hidden()
                            .child(
                                div()
                                    .id("operator-transcript")
                                    .relative()
                                    .flex_shrink_0()
                                    .h_full()
                                    .w(px(self.transcript_width))
                                    .when_some(self.transcript.clone(), |this, view| {
                                        this.child(view)
                                    })
                                    .child(
                                        div()
                                            .id("operator-transcript-edge")
                                            .test_support()
                                            .absolute()
                                            .top_0()
                                            .bottom_0()
                                            .right_0()
                                            .w_2()
                                            .track_focus(&self.edge_focus)
                                            .tab_index(0)
                                            .aria_label("Resize transcript panel")
                                            .on_key_down(cx.listener(
                                                |this, event: &KeyDownEvent, _, cx| {
                                                    let delta = match event.keystroke.key.as_str() {
                                                        "left" => -KEYBOARD_STEP,
                                                        "right" => KEYBOARD_STEP,
                                                        _ => return,
                                                    };
                                                    this.resize_transcript(delta, cx);
                                                    cx.stop_propagation();
                                                },
                                            ))
                                            .child(
                                                div()
                                                    .id("operator-transcript-handle")
                                                    .size_full()
                                                    .cursor_col_resize()
                                                    .on_drag(TranscriptResize, |drag, _, _, cx| {
                                                        cx.new(|_| *drag)
                                                    })
                                                    .on_mouse_down(
                                                        MouseButton::Left,
                                                        cx.listener(
                                                            |this, event: &MouseDownEvent, window, cx| {
                                                                this.edge_focus.focus(window, cx);
                                                                this.press = Some((
                                                                    event.position.x,
                                                                    this.transcript_width,
                                                                ));
                                                            },
                                                        ),
                                                    )
                                                    .on_drag_move(cx.listener(
                                                        |this,
                                                         event: &DragMoveEvent<TranscriptResize>,
                                                         _,
                                                         cx| {
                                                            let Some((origin, start)) = this.press else {
                                                                return;
                                                            };
                                                            let delta =
                                                                f32::from(event.event.position.x - origin);
                                                            let width = layout::resize_panel(
                                                                PanelSide::Left,
                                                                start,
                                                                delta,
                                                            );
                                                            this.set_transcript_width(width, cx);
                                                        },
                                                    )),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .id("operator-content")
                                    .min_w_0()
                                    .flex_1()
                                    .h_full()
                                    .overflow_hidden()
                                    .when_some(self.content.clone(), |this, view| this.child(view)),
                            ),
                    )
                    .when_some(self.dock.clone(), |this, dock| this.child(dock)),
            )
            .child(self.rail.clone())
            .children(dialog)
    }
}

/// Lets an `App` reach the workspace's state in tests without a window.
pub fn transcript_width(workspace: &Entity<OperatorWorkspace>, cx: &App) -> f32 {
    workspace.read(cx).transcript_width
}
