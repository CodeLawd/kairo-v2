//! The booth toolbox under the monitor (`BoothToolbox.tsx`) and the program
//! panels it holds (`ProgramPanels.tsx`, `VideoLayoutCanvas.tsx`): Scripture,
//! Audio, Timers, Messages, Props and Video.
//!
//! Every panel acts on Kairo's own screens and NDI feeds. Presentation edits
//! show at once and are saved behind (`useDraftPresentation`).

use kairo_ui_kit::ControlSize;
use kairo_ui_kit::components::PopoverButtonTriggerExt;
use kairo_ui_kit::components::PopoverTriggerExt;
use std::time::Duration;

use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::color_picker::{ColorPicker, ColorPickerState};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{ContextMenuExt, DropdownMenu, PopupMenuItem};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Disableable, Icon, Selectable, Sizable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, AnyView, App, AppContext, Bounds, Context, DragMoveEvent, Entity, FontWeight, Hsla,
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Pixels, Render,
    Rgba, SharedString, StatefulInteractiveElement, Styled, Subscription, Task, Window, canvas,
    div, prelude::FluentBuilder, px, relative,
};
use kairo_domain::program::{MAX_PROGRAM_TIMERS, MAX_SAVED_MESSAGES};
use kairo_domain::program::{
    PresentationSettings, ProgramProp, ProgramSavedMessage, ProgramTimerDef, PropObject, Side,
    TimerKind, TopOrBottom, VideoBox, VideoLayout, VideoSettings, default_timer_theme,
    format_clock, idle_timer, next_time_of_day, screen_timer_def, timer_readout,
    timer_readout_object, timer_running,
};
use kairo_ui_kit::tokens::{Step, TextStep, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};

use super::queue::QueuePanel;
use super::text::{
    format_time_of_day, format_timer_padded, hex_or, parse_duration, read_time_of_day, time_of_day,
};
use super::widgets::{PressedExt, hint, icon_button, primary_button, secondary_button, section};
use super::{Booth, BoothCall, BoothEvent, OnScreensLayer, ToolTab, now_ms};

/// `MAX_SAVED_MESSAGE_LENGTH`.
pub const MAX_SAVED_MESSAGE_LENGTH: usize = 500;

/// Element ids, for tests.
pub mod ids {
    pub fn tab(id: &str) -> String {
        format!("toolbox-tab-{id}")
    }
    pub const NEW_TIMER: &str = "timers-new";
    pub const STAGE_INPUT: &str = "stage-message-input";
    pub const STAGE_SEND: &str = "stage-message-send";
    pub const STAGE_DOWN: &str = "stage-message-down";
    pub const CLOCK: &str = "clock-on-screens";
    pub const MESSAGE_SHOW: &str = "message-show";
    pub const MESSAGE_SAVE: &str = "message-save";
    pub const PROPS_EMPTY: &str = "props-empty";
    pub const PROPS_DOWN: &str = "props-take-all-down";
    pub const CAMERA_ACCESS: &str = "video-access";
    pub const CAMERA_LIVE: &str = "video-go-live";
    pub const CAMERA_OFF: &str = "video-take-off";
    pub fn timer_play(id: &str) -> String {
        format!("timer-play-{id}")
    }
    pub fn timer_reset(id: &str) -> String {
        format!("timer-reset-{id}")
    }
    pub fn timer_screens(id: &str) -> String {
        format!("timer-screens-{id}")
    }
    pub fn timer_toggle(id: &str) -> String {
        format!("timer-toggle-{id}")
    }
    pub fn timer_delete(id: &str) -> String {
        format!("timer-delete-{id}")
    }
    pub fn message(id: &str) -> String {
        format!("message-{id}")
    }
    pub fn prop(id: &str) -> String {
        format!("prop-{id}")
    }
    pub fn layout(layout: &str) -> String {
        format!("video-layout-{layout}")
    }
}

/// `#rrggbb` from a colour the picker returns.
pub fn hex_of(color: Hsla) -> String {
    let rgba = Rgba::from(color);
    let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", c(rgba.r), c(rgba.g), c(rgba.b))
}

/// A stored colour string as a colour, or the fallback.
pub fn color_of(value: &str, fallback: Hsla) -> Hsla {
    Rgba::try_from(value).map(Hsla::from).unwrap_or(fallback)
}

fn stamp36() -> String {
    // `Date.now().toString(36)`.
    let mut n = now_ms() as u64;
    let mut out = Vec::new();
    loop {
        out.push(std::char::from_digit((n % 36) as u32, 36).unwrap_or('0'));
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.iter().rev().collect()
}

// ─── The toolbox ─────────────────────────────────────────────────────────────

pub struct Toolbox {
    booth: Entity<Booth>,
    queue: Entity<QueuePanel>,
    timers: Entity<TimersPanel>,
    messages: Entity<MessagesPanel>,
    props: Entity<PropsPanel>,
    video: Entity<VideoPanel>,
    sound: Entity<SoundPanel>,
    /// The tracks list above program sound (prompt 084).
    tracks: Option<AnyView>,
    _subscriptions: Vec<Subscription>,
}

impl Toolbox {
    pub fn new(booth: Entity<Booth>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let queue = cx.new(|cx| QueuePanel::new(booth.clone(), window, cx));
        let timers = cx.new(|cx| TimersPanel::new(booth.clone(), window, cx));
        let messages = cx.new(|cx| MessagesPanel::new(booth.clone(), window, cx));
        let props = cx.new(|cx| PropsPanel::new(booth.clone(), window, cx));
        let video = cx.new(|cx| VideoPanel::new(booth.clone(), cx));
        let sound = cx.new(|cx| SoundPanel::new(booth.clone(), window, cx));
        let subscriptions = vec![
            cx.observe(&booth, |_, _, cx| cx.notify()),
            cx.subscribe_in(&booth, window, |this, _, event: &BoothEvent, window, cx| {
                match event {
                    BoothEvent::FocusSearch => {
                        let search = this.queue.read(cx).search().clone();
                        search.update(cx, |s, cx| s.focus(window, cx));
                    }
                    // `report`: a program request failed.
                    BoothEvent::Alert(message) => {
                        kairo_ui_kit::components::alert(message.clone(), window, cx)
                    }
                    _ => {}
                }
            }),
        ];
        Self {
            booth,
            queue,
            timers,
            messages,
            props,
            video,
            sound,
            tracks: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn queue(&self) -> &Entity<QueuePanel> {
        &self.queue
    }
    pub fn timers(&self) -> &Entity<TimersPanel> {
        &self.timers
    }
    pub fn messages(&self) -> &Entity<MessagesPanel> {
        &self.messages
    }

    pub fn set_tracks(&mut self, tracks: Option<AnyView>, cx: &mut Context<Self>) {
        self.tracks = tracks;
        cx.notify();
    }
}

impl Render for Toolbox {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = *cx.kairo();
        let active = self.booth.read(cx).tab;
        let tabs = h_flex()
            .id("toolbox-tabs")
            .role(gpui_kit::Role::TabList)
            .aria_label("Presentation tools")
            .flex_shrink_0()
            .items_stretch()
            .bg(k.neutral(Step::S900))
            .border_b_1()
            .border_color(k.surface_line)
            .children(ToolTab::ALL.into_iter().enumerate().map(|(i, tab)| {
                let selected = tab == active;
                let icon = match tab {
                    ToolTab::Search => KairoIcon::Bible,
                    ToolTab::Audio => KairoIcon::Music2,
                    ToolTab::Timers => KairoIcon::Clock,
                    ToolTab::Messages => KairoIcon::MessageSquare,
                    ToolTab::Show => KairoIcon::Layers,
                    ToolTab::Live => KairoIcon::Video,
                };
                v_flex()
                    .id(SharedString::from(ids::tab(tab.id())))
                    .test_support()
                    .role(gpui_kit::Role::Tab)
                    .aria_label(tab.label())
                    .aria_selected(selected)
                    .tooltip(super::widgets::tip(tab.label().to_owned()))
                    .h(px(44.))
                    .min_w_0()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap_0p5()
                    .relative()
                    .cursor_pointer()
                    .when(i > 0, |s| s.border_l_1())
                    .border_color(k.neutral(Step::S800))
                    .text_size(text::MICRO.size)
                    .font_weight(FontWeight::MEDIUM)
                    .map(|s| {
                        if selected {
                            s.bg(k.accent(Step::S500)).text_color(k.on_accent)
                        } else {
                            s.text_color(k.text(TextStep::T400))
                                .hover(|s| s.bg(k.neutral(Step::S800)).text_color(k.text_primary))
                        }
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.booth.update(cx, |b, cx| b.set_tab(tab, cx))
                    }))
                    .child(Icon::new(icon).size(px(14.)))
                    .child(div().max_w_full().truncate().px_0p5().child(tab.label()))
            }));
        let body: AnyElement = match active {
            ToolTab::Search => self.queue.clone().into_any_element(),
            ToolTab::Audio => div()
                .id("toolbox-audio")
                .size_full()
                .overflow_y_scroll()
                .children(self.tracks.clone())
                .child(self.sound.clone())
                .into_any_element(),
            ToolTab::Timers => self.timers.clone().into_any_element(),
            ToolTab::Messages => self.messages.clone().into_any_element(),
            ToolTab::Show => self.props.clone().into_any_element(),
            ToolTab::Live => self.video.clone().into_any_element(),
        };
        v_flex()
            .id("booth-toolbox")
            .min_h_0()
            .flex_1()
            .overflow_hidden()
            .bg(k.surface_rail)
            .child(tabs)
            .child(
                div()
                    .id("toolbox-panel")
                    .role(gpui_kit::Role::TabPanel)
                    .min_h_0()
                    .flex_1()
                    .overflow_hidden()
                    .child(body),
            )
    }
}

/// Mutates the presentation through the booth (shown now, saved behind).
fn patch_presentation(
    booth: &Entity<Booth>,
    cx: &mut App,
    patch: impl FnOnce(&mut PresentationSettings),
) {
    booth.update(cx, |b, cx| {
        let mut next = b.presentation.clone();
        patch(&mut next);
        b.set_presentation(next, cx);
    });
}

fn program(booth: &Entity<Booth>, call: BoothCall, cx: &mut App) {
    booth.update(cx, |b, cx| b.program_call(call, cx));
}

// ─── Timers ─────────────────────────────────────────────────────────────────

/// The fields of the timer being edited.
struct TimerEditor {
    id: String,
    name: Entity<InputState>,
    duration: Entity<InputState>,
    hour: Entity<InputState>,
    minute: Entity<InputState>,
    /// The duration field is being typed in (`durationDraft`).
    drafting: bool,
    _subscriptions: Vec<Subscription>,
}

pub struct TimersPanel {
    booth: Entity<Booth>,
    expanded: Option<TimerEditor>,
    stage: Entity<InputState>,
    _tick: Task<()>,
    _reset: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl TimersPanel {
    pub fn new(booth: Entity<Booth>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let stage = cx.new(|cx| InputState::new(window, cx).placeholder("Wrap up · 2 minutes"));
        let subscriptions = vec![
            cx.observe(&booth, |_, _, cx| cx.notify()),
            cx.subscribe_in(
                &stage,
                window,
                |this, _, event: &InputEvent, _, cx| match event {
                    InputEvent::PressEnter { .. } => this.send_stage(cx),
                    InputEvent::Change => cx.notify(),
                    _ => {}
                },
            ),
        ];
        // `useNow(250)`: one clock while something counts.
        let tick = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                let alive = this.update(cx, |this, cx| {
                    let booth = this.booth.read(cx);
                    let moving = booth.presentation.timers.iter().any(|def| {
                        def.kind == TimerKind::CountdownTo
                            || booth.program.timers.get(&def.id).is_some_and(timer_running)
                    });
                    if moving {
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        });
        Self {
            booth,
            expanded: None,
            stage,
            _tick: tick,
            _reset: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn expanded(&self) -> Option<&str> {
        self.expanded.as_ref().map(|e| e.id.as_str())
    }

    fn patch_timer(&self, id: &str, cx: &mut App, patch: impl FnOnce(&mut ProgramTimerDef)) {
        let id = id.to_owned();
        patch_presentation(&self.booth, cx, move |p| {
            if let Some(def) = p.timers.iter_mut().find(|t| t.id == id) {
                patch(def);
            }
        });
    }

    pub fn toggle(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.expanded.as_ref().is_some_and(|e| e.id == id) {
            self.expanded = None;
            cx.notify();
            return;
        }
        let Some(def) = self
            .booth
            .read(cx)
            .presentation
            .timers
            .iter()
            .find(|t| t.id == id)
            .cloned()
        else {
            return;
        };
        let name = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_value(def.name.clone(), window, cx);
            s
        });
        let duration = cx.new(|cx| {
            let mut s = InputState::new(window, cx).placeholder("00:05:00");
            s.set_value(format_timer_padded(def.duration_sec), window, cx);
            s
        });
        let (h12, minute, pm) = read_time_of_day(&def.target_time);
        let hour = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_value(h12.to_string(), window, cx);
            s
        });
        let minute_input = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_value(minute.to_string(), window, cx);
            s
        });
        let tid = def.id.clone();
        let subs = vec![
            cx.subscribe_in(&name, window, {
                let tid = tid.clone();
                move |this, input, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::Change) {
                        let value: String = input.read(cx).value().chars().take(80).collect();
                        this.patch_timer(&tid, cx, |d| d.name = value);
                    }
                }
            }),
            cx.subscribe_in(&duration, window, {
                let tid = tid.clone();
                move |this, input, event: &InputEvent, window, cx| match event {
                    InputEvent::Focus => {
                        if let Some(e) = this.expanded.as_mut() {
                            e.drafting = true;
                        }
                    }
                    InputEvent::Blur | InputEvent::PressEnter { .. } => {
                        let text = input.read(cx).value().to_string();
                        this.commit_duration(&tid, &text, window, cx)
                    }
                    _ => {}
                }
            }),
            cx.subscribe_in(&hour, window, {
                let tid = tid.clone();
                move |this, _, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                        this.write_time(&tid, None, cx)
                    }
                }
            }),
            cx.subscribe_in(&minute_input, window, {
                let tid = tid.clone();
                move |this, _, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                        this.write_time(&tid, None, cx)
                    }
                }
            }),
        ];
        let _ = pm;
        self.expanded = Some(TimerEditor {
            id: def.id,
            name,
            duration,
            hour,
            minute: minute_input,
            drafting: false,
            _subscriptions: subs,
        });
        cx.notify();
    }

    /// `commitDuration`: a valid, positive time replaces the duration; while
    /// stopped the timer is set to it at once.
    fn commit_duration(
        &mut self,
        id: &str,
        draft: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.expanded.as_mut() else {
            return;
        };
        if !editor.drafting {
            return;
        }
        editor.drafting = false;
        let field = editor.duration.clone();
        let next = parse_duration(draft).filter(|n| *n > 0.0);
        let running = self
            .booth
            .read(cx)
            .program
            .timers
            .get(id)
            .is_some_and(timer_running);
        match next {
            Some(seconds) => {
                self.patch_timer(id, cx, |d| d.duration_sec = seconds);
                if !running {
                    program(
                        &self.booth,
                        BoothCall::TimerSet {
                            seconds,
                            id: id.to_owned(),
                        },
                        cx,
                    );
                }
            }
            None => {
                let current = self
                    .booth
                    .read(cx)
                    .presentation
                    .timers
                    .iter()
                    .find(|t| t.id == id)
                    .map_or(0.0, |t| t.duration_sec);
                field.update(cx, |f, cx| {
                    f.set_value(format_timer_padded(current), window, cx)
                });
            }
        }
    }

    /// The time of day from the hour, minute and AM/PM fields.
    fn write_time(&mut self, id: &str, pm: Option<bool>, cx: &mut Context<Self>) {
        let Some(editor) = self.expanded.as_ref() else {
            return;
        };
        let current = self
            .booth
            .read(cx)
            .presentation
            .timers
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.target_time.clone())
            .unwrap_or_default();
        let (_, _, was_pm) = read_time_of_day(&current);
        let read = |input: &Entity<InputState>, fallback: i64, min: i64, max: i64| {
            let v = input
                .read(cx)
                .value()
                .trim()
                .parse::<f64>()
                .ok()
                .map(|v| v.trunc() as i64);
            v.filter(|v| *v != 0 || min == 0)
                .unwrap_or(fallback)
                .clamp(min, max) as u32
        };
        let hour = read(&editor.hour, 12, 1, 12);
        let minute = read(&editor.minute, 0, 0, 59);
        let target = time_of_day(hour, minute, pm.unwrap_or(was_pm));
        if target == current {
            return;
        }
        let running = self
            .booth
            .read(cx)
            .program
            .timers
            .get(id)
            .is_some_and(timer_running);
        self.patch_timer(id, cx, |d| d.target_time = target);
        if !running {
            self.reset_soon(id, cx);
        }
    }

    /// A countdown to a time is worked out on reset; let the save land first.
    fn reset_soon(&mut self, id: &str, cx: &mut Context<Self>) {
        let id = id.to_owned();
        self._reset = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(400))
                .await;
            let _ = this.update(cx, |this, cx| {
                program(&this.booth, BoothCall::TimerReset(id), cx)
            });
        }));
    }

    pub fn add_timer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.booth.read(cx).presentation.timers.len();
        if count >= MAX_PROGRAM_TIMERS {
            return;
        }
        let id = format!("timer-{}", stamp36());
        let def = ProgramTimerDef {
            id: id.clone(),
            name: format!("Timer {}", count + 1),
            overrun: true,
            theme: default_timer_theme("#5eead4", "#f87171"),
            ..ProgramTimerDef::default()
        };
        patch_presentation(&self.booth, cx, |p| p.timers.push(def));
        self.toggle(&id, window, cx);
    }

    pub fn remove_timer(&mut self, id: &str, cx: &mut Context<Self>) {
        let booth = self.booth.read(cx);
        let timers = booth.presentation.timers.clone();
        if timers.len() <= 1 {
            return;
        }
        let on_screens = screen_timer_def(&timers, booth.program.screen_timer_id.as_deref())
            .id
            .clone();
        let next: Vec<_> = timers.into_iter().filter(|t| t.id != id).collect();
        let first = next.first().map(|t| t.id.clone());
        patch_presentation(&self.booth, cx, |p| p.timers = next);
        // The timer on screens was deleted: screens move to the next one.
        if on_screens == id
            && let Some(first) = first
        {
            program(&self.booth, BoothCall::TimerShow(first), cx);
        }
        if self.expanded.as_ref().is_some_and(|e| e.id == id) {
            self.expanded = None;
        }
        cx.notify();
    }

    pub fn send_stage(&mut self, cx: &mut Context<Self>) {
        let draft = self.stage.read(cx).value().to_string();
        if draft.trim().is_empty() {
            return;
        }
        let field = self.stage.downgrade();
        self.booth.update(cx, |b, cx| {
            b.call(
                BoothCall::SetStageMessage(Some(draft)),
                cx,
                move |b, result, cx| match result {
                    Ok(_) => {
                        // Cleared on the next frame with a window (the field needs one).
                        let _ = field.update(cx, |_, cx| cx.notify());
                        b.clear_stage_draft = true;
                    }
                    Err(error) => cx.emit(BoothEvent::Alert(error)),
                },
            );
        });
    }

    fn row(&self, def: &ProgramTimerDef, can_remove: bool, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let booth = self.booth.read(cx);
        let run = booth.program.timers.get(&def.id).cloned();
        let running = run.as_ref().is_some_and(timer_running);
        let now = now_ms();
        let current = run
            .clone()
            .unwrap_or_else(|| idle_timer(def, now, |t, n| next_time_of_day(t, n, &chrono::Local)));
        let readout = timer_readout(&current, now, def.overrun);
        let started = running
            || run
                .as_ref()
                .is_some_and(|r| r.remaining_sec != r.duration_sec);
        let overrun = match timer_readout_object(def) {
            Some(PropObject::Timer { overrun_color, .. }) => color_of(overrun_color, k.danger_text),
            _ => k.danger_text,
        };
        let on_screens = screen_timer_def(
            &booth.presentation.timers,
            booth.program.screen_timer_id.as_deref(),
        )
        .id == def.id;
        let visible = booth.program.countdown_on_screens;
        let up = on_screens && visible;
        let expanded = self.expanded.as_ref().is_some_and(|e| e.id == def.id);
        let id = def.id.clone();
        let header = h_flex()
            .items_center()
            .gap_1p5()
            .px_2p5()
            .py_3()
            .child(
                Button::new(SharedString::from(ids::timer_toggle(&def.id)))
                    .cursor_pointer()
                    .ghost()
                    .xsmall()
                    .icon(
                        Icon::new(if expanded {
                            KairoIcon::ChevronDown
                        } else {
                            KairoIcon::ChevronRight
                        })
                        .size(px(14.)),
                    )
                    .accessibility_label(if expanded {
                        format!("Close {}", def.name)
                    } else {
                        format!("Edit {}", def.name)
                    })
                    .on_click(cx.listener({
                        let id = id.clone();
                        move |this, _, window, cx| this.toggle(&id, window, cx)
                    })),
            )
            .child(
                v_flex()
                    .id(SharedString::from(format!("timer-summary-{}", def.id)))
                    .min_w_0()
                    .flex_1()
                    .cursor_pointer()
                    .on_click(cx.listener({
                        let id = id.clone();
                        move |this, _, window, cx| this.toggle(&id, window, cx)
                    }))
                    .child(
                        div()
                            .truncate()
                            .text_size(text::SM.size)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(k.text_primary)
                            .child(def.name.clone()),
                    )
                    .child(
                        h_flex()
                            .id(SharedString::from(format!("timer-readout-{}", def.id)))
                            .test_support()
                            .mt_1()
                            .items_center()
                            .gap_1p5()
                            .font_family(text::MONO_FAMILY)
                            .text_size(px(19.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if readout.time_up {
                                overrun
                            } else if running {
                                k.white
                            } else {
                                k.text(TextStep::T300)
                            })
                            .child(
                                Icon::new(KairoIcon::Clock)
                                    .size(px(13.))
                                    .text_color(k.text(TextStep::T500)),
                            )
                            .child(if def.kind == TimerKind::CountdownTo && !started {
                                div()
                                    .font_family(text::FAMILY)
                                    .text_size(text::XS.size)
                                    .font_weight(FontWeight::NORMAL)
                                    .text_color(k.text(TextStep::T400))
                                    .child(format!(
                                        "Countdown to {}",
                                        format_time_of_day(&def.target_time)
                                    ))
                            } else {
                                div().child(format_timer_padded(readout.seconds))
                            }),
                    ),
            )
            .child(
                Button::new(SharedString::from(ids::timer_screens(&def.id)))
                    .cursor_pointer()
                    .custom(
                        ButtonCustomVariant::new(cx)
                            .color(if up {
                                k.tints.teal
                            } else {
                                gpui_kit::transparent_black()
                            })
                            .foreground(if on_screens {
                                k.accent(Step::S300)
                            } else {
                                k.text(TextStep::T500)
                            })
                            .hover(k.surface_tertiary)
                            .active(k.surface_tertiary),
                    )
                    // Solid fill: the custom variant paints its colour at 20%.
                    .bg(if up {
                        k.tints.teal
                    } else {
                        gpui_kit::transparent_black()
                    })
                    .small()
                    .icon(Icon::new(KairoIcon::MonitorPlay).size(px(15.)))
                    .selected(up)
                    .accessibility_label(if up {
                        format!("Take {} off screens", def.name)
                    } else {
                        format!("Show {} on screens", def.name)
                    })
                    .tooltip(if up {
                        "Take this timer off screens"
                    } else {
                        "Show this timer on screens"
                    })
                    .on_click(cx.listener({
                        let id = id.clone();
                        move |this, _, _, cx| {
                            // Clicking the live one takes it down; any other puts it up.
                            if up {
                                program(
                                    &this.booth,
                                    BoothCall::SetOnScreens {
                                        layer: OnScreensLayer::Countdown,
                                        on: false,
                                    },
                                    cx,
                                );
                                return;
                            }
                            if !on_screens {
                                program(&this.booth, BoothCall::TimerShow(id.clone()), cx);
                            }
                            program(
                                &this.booth,
                                BoothCall::SetOnScreens {
                                    layer: OnScreensLayer::Countdown,
                                    on: true,
                                },
                                cx,
                            );
                        }
                    })),
            )
            .child(
                icon_button(
                    SharedString::from(ids::timer_reset(&def.id)),
                    KairoIcon::SkipBack,
                    format!("Reset {}", def.name),
                    cx,
                )
                .small()
                .on_click(cx.listener({
                    let id = id.clone();
                    move |this, _, _, cx| {
                        program(&this.booth, BoothCall::TimerReset(id.clone()), cx)
                    }
                })),
            )
            .child(
                icon_button(
                    SharedString::from(ids::timer_play(&def.id)),
                    if running {
                        KairoIcon::Pause
                    } else {
                        KairoIcon::Play
                    },
                    if running {
                        format!("Pause {}", def.name)
                    } else {
                        format!("Start {}", def.name)
                    },
                    cx,
                )
                .small()
                .on_click(cx.listener({
                    let id = id.clone();
                    move |this, _, _, cx| {
                        let call = if running {
                            BoothCall::TimerPause(id.clone())
                        } else {
                            BoothCall::TimerStart(id.clone())
                        };
                        program(&this.booth, call, cx)
                    }
                })),
            );
        let editor = self.expanded.as_ref().filter(|e| e.id == def.id).map(|e| {
            let label = |copy: &str| {
                div()
                    .w(px(52.))
                    .flex_shrink_0()
                    .text_color(k.text(TextStep::T400))
                    .child(copy.to_owned())
            };
            let kind_label = match def.kind {
                TimerKind::Countdown => "Countdown",
                TimerKind::CountdownTo => "Countdown to Time",
            };
            let panel = cx.entity().downgrade();
            let tid = def.id.clone();
            let (_, _, pm) = read_time_of_day(&def.target_time);
            v_flex()
                .gap_3()
                .px_4()
                .pb_4()
                .pt_1()
                .child(
                    Input::new(&e.name)
                        .kairo_size()
                        .small()
                        .aria_label("Timer name"),
                )
                .child(
                    v_flex()
                        .gap_2p5()
                        .text_size(text::XS.size)
                        .child(
                            h_flex().items_center().gap_3().child(label("Type")).child(
                                Button::new("timer-kind")
                                    .cursor_pointer()
                                    .outline()
                                    .small()
                                    .flex_1()
                                    .label(kind_label)
                                    .dropdown_caret(true)
                                    .disabled(running)
                                    .accessibility_label("Timer type")
                                    .popover_trigger()
                                    .dropdown_menu({
                                        let panel = panel.clone();
                                        let tid = tid.clone();
                                        move |mut menu, _, _| {
                                            for (kind, name) in [
                                                (TimerKind::Countdown, "Countdown"),
                                                (TimerKind::CountdownTo, "Countdown to Time"),
                                            ] {
                                                let panel = panel.clone();
                                                let tid = tid.clone();
                                                menu =
                                                    menu.item(PopupMenuItem::new(name).on_click(
                                                        move |_, _, cx| {
                                                            let _ = panel.update(cx, |this, cx| {
                                                                this.patch_timer(&tid, cx, |d| {
                                                                    d.kind = kind
                                                                });
                                                                this.reset_soon(&tid, cx);
                                                            });
                                                        },
                                                    ));
                                            }
                                            menu
                                        }
                                    }),
                            ),
                        )
                        .child(match def.kind {
                            TimerKind::Countdown => h_flex()
                                .items_center()
                                .gap_3()
                                .child(label("Time"))
                                .child(
                                    div().flex_1().child(
                                        Input::new(&e.duration)
                                            .kairo_size()
                                            .small()
                                            .aria_label("Duration"),
                                    ),
                                )
                                .into_any_element(),
                            TimerKind::CountdownTo => h_flex()
                                .items_center()
                                .gap_3()
                                .child(label("Time"))
                                .child(
                                    h_flex()
                                        .min_w_0()
                                        .flex_1()
                                        .items_center()
                                        .gap_1p5()
                                        .child(
                                            div().w_16().child(
                                                Input::new(&e.hour)
                                                    .kairo_size()
                                                    .small()
                                                    .aria_label("Hour"),
                                            ),
                                        )
                                        .child(div().text_color(k.text(TextStep::T500)).child(":"))
                                        .child(
                                            div().w_16().child(
                                                Input::new(&e.minute)
                                                    .kairo_size()
                                                    .small()
                                                    .aria_label("Minute"),
                                            ),
                                        )
                                        .child(
                                            Button::new("timer-ampm")
                                                .cursor_pointer()
                                                .outline()
                                                .small()
                                                .label(if pm { "PM" } else { "AM" })
                                                .dropdown_caret(true)
                                                .accessibility_label("AM or PM")
                                                .popover_trigger()
                                                .dropdown_menu({
                                                    let panel = panel.clone();
                                                    let tid = tid.clone();
                                                    move |mut menu, _, _| {
                                                        for (value, name) in
                                                            [(false, "AM"), (true, "PM")]
                                                        {
                                                            let panel = panel.clone();
                                                            let tid = tid.clone();
                                                            menu = menu.item(
                                                                PopupMenuItem::new(name).on_click(
                                                                    move |_, _, cx| {
                                                                        let _ = panel.update(
                                                                            cx,
                                                                            |this, cx| {
                                                                                this.write_time(
                                                                                    &tid,
                                                                                    Some(value),
                                                                                    cx,
                                                                                )
                                                                            },
                                                                        );
                                                                    },
                                                                ),
                                                            );
                                                        }
                                                        menu
                                                    }
                                                }),
                                        ),
                                )
                                .into_any_element(),
                        })
                        .child(
                            div().pl(px(64.)).child(
                                Checkbox::new(SharedString::from(format!(
                                    "timer-overrun-{}",
                                    def.id
                                )))
                                .label("Allows Overrun")
                                .checked(def.overrun)
                                .on_change(cx.listener({
                                    let tid = tid.clone();
                                    move |this, on: &bool, _, cx| {
                                        let on = *on;
                                        this.patch_timer(&tid, cx, |d| d.overrun = on)
                                    }
                                })),
                            ),
                        ),
                )
                .child(
                    h_flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("timer-edit-design")
                                .cursor_pointer()
                                .ghost()
                                .xsmall()
                                .icon(Icon::new(KairoIcon::Edit2).size(px(11.)))
                                .label("Edit")
                                .on_click(cx.listener({
                                    let tid = tid.clone();
                                    move |this, _, _, cx| {
                                        let select = Some(format!("timer:{tid}"));
                                        this.booth.update(cx, |b, cx| b.open_props(select, cx))
                                    }
                                })),
                        )
                        .when(can_remove, |this| {
                            this.child(
                                Button::new(SharedString::from(ids::timer_delete(&def.id)))
                                    .cursor_pointer()
                                    .ghost()
                                    .xsmall()
                                    .icon(
                                        Icon::new(KairoIcon::Trash2)
                                            .size(px(12.))
                                            .text_color(k.destructive),
                                    )
                                    .label("Delete")
                                    .on_click(cx.listener({
                                        let tid = tid.clone();
                                        move |this, _, _, cx| this.remove_timer(&tid, cx)
                                    })),
                            )
                        }),
                )
        });
        v_flex()
            .id(SharedString::from(format!("timer-{}", def.id)))
            .test_support()
            .when(expanded, |s| s.bg(k.surface_secondary))
            .when(!expanded, |s| {
                s.hover(|s| s.bg(k.surface_tertiary.opacity(0.5)))
            })
            .child(header)
            .children(editor)
            .into_any_element()
    }
}

impl Render for TimersPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = *cx.kairo();
        // A sent stage message empties the field.
        if self.booth.read(cx).clear_stage_draft {
            self.booth.update(cx, |b, _| b.clear_stage_draft = false);
            self.stage.update(cx, |s, cx| s.set_value("", window, cx));
        }
        let booth = self.booth.read(cx);
        let timers = booth.presentation.timers.clone();
        let clock_on = booth.program.clock_on_screens;
        let stage_message = booth.program.stage_message.clone();
        let screen_id = screen_timer_def(&timers, booth.program.screen_timer_id.as_deref())
            .id
            .clone();
        let can_remove = timers.len() > 1;
        let rows: Vec<AnyElement> = timers
            .iter()
            .map(|def| self.row(def, can_remove, cx))
            .collect();
        let draft_empty = self.stage.read(cx).value().trim().is_empty();
        let clock = section(
            "Clock",
            Some(
                Button::new(ids::CLOCK)
                    .cursor_pointer()
                    .custom(
                        ButtonCustomVariant::new(cx)
                            .color(if clock_on {
                                k.neutral(Step::S200)
                            } else {
                                k.surface_tertiary
                            })
                            .foreground(if clock_on {
                                k.surface
                            } else {
                                k.text(TextStep::T300)
                            })
                            .hover(k.surface_elevated)
                            .active(k.surface_elevated),
                    )
                    // Solid fill: the custom variant paints its colour at 20%.
                    .bg(if clock_on {
                        k.neutral(Step::S200)
                    } else {
                        k.surface_tertiary
                    })
                    .xsmall()
                    .rounded_full()
                    .label(if clock_on { "On screens" } else { "Show" })
                    .accessibility_label("Clock on screens")
                    .selected(clock_on)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        program(
                            &this.booth,
                            BoothCall::SetOnScreens {
                                layer: OnScreensLayer::Clock,
                                on: !clock_on,
                            },
                            cx,
                        )
                    }))
                    .into_any_element(),
            ),
            false,
            h_flex()
                .items_center()
                .justify_between()
                .gap_3()
                .text_size(text::XS.size)
                .text_color(k.text(TextStep::T300))
                .child("Time now")
                .child(
                    div()
                        .font_family(text::MONO_FAMILY)
                        .text_size(text::SM.size)
                        .child(format_clock(chrono::Local::now())),
                ),
            cx,
        );
        let stage = section(
            "Stage message",
            None,
            false,
            v_flex()
                .gap_2p5()
                .when_some(stage_message, |this, message| {
                    this.child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .rounded_md()
                            .bg(k.tints.red)
                            .px_2p5()
                            .py_1p5()
                            .text_size(text::XS.size)
                            .text_color(k.danger_text_soft)
                            .child(div().min_w_0().child(message))
                            .child(
                                icon_button(
                                    ids::STAGE_DOWN,
                                    KairoIcon::X,
                                    "Take the stage message down",
                                    cx,
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        program(&this.booth, BoothCall::SetStageMessage(None), cx)
                                    },
                                )),
                            ),
                    )
                })
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            div().id(ids::STAGE_INPUT).flex_1().child(
                                Input::new(&self.stage)
                                    .kairo_size()
                                    .small()
                                    .aria_label("Stage message"),
                            ),
                        )
                        .child(
                            primary_button(ids::STAGE_SEND, "Send", cx)
                                .icon(Icon::new(KairoIcon::Send).size(px(12.)))
                                .disabled(draft_empty)
                                .on_click(cx.listener(|this, _, _, cx| this.send_stage(cx))),
                        ),
                ),
            cx,
        );
        v_flex()
            .id("timers-panel")
            .size_full()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .px_3()
                    .pt_3()
                    .child(
                        h_flex()
                            .h_9()
                            .items_center()
                            .justify_between()
                            .px_1()
                            .child(
                                div()
                                    .text_size(text::LABEL.size)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(k.text(TextStep::T400))
                                    .child("TIMERS"),
                            )
                            .child(
                                icon_button(ids::NEW_TIMER, KairoIcon::Plus, "New timer", cx)
                                    .disabled(timers.len() >= MAX_PROGRAM_TIMERS)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.add_timer(window, cx)
                                    })),
                            ),
                    )
                    .child(
                        v_flex()
                            .overflow_hidden()
                            .rounded_xl()
                            .bg(k.surface)
                            .border_1()
                            .border_color(k.surface_border)
                            .children(rows.into_iter().enumerate().map(|(i, row)| {
                                div()
                                    .when(i > 0, |s| s.border_t_1().border_color(k.surface_border))
                                    .child(row)
                            })),
                    )
                    .child(
                        Button::new("timers-design")
                            .cursor_pointer()
                            .ghost()
                            .xsmall()
                            .mt_1p5()
                            .icon(Icon::new(KairoIcon::Layers).size(px(11.)))
                            .label("Edit the timer design")
                            .text_color(k.text(TextStep::T500))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let select = Some(format!("timer:{screen_id}"));
                                this.booth.update(cx, |b, cx| b.open_props(select, cx))
                            })),
                    ),
            )
            .child(clock)
            .child(stage.pb_3())
    }
}

// ─── Messages ───────────────────────────────────────────────────────────────

pub struct MessagesPanel {
    booth: Entity<Booth>,
    draft: Entity<TextareaState>,
    size: Entity<SliderState>,
    text_color: Entity<ColorPickerState>,
    bar_color: Entity<ColorPickerState>,
    _subscriptions: Vec<Subscription>,
}

impl MessagesPanel {
    pub fn new(booth: Entity<Booth>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let draft = cx.new(|cx| {
            let mut s = TextareaState::new(window, cx)
                .placeholder("Parents of child 42, please come to the nursery");
            s.set_auto_grow(2, 6, cx);
            s
        });
        let style = booth.read(cx).presentation.message.clone();
        let size = cx.new(|_| {
            SliderState::new()
                .min(16.)
                .max(160.)
                .step(1.)
                .default_value(style.font_size_px as f32)
        });
        let text_color = cx.new(|cx| {
            ColorPickerState::new(window, cx)
                .default_value(color_of(hex_or(&style.color, "#ffffff"), Hsla::white()))
        });
        let bar_color = cx.new(|cx| {
            ColorPickerState::new(window, cx).default_value(color_of(
                hex_or(&style.background, "#000000"),
                Hsla::black(),
            ))
        });
        let subscriptions = vec![
            cx.observe(&booth, |_, _, cx| cx.notify()),
            cx.subscribe_in(&draft, window, |this, _, event: &InputEvent, window, cx| {
                match event {
                    // Enter shows it; Shift+Enter starts a new line.
                    InputEvent::PressEnter { shift: false, .. } => this.send_draft(window, cx),
                    InputEvent::Change => cx.notify(),
                    _ => {}
                }
            }),
            cx.subscribe(&size, |this, _, event: &SliderEvent, cx| {
                let (SliderEvent::Change(value) | SliderEvent::Release(value)) = event;
                let px = value.start().round() as f64;
                patch_presentation(&this.booth, cx, |p| p.message.font_size_px = px);
            }),
            cx.subscribe(&text_color, |this, _, event, cx| {
                let gpui_kit::component::color_picker::ColorPickerEvent::Change(Some(color)) =
                    event
                else {
                    return;
                };
                let hex = hex_of(*color);
                patch_presentation(&this.booth, cx, |p| p.message.color = hex);
            }),
            cx.subscribe(&bar_color, |this, _, event, cx| {
                let gpui_kit::component::color_picker::ColorPickerEvent::Change(Some(color)) =
                    event
                else {
                    return;
                };
                let hex = hex_of(*color);
                patch_presentation(&this.booth, cx, |p| p.message.background = hex);
            }),
        ];
        Self {
            booth,
            draft,
            size,
            text_color,
            bar_color,
            _subscriptions: subscriptions,
        }
    }

    pub fn draft(&self, cx: &App) -> String {
        self.draft.read(cx).value().to_string()
    }

    /// Keeps `text` in the saved list (once), newest first.
    fn remember(&self, text: &str, cx: &mut App) {
        let trimmed = text.trim().to_owned();
        if trimmed.is_empty() {
            return;
        }
        patch_presentation(&self.booth, cx, |p| {
            if p.messages.iter().any(|m| m.text == trimmed) {
                return;
            }
            let message = ProgramSavedMessage {
                id: format!("msg-{}", stamp36()),
                text: trimmed.chars().take(MAX_SAVED_MESSAGE_LENGTH).collect(),
                extra: Default::default(),
            };
            p.messages.insert(0, message);
            p.messages.truncate(MAX_SAVED_MESSAGES);
        });
    }

    /// Showing a new message keeps it for next time.
    pub fn send_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let draft = self.draft(cx);
        if draft.trim().is_empty() {
            return;
        }
        self.remember(&draft, cx);
        let field = self.draft.downgrade();
        let handle = window.window_handle();
        self.booth.update(cx, |b, cx| {
            b.call(
                BoothCall::ShowMessage(draft),
                cx,
                move |_, result, cx| match result {
                    Ok(_) => {
                        let _ = handle.update(cx, |_, window, cx| {
                            let _ = field.update(cx, |f, cx| f.set_value("", window, cx));
                        });
                    }
                    Err(error) => cx.emit(BoothEvent::Alert(error)),
                },
            );
        });
    }

    pub fn save_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let draft = self.draft(cx);
        if draft.trim().is_empty() {
            return;
        }
        self.remember(&draft, cx);
        self.draft.update(cx, |f, cx| f.set_value("", window, cx));
    }

    fn row(
        &self,
        id: &str,
        body: &str,
        live: bool,
        saved: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let text_body = body.to_owned();
        let forget = id.to_owned();
        h_flex()
            .id(SharedString::from(ids::message(id)))
            .test_support()
            .group("message-row")
            .items_center()
            .gap_2()
            .when(live, |this| {
                this.child(
                    div()
                        .id("message-live-dot")
                        .aria_label("On screen")
                        .size(px(6.))
                        .flex_shrink_0()
                        .rounded_full()
                        .bg(k.live),
                )
            })
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .line_clamp(2)
                    .text_size(text::XS.size)
                    .text_color(if live {
                        k.white
                    } else {
                        k.text(TextStep::T300)
                    })
                    .child(body.to_owned()),
            )
            .when(saved && !live, |this| {
                this.child(
                    div()
                        .opacity(0.)
                        .group_hover("message-row", |s| s.opacity(1.))
                        .child(
                            icon_button(
                                SharedString::from(format!("message-delete-{id}")),
                                KairoIcon::Trash2,
                                "Delete message",
                                cx,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    let id = forget.clone();
                                    patch_presentation(&this.booth, cx, |p| {
                                        p.messages.retain(|m| m.id != id)
                                    })
                                },
                            )),
                        ),
                )
            })
            .child(if live {
                secondary_button(
                    SharedString::from(format!("message-down-{id}")),
                    "Take down",
                    cx,
                )
                .xsmall()
                .on_click(
                    cx.listener(|this, _, _, cx| program(&this.booth, BoothCall::ClearMessage, cx)),
                )
            } else {
                secondary_button(SharedString::from(format!("message-show-{id}")), "Show", cx)
                    .xsmall()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        program(&this.booth, BoothCall::ShowMessage(text_body.clone()), cx)
                    }))
            })
            .into_any_element()
    }

    fn style_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let style = self.booth.read(cx).presentation.message.clone();
        let booth = self.booth.clone();
        let size = self.size.clone();
        let text_color = self.text_color.clone();
        let bar_color = self.bar_color.clone();
        Popover::new("message-style")
            .button_trigger(icon_button(
                "message-style-trigger",
                KairoIcon::Settings,
                "Message style",
                cx,
            ))
            .content(move |_, _, _| {
                let row = || {
                    h_flex()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .py_2p5()
                        .text_size(text::XS.size)
                        .text_color(k.text(TextStep::T300))
                };
                let position = style.position;
                let seg = |label: &'static str, value: TopOrBottom, booth: Entity<Booth>| {
                    let on = position == value;
                    Button::new(SharedString::from(format!("message-position-{label}")))
                        .cursor_pointer()
                        .xsmall()
                        .when(on, |b| b.primary())
                        .when(!on, |b| b.ghost())
                        .label(label)
                        .on_click(move |_, _, cx| {
                            patch_presentation(&booth, cx, |p| p.message.position = value)
                        })
                };
                let ticker_booth = booth.clone();
                v_flex()
                    .w(px(256.))
                    .child(
                        div()
                            .text_size(text::XS.size)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(k.white)
                            .child("Message style"),
                    )
                    .child(
                        row().child("Position").child(
                            h_flex()
                                .gap_0p5()
                                .child(seg("Top", TopOrBottom::Top, booth.clone()))
                                .child(seg("Bottom", TopOrBottom::Bottom, booth.clone())),
                        ),
                    )
                    .child(
                        row().child("Size").child(
                            h_flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .w_24()
                                        .child(Slider::new(&size).accessibility_label("Size")),
                                )
                                .child(
                                    div()
                                        .w_8()
                                        .text_right()
                                        .text_size(text::LABEL.size)
                                        .text_color(k.text(TextStep::T400))
                                        .child(format!("{}", style.font_size_px as i64)),
                                ),
                        ),
                    )
                    .child(
                        row().child("Text").child(
                            ColorPicker::new(&text_color).accessibility_label("Text colour"),
                        ),
                    )
                    .child(
                        row()
                            .child("Bar")
                            .child(ColorPicker::new(&bar_color).accessibility_label("Bar colour")),
                    )
                    .child(
                        row().child("Scroll as a ticker").child(
                            Switch::new("message-ticker")
                                .checked(style.scroll)
                                .accessibility_label("Scroll as a ticker")
                                .on_change(move |on: &bool, _, cx| {
                                    let on = *on;
                                    patch_presentation(&ticker_booth, cx, |p| p.message.scroll = on)
                                }),
                        ),
                    )
            })
            .into_any_element()
    }
}

impl Render for MessagesPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = *cx.kairo();
        let booth = self.booth.read(cx);
        let saved = booth.presentation.messages.clone();
        let live = booth.program.message.clone();
        let live_unsaved = live
            .as_ref()
            .filter(|m| !saved.iter().any(|s| &s.text == *m))
            .cloned();
        let draft_empty = self.draft(cx).trim().is_empty();
        let mut rows: Vec<AnyElement> = Vec::new();
        if let Some(message) = &live_unsaved {
            rows.push(self.row("live", message, true, false, cx));
        }
        for m in &saved {
            rows.push(self.row(
                &m.id,
                &m.text,
                live.as_deref() == Some(m.text.as_str()),
                true,
                cx,
            ));
        }
        let empty = saved.is_empty() && live_unsaved.is_none();
        let style = self.style_button(cx);
        v_flex()
            .id("messages-panel")
            .size_full()
            .overflow_y_scroll()
            .child(section(
                "Messages",
                Some(style),
                false,
                v_flex()
                    .children(rows.into_iter().enumerate().map(|(i, row)| {
                        div()
                            .py_2p5()
                            .when(i > 0, |s| s.border_t_1().border_color(k.pref_line(true)))
                            .child(row)
                    }))
                    .when(empty, |this| {
                        this.child(
                            div()
                                .py_1()
                                .text_center()
                                .text_size(text::LABEL.size)
                                .text_color(k.text(TextStep::T500))
                                .child("No saved messages yet. Write one below."),
                        )
                    }),
                cx,
            ))
            .child(
                section(
                    "New message",
                    None,
                    false,
                    v_flex()
                        .gap_2p5()
                        .child(
                            div().id("message-draft").child(
                                Textarea::new(&self.draft).small().aria_label("New message"),
                            ),
                        )
                        .child(
                            h_flex()
                                .justify_end()
                                .gap_2()
                                .child(
                                    secondary_button(ids::MESSAGE_SAVE, "Save", cx)
                                        .disabled(draft_empty)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.save_draft(window, cx)
                                        })),
                                )
                                .child(
                                    primary_button(ids::MESSAGE_SHOW, "Show", cx)
                                        .icon(Icon::new(KairoIcon::Send).size(px(12.)))
                                        .disabled(draft_empty)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.send_draft(window, cx)
                                        })),
                                ),
                        ),
                    cx,
                )
                .pb_3(),
            )
    }
}

// ─── Props ──────────────────────────────────────────────────────────────────

pub struct PropsPanel {
    booth: Entity<Booth>,
    renaming: Option<(String, Entity<InputState>, Subscription)>,
    _subscriptions: Vec<Subscription>,
    clock: Option<Task<()>>,
}

impl PropsPanel {
    pub fn new(booth: Entity<Booth>, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![cx.observe(&booth, |this, _, cx| {
            this.sync_clock(cx);
            cx.notify();
        })];
        let mut this = Self {
            booth,
            renaming: None,
            _subscriptions: subscriptions,
            clock: None,
        };
        this.sync_clock(cx);
        this
    }

    fn sync_clock(&mut self, cx: &mut Context<Self>) {
        let has_timer = self
            .booth
            .read(cx)
            .presentation
            .props
            .iter()
            .flat_map(|prop| &prop.objects)
            .any(|o| matches!(o, PropObject::Timer { .. }));
        if !has_timer {
            self.clock = None;
        } else if self.clock.is_none() {
            self.clock = Some(cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(250))
                        .await;
                    if this.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                }
            }));
        }
    }

    fn open_editor(&self, select: Option<String>, cx: &mut App) {
        self.booth.update(cx, |b, cx| b.open_props(select, cx));
    }

    pub fn toggle(&mut self, id: &str, cx: &mut Context<Self>) {
        let booth = self.booth.read(cx);
        let Some(prop) = booth
            .presentation
            .props
            .iter()
            .find(|p| p.id == id)
            .cloned()
        else {
            return;
        };
        let on = booth.program.active_prop_ids.contains(&prop.id);
        let empty = prop.objects.is_empty() && prop.background.is_none();
        // An empty prop has nothing to show: clicking it opens it to edit.
        if empty && !on {
            self.open_editor(Some(prop.id), cx);
        } else {
            program(
                &self.booth,
                BoothCall::SetProp {
                    id: prop.id,
                    on: !on,
                },
                cx,
            );
        }
    }

    pub fn delete(&mut self, id: &str, cx: &mut Context<Self>) {
        if self
            .booth
            .read(cx)
            .program
            .active_prop_ids
            .iter()
            .any(|p| p == id)
        {
            program(
                &self.booth,
                BoothCall::SetProp {
                    id: id.to_owned(),
                    on: false,
                },
                cx,
            );
        }
        let id = id.to_owned();
        patch_presentation(&self.booth, cx, |p| p.props.retain(|prop| prop.id != id));
    }

    /// A copy just after the original, with fresh ids.
    pub fn duplicate(&mut self, id: &str, cx: &mut Context<Self>) {
        let id = id.to_owned();
        let stamp = stamp36();
        patch_presentation(&self.booth, cx, |p| {
            let Some(index) = p.props.iter().position(|prop| prop.id == id) else {
                return;
            };
            let mut copy: ProgramProp = p.props[index].clone();
            copy.id = format!("prop-{stamp}");
            copy.name = format!("{} copy", copy.name).chars().take(80).collect();
            // Fresh object ids (`${kind}-${stamp}-${i}`), through the stored shape.
            copy.objects = copy
                .objects
                .iter()
                .enumerate()
                .filter_map(|(i, object)| {
                    let mut value = serde_json::to_value(object).ok()?;
                    let kind = value
                        .get("kind")
                        .and_then(|k| k.as_str())
                        .unwrap_or("object")
                        .to_owned();
                    value["id"] = serde_json::Value::String(format!("{kind}-{stamp}-{i}"));
                    serde_json::from_value(value).ok()
                })
                .collect();
            p.props.insert(index + 1, copy);
        });
    }

    fn start_rename(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let name = self
            .booth
            .read(cx)
            .presentation
            .props
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        let input = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_value(name, window, cx);
            s
        });
        input.update(cx, |s, cx| s.focus(window, cx));
        let pid = id.to_owned();
        let sub = cx.subscribe_in(
            &input,
            window,
            move |this, input, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    let value = input.read(cx).value().to_string();
                    this.renaming = None;
                    let trimmed = value.trim().to_owned();
                    if !trimmed.is_empty() {
                        let pid = pid.clone();
                        patch_presentation(&this.booth, cx, |p| {
                            if let Some(prop) = p.props.iter_mut().find(|prop| prop.id == pid) {
                                prop.name = trimmed.chars().take(80).collect();
                            }
                        });
                    }
                    cx.notify();
                }
            },
        );
        self.renaming = Some((id.to_owned(), input, sub));
        cx.notify();
    }
}

impl Render for PropsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = *cx.kairo();
        let booth = self.booth.read(cx);
        let props = booth.presentation.props.clone();
        let active = booth.program.active_prop_ids.clone();
        if props.is_empty() {
            return v_flex()
                .id(ids::PROPS_EMPTY)
                .test_support()
                .size_full()
                .items_center()
                .justify_center()
                .gap_2p5()
                .px_6()
                .text_center()
                .child(
                    div()
                        .size_10()
                        .rounded_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(k.surface_tertiary)
                        .text_color(k.text(TextStep::T400))
                        .child(Icon::new(KairoIcon::Layers).size(px(18.))),
                )
                .child(
                    div()
                        .text_size(text::SM.size)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(k.white)
                        .child("No props yet"),
                )
                .child(
                    div()
                        .max_w(px(272.))
                        .text_size(text::LABEL.size)
                        .text_color(k.text(TextStep::T500))
                        .child("A prop sits over the slides — a logo in the corner, a welcome banner, a giving card."),
                )
                .child(
                    primary_button("props-create", "Create a prop", cx)
                        .mt_1()
                        .icon(Icon::new(KairoIcon::Plus).size(px(12.)))
                        .on_click(cx.listener(|this, _, _, cx| this.open_editor(None, cx))),
                )
                .into_any_element();
        }
        let timer_view = {
            let booth = self.booth.read(cx);
            kairo_domain::program::prop_timer_view_for(
                booth.program(),
                booth.presentation(),
                kairo_domain::js::now_ms(),
                |t, n| kairo_domain::program::next_time_of_day(t, n, &chrono::Local),
            )
        };
        let tiles = props.iter().map(|prop| {
            let on = active.contains(&prop.id);
            let empty = prop.objects.is_empty() && prop.background.is_none();
            let renaming = self
                .renaming
                .as_ref()
                .filter(|(id, _, _)| id == &prop.id)
                .map(|(_, input, _)| input.clone());
            let id = prop.id.clone();
            let menu_entity = cx.entity().downgrade();
            let pid = prop.id.clone();
            let tile = v_flex()
                .id(SharedString::from(ids::prop(&prop.id)))
                .test_support()
                .role(gpui_kit::Role::Button)
                .aria_toggled_bool(on)
                .tooltip(super::widgets::tip(if empty && !on {
                    "Empty — click to add something".to_owned()
                } else if on {
                    "Take down".to_owned()
                } else {
                    "Show".to_owned()
                }))
                .w_full()
                .cursor_pointer()
                .when(empty && !on, |s| s.opacity(0.6))
                .on_click(cx.listener(move |this, _, _, cx| this.toggle(&id, cx)))
                .child(
                    // The thumbnail: the prop as the props editor draws it.
                    div()
                        .w(px(116.))
                        .h(px(67.))
                        .mx_auto()
                        .rounded_md()
                        .overflow_hidden()
                        .border_2()
                        .border_color(if on {
                            k.live
                        } else {
                            gpui_kit::transparent_black()
                        })
                        .child(crate::props::prop_picture(
                            prop,
                            &timer_view,
                            112.0,
                            4.0,
                            cx,
                        )),
                )
                .when(renaming.is_none(), |this| {
                    this.child(
                        div()
                            .mt_1p5()
                            .truncate()
                            .text_center()
                            .text_size(text::LABEL.size)
                            .font_weight(if on {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_color(if on { k.white } else { k.text(TextStep::T300) })
                            .child(prop.name.clone()),
                    )
                })
                .context_menu(move |menu, _, _| {
                    let item = |label: &str, f: PropAction| {
                        let entity = menu_entity.clone();
                        PopupMenuItem::new(label.to_owned()).on_click(move |_, window, cx| {
                            let _ = entity.update(cx, |this, cx| f(this, window, cx));
                        })
                    };
                    let (a, b, c, d) = (pid.clone(), pid.clone(), pid.clone(), pid.clone());
                    let e = pid.clone();
                    menu.item(item(
                        if on { "Take down" } else { "Show" },
                        Box::new(move |this, _, cx| {
                            program(
                                &this.booth,
                                BoothCall::SetProp {
                                    id: a.clone(),
                                    on: !on,
                                },
                                cx,
                            )
                        }),
                    ))
                    .item(item(
                        "Edit…",
                        Box::new(move |this, _, cx| this.open_editor(Some(b.clone()), cx)),
                    ))
                    .item(item(
                        "Rename",
                        Box::new(move |this, window, cx| this.start_rename(&c, window, cx)),
                    ))
                    .item(item(
                        "Duplicate",
                        Box::new(move |this, _, cx| this.duplicate(&d, cx)),
                    ))
                    .item(item(
                        "Delete",
                        Box::new(move |this, _, cx| this.delete(&e, cx)),
                    ))
                });
            v_flex()
                .min_w_0()
                .w(px(112.))
                .flex_1()
                .child(tile)
                .when_some(renaming, |this, input| {
                    this.child(
                        div()
                            .mt_1()
                            .child(Input::new(&input).kairo_size().xsmall().aria_label("Name")),
                    )
                })
        });
        v_flex()
            .id("props-panel")
            .size_full()
            .overflow_y_scroll()
            .px_3()
            .pb_3()
            .pt_3()
            .child(
                h_flex()
                    .min_h_7()
                    .items_center()
                    .gap_1()
                    .px_1()
                    .pb_2()
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .text_size(text::XS.size)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(k.text(TextStep::T200))
                            .child("Props"),
                    )
                    .when(active.len() > 1, |this| {
                        this.child(
                            Button::new(ids::PROPS_DOWN)
                                .cursor_pointer()
                                .ghost()
                                .xsmall()
                                .label("Take all down")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    program(&this.booth, BoothCall::ClearProps, cx)
                                })),
                        )
                    })
                    .child(
                        icon_button("props-edit", KairoIcon::Edit2, "Edit props", cx)
                            .on_click(cx.listener(|this, _, _, cx| this.open_editor(None, cx))),
                    )
                    .child(
                        icon_button("props-new", KairoIcon::Plus, "New prop", cx)
                            .on_click(cx.listener(|this, _, _, cx| this.open_editor(None, cx))),
                    ),
            )
            .child(h_flex().flex_wrap().gap_3().children(tiles))
            .into_any_element()
    }
}

/// One prop context-menu action.
type PropAction = Box<dyn Fn(&mut PropsPanel, &mut Window, &mut Context<PropsPanel>)>;

// ─── Video input ────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DragMode {
    Move,
    Resize,
    Divider,
}

/// The drag on the layout canvas.
#[derive(Clone, Copy)]
struct LayoutDrag;
impl Render for LayoutDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

const MIN_BOX: f64 = 10.0;
const SNAP_PCT: f64 = 2.0;

/// Pulls a value to the nearest target when close enough.
pub fn snap(value: f64, targets: &[f64]) -> f64 {
    let mut best = value;
    let mut distance = SNAP_PCT;
    for target in targets {
        let next = (value - target).abs();
        if next < distance {
            distance = next;
            best = *target;
        }
    }
    best
}

/// The new video settings for a drag moved by (`dx`, `dy`) percent, or at
/// `at_pct` across the stage for the divider.
pub fn drag_video(start: &VideoSettings, mode: u8, dx: f64, dy: f64, at_pct: f64) -> VideoSettings {
    let mut next = start.clone();
    let b = &start.frame;
    match mode {
        2 => {
            let share = if start.side == Side::Left {
                at_pct
            } else {
                100.0 - at_pct
            };
            next.split_pct = share.clamp(20.0, 80.0).round();
        }
        1 => {
            next.frame = VideoBox {
                width_pct: (b.width_pct + dx).clamp(MIN_BOX, 100.0 - b.x_pct).round(),
                height_pct: (b.height_pct + dy).clamp(MIN_BOX, 100.0 - b.y_pct).round(),
                ..b.clone()
            };
        }
        _ => {
            let (w, h) = (b.width_pct, b.height_pct);
            let x = snap(
                (b.x_pct + dx).clamp(0.0, 100.0 - w),
                &[2.0, (100.0 - w) / 2.0, 100.0 - w - 2.0],
            );
            let y = snap(
                (b.y_pct + dy).clamp(0.0, 100.0 - h),
                &[2.0, (100.0 - h) / 2.0, 100.0 - h - 2.0],
            );
            next.frame = VideoBox {
                x_pct: x.round(),
                y_pct: y.round(),
                ..b.clone()
            };
        }
    }
    next
}

pub struct VideoPanel {
    booth: Entity<Booth>,
    camera: String,
    camera_audio: String,
    stage: Bounds<Pixels>,
    drag: Option<(DragMode, gpui_kit::Point<Pixels>, VideoSettings)>,
    _subscriptions: Vec<Subscription>,
}

impl VideoPanel {
    pub fn new(booth: Entity<Booth>, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![cx.observe(&booth, |_, _, cx| cx.notify())];
        Self {
            booth,
            camera: String::new(),
            camera_audio: String::new(),
            stage: Bounds::default(),
            drag: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn pick_camera(&mut self, label: String, cx: &mut Context<Self>) {
        self.camera = label.clone();
        let program_state = self.booth.read(cx).program.clone();
        // Switching while live reopens the stream on the new input.
        if program_state.camera.is_some() && !label.is_empty() {
            let audio = program_state
                .camera_audio
                .clone()
                .or_else(|| (!self.camera_audio.is_empty()).then(|| self.camera_audio.clone()));
            program(
                &self.booth,
                BoothCall::SetCamera {
                    label: Some(label),
                    audio,
                },
                cx,
            );
        }
        cx.notify();
    }

    pub fn pick_audio(&mut self, label: String, cx: &mut Context<Self>) {
        self.camera_audio = label.clone();
        let live = self.booth.read(cx).program.camera.clone();
        if let Some(camera) = live {
            let audio = (!label.is_empty()).then_some(label);
            program(
                &self.booth,
                BoothCall::SetCamera {
                    label: Some(camera),
                    audio,
                },
                cx,
            );
        }
        cx.notify();
    }

    fn patch_video(&self, next: VideoSettings, cx: &mut App) {
        patch_presentation(&self.booth, cx, |p| p.video = next);
    }

    fn canvas(&self, video: &VideoSettings, live: bool, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let presets = [
            (VideoLayout::Full, "full", "Full"),
            (VideoLayout::Pip, "pip", "In a corner"),
            (VideoLayout::Split, "split", "Side by side"),
        ];
        let piece = |rounded: f32| {
            h_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_1()
                .overflow_hidden()
                .bg(k.neutral(Step::S900))
                .border_1()
                .border_color(k.white.opacity(0.2))
                .rounded(px(rounded))
                .text_size(px(9.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(k.text(TextStep::T300))
                .child(Icon::new(KairoIcon::Video).size(px(11.)))
                .child(if live { "Live video" } else { "Video" })
        };
        let begin = |mode: DragMode| {
            cx.listener(move |this: &mut Self, event: &MouseDownEvent, _, cx| {
                let video = this.booth.read(cx).presentation.video.clone();
                this.drag = Some((mode, event.position, video));
                cx.stop_propagation();
            })
        };
        let pct = |v: f64| relative((v / 100.0) as f32);
        let split_left = if video.side == Side::Left {
            video.split_pct
        } else {
            100.0 - video.split_pct
        };
        let stage = div()
            .id("video-layout-stage")
            .test_support()
            .relative()
            .w_full()
            // 16:9 like the screen it stands for (it was a short 150 px).
            .h(px({
                let w = f32::from(self.stage.size.width);
                if w > 0.0 {
                    (w * 9.0 / 16.0).round()
                } else {
                    220.0
                }
            }))
            .overflow_hidden()
            .rounded_lg()
            .bg(k.surface_secondary)
            .border_1()
            .border_color(k.surface_border)
            .child(
                canvas(
                    {
                        let this = cx.entity().downgrade();
                        move |bounds, window, cx| {
                            let resized = this
                                .update(cx, |this, _| {
                                    let resized = this.stage.size.width != bounds.size.width;
                                    this.stage = bounds;
                                    resized
                                })
                                .unwrap_or(false);
                            // The height follows the width: one more frame.
                            if resized {
                                let this = this.clone();
                                window.on_next_frame(move |_, cx| {
                                    let _ = this.update(cx, |_, cx| cx.notify());
                                });
                            }
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(div().absolute().inset_0().bg(k.neutral(Step::S800)))
            .child(
                div()
                    .absolute()
                    .top_1p5()
                    .when(
                        video.layout == VideoLayout::Split && video.side == Side::Left,
                        |s| s.right_2(),
                    )
                    .when(
                        !(video.layout == VideoLayout::Split && video.side == Side::Left),
                        |s| s.left_2(),
                    )
                    .text_size(px(9.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(k.text(TextStep::T500))
                    .child("PRESENTATION"),
            )
            .on_drag_move(
                cx.listener(|this, event: &DragMoveEvent<LayoutDrag>, _, cx| {
                    let Some((mode, origin, start)) = this.drag.clone() else {
                        return;
                    };
                    let b = this.stage;
                    let w = f32::from(b.size.width).max(1.0) as f64;
                    let h = f32::from(b.size.height).max(1.0) as f64;
                    let p = event.event.position;
                    let dx = f32::from(p.x - origin.x) as f64 / w * 100.0;
                    let dy = f32::from(p.y - origin.y) as f64 / h * 100.0;
                    let at = f32::from(p.x - b.origin.x) as f64 / w * 100.0;
                    let code = match mode {
                        DragMode::Move => 0,
                        DragMode::Resize => 1,
                        DragMode::Divider => 2,
                    };
                    let next = drag_video(&start, code, dx, dy, at);
                    this.patch_video(next, cx);
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.drag = None),
            )
            .map(|stage| match video.layout {
                VideoLayout::Full => stage.child(div().absolute().inset_0().child(piece(0.0))),
                VideoLayout::Pip => stage.child(
                    div()
                        .id("video-pip")
                        .absolute()
                        .left(pct(video.frame.x_pct))
                        .top(pct(video.frame.y_pct))
                        .w(pct(video.frame.width_pct))
                        .h(pct(video.frame.height_pct))
                        .cursor_move()
                        .on_drag(LayoutDrag, |d, _, _, cx| cx.new(|_| *d))
                        .on_mouse_down(MouseButton::Left, begin(DragMode::Move))
                        .child(piece(6.0))
                        .child(
                            div()
                                .id("video-pip-resize")
                                .absolute()
                                .bottom_0p5()
                                .right_0p5()
                                .size_3()
                                .rounded_sm()
                                .bg(k.white)
                                .cursor_nwse_resize()
                                .tooltip(super::widgets::tip("Drag to resize".to_owned()))
                                .on_drag(LayoutDrag, |d, _, _, cx| cx.new(|_| *d))
                                .on_mouse_down(MouseButton::Left, begin(DragMode::Resize)),
                        ),
                ),
                VideoLayout::Split => stage
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .w(pct(video.split_pct))
                            .when(video.side == Side::Left, |s| s.left_0())
                            .when(video.side == Side::Right, |s| s.right_0())
                            .child(piece(0.0)),
                    )
                    .child(
                        div()
                            .id("video-divider")
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left(pct(split_left))
                            .ml(px(-4.))
                            .w_2()
                            .cursor_col_resize()
                            .on_drag(LayoutDrag, |d, _, _, cx| cx.new(|_| *d))
                            .on_mouse_down(MouseButton::Left, begin(DragMode::Divider))
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .bottom_0()
                                    .left(px(3.5))
                                    .w(px(1.))
                                    .bg(k.white.opacity(0.7)),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .top(relative(0.5))
                                    .mt(px(-14.))
                                    .left_0()
                                    .h_7()
                                    .w_2()
                                    .rounded_full()
                                    .bg(k.white),
                            ),
                    ),
            });
        let help = match video.layout {
            VideoLayout::Full => "The video fills the frame; the words sit over it.",
            VideoLayout::Pip => {
                "Drag the video anywhere; pull its corner to resize. It snaps to the edges and centre."
            }
            VideoLayout::Split => "Drag the divider to change the split.",
        };
        let current = video.clone();
        v_flex()
            .gap_2p5()
            .child(
                h_flex()
                    .gap_2()
                    .children(presets.into_iter().map(|(layout, key, label)| {
                        let selected = video.layout == layout;
                        let base = current.clone();
                        v_flex()
                            .id(SharedString::from(ids::layout(key)))
                            .test_support()
                            .role(gpui_kit::Role::Button)
                            .aria_label(label)
                            .aria_toggled_bool(selected)
                            .flex_1()
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let mut next = base.clone();
                                next.layout = layout;
                                this.patch_video(next, cx)
                            }))
                            .child(
                                div()
                                    .relative()
                                    .w_full()
                                    .h(px(40.))
                                    .overflow_hidden()
                                    .rounded_md()
                                    .bg(k.surface_tertiary)
                                    .border(if selected { px(2.) } else { px(1.) })
                                    .border_color(if selected {
                                        k.accent(Step::S500)
                                    } else {
                                        k.surface_border
                                    })
                                    .map(|s| match layout {
                                        VideoLayout::Full => s.child(
                                            div().absolute().inset_0().bg(k.neutral(Step::S500)),
                                        ),
                                        VideoLayout::Pip => s.child(
                                            div()
                                                .absolute()
                                                .bottom(relative(0.12))
                                                .right(relative(0.10))
                                                .h(relative(0.38))
                                                .w(relative(0.44))
                                                .rounded(px(2.))
                                                .bg(k.neutral(Step::S300)),
                                        ),
                                        VideoLayout::Split => s.child(
                                            div()
                                                .absolute()
                                                .top_0()
                                                .bottom_0()
                                                .right_0()
                                                .w(relative(0.5))
                                                .bg(k.neutral(Step::S300)),
                                        ),
                                    }),
                            )
                            .child(
                                div()
                                    .mt_1()
                                    .truncate()
                                    .text_center()
                                    .text_size(text::MICRO.size)
                                    .font_weight(if selected {
                                        FontWeight::SEMIBOLD
                                    } else {
                                        FontWeight::NORMAL
                                    })
                                    .text_color(if selected {
                                        k.white
                                    } else {
                                        k.text(TextStep::T400)
                                    })
                                    .child(label),
                            )
                    })),
            )
            .child(stage)
            .child(
                h_flex()
                    .items_start()
                    .justify_between()
                    .gap_2()
                    .child(hint(help, cx))
                    .when(video.layout == VideoLayout::Split, |this| {
                        let base = current.clone();
                        this.child(
                            secondary_button("video-swap", "Swap sides", cx)
                                .xsmall()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let mut next = base.clone();
                                    next.side = if base.side == Side::Left {
                                        Side::Right
                                    } else {
                                        Side::Left
                                    };
                                    this.patch_video(next, cx)
                                })),
                        )
                    }),
            )
            .into_any_element()
    }
}

impl Render for VideoPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = *cx.kairo();
        let booth = self.booth.read(cx);
        let state = booth.program.clone();
        let cameras = booth.devices.video_inputs.clone();
        let mics = booth.devices.audio_inputs.clone();
        let video = booth.presentation.video.clone();
        let live_audio = if state.camera.is_some() {
            state.camera_audio.clone().unwrap_or_default()
        } else {
            self.camera_audio.clone()
        };
        let chosen = if self.camera.is_empty() {
            state.camera.clone().unwrap_or_default()
        } else {
            self.camera.clone()
        };
        let panel = cx.entity().downgrade();
        let status = h_flex()
            .items_center()
            .gap_2p5()
            .child(
                div()
                    .size_8()
                    .flex_shrink_0()
                    .rounded_md()
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(state.camera.is_some(), |s| {
                        s.bg(k.tints.live).text_color(k.live)
                    })
                    .when(state.camera.is_none(), |s| {
                        s.bg(k.surface_tertiary).text_color(k.text(TextStep::T400))
                    })
                    .child(Icon::new(KairoIcon::Video).size(px(15.))),
            )
            .child(
                v_flex()
                    .min_w_0()
                    .flex_1()
                    .child(
                        div()
                            .truncate()
                            .text_size(text::XS.size)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(k.text_primary)
                            .child(
                                state
                                    .camera
                                    .clone()
                                    .unwrap_or_else(|| "Nothing on screens".into()),
                            ),
                    )
                    .child(hint(
                        if state.camera.is_some() {
                            "Live on every screen"
                        } else {
                            "Choose an input and put it up"
                        },
                        cx,
                    )),
            )
            .when(state.camera.is_some(), |this| {
                this.child(
                    div()
                        .id("video-live-dot")
                        .aria_label("Live on screens")
                        .size_2()
                        .flex_shrink_0()
                        .rounded_full()
                        .bg(k.live),
                )
            });
        let picker =
            |id: &'static str, label: String, aria: &'static str, items: Vec<(String, String)>| {
                let panel = panel.clone();
                Button::new(id)
                    .cursor_pointer()
                    .outline()
                    .small()
                    .w_full()
                    .label(label)
                    .dropdown_caret(true)
                    .accessibility_label(aria)
                    .popover_trigger()
                    .dropdown_menu(move |mut menu, _, _| {
                        for (value, name) in &items {
                            let panel = panel.clone();
                            let value = value.clone();
                            menu = menu.item(PopupMenuItem::new(name.clone()).on_click(
                                move |_, _, cx| {
                                    let _ = panel.update(cx, |this, cx| {
                                        if id == "video-input" {
                                            this.pick_camera(value.clone(), cx)
                                        } else {
                                            this.pick_audio(value.clone(), cx)
                                        }
                                    });
                                },
                            ));
                        }
                        menu
                    })
            };
        let mut body = v_flex().gap_2p5().child(status);
        if cameras.is_empty() {
            body = body.child(
                secondary_button(ids::CAMERA_ACCESS, "Allow video access to list inputs", cx)
                    .w_full()
                    .on_click(cx.listener(|this, _, _, cx| {
                        program(&this.booth, BoothCall::RequestCameraAccess, cx)
                    })),
            );
        } else {
            let mut inputs = vec![(String::new(), "— choose an input —".to_owned())];
            inputs.extend(cameras.iter().map(|c| (c.clone(), c.clone())));
            let camera_label = if chosen.is_empty() {
                "— choose an input —".to_owned()
            } else {
                chosen.clone()
            };
            body = body.child(
                h_flex()
                    .gap_2()
                    .child(div().min_w_0().flex_1().child(picker(
                        "video-input",
                        camera_label,
                        "Video input",
                        inputs,
                    )))
                    .child(if state.camera.is_some() {
                        Button::new(ids::CAMERA_OFF)
                            .cursor_pointer()
                            .custom(
                                ButtonCustomVariant::new(cx)
                                    .color(k.live)
                                    .foreground(k.white)
                                    .hover(k.live.opacity(0.9))
                                    .active(k.live.opacity(0.9)),
                            )
                            // Solid fill: the custom variant paints its colour at 20%.
                            .bg(k.live)
                            .small()
                            .label("Take off")
                            .on_click(cx.listener(|this, _, _, cx| {
                                program(
                                    &this.booth,
                                    BoothCall::SetCamera {
                                        label: None,
                                        audio: None,
                                    },
                                    cx,
                                )
                            }))
                    } else {
                        let camera = self.camera.clone();
                        let audio = self.camera_audio.clone();
                        primary_button(ids::CAMERA_LIVE, "Go live", cx)
                            .icon(Icon::new(KairoIcon::Video).size(px(12.)))
                            .disabled(camera.is_empty())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                program(
                                    &this.booth,
                                    BoothCall::SetCamera {
                                        label: Some(camera.clone()),
                                        audio: (!audio.is_empty()).then(|| audio.clone()),
                                    },
                                    cx,
                                )
                            }))
                    }),
            );
            let mut sounds = vec![(String::new(), "No sound".to_owned())];
            sounds.extend(mics.iter().map(|m| (m.clone(), format!("Sound: {m}"))));
            let sound_label = if live_audio.is_empty() {
                "No sound".to_owned()
            } else {
                format!("Sound: {live_audio}")
            };
            body = body.child(picker(
                "video-input-sound",
                sound_label,
                "Video input sound",
                sounds,
            ));
        }
        if !live_audio.is_empty() {
            body = body.child(hint(
                "Plays through the sound output below. Keep the mic away from the room speakers to avoid feedback.",
                cx,
            ));
        }
        let canvas = self.canvas(&video, state.camera.is_some(), cx);
        v_flex()
            .id("video-panel")
            .size_full()
            .overflow_y_scroll()
            .bg(k.surface_rail)
            .child(section("Video input", None, true, body, cx))
            .child(
                div()
                    .mx_4()
                    .mt_2()
                    .border_t_1()
                    .border_color(k.white.opacity(0.08)),
            )
            .child(section("Video layout", None, true, canvas, cx))
    }
}

// ─── Program sound (on the Audio tab) ───────────────────────────────────────

pub struct SoundPanel {
    booth: Entity<Booth>,
    volume: Entity<SliderState>,
    _subscriptions: Vec<Subscription>,
}

impl SoundPanel {
    pub fn new(booth: Entity<Booth>, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let value = booth.read(cx).presentation.audio.volume as f32;
        let volume = cx.new(|_| {
            SliderState::new()
                .min(0.)
                .max(1.)
                .step(0.05)
                .default_value(value)
        });
        let subscriptions = vec![
            cx.observe(&booth, |_, _, cx| cx.notify()),
            cx.subscribe(&volume, |this, _, event: &SliderEvent, cx| {
                let (SliderEvent::Change(value) | SliderEvent::Release(value)) = event;
                let volume = value.start() as f64;
                patch_presentation(&this.booth, cx, |p| p.audio.volume = volume);
            }),
        ];
        Self {
            booth,
            volume,
            _subscriptions: subscriptions,
        }
    }
}

impl Render for SoundPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let booth = self.booth.read(cx);
        let audio = booth.presentation.audio.clone();
        let speakers = booth.devices.audio_outputs.clone();
        let live_camera_audio =
            booth.program.camera.is_some() && booth.program.camera_audio.is_some();
        let target = self.booth.clone();
        let output_label = if audio.output_label.is_empty() {
            "System default output".to_owned()
        } else {
            audio.output_label.clone()
        };
        let body = v_flex()
            .gap_2p5()
            .child(
                Checkbox::new("sound-enabled")
                    .label("Play sound from background videos")
                    .checked(audio.enabled)
                    .on_change(cx.listener(|this, on: &bool, _, cx| {
                        let on = *on;
                        patch_presentation(&this.booth, cx, |p| p.audio.enabled = on)
                    })),
            )
            .when(audio.enabled || live_camera_audio, |this| {
                this.child(
                    v_flex()
                        .gap_1p5()
                        .child(Slider::new(&self.volume).accessibility_label("Volume"))
                        .child(
                            Button::new("sound-output").cursor_pointer()
                                .outline()
                                .small()
                                .w_full()
                                .label(output_label)
                                .dropdown_caret(true)
                                .accessibility_label("Sound output")
                                .popover_trigger().dropdown_menu(move |mut menu, _, _| {
                                    let mut options = vec![(String::new(), "System default output".to_owned())];
                                    options.extend(speakers.iter().map(|s| (s.clone(), s.clone())));
                                    for (value, name) in options {
                                        let target = target.clone();
                                        menu = menu.item(PopupMenuItem::new(name).on_click(move |_, _, cx| {
                                            let value = value.clone();
                                            patch_presentation(&target, cx, |p| p.audio.output_label = value)
                                        }));
                                    }
                                    menu
                                }),
                        )
                        .child(hint(
                            "Video and input sound play from the first external display (else the NDI feed) only, so screens never echo each other.",
                            cx,
                        )),
                )
            });
        div()
            .id("sound-panel")
            .pb_3()
            .child(section("Program sound", None, false, body, cx))
    }
}
