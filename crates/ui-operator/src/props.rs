//! The Props editor (prompt 088), from `PropsEditor.tsx` and
//! `PropPreview.tsx`: the props and timers on the left, the selected one on
//! a 16:9 stage in the middle (add text, shapes and pictures, drag to place,
//! drag a corner to size), and what is selected on the right.
//!
//! A timer is designed as a prop-shaped picture: same stage, same objects,
//! same inspector, written to the timer's theme. Edits show at once and
//! reach settings 250 ms after they settle (`useDraftPresentation`); Save
//! writes the last one now and closes.
//!
//! The pictures are GPUI drawings of the prop's objects at the frame's
//! scale, as `renderPropHTML` lays them out. Every timer readout here is
//! driven by one 250 ms clock, which only asks for a frame while a timer
//! object is in view.

use kairo_ui_kit::ControlSize;
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Disableable, Icon, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyElement, App, AppContext, Bounds, Context, CursorStyle, Entity, EventEmitter, FocusHandle,
    Focusable, FontWeight, Hsla, InteractiveElement, IntoElement, KeyDownEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, ObjectFit, ParentElement, Pixels, Render, Role, SharedString,
    StatefulInteractiveElement, Styled, StyledImage, Subscription, Task, Window, canvas, div, img,
    point, px, relative, size,
};
use kairo_domain::js::{clamp, now_ms};
use kairo_domain::program::{
    MAX_PROGRAM_TIMERS, MAX_PROP_OBJECTS, PresentationSettings, ProgramProp, ProgramState,
    ProgramTimerDef, ProgramTimerState, PropAlign, PropFrame, PropImageFit, PropObject, PropShape,
    PropTimerView, default_timer_theme, format_timer, next_time_of_day, prop_timer_view_for,
    screen_timer_def, timer_readout, timer_view_for,
};
use kairo_domain::prop_geometry::{
    MIN_PROP_OBJECT_SIZE, PropBox, PropCorner, PropGesture, apply_prop_gesture,
};
use kairo_domain::serde_util::Extra;
use kairo_ui_kit::components::{
    MovableDialog, MovableDialogEvent, NumberField, NumberFieldChanged, NumberModel, SegmentOption,
    SegmentedControl,
};
use kairo_ui_kit::tokens::{TextStep, radius, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};

use crate::booth::Booth;
use crate::booth::backend::{BoothAnswer, BoothCall};

/// Overlays are laid out on a 1920 × 1080 frame.
pub const FRAME_W: f64 = 1920.0;
pub const FRAME_H: f64 = 1080.0;
/// `useDraftPresentation`'s pause before a write.
pub const SAVE_DELAY_MS: u64 = 250;
/// `usePropTimerTick`'s interval.
pub const TICK_MS: u64 = 250;
/// The window's size.
pub const WINDOW: (f32, f32) = (1180.0, 720.0);

pub mod ids {
    pub const ROOT: &str = "props-editor";
    pub const NEW_PROP: &str = "props-new";
    pub const NEW_TIMER: &str = "props-new-timer";
    pub const STAGE: &str = "props-stage";
    pub const EMPTY: &str = "props-empty";
    pub const EMPTY_NEW: &str = "props-empty-new";
    pub const STAGE_HINT: &str = "props-stage-hint";
    pub const SAVE: &str = "props-save";
    pub const NAME: &str = "props-name";
    pub const BACKGROUND: &str = "props-background";
    pub const REMOVE: &str = "props-remove";
    pub const NOTE: &str = "props-timer-note";
    pub const DELETE_OBJECT: &str = "props-object-delete";
    pub const TEXT: &str = "props-object-text";
    pub const BOLD: &str = "props-object-bold";
    pub const FORWARD: &str = "props-object-forward";
    pub const BACK: &str = "props-object-back";
    pub const CHANGE_IMAGE: &str = "props-object-image";
    pub fn tool(kind: &str) -> String {
        format!("props-add-{kind}")
    }
    pub fn prop(id: &str) -> String {
        format!("props-prop-{id}")
    }
    pub fn delete_prop(id: &str) -> String {
        format!("props-prop-delete-{id}")
    }
    pub fn timer(id: &str) -> String {
        format!("props-timer-{id}")
    }
    pub fn object(id: &str) -> String {
        format!("props-object-{id}")
    }
    pub fn list_object(id: &str) -> String {
        format!("props-list-{id}")
    }
    pub fn field(key: &str) -> String {
        format!("props-field-{key}")
    }
    pub fn align(key: &str) -> String {
        format!("props-align-{key}")
    }
    pub fn slide(key: &str) -> String {
        format!("props-slide-{key}")
    }
}

/// What the editor asks of the application.
#[derive(Clone, Debug, PartialEq)]
pub enum PropsEvent {
    /// Save or Escape: the window should go (the last edit is written).
    Close,
}

/// The four things the bar adds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Text,
    Rectangle,
    Ellipse,
    Image,
}

impl Tool {
    pub const ALL: [Tool; 4] = [Tool::Text, Tool::Rectangle, Tool::Ellipse, Tool::Image];
    pub fn key(self) -> &'static str {
        match self {
            Tool::Text => "text",
            Tool::Rectangle => "rectangle",
            Tool::Ellipse => "ellipse",
            Tool::Image => "image",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Tool::Text => "Text",
            Tool::Rectangle => "Rectangle",
            Tool::Ellipse => "Ellipse",
            Tool::Image => "Image",
        }
    }
    fn icon(self) -> KairoIcon {
        match self {
            Tool::Text => KairoIcon::TextT,
            Tool::Rectangle => KairoIcon::Square,
            Tool::Ellipse => KairoIcon::Circle,
            Tool::Image => KairoIcon::Image,
        }
    }
}

// ─── Pure rules ────────────────────────────────────────────────────────────

/// An object's place and opacity.
pub fn frame_of(o: &PropObject) -> &PropFrame {
    match o {
        PropObject::Text { frame, .. }
        | PropObject::Timer { frame, .. }
        | PropObject::Shape { frame, .. }
        | PropObject::Image { frame, .. } => frame,
    }
}

pub fn frame_mut(o: &mut PropObject) -> &mut PropFrame {
    match o {
        PropObject::Text { frame, .. }
        | PropObject::Timer { frame, .. }
        | PropObject::Shape { frame, .. }
        | PropObject::Image { frame, .. } => frame,
    }
}

/// `KIND_LABEL`.
pub fn kind_label(o: &PropObject) -> &'static str {
    match o {
        PropObject::Text { .. } => "Text",
        PropObject::Timer { .. } => "Timer",
        PropObject::Shape { .. } => "Shape",
        PropObject::Image { .. } => "Image",
    }
}

/// The Objects list's second column.
pub fn object_summary(o: &PropObject) -> String {
    match o {
        PropObject::Text { text, .. } if text.is_empty() => "Empty text".into(),
        PropObject::Text { text, .. } => text.clone(),
        PropObject::Timer { .. } => "Countdown".into(),
        PropObject::Image { media_path, .. } => file_name(media_path).into(),
        PropObject::Shape { shape, .. } => match shape {
            PropShape::Ellipse => "Ellipse".into(),
            PropShape::Rectangle => "Rectangle".into(),
        },
    }
}

pub fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// `hexOr`: a six-digit hex colour, or `fallback`.
pub fn hex_or<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    let ok = value.len() == 7
        && value.starts_with('#')
        && value[1..].chars().all(|c| c.is_ascii_hexdigit());
    if ok { value } else { fallback }
}

/// `newId(prefix)`.
pub fn new_id(prefix: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{:x}-{n:x}", now_ms() as u64)
}

fn frame(id: String, x: f64, y: f64, w: f64, h: f64) -> PropFrame {
    PropFrame {
        id,
        opacity: 1.0,
        x,
        y,
        w,
        h,
    }
}

/// The object a tool adds (`addObject`). An image needs its path.
pub fn new_object(tool: Tool, media_path: Option<String>) -> Option<PropObject> {
    Some(match tool {
        Tool::Text => PropObject::Text {
            frame: frame(new_id("text"), 0.1, 0.4, 0.8, 0.2),
            text: "Your text".into(),
            font_size_px: 96.0,
            color: "#ffffff".into(),
            bold: true,
            align: PropAlign::Center,
            extra: Extra::new(),
        },
        Tool::Image => PropObject::Image {
            frame: frame(new_id("image"), 0.35, 0.25, 0.3, 0.5),
            media_path: media_path?,
            fit: PropImageFit::Contain,
            extra: Extra::new(),
        },
        Tool::Rectangle | Tool::Ellipse => PropObject::Shape {
            frame: frame(new_id("shape"), 0.3, 0.3, 0.4, 0.4),
            shape: if tool == Tool::Ellipse {
                PropShape::Ellipse
            } else {
                PropShape::Rectangle
            },
            fill: "#315ede".into(),
            radius_px: if tool == Tool::Rectangle { 24.0 } else { 0.0 },
            extra: Extra::new(),
        },
    })
}

/// `addTimer`'s new timer.
pub fn new_timer(count: usize) -> ProgramTimerDef {
    ProgramTimerDef {
        id: format!("timer-{:x}", now_ms() as u64),
        name: format!("Timer {}", count + 1),
        overrun: true,
        theme: default_timer_theme("#5eead4", "#f87171"),
        ..ProgramTimerDef::default()
    }
}

/// `restack`: one step toward the front (+1) or the back (-1).
pub fn restack(objects: &[PropObject], id: &str, step: i32) -> Option<Vec<PropObject>> {
    let index = objects.iter().position(|o| frame_of(o).id == id)? as i32;
    let target = index + step;
    if target < 0 || target >= objects.len() as i32 {
        return None;
    }
    let mut next = objects.to_vec();
    next.swap(index as usize, target as usize);
    Some(next)
}

/// Arrow keys: 0.5 % of the frame, 5 % with Shift, kept on the frame.
pub fn nudge(f: &PropFrame, key: &str, shift: bool) -> Option<(f64, f64)> {
    let step = if shift { 0.05 } else { 0.005 };
    let (dx, dy) = match key {
        "left" => (-step, 0.0),
        "right" => (step, 0.0),
        "up" => (0.0, -step),
        "down" => (0.0, step),
        _ => return None,
    };
    Some((
        clamp(f.x + dx, 0.0, 1.0 - f.w),
        clamp(f.y + dy, 0.0, 1.0 - f.h),
    ))
}

/// A timer readout at `now`: text, colour and whether it is dimmed
/// (`updateTimerNode`).
pub fn readout(view: &PropTimerView, now: f64) -> (String, bool, bool) {
    let run = ProgramTimerState {
        duration_sec: view.duration_sec,
        ends_at: view.ends_at,
        remaining_sec: view.remaining_sec,
    };
    let r = timer_readout(&run, now, view.rollover);
    (format_timer(r.seconds), r.time_up, r.paused)
}

fn next_tod(target: &str, now: f64) -> f64 {
    next_time_of_day(target, now, &chrono::Local)
}

/// Where the stage's object is, in pixels of the stage.
fn place(f: &PropFrame, w: f32, h: f32) -> (f32, f32, f32, f32) {
    (
        (f.x * f64::from(w)) as f32,
        (f.y * f64::from(h)) as f32,
        (f.w * f64::from(w)) as f32,
        (f.h * f64::from(h)) as f32,
    )
}

// ─── The editor ────────────────────────────────────────────────────────────

/// A field kept between frames, with its subscription.
enum Field {
    Number(Entity<NumberField>),
    Color(Entity<ColorPickerState>),
    Slider(Entity<SliderState>),
}

pub struct PropsEditor {
    booth: Entity<Booth>,
    dialog: Entity<MovableDialog>,
    focus: FocusHandle,
    draft: Option<PresentationSettings>,
    save_task: Option<Task<()>>,
    saving: bool,
    selected_timer: Option<String>,
    selected_prop: Option<String>,
    selected_object: Option<String>,
    gesture: Option<PropGesture>,
    stage: Rc<Cell<Bounds<Pixels>>>,
    stage_area: Rc<Cell<Bounds<Pixels>>>,
    name: Entity<InputState>,
    text: Entity<TextareaState>,
    fields: HashMap<String, Field>,
    field_subs: Vec<Subscription>,
    origin: Option<String>,
    _tick: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<PropsEvent> for PropsEditor {}

impl Focusable for PropsEditor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl PropsEditor {
    /// Opens at `select`: a prop id or `timer:<id>`.
    pub fn new(
        booth: Entity<Booth>,
        select: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let dialog = cx.new(|cx| {
            MovableDialog::new(
                "props-window",
                "Props",
                size(px(WINDOW.0), px(WINDOW.1)),
                window,
                cx,
            )
        });
        let focus = cx.focus_handle();
        let name = cx.new(|cx| InputState::new(window, cx));
        let text = cx.new(|cx| TextareaState::new(window, cx));
        let subscriptions = vec![
            cx.observe(&booth, |_, _, cx| cx.notify()),
            cx.subscribe(&dialog, |this, _, _: &MovableDialogEvent, cx| {
                this.close(cx)
            }),
            cx.subscribe_in(&name, window, |this, input, e: &InputEvent, _, cx| {
                if matches!(e, InputEvent::Change) {
                    let value: String = input.read(cx).value().chars().take(80).collect();
                    this.rename(value, cx);
                }
            }),
            cx.subscribe_in(&text, window, |this, input, e: &InputEvent, _, cx| {
                if matches!(e, InputEvent::Change) {
                    let value = input.read(cx).value().to_string();
                    this.origin = Some("text".into());
                    this.patch_selected(
                        |o| {
                            if let PropObject::Text { text, .. } = o {
                                *text = value.clone();
                            }
                        },
                        cx,
                    );
                }
            }),
            // Whatever is open is written when the window goes.
            cx.on_release(|this, cx| {
                if let Some(next) = this.draft.take() {
                    this.booth.update(cx, |b, cx| b.set_presentation(next, cx));
                }
            }),
        ];
        // One clock for every readout on this window.
        let tick = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(TICK_MS))
                    .await;
                if this
                    .update(cx, |this, cx| {
                        if this.timer_in_view(cx) {
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        let mut editor = Self {
            booth,
            dialog,
            focus,
            draft: None,
            save_task: None,
            saving: false,
            selected_timer: None,
            selected_prop: None,
            selected_object: None,
            gesture: None,
            stage: Rc::default(),
            stage_area: Rc::default(),
            name,
            text,
            fields: HashMap::new(),
            field_subs: Vec::new(),
            origin: None,
            _tick: tick,
            _subscriptions: subscriptions,
        };
        editor.focus.focus(window, cx);
        editor.select(select, cx);
        let first = editor.presentation(cx).props.first().map(|p| p.id.clone());
        if editor.selected_timer.is_none() && editor.selected_prop.is_none() {
            editor.selected_prop = first;
        }
        editor
    }

    // ─── State ─────────────────────────────────────────────────────────────

    /// The presentation shown: the waiting edit, or what is stored.
    pub fn presentation<'a>(&'a self, cx: &'a App) -> &'a PresentationSettings {
        self.draft
            .as_ref()
            .unwrap_or_else(|| self.booth.read(cx).presentation())
    }

    fn program<'a>(&self, cx: &'a App) -> &'a ProgramState {
        self.booth.read(cx).program()
    }

    pub fn selected_timer(&self) -> Option<&str> {
        self.selected_timer.as_deref()
    }

    pub fn selected_object(&self) -> Option<&str> {
        self.selected_object.as_deref()
    }

    /// The selected design: a prop, or the selected timer drawn as one.
    pub fn current(&self, cx: &App) -> Option<ProgramProp> {
        let p = self.presentation(cx);
        if let Some(id) = &self.selected_timer
            && let Some(t) = p.timers.iter().find(|t| t.id == *id)
        {
            return Some(ProgramProp {
                id: format!("timer:{}", t.id),
                name: t.name.clone(),
                background: t.theme.background.clone(),
                objects: t.theme.objects.clone(),
                extra: Extra::new(),
            });
        }
        self.selected_prop
            .as_ref()
            .and_then(|id| p.props.iter().find(|x| x.id == *id))
            .or_else(|| p.props.first())
            .cloned()
    }

    pub fn object(&self, cx: &App) -> Option<PropObject> {
        let id = self.selected_object.as_ref()?;
        self.current(cx)?
            .objects
            .into_iter()
            .find(|o| frame_of(o).id == *id)
    }

    fn timer_in_view(&self, cx: &App) -> bool {
        let p = self.presentation(cx);
        p.props
            .iter()
            .flat_map(|x| &x.objects)
            .chain(p.timers.iter().flat_map(|t| &t.theme.objects))
            .any(|o| matches!(o, PropObject::Timer { .. }))
    }

    /// The readout a design shows: the selected timer's own run, or the
    /// timer on screens.
    fn view_for(&self, timer: Option<&ProgramTimerDef>, cx: &App) -> PropTimerView {
        let now = now_ms();
        let program = self.program(cx);
        match timer {
            Some(t) => timer_view_for(t, program.timers.get(&t.id), now, next_tod),
            None => prop_timer_view_for(program, self.presentation(cx), now, next_tod),
        }
    }

    /// The Timers panel's request to open at a prop or `timer:<id>`.
    pub fn select(&mut self, select: Option<String>, cx: &mut Context<Self>) {
        let Some(select) = select else {
            return;
        };
        let p = self.presentation(cx);
        if let Some(id) = select.strip_prefix("timer:") {
            if p.timers.iter().any(|t| t.id == id) {
                let id = id.to_owned();
                self.select_timer(&id, cx);
            }
            return;
        }
        if p.props.iter().any(|x| x.id == select) {
            self.select_prop(Some(select), cx);
        }
    }

    pub fn select_prop(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        self.selected_timer = None;
        self.selected_prop = id;
        self.select_object(None, cx);
    }

    pub fn select_timer(&mut self, id: &str, cx: &mut Context<Self>) {
        self.selected_timer = Some(id.to_owned());
        self.selected_prop = None;
        self.select_object(None, cx);
    }

    pub fn select_object(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        if self.selected_object != id {
            self.fields.clear();
            self.field_subs.clear();
            self.origin = None;
        }
        self.selected_object = id;
        cx.notify();
    }

    // ─── Writes ────────────────────────────────────────────────────────────

    /// Shows `next` at once and saves it after a pause.
    pub fn set_presentation(&mut self, next: PresentationSettings, cx: &mut Context<Self>) {
        self.draft = Some(next);
        self.save_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(SAVE_DELAY_MS))
                .await;
            let _ = this.update(cx, |this, cx| this.flush(cx));
        }));
        cx.notify();
    }

    /// Writes a waiting change now.
    pub fn flush(&mut self, cx: &mut Context<Self>) {
        self.save_task = None;
        if let Some(next) = self.draft.take() {
            self.booth.update(cx, |b, cx| b.set_presentation(next, cx));
        }
        cx.notify();
    }

    /// Save: the last edit now, then close.
    pub fn save(&mut self, cx: &mut Context<Self>) {
        self.saving = true;
        self.flush(cx);
        self.saving = false;
        cx.emit(PropsEvent::Close);
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.flush(cx);
        cx.emit(PropsEvent::Close);
    }

    /// Hands focus back to what had it before the window opened.
    pub fn restore_focus(&self, window: &mut Window, cx: &mut App) {
        let dialog = self.dialog.clone();
        dialog.update(cx, |d, cx| d.restore(window, cx));
    }

    /// `patchProp`: the selected prop, or the selected timer's theme.
    fn patch_design(&mut self, edit: impl FnOnce(&mut ProgramProp), cx: &mut Context<Self>) {
        let Some(mut design) = self.current(cx) else {
            return;
        };
        edit(&mut design);
        let mut next = self.presentation(cx).clone();
        if let Some(tid) = &self.selected_timer {
            if let Some(t) = next.timers.iter_mut().find(|t| t.id == *tid) {
                t.name = design.name;
                t.theme.background = design.background;
                t.theme.objects = design.objects;
            }
        } else if let Some(p) = next.props.iter_mut().find(|p| p.id == design.id) {
            *p = design;
        } else {
            return;
        }
        self.set_presentation(next, cx);
    }

    fn patch_object(
        &mut self,
        id: &str,
        edit: impl FnOnce(&mut PropObject),
        cx: &mut Context<Self>,
    ) {
        let id = id.to_owned();
        self.patch_design(
            move |d| {
                if let Some(o) = d.objects.iter_mut().find(|o| frame_of(o).id == id) {
                    edit(o);
                }
            },
            cx,
        );
    }

    fn patch_selected(&mut self, edit: impl FnOnce(&mut PropObject), cx: &mut Context<Self>) {
        if let Some(id) = self.selected_object.clone() {
            self.patch_object(&id, edit, cx);
        }
    }

    pub fn rename(&mut self, name: String, cx: &mut Context<Self>) {
        self.patch_design(|d| d.name = name, cx);
    }

    pub fn set_background(&mut self, background: Option<String>, cx: &mut Context<Self>) {
        self.patch_design(|d| d.background = background, cx);
    }

    pub fn add_prop(&mut self, cx: &mut Context<Self>) {
        let mut next = self.presentation(cx).clone();
        let id = new_id("prop");
        next.props.push(ProgramProp {
            id: id.clone(),
            name: format!("Prop {}", next.props.len() + 1),
            background: None,
            objects: vec![],
            extra: Extra::new(),
        });
        self.set_presentation(next, cx);
        self.select_prop(Some(id), cx);
    }

    /// Deletes a prop; one on screens comes down first.
    pub fn remove_prop(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.program(cx).active_prop_ids.iter().any(|x| x == id) {
            let call = BoothCall::SetProp {
                id: id.to_owned(),
                on: false,
            };
            self.booth.update(cx, |b, cx| b.program_call(call, cx));
        }
        let mut next = self.presentation(cx).clone();
        next.props.retain(|p| p.id != id);
        let first = next.props.first().map(|p| p.id.clone());
        self.set_presentation(next, cx);
        let selected = self.current(cx).map(|p| p.id);
        if self.selected_timer.is_none()
            && (self.selected_prop.as_deref() == Some(id) || selected.is_none())
        {
            self.select_prop(first, cx);
        }
    }

    pub fn add_timer(&mut self, cx: &mut Context<Self>) {
        let mut next = self.presentation(cx).clone();
        if next.timers.len() >= MAX_PROGRAM_TIMERS {
            return;
        }
        let timer = new_timer(next.timers.len());
        let id = timer.id.clone();
        next.timers.push(timer);
        self.set_presentation(next, cx);
        self.select_timer(&id, cx);
    }

    /// Deletes a timer, never the last. Screens move off a deleted one.
    pub fn remove_timer(&mut self, id: &str, cx: &mut Context<Self>) {
        let current = self.presentation(cx).clone();
        if current.timers.len() <= 1 {
            return;
        }
        let on_screens =
            screen_timer_def(&current.timers, self.program(cx).screen_timer_id.as_deref()).id == id;
        let mut next = current.clone();
        next.timers.retain(|t| t.id != id);
        let first = next.timers.first().map(|t| t.id.clone());
        self.set_presentation(next, cx);
        if on_screens && let Some(first) = first {
            self.booth.update(cx, |b, cx| {
                b.call(BoothCall::TimerShow(first), cx, |_, _, _| {})
            });
        }
        if self.selected_timer.as_deref() == Some(id) {
            self.select_prop(current.props.first().map(|p| p.id.clone()), cx);
        }
    }

    /// Adds an object from the bar; a picture asks for its file first.
    pub fn add_object(&mut self, tool: Tool, cx: &mut Context<Self>) {
        let Some(design) = self.current(cx) else {
            return;
        };
        if design.objects.len() >= MAX_PROP_OBJECTS {
            return;
        }
        if tool == Tool::Image {
            self.pick_image(cx, |this, path, cx| {
                // The design may have changed or filled up while picking.
                if this
                    .current(cx)
                    .is_some_and(|d| d.objects.len() < MAX_PROP_OBJECTS)
                    && let Some(object) = new_object(Tool::Image, Some(path))
                {
                    this.push_object(object, cx);
                }
            });
            return;
        }
        if let Some(object) = new_object(tool, None) {
            self.push_object(object, cx);
        }
    }

    fn push_object(&mut self, object: PropObject, cx: &mut Context<Self>) {
        let id = frame_of(&object).id.clone();
        self.patch_design(|d| d.objects.push(object), cx);
        self.select_object(Some(id), cx);
    }

    /// `program.pickImage`; a closed picker does nothing.
    fn pick_image(
        &mut self,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, String, &mut Context<Self>) + 'static,
    ) {
        let editor = cx.weak_entity();
        self.booth.update(cx, |b, cx| {
            b.call(
                BoothCall::PickImage,
                cx,
                move |_, result, cx| match result {
                    Ok(BoothAnswer::Picked(Some(path))) => {
                        // The booth is still being updated here; the editor reads it.
                        cx.defer(move |cx| {
                            let _ = editor.update(cx, |this, cx| then(this, path, cx));
                        });
                    }
                    Ok(_) => {}
                    Err(error) => tracing::error!(%error, "[Props] image picker failed"),
                },
            )
        });
    }

    pub fn change_image(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_object.clone() else {
            return;
        };
        self.pick_image(cx, move |this, path, cx| {
            this.patch_object(
                &id,
                |o| {
                    if let PropObject::Image { media_path, .. } = o {
                        *media_path = path;
                    }
                },
                cx,
            );
        });
    }

    /// Deletes an object; a timer design keeps its readout.
    pub fn remove_object(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(design) = self.current(cx) else {
            return;
        };
        let timer_readout = self.selected_timer.is_some()
            && design
                .objects
                .iter()
                .any(|o| frame_of(o).id == id && matches!(o, PropObject::Timer { .. }));
        if timer_readout {
            return;
        }
        let id = id.to_owned();
        self.patch_design(|d| d.objects.retain(|o| frame_of(o).id != id), cx);
        self.select_object(None, cx);
    }

    pub fn restack_selected(&mut self, step: i32, cx: &mut Context<Self>) {
        let (Some(id), Some(design)) = (self.selected_object.clone(), self.current(cx)) else {
            return;
        };
        if let Some(objects) = restack(&design.objects, &id, step) {
            self.patch_design(|d| d.objects = objects, cx);
        }
    }

    fn set_frame(&mut self, id: &str, b: PropBox, cx: &mut Context<Self>) {
        self.patch_object(
            id,
            |o| {
                let f = frame_mut(o);
                f.x = b.x;
                f.y = b.y;
                f.w = b.w;
                f.h = b.h;
            },
            cx,
        );
    }

    // ─── Pointer and keys ──────────────────────────────────────────────────

    /// Starts a move (`corner` none) or a resize on `object`, at a point in
    /// shares of the frame.
    pub fn pointer_down(
        &mut self,
        object: &str,
        corner: Option<PropCorner>,
        at: (f64, f64),
        cx: &mut Context<Self>,
    ) {
        let Some(o) = self
            .current(cx)
            .and_then(|d| d.objects.into_iter().find(|o| frame_of(o).id == object))
        else {
            return;
        };
        let f = frame_of(&o);
        let origin = PropBox {
            x: f.x,
            y: f.y,
            w: f.w,
            h: f.h,
        };
        self.gesture = Some(match corner {
            None => PropGesture::Move {
                object_id: object.to_owned(),
                start_x: at.0,
                start_y: at.1,
                origin,
            },
            Some(corner) => PropGesture::Resize {
                object_id: object.to_owned(),
                corner,
                start_x: at.0,
                start_y: at.1,
                origin,
            },
        });
        self.select_object(Some(object.to_owned()), cx);
    }

    pub fn pointer_move(&mut self, at: (f64, f64), cx: &mut Context<Self>) {
        let Some(g) = self.gesture.clone() else {
            return;
        };
        let (id, sx, sy) = match &g {
            PropGesture::Move {
                object_id,
                start_x,
                start_y,
                ..
            }
            | PropGesture::Resize {
                object_id,
                start_x,
                start_y,
                ..
            } => (object_id.clone(), *start_x, *start_y),
        };
        let b = apply_prop_gesture(&g, at.0 - sx, at.1 - sy);
        self.origin = None;
        self.set_frame(&id, b, cx);
    }

    pub fn pointer_up(&mut self, cx: &mut Context<Self>) {
        if self.gesture.take().is_some() {
            cx.notify();
        }
    }

    /// Delete removes the selected object, arrows nudge it, Escape lets it
    /// go. Returns whether the key was used; an unused Escape closes.
    pub fn key(&mut self, key: &str, shift: bool, cx: &mut Context<Self>) -> bool {
        let Some(o) = self.object(cx) else {
            return false;
        };
        let id = frame_of(&o).id.clone();
        match key {
            "escape" => {
                self.select_object(None, cx);
                true
            }
            "delete" | "backspace" => {
                self.remove_object(&id, cx);
                true
            }
            _ => match nudge(frame_of(&o), key, shift) {
                Some((x, y)) => {
                    self.origin = None;
                    self.patch_object(
                        &id,
                        |o| {
                            let f = frame_mut(o);
                            f.x = x;
                            f.y = y;
                        },
                        cx,
                    );
                    true
                }
                None => false,
            },
        }
    }

    /// The stage point under the mouse, in shares of the frame.
    fn share(&self, p: gpui_kit::Point<Pixels>) -> (f64, f64) {
        let b = self.stage.get();
        let w = f64::from(f32::from(b.size.width)).max(1.0);
        let h = f64::from(f32::from(b.size.height)).max(1.0);
        (
            f64::from(f32::from(p.x - b.origin.x)) / w,
            f64::from(f32::from(p.y - b.origin.y)) / h,
        )
    }

    // ─── Fields ────────────────────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    fn number(
        &mut self,
        key: &str,
        label: &str,
        value: f64,
        (step, min, max): (f64, f64, f64),
        on: impl Fn(&mut PropObject, f64) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let field = match self.fields.get(key) {
            Some(Field::Number(f)) => {
                let f = f.clone();
                if self.origin.as_deref() != Some(key) && f.read(cx).value() != value {
                    f.update(cx, |f, cx| f.set_value(value, window, cx));
                }
                f
            }
            _ => {
                let model = NumberModel::new(value, step).min(min).max(max);
                let label = label.to_owned();
                let id = SharedString::from(ids::field(key));
                let f = cx.new(|cx| NumberField::new(id, label, model, window, cx));
                let on = Rc::new(on);
                let k = key.to_owned();
                self.field_subs.push(cx.subscribe(
                    &f,
                    move |this, _, e: &NumberFieldChanged, cx| {
                        let (on, v) = (on.clone(), e.0);
                        this.origin = Some(k.clone());
                        this.patch_selected(|o| on(o, v), cx);
                    },
                ));
                self.fields.insert(key.to_owned(), Field::Number(f.clone()));
                f
            }
        };
        div().min_w_0().child(field).into_any_element()
    }

    fn color(
        &mut self,
        key: &str,
        label: &str,
        value: &str,
        on: impl Fn(&mut Self, String, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let current = hex_hsla(value);
        let state = match self.fields.get(key) {
            Some(Field::Color(s)) => {
                let s = s.clone();
                if self.origin.as_deref() != Some(key)
                    && let Some(c) = current
                    && s.read(cx).value() != Some(c)
                {
                    s.update(cx, |s, cx| s.set_value(c, window, cx));
                }
                s
            }
            _ => {
                let s = cx.new(|cx| {
                    let s = ColorPickerState::new(window, cx);
                    match current {
                        Some(c) => s.default_value(c),
                        None => s,
                    }
                });
                let on = Rc::new(on);
                let k = key.to_owned();
                self.field_subs
                    .push(cx.subscribe(&s, move |this, _, e: &ColorPickerEvent, cx| {
                        if let ColorPickerEvent::Change(Some(c)) = e {
                            this.origin = Some(k.clone());
                            on(this, hsla_hex(*c), cx);
                        }
                    }));
                self.fields.insert(key.to_owned(), Field::Color(s.clone()));
                s
            }
        };
        div()
            .id(SharedString::from(ids::field(key)))
            .test_support()
            .child(ColorPicker::new(&state).accessibility_label(label.to_owned()))
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn slider(
        &mut self,
        key: &str,
        label: &str,
        value: f64,
        (min, max): (f64, f64),
        on: impl Fn(&mut PropObject, f64) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let state = match self.fields.get(key) {
            Some(Field::Slider(s)) => {
                let s = s.clone();
                if self.origin.as_deref() != Some(key)
                    && f64::from(s.read(cx).value().start()) != value
                {
                    s.update(cx, |s, cx| s.set_value(value as f32, window, cx));
                }
                s
            }
            _ => {
                let s = cx.new(|_| {
                    SliderState::new()
                        .min(min as f32)
                        .max(max as f32)
                        .step(1.0)
                        .default_value(value as f32)
                });
                let on = Rc::new(on);
                let k = key.to_owned();
                self.field_subs
                    .push(cx.subscribe(&s, move |this, _, e: &SliderEvent, cx| {
                        if let SliderEvent::Change(v) = e {
                            let (on, v) = (on.clone(), f64::from(v.start()));
                            this.origin = Some(k.clone());
                            this.patch_selected(|o| on(o, v), cx);
                        }
                    }));
                self.fields.insert(key.to_owned(), Field::Slider(s.clone()));
                s
            }
        };
        div()
            .id(SharedString::from(ids::field(key)))
            .test_support()
            .w(px(96.))
            .child(Slider::new(&state).accessibility_label(label.to_owned()))
            .into_any_element()
    }

    /// A number box by key while shown, for tests.
    pub fn number_field(&self, key: &str) -> Option<Entity<NumberField>> {
        match self.fields.get(key) {
            Some(Field::Number(f)) => Some(f.clone()),
            _ => None,
        }
    }

    pub fn name_input(&self) -> Entity<InputState> {
        self.name.clone()
    }

    pub fn text_input(&self) -> Entity<TextareaState> {
        self.text.clone()
    }
}

/// `#rrggbb` as GPUI paints it.
pub fn hex_hsla(value: &str) -> Option<Hsla> {
    let v = value.trim();
    let d = v.strip_prefix('#')?;
    let full: String = match d.len() {
        6 => d.to_owned(),
        3 => d.chars().flat_map(|c| [c, c]).collect(),
        8 => d[..6].to_owned(),
        _ => return None,
    };
    if !full.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let ch = |i: usize| f32::from(u8::from_str_radix(&full[i..i + 2], 16).unwrap_or(0)) / 255.0;
    let alpha = if d.len() == 8 {
        f32::from(u8::from_str_radix(&d[6..8], 16).unwrap_or(255)) / 255.0
    } else {
        1.0
    };
    Some(
        gpui_kit::Rgba {
            r: ch(0),
            g: ch(2),
            b: ch(4),
            a: alpha,
        }
        .into(),
    )
}

fn hsla_hex(c: Hsla) -> String {
    let c = c.to_rgb();
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", b(c.r), b(c.g), b(c.b))
}

/// A CSS colour of the kinds props store, as GPUI paints it.
fn css(value: &str) -> Option<Hsla> {
    kairo_scene_css(value)
}

fn kairo_scene_css(value: &str) -> Option<Hsla> {
    hex_hsla(value).or_else(|| match value.trim() {
        "transparent" => Some(gpui_kit::transparent_black()),
        "white" => Some(gpui_kit::white()),
        "black" => Some(gpui_kit::black()),
        _ => None,
    })
}

// ─── Drawing ───────────────────────────────────────────────────────────────

/// `PropPreview`: the prop at `width`, on a checkerboard when clear.
pub fn prop_picture(
    prop: &ProgramProp,
    timer: &PropTimerView,
    width: f32,
    check: f32,
    cx: &App,
) -> gpui_kit::Div {
    let k = *cx.kairo();
    let height = width * 9.0 / 16.0;
    let scale = width / FRAME_W as f32;
    let now = now_ms();
    let (light, dark) = (k.checker_light, k.checker_dark);
    let mut slide = div()
        .relative()
        .flex_none()
        .w(px(width))
        .h(px(height))
        .overflow_hidden()
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    window.paint_quad(gpui_kit::fill(bounds, dark));
                    let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
                    let mut y = 0.0;
                    let mut row = 0;
                    while y < h {
                        let mut x = if row % 2 == 0 { 0.0 } else { check };
                        while x < w {
                            let cell = Bounds::new(
                                bounds.origin + point(px(x), px(y)),
                                size(px(check.min(w - x)), px(check.min(h - y))),
                            );
                            window.paint_quad(gpui_kit::fill(cell, light));
                            x += check * 2.0;
                        }
                        y += check;
                        row += 1;
                    }
                },
            )
            .absolute()
            .size_full(),
        );
    if let Some(bg) = prop.background.as_deref().and_then(css) {
        slide = slide.child(div().absolute().inset_0().bg(bg));
    }
    for o in &prop.objects {
        let f = frame_of(o);
        let (x, y, w, h) = place(f, width, height);
        let base = div()
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px(w))
            .h(px(h))
            .overflow_hidden()
            .opacity(clamp(if f.opacity.is_nan() { 1.0 } else { f.opacity }, 0.0, 1.0) as f32);
        let words = |d: gpui_kit::Div, align: PropAlign, size_px: f64, bold: bool| {
            d.flex()
                .items_center()
                .when(align == PropAlign::Center, |d| {
                    d.justify_center().text_center()
                })
                .when(align == PropAlign::Right, |d| d.justify_end().text_right())
                .text_size(px((size_px as f32 * scale).max(1.0)))
                .line_height(relative(1.2))
                .font_weight(if bold {
                    FontWeight::BOLD
                } else {
                    FontWeight::NORMAL
                })
        };
        let el: AnyElement = match o {
            PropObject::Text {
                text,
                font_size_px,
                color,
                bold,
                align,
                ..
            } => words(base, *align, *font_size_px, *bold)
                .text_color(css(color).unwrap_or(gpui_kit::black()))
                .child(div().w_full().whitespace_normal().child(text.clone()))
                .into_any_element(),
            PropObject::Timer {
                font_size_px,
                color,
                overrun_color,
                bold,
                align,
                ..
            } => {
                let (shown, up, dim) = readout(timer, now);
                let c = if up {
                    css(overrun_color).unwrap_or(k.danger_text)
                } else {
                    css(color).unwrap_or(gpui_kit::white())
                };
                words(base, *align, *font_size_px, *bold)
                    .child(
                        div()
                            .text_color(c)
                            .when(dim, |d| d.opacity(0.55))
                            .child(shown),
                    )
                    .into_any_element()
            }
            PropObject::Shape {
                shape,
                fill,
                radius_px,
                ..
            } => base
                .when_some(css(fill), |d, c| d.bg(c))
                .map(|d| match shape {
                    PropShape::Ellipse => d.rounded_full(),
                    PropShape::Rectangle => d.rounded(px(*radius_px as f32 * scale)),
                })
                .into_any_element(),
            PropObject::Image {
                media_path, fit, ..
            } => base
                .when(!media_path.is_empty(), |d| {
                    d.child(
                        img(std::path::PathBuf::from(media_path))
                            .size_full()
                            .object_fit(match fit {
                                PropImageFit::Contain => ObjectFit::Contain,
                                PropImageFit::Cover => ObjectFit::Cover,
                            }),
                    )
                })
                .into_any_element(),
        };
        slide = slide.child(el);
    }
    slide
}

impl PropsEditor {
    fn render_lists(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let this = cx.entity();
        let p = self.presentation(cx).clone();
        let program = self.program(cx).clone();
        let current = self.current(cx).map(|d| d.id);
        let on_screens = screen_timer_def(&p.timers, program.screen_timer_id.as_deref())
            .id
            .clone();
        let prop_view = self.view_for(None, cx);
        let row = |id: SharedString, selected: bool| {
            h_flex()
                .id(id)
                .test_support()
                .role(Role::Button)
                .w_full()
                .gap_2p5()
                .px_2()
                .py_1p5()
                .rounded(radius::lg())
                .cursor_pointer()
                .when(selected, |d| d.bg(k.accent[5]).text_color(k.on_accent))
                .when(!selected, |d| d.hover(|s| s.bg(k.surface_tertiary)))
        };
        let props = p.props.iter().map(|prop| {
            let selected =
                self.selected_timer.is_none() && current.as_deref() == Some(prop.id.as_str());
            let on = program.active_prop_ids.contains(&prop.id);
            let (pick, del, id, del_id) =
                (this.clone(), this.clone(), prop.id.clone(), prop.id.clone());
            div()
                .relative()
                .group("prop-row")
                .child(
                    row(SharedString::from(ids::prop(&prop.id)), selected)
                        .aria_label(prop.name.clone())
                        .on_click(move |_, _, cx| {
                            let id = id.clone();
                            pick.update(cx, |t, cx| t.select_prop(Some(id), cx))
                        })
                        .child(prop_picture(prop, &prop_view, 44.0, 3.0, cx).rounded(px(3.)))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(text::SM.size)
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(prop.name.clone()),
                        )
                        .when(on, |d| {
                            d.child(
                                div()
                                    .id(SharedString::from(format!("props-prop-on-{}", prop.id)))
                                    .flex_none()
                                    .size_1p5()
                                    .rounded_full()
                                    .bg(k.live)
                                    .role(Role::Status)
                                    .aria_label("On screen"),
                            )
                        }),
                )
                .child(
                    div()
                        .absolute()
                        .right_1p5()
                        .top_1p5()
                        .invisible()
                        .group_hover("prop-row", |s| s.visible())
                        .child(
                            Button::new(SharedString::from(ids::delete_prop(&prop.id)))
                                .cursor_pointer()
                                .xsmall()
                                .ghost()
                                .icon(Icon::new(KairoIcon::Trash2))
                                .accessibility_label(format!("Delete {}", prop.name))
                                .tooltip("Delete prop")
                                .on_click(move |_, _, cx| {
                                    let id = del_id.clone();
                                    del.update(cx, |t, cx| t.remove_prop(&id, cx))
                                }),
                        ),
                )
        });
        let timers = p.timers.iter().map(|t| {
            let selected = self.selected_timer.as_deref() == Some(t.id.as_str());
            let screens = t.id == on_screens;
            let view = timer_view_for(t, program.timers.get(&t.id), now_ms(), next_tod);
            let design = ProgramProp {
                id: format!("timer-preview-{}", t.id),
                name: t.name.clone(),
                background: t.theme.background.clone(),
                objects: t.theme.objects.clone(),
                extra: Extra::new(),
            };
            let (pick, id) = (this.clone(), t.id.clone());
            row(SharedString::from(ids::timer(&t.id)), selected)
                .aria_label(t.name.clone())
                .on_click(move |_, _, cx| {
                    let id = id.clone();
                    pick.update(cx, |e, cx| e.select_timer(&id, cx))
                })
                .child(prop_picture(&design, &view, 44.0, 3.0, cx).rounded(px(3.)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(text::SM.size)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(t.name.clone()),
                )
                .when(screens, |d| {
                    d.child(
                        div()
                            .id(SharedString::from(format!("props-timer-on-{}", t.id)))
                            .flex_none()
                            .size_1p5()
                            .rounded_full()
                            .bg(if selected { k.surface } else { k.accent[4] })
                            .role(Role::Status)
                            .aria_label("On screens"),
                    )
                })
        });
        let (add_prop, add_timer) = (this.clone(), this.clone());
        let header = |title: &'static str,
                      id: &'static str,
                      label: &'static str,
                      disabled: bool,
                      on: Entity<Self>,
                      timer: bool| {
            h_flex()
                .h_10()
                .flex_none()
                .gap_2()
                .px_3()
                .child(
                    div()
                        .flex_1()
                        .text_size(if timer { text::XS.size } else { text::SM.size })
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title),
                )
                .child(
                    Button::new(id)
                        .cursor_pointer()
                        .ghost()
                        .small()
                        .icon(Icon::new(KairoIcon::Plus))
                        .accessibility_label(label)
                        .tooltip(label)
                        .disabled(disabled)
                        .on_click(move |_, _, cx| {
                            on.update(cx, |t, cx| {
                                if timer {
                                    t.add_timer(cx)
                                } else {
                                    t.add_prop(cx)
                                }
                            })
                        }),
                )
        };
        v_flex()
            .flex_none()
            .w(px(220.))
            .h_full()
            .min_h_0()
            .bg(k.surface_secondary)
            .border_r_1()
            .border_color(k.surface_line)
            .child(header(
                "Props",
                ids::NEW_PROP,
                "New prop",
                false,
                add_prop,
                false,
            ))
            .child(
                v_flex()
                    .id("props-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .gap_0p5()
                    .px_2p5()
                    .py_2()
                    .children(props),
            )
            .child(
                v_flex()
                    .flex_none()
                    .max_h(relative(0.42))
                    .border_t_1()
                    .border_color(k.surface_line)
                    .child(header(
                        "Timers",
                        ids::NEW_TIMER,
                        "New timer",
                        p.timers.len() >= MAX_PROGRAM_TIMERS,
                        add_timer,
                        true,
                    ))
                    .child(
                        v_flex()
                            .id("props-timers")
                            .min_h_0()
                            .overflow_y_scroll()
                            .gap_0p5()
                            .px_2p5()
                            .pb_2()
                            .children(timers),
                    ),
            )
            .into_any_element()
    }

    fn render_stage(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let this = cx.entity();
        let design = self.current(cx);
        let full = design
            .as_ref()
            .is_none_or(|d| d.objects.len() >= MAX_PROP_OBJECTS);
        let tools = h_flex()
            .p_0p5()
            .rounded(radius::lg())
            .bg(k.surface_secondary)
            .children(Tool::ALL.into_iter().map(|tool| {
                let this = this.clone();
                Button::new(SharedString::from(ids::tool(tool.key())))
                    .cursor_pointer()
                    .ghost()
                    .small()
                    .icon(Icon::new(tool.icon()))
                    .label(tool.label())
                    .tooltip(format!("Add {}", tool.label().to_lowercase()))
                    .disabled(full)
                    .on_click(move |_, _, cx| this.update(cx, |t, cx| t.add_object(tool, cx)))
            }));
        let body: AnyElement = match &design {
            None => {
                let this = this.clone();
                v_flex()
                    .id(ids::EMPTY)
                    .test_support()
                    .size_full()
                    .items_center()
                    .justify_center()
                    .gap_3()
                    .text_center()
                    .child(
                        div()
                            .size_11()
                            .rounded_full()
                            .bg(k.surface_secondary)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(Icon::new(KairoIcon::Layers).size_5().text_color(k.text(TextStep::T400))),
                    )
                    .child(div().text_size(text::BASE.size).font_weight(FontWeight::SEMIBOLD).child("No props yet"))
                    .child(
                        div()
                            .max_w(px(352.))
                            .text_size(text::XS.size)
                            .text_color(k.text(TextStep::T400))
                            .child("A prop sits over the slides — a logo in the corner, a welcome banner, a giving card. Build one here, then show it from Operator → Show."),
                    )
                    .child(
                        Button::new(ids::EMPTY_NEW).cursor_pointer()
                            .primary()
                            .small()
                            .icon(Icon::new(KairoIcon::Plus))
                            .label("New prop")
                            .on_click(move |_, _, cx| this.update(cx, |t, cx| t.add_prop(cx))),
                    )
                    .into_any_element()
            }
            Some(d) => self.render_canvas(d, cx),
        };
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(k.surface)
            .child(
                h_flex()
                    .flex_none()
                    .justify_center()
                    .px_4()
                    .pt_3()
                    .pb_2()
                    .child(tools),
            )
            .child(div().flex_1().min_h_0().relative().child(body))
            .into_any_element()
    }

    fn render_canvas(&self, design: &ProgramProp, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let this = cx.entity();
        let selected_timer = self
            .selected_timer
            .as_ref()
            .and_then(|id| self.presentation(cx).timers.iter().find(|t| t.id == *id))
            .cloned();
        let view = self.view_for(selected_timer.as_ref(), cx);
        let bounds = self.stage.get();
        // The stage fits its area with 24 px around it.
        let area = self.stage_area.get();
        let scale = ((f32::from(area.size.width) - 48.0) / FRAME_W as f32)
            .min((f32::from(area.size.height) - 48.0) / FRAME_H as f32)
            .max(0.1);
        let (w, h) = (FRAME_W as f32 * scale, FRAME_H as f32 * scale);
        let _ = bounds;
        let cell = self.stage.clone();
        let area_cell = self.stage_area.clone();
        let selected = self.selected_object.clone();
        let objects = design.objects.iter().map(|o| {
            let f = frame_of(o);
            let (x, y, ow, oh) = place(f, w, h);
            let is_selected = selected.as_deref() == Some(f.id.as_str());
            let id = f.id.clone();
            let mover = this.clone();
            let (picker, pick_id) = (this.clone(), f.id.clone());
            let mut el = div()
                .id(SharedString::from(ids::object(&f.id)))
                .test_support()
                .role(Role::Button)
                .aria_label(format!("{} object", kind_label(o)))
                .aria_description(object_summary(o))
                .aria_selected(is_selected)
                // The pointer selects on press to start a drag; assistive
                // technology presses without a pointer, so it selects here.
                .on_a11y_action(gpui_kit::AccessibleAction::Click, move |_, _, cx| {
                    let id = pick_id.clone();
                    picker.update(cx, |t, cx| t.select_object(Some(id), cx))
                })
                .absolute()
                .left(px(x))
                .top(px(y))
                .w(px(ow))
                .h(px(oh))
                .cursor(CursorStyle::OpenHand)
                .when(is_selected, |d| d.border_2().border_color(k.accent[5]))
                .when(!is_selected, |d| {
                    d.hover(|s| s.border_1().border_color(k.ink(0.5)))
                })
                .on_mouse_down(MouseButton::Left, move |e: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    let id = id.clone();
                    let pos = e.position;
                    mover.update(cx, |t, cx| {
                        let at = t.share(pos);
                        t.focus.focus(window, cx);
                        t.pointer_down(&id, None, at, cx)
                    })
                });
            if is_selected {
                for (corner, left, top, cursor) in [
                    (
                        PropCorner::Nw,
                        true,
                        true,
                        CursorStyle::ResizeUpLeftDownRight,
                    ),
                    (
                        PropCorner::Ne,
                        false,
                        true,
                        CursorStyle::ResizeUpRightDownLeft,
                    ),
                    (
                        PropCorner::Sw,
                        true,
                        false,
                        CursorStyle::ResizeUpRightDownLeft,
                    ),
                    (
                        PropCorner::Se,
                        false,
                        false,
                        CursorStyle::ResizeUpLeftDownRight,
                    ),
                ] {
                    let (t, id) = (this.clone(), f.id.clone());
                    el = el.child(
                        div()
                            .absolute()
                            .size(px(10.))
                            .rounded(px(2.))
                            .bg(k.white)
                            .border_1()
                            .border_color(k.accent[5])
                            .cursor(cursor)
                            .when(left, |d| d.left(px(-6.)))
                            .when(!left, |d| d.right(px(-6.)))
                            .when(top, |d| d.top(px(-6.)))
                            .when(!top, |d| d.bottom(px(-6.)))
                            .on_mouse_down(MouseButton::Left, move |e: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                let (id, pos) = (id.clone(), e.position);
                                t.update(cx, |t, cx| {
                                    let at = t.share(pos);
                                    t.pointer_down(&id, Some(corner), at, cx)
                                })
                            }),
                    );
                }
            }
            el
        });
        let (deselect, mover, upper) = (this.clone(), this.clone(), this.clone());
        div()
            .id("props-stage-area")
            .size_full()
            .relative()
            .flex()
            .items_center()
            .justify_center()
            .overflow_hidden()
            .child(
                canvas(move |b, _, _| area_cell.set(b), |_, _, _, _| {})
                    .absolute()
                    .inset_0(),
            )
            .on_mouse_move(move |e: &MouseMoveEvent, _, cx| {
                let pos = e.position;
                mover.update(cx, |t, cx| {
                    if t.gesture.is_some() {
                        let at = t.share(pos);
                        t.pointer_move(at, cx)
                    }
                })
            })
            .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                upper.update(cx, |t, cx| t.pointer_up(cx))
            })
            .on_mouse_up_out(MouseButton::Left, {
                let t = this.clone();
                move |_, _, cx| t.update(cx, |t, cx| t.pointer_up(cx))
            })
            .child(
                div()
                    .id(ids::STAGE)
                    .test_support()
                    .relative()
                    .flex_none()
                    .w(px(w))
                    .h(px(h))
                    .rounded(radius::sm())
                    .overflow_hidden()
                    .border_1()
                    .border_color(k.surface_border)
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        deselect.update(cx, |t, cx| {
                            t.focus.focus(window, cx);
                            t.select_object(None, cx)
                        })
                    })
                    .child(
                        canvas(move |b, _, _| cell.set(b), |_, _, _, _| {})
                            .absolute()
                            .inset_0(),
                    )
                    .child(
                        prop_picture(design, &view, w, 12.0, cx)
                            .absolute()
                            .top_0()
                            .left_0(),
                    )
                    .children(objects)
                    .when(
                        design.objects.is_empty() && design.background.is_none(),
                        |d| {
                            d.child(
                                div()
                                    .id(ids::STAGE_HINT)
                                    .test_support()
                                    .absolute()
                                    .inset_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(text::SM.size)
                                    .text_color(k.text(TextStep::T400))
                                    .child("Add text, a shape or an image from the bar above."),
                            )
                        },
                    ),
            )
            .into_any_element()
    }

    fn render_inspector(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let this = cx.entity();
        let design = self.current(cx);
        let object = self.object(cx);
        let body: AnyElement = match (&design, object) {
            (Some(d), Some(o)) => self.object_inspector(d, &o, window, cx),
            (Some(d), None) => self.design_inspector(d, window, cx),
            _ => div().into_any_element(),
        };
        let saving = self.saving;
        v_flex()
            .flex_none()
            .w(px(260.))
            .h_full()
            .bg(k.surface_secondary)
            .border_l_1()
            .border_color(k.surface_line)
            .child(
                div()
                    .id("props-inspector")
                    .test_support()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_3p5()
                    .py_4()
                    .child(body),
            )
            .when(design.is_some(), |d| {
                d.child(
                    div()
                        .flex_none()
                        .p_3()
                        .border_t_1()
                        .border_color(k.surface_line)
                        .child(
                            Button::new(ids::SAVE)
                                .cursor_pointer()
                                .primary()
                                .small()
                                .w_full()
                                .label(if saving { "Saving…" } else { "Save" })
                                .disabled(saving)
                                .on_click(move |_, _, cx| this.update(cx, |t, cx| t.save(cx))),
                        ),
                )
            })
            .into_any_element()
    }

    fn design_inspector(
        &mut self,
        d: &ProgramProp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let this = cx.entity();
        let timer = self.selected_timer.is_some();
        let title = if timer { "Timer" } else { "Prop" };
        // The name box follows the design unless it is being typed in.
        let typing = self.name.read(cx).focus_handle(cx).is_focused(window);
        if !typing && self.name.read(cx).value() != d.name.as_str() {
            let name = d.name.clone();
            self.name.update(cx, |i, cx| i.set_value(name, window, cx));
        }
        let bg_on = d.background.is_some();
        let bg = d.background.clone().unwrap_or_default();
        let background_color = bg_on.then(|| {
            self.color(
                "background",
                "Background colour",
                hex_or(&bg, "#000000"),
                |t, c, cx| t.set_background(Some(c), cx),
                window,
                cx,
            )
        });
        let objects: Vec<AnyElement> = d
            .objects
            .iter()
            .rev()
            .map(|o| {
                let (pick, id) = (this.clone(), frame_of(o).id.clone());
                h_flex()
                    .id(SharedString::from(ids::list_object(&frame_of(o).id)))
                    .test_support()
                    .role(Role::Button)
                    .aria_label(format!("{} {}", kind_label(o), object_summary(o)))
                    .gap_2()
                    .px_3()
                    .py_2()
                    .text_size(text::XS.size)
                    .cursor_pointer()
                    .hover(|s| s.bg(k.surface_tertiary))
                    .on_click(move |_, _, cx| {
                        let id = id.clone();
                        pick.update(cx, |t, cx| t.select_object(Some(id), cx))
                    })
                    .child(
                        div()
                            .w_12()
                            .flex_none()
                            .text_color(k.text(TextStep::T500))
                            .child(kind_label(o)),
                    )
                    .child(div().min_w_0().truncate().child(object_summary(o)))
                    .into_any_element()
            })
            .collect();
        let remove = {
            let (this, id) = (this.clone(), d.id.clone());
            Button::new(ids::REMOVE)
                .cursor_pointer()
                .ghost()
                .small()
                .w_full()
                .icon(Icon::new(KairoIcon::Trash2))
                .label(if timer { "Delete timer" } else { "Delete prop" })
                .on_click(move |_, _, cx| {
                    let id = id.clone();
                    this.update(cx, |t, cx| match id.strip_prefix("timer:") {
                        Some(tid) => t.remove_timer(tid, cx),
                        None => t.remove_prop(&id, cx),
                    })
                })
        };
        let switch = {
            let this = this.clone();
            Switch::new(ids::BACKGROUND)
                .checked(bg_on)
                .accessibility_label("Background colour")
                .on_change(move |on, _, cx| {
                    let on = *on;
                    this.update(cx, |t, cx| {
                        t.set_background(on.then(|| "#000000".into()), cx)
                    })
                })
        };
        v_flex()
            .gap_4()
            .when(timer, |d| {
                d.child(
                    div()
                        .id(ids::NOTE)
                        .test_support()
                        .rounded(radius::md())
                        .bg(k.surface)
                        .px_2p5()
                        .py_2()
                        .text_size(text::LABEL.size)
                        .text_color(k.text(TextStep::T400))
                        .child("Run, reset and show this timer from Operator → Timers. This design is what appears on screen."),
                )
            })
            .child(group(
                title,
                vec![
                    row("Name", div().w(px(140.)).child(Input::new(&self.name).kairo_size().small().aria_label(format!("{title} name"))).into_any_element(), cx),
                    row("Background", h_flex().gap_1p5().child(switch).children(background_color).into_any_element(), cx),
                ],
                cx,
            ))
            .child(group(
                "Objects",
                if objects.is_empty() {
                    vec![div()
                        .px_3()
                        .py_3()
                        .text_size(text::XS.size)
                        .text_color(k.text(TextStep::T500))
                        .child("Nothing yet. Add text, a shape or an image.")
                        .into_any_element()]
                } else {
                    objects
                },
                cx,
            ))
            .child(
                div()
                    .px_1()
                    .text_size(text::LABEL.size)
                    .text_color(k.text(TextStep::T500))
                    .child("Objects stack in this order, top of the list in front. Drag on the stage to place them; drag a corner to size them."),
            )
            .child(remove)
            .into_any_element()
    }

    fn object_inspector(
        &mut self,
        d: &ProgramProp,
        o: &PropObject,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let this = cx.entity();
        let f = frame_of(o).clone();
        let index = d
            .objects
            .iter()
            .position(|x| frame_of(x).id == f.id)
            .unwrap_or(0);
        let count = d.objects.len();
        let is_timer = matches!(o, PropObject::Timer { .. });
        let mut col = v_flex().gap_4().child(
            h_flex()
                .justify_between()
                .px_1()
                .child(
                    div()
                        .text_size(text::SM.size)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(kind_label(o)),
                )
                .child({
                    let (this, id) = (this.clone(), f.id.clone());
                    Button::new(ids::DELETE_OBJECT)
                        .cursor_pointer()
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(KairoIcon::Trash2))
                        .label("Delete")
                        .disabled(is_timer)
                        .tooltip(if is_timer {
                            "A timer design needs its readout"
                        } else {
                            "Delete this object"
                        })
                        .on_click(move |_, _, cx| {
                            let id = id.clone();
                            this.update(cx, |t, cx| t.remove_object(&id, cx))
                        })
                }),
        );

        let align_group = |key: &'static str, current: PropAlign, this: Entity<Self>| {
            h_flex()
                .p_0p5()
                .rounded(radius::md())
                .bg(k.surface_tertiary)
                .children(
                    [
                        (PropAlign::Left, "left", KairoIcon::AlignLeft),
                        (PropAlign::Center, "center", KairoIcon::AlignCenter),
                        (PropAlign::Right, "right", KairoIcon::AlignRight),
                    ]
                    .into_iter()
                    .map(move |(a, name, icon)| {
                        let this = this.clone();
                        let on = current == a;
                        Button::new(SharedString::from(ids::align(&format!("{key}-{name}"))))
                            .cursor_pointer()
                            .xsmall()
                            .icon(Icon::new(icon))
                            .when(on, |b| b.primary())
                            .when(!on, |b| b.ghost())
                            .toggled(on)
                            .accessibility_label(format!("Align {name}"))
                            .on_click(move |_, _, cx| {
                                this.update(cx, |t, cx| {
                                    t.patch_selected(
                                        |o| match o {
                                            PropObject::Text { align, .. }
                                            | PropObject::Timer { align, .. } => *align = a,
                                            _ => {}
                                        },
                                        cx,
                                    )
                                })
                            })
                    }),
                )
                .into_any_element()
        };
        let bold_switch = |on: bool, this: Entity<Self>| {
            Switch::new(ids::BOLD)
                .checked(on)
                .accessibility_label("Bold")
                .on_change(move |v, _, cx| {
                    let v = *v;
                    this.update(cx, |t, cx| {
                        t.patch_selected(
                            |o| match o {
                                PropObject::Text { bold, .. } | PropObject::Timer { bold, .. } => {
                                    *bold = v
                                }
                                _ => {}
                            },
                            cx,
                        )
                    })
                })
                .into_any_element()
        };
        let set_size = |o: &mut PropObject, v: f64| match o {
            PropObject::Text { font_size_px, .. } | PropObject::Timer { font_size_px, .. } => {
                *font_size_px = clamp(v, 8.0, 400.0)
            }
            _ => {}
        };

        match o {
            PropObject::Text {
                text: words,
                font_size_px,
                color,
                bold,
                align,
                ..
            } => {
                let typing = self.text.read(cx).focus_handle(cx).is_focused(window);
                if !typing && self.text.read(cx).value() != words.as_str() {
                    let w = words.clone();
                    self.text.update(cx, |i, cx| i.set_value(w, window, cx));
                }
                let size = self.number(
                    "size",
                    "Text size",
                    *font_size_px,
                    (1.0, 8.0, 400.0),
                    set_size,
                    window,
                    cx,
                );
                let colour = self.color(
                    "color",
                    "Text colour",
                    hex_or(color, "#ffffff"),
                    |t, c, cx| {
                        t.patch_selected(
                            |o| {
                                if let PropObject::Text { color, .. } = o {
                                    *color = c
                                }
                            },
                            cx,
                        )
                    },
                    window,
                    cx,
                );
                col = col.child(group(
                    "Text",
                    vec![
                        div()
                            .id(ids::TEXT)
                            .test_support()
                            .px_3()
                            .py_2p5()
                            .child(Textarea::new(&self.text).h(px(64.)).aria_label("Text"))
                            .into_any_element(),
                        row("Size", div().w(px(96.)).child(size).into_any_element(), cx),
                        row("Colour", colour, cx),
                        row("Bold", bold_switch(*bold, this.clone()), cx),
                        row("Text align", align_group("text", *align, this.clone()), cx),
                    ],
                    cx,
                ));
            }
            PropObject::Timer {
                font_size_px,
                color,
                overrun_color,
                bold,
                align,
                ..
            } => {
                let size = self.number(
                    "size",
                    "Timer size",
                    *font_size_px,
                    (1.0, 8.0, 400.0),
                    set_size,
                    window,
                    cx,
                );
                let colour = self.color(
                    "color",
                    "Timer colour",
                    hex_or(color, "#ffffff"),
                    |t, c, cx| {
                        t.patch_selected(
                            |o| {
                                if let PropObject::Timer { color, .. } = o {
                                    *color = c
                                }
                            },
                            cx,
                        )
                    },
                    window,
                    cx,
                );
                let overrun = self.color(
                    "overrun",
                    "Time-up colour",
                    hex_or(overrun_color, "#f87171"),
                    |t, c, cx| {
                        t.patch_selected(
                            |o| {
                                if let PropObject::Timer { overrun_color, .. } = o {
                                    *overrun_color = c
                                }
                            },
                            cx,
                        )
                    },
                    window,
                    cx,
                );
                col = col.child(group("Timer", vec![
                    div().px_3().py_2p5().text_size(text::LABEL.size).text_color(k.text(TextStep::T500)).child("Shows the timer that is on screens. Place and size it like any other prop object.").into_any_element(),
                    row("Size", div().w(px(96.)).child(size).into_any_element(), cx),
                    row("Colour", colour, cx),
                    row("Time-up", overrun, cx),
                    row("Bold", bold_switch(*bold, this.clone()), cx),
                    row("Text align", align_group("timer", *align, this.clone()), cx),
                ], cx));
            }
            PropObject::Shape {
                shape,
                fill,
                radius_px,
                ..
            } => {
                let shape_ctl = {
                    let this = this.clone();
                    SegmentedControl::new(SharedString::from(ids::field("shape")), "Shape")
                        .fit()
                        .options([
                            SegmentOption::new("rectangle", "Rectangle"),
                            SegmentOption::new("ellipse", "Ellipse"),
                        ])
                        .selected(shape.as_str())
                        .on_change(move |v, _, cx| {
                            let next = if v.as_ref() == "ellipse" {
                                PropShape::Ellipse
                            } else {
                                PropShape::Rectangle
                            };
                            this.update(cx, |t, cx| {
                                t.patch_selected(
                                    |o| {
                                        if let PropObject::Shape { shape, .. } = o {
                                            *shape = next
                                        }
                                    },
                                    cx,
                                )
                            })
                        })
                        .into_any_element()
                };
                let fill_ctl = self.color(
                    "fill",
                    "Fill colour",
                    hex_or(fill, "#315ede"),
                    |t, c, cx| {
                        t.patch_selected(
                            |o| {
                                if let PropObject::Shape { fill, .. } = o {
                                    *fill = c
                                }
                            },
                            cx,
                        )
                    },
                    window,
                    cx,
                );
                let mut rows = vec![row("Shape", shape_ctl, cx), row("Fill", fill_ctl, cx)];
                if *shape == PropShape::Rectangle {
                    let r = self.slider(
                        "corners",
                        "Corner rounding",
                        *radius_px,
                        (0.0, 200.0),
                        |o, v| {
                            if let PropObject::Shape { radius_px, .. } = o {
                                *radius_px = v
                            }
                        },
                        window,
                        cx,
                    );
                    rows.push(row("Corners", r, cx));
                }
                col = col.child(group("Shape", rows, cx));
            }
            PropObject::Image {
                media_path, fit, ..
            } => {
                let change = {
                    let this = this.clone();
                    Button::new(ids::CHANGE_IMAGE)
                        .cursor_pointer()
                        .xsmall()
                        .label("Change")
                        .on_click(move |_, _, cx| this.update(cx, |t, cx| t.change_image(cx)))
                };
                let fit_ctl = {
                    let this = this.clone();
                    SegmentedControl::new(SharedString::from(ids::field("fit")), "Fit")
                        .fit()
                        .options([
                            SegmentOption::new("contain", "Whole"),
                            SegmentOption::new("cover", "Fill"),
                        ])
                        .selected(fit.as_str())
                        .on_change(move |v, _, cx| {
                            let next = if v.as_ref() == "cover" {
                                PropImageFit::Cover
                            } else {
                                PropImageFit::Contain
                            };
                            this.update(cx, |t, cx| {
                                t.patch_selected(
                                    |o| {
                                        if let PropObject::Image { fit, .. } = o {
                                            *fit = next
                                        }
                                    },
                                    cx,
                                )
                            })
                        })
                        .into_any_element()
                };
                col = col.child(group(
                    "Image",
                    vec![
                        row(
                            "File",
                            h_flex()
                                .gap_1p5()
                                .min_w_0()
                                .child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .text_color(k.text(TextStep::T400))
                                        .child(file_name(media_path).to_owned()),
                                )
                                .child(change)
                                .into_any_element(),
                            cx,
                        ),
                        row("Fit", fit_ctl, cx),
                    ],
                    cx,
                ));
            }
        }

        // Align to slide.
        let slide_buttons = |items: [(&'static str, &'static str, KairoIcon, f64, bool); 3],
                             current: f64,
                             this: Entity<Self>| {
            h_flex()
                .p_0p5()
                .rounded(radius::md())
                .bg(k.surface_tertiary)
                .children(
                    items
                        .into_iter()
                        .map(move |(key, label, icon, value, across)| {
                            let this = this.clone();
                            let on = (current - value).abs() < 0.005;
                            Button::new(SharedString::from(ids::slide(key)))
                                .cursor_pointer()
                                .xsmall()
                                .icon(Icon::new(icon))
                                .when(on, |b| b.primary())
                                .when(!on, |b| b.ghost())
                                .toggled(on)
                                .accessibility_label(format!(
                                    "Align {} on the slide",
                                    label.to_lowercase()
                                ))
                                .on_click(move |_, _, cx| {
                                    this.update(cx, |t, cx| {
                                        t.origin = None;
                                        t.patch_selected(
                                            |o| {
                                                let f = frame_mut(o);
                                                if across { f.x = value } else { f.y = value }
                                            },
                                            cx,
                                        )
                                    })
                                })
                        }),
                )
                .into_any_element()
        };
        col = col.child(group(
            "Align to slide",
            vec![
                row(
                    "Across",
                    slide_buttons(
                        [
                            ("left", "Left", KairoIcon::ObjectAlignLeft, 0.0, true),
                            (
                                "centre",
                                "Centre",
                                KairoIcon::ObjectAlignCenter,
                                (1.0 - f.w) / 2.0,
                                true,
                            ),
                            (
                                "right",
                                "Right",
                                KairoIcon::ObjectAlignRight,
                                1.0 - f.w,
                                true,
                            ),
                        ],
                        f.x,
                        this.clone(),
                    ),
                    cx,
                ),
                row(
                    "Down",
                    slide_buttons(
                        [
                            ("top", "Top", KairoIcon::ObjectAlignTop, 0.0, false),
                            (
                                "middle",
                                "Middle",
                                KairoIcon::ObjectAlignMiddle,
                                (1.0 - f.h) / 2.0,
                                false,
                            ),
                            (
                                "bottom",
                                "Bottom",
                                KairoIcon::ObjectAlignBottom,
                                1.0 - f.h,
                                false,
                            ),
                        ],
                        f.y,
                        this.clone(),
                    ),
                    cx,
                ),
            ],
            cx,
        ));

        // Place: shares typed as percentages, each kept on the frame.
        let pctv = |v: f64| (v * 1000.0).round() / 10.0;
        let x = self.number(
            "x",
            "X",
            pctv(f.x),
            (0.5, 0.0, 100.0),
            |o, v| {
                let f = frame_mut(o);
                f.x = clamp(v / 100.0, 0.0, 1.0 - f.w);
            },
            window,
            cx,
        );
        let y = self.number(
            "y",
            "Y",
            pctv(f.y),
            (0.5, 0.0, 100.0),
            |o, v| {
                let f = frame_mut(o);
                f.y = clamp(v / 100.0, 0.0, 1.0 - f.h);
            },
            window,
            cx,
        );
        let w = self.number(
            "w",
            "W",
            pctv(f.w),
            (0.5, MIN_PROP_OBJECT_SIZE * 100.0, 100.0),
            |o, v| {
                let f = frame_mut(o);
                f.w = clamp(v / 100.0, MIN_PROP_OBJECT_SIZE, 1.0 - f.x);
            },
            window,
            cx,
        );
        let h = self.number(
            "h",
            "H",
            pctv(f.h),
            (0.5, MIN_PROP_OBJECT_SIZE * 100.0, 100.0),
            |o, v| {
                let f = frame_mut(o);
                f.h = clamp(v / 100.0, MIN_PROP_OBJECT_SIZE, 1.0 - f.y);
            },
            window,
            cx,
        );
        let opacity = self.slider(
            "opacity",
            "Opacity",
            (f.opacity * 100.0).round(),
            (0.0, 100.0),
            |o, v| frame_mut(o).opacity = v / 100.0,
            window,
            cx,
        );
        let order = {
            let (fwd, back) = (this.clone(), this.clone());
            h_flex()
                .gap_1p5()
                .child(
                    Button::new(ids::FORWARD)
                        .cursor_pointer()
                        .xsmall()
                        .icon(Icon::new(KairoIcon::ArrowUp))
                        .label("Forward")
                        .disabled(index + 1 >= count)
                        .on_click(move |_, _, cx| {
                            fwd.update(cx, |t, cx| t.restack_selected(1, cx))
                        }),
                )
                .child(
                    Button::new(ids::BACK)
                        .cursor_pointer()
                        .xsmall()
                        .icon(Icon::new(KairoIcon::ArrowDown))
                        .label("Back")
                        .disabled(index == 0)
                        .on_click(move |_, _, cx| {
                            back.update(cx, |t, cx| t.restack_selected(-1, cx))
                        }),
                )
                .into_any_element()
        };
        col = col.child(group(
            "Place",
            vec![
                row(
                    "Position",
                    h_flex()
                        .gap_1()
                        .child(div().w(px(80.)).child(x))
                        .child(div().w(px(80.)).child(y))
                        .into_any_element(),
                    cx,
                ),
                row(
                    "Size",
                    h_flex()
                        .gap_1()
                        .child(div().w(px(80.)).child(w))
                        .child(div().w(px(80.)).child(h))
                        .into_any_element(),
                    cx,
                ),
                row(
                    "Opacity",
                    h_flex()
                        .gap_1()
                        .child(opacity)
                        .child(
                            div()
                                .w_8()
                                .text_right()
                                .text_size(text::LABEL.size)
                                .text_color(k.text(TextStep::T500))
                                .child(format!("{}%", (f.opacity * 100.0).round())),
                        )
                        .into_any_element(),
                    cx,
                ),
                row("Order", order, cx),
            ],
            cx,
        ));
        col.into_any_element()
    }
}

fn row(label: &str, control: AnyElement, cx: &App) -> AnyElement {
    h_flex()
        .min_h_9()
        .justify_between()
        .gap_3()
        .px_3()
        .py_1p5()
        .text_size(text::XS.size)
        .text_color(cx.kairo().text(TextStep::T300))
        .child(div().flex_none().child(label.to_owned()))
        .child(h_flex().min_w_0().justify_end().gap_1p5().child(control))
        .into_any_element()
}

fn group(title: &str, rows: Vec<AnyElement>, cx: &App) -> AnyElement {
    let k = *cx.kairo();
    v_flex()
        .gap_1p5()
        .child(
            div()
                .px_1()
                .text_size(text::XS.size)
                .font_weight(FontWeight::SEMIBOLD)
                .child(title.to_owned()),
        )
        .child(
            v_flex()
                .overflow_hidden()
                .rounded(radius::lg())
                .bg(k.surface)
                .children(rows),
        )
        .into_any_element()
}

impl Render for PropsEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity();
        let lists = self.render_lists(cx);
        let stage = self.render_stage(cx);
        let inspector = self.render_inspector(window, cx);
        let content = h_flex()
            .id(ids::ROOT)
            .test_support()
            .track_focus(&self.focus)
            .key_context("PropsEditor")
            .on_key_down(move |e: &KeyDownEvent, window, cx| {
                let key = e.keystroke.key.clone();
                let shift = e.keystroke.modifiers.shift;
                let used = this.update(cx, |t, cx| {
                    // Keys typed in a field are the field's.
                    t.focus.is_focused(window) && t.key(&key, shift, cx)
                });
                if used {
                    cx.stop_propagation();
                }
            })
            .size_full()
            .child(lists)
            .child(stage)
            .child(inspector);
        self.dialog.update(cx, |d, cx| d.frame(content, window, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(id: &str) -> PropObject {
        let mut o = new_object(Tool::Rectangle, None).unwrap();
        frame_mut(&mut o).id = id.into();
        o
    }

    #[test]
    fn tools_make_the_objects_electron_does() {
        let t = new_object(Tool::Text, None).unwrap();
        assert!(
            matches!(&t, PropObject::Text { text, font_size_px, .. } if text == "Your text" && *font_size_px == 96.0)
        );
        assert_eq!(frame_of(&t).x, 0.1);
        let r = new_object(Tool::Rectangle, None).unwrap();
        assert!(
            matches!(r, PropObject::Shape { radius_px, shape: PropShape::Rectangle, .. } if radius_px == 24.0)
        );
        let e = new_object(Tool::Ellipse, None).unwrap();
        assert!(
            matches!(e, PropObject::Shape { radius_px, shape: PropShape::Ellipse, .. } if radius_px == 0.0)
        );
        assert!(
            new_object(Tool::Image, None).is_none(),
            "an image needs its file"
        );
        let i = new_object(Tool::Image, Some("/a/b.png".into())).unwrap();
        assert_eq!(object_summary(&i), "b.png");
        let timer = new_timer(2);
        assert_eq!(timer.name, "Timer 3");
        assert!(timer.overrun);
        assert!(matches!(timer.theme.objects[0], PropObject::Timer { .. }));
    }

    #[test]
    fn restack_moves_one_step_and_stops_at_the_ends() {
        let objects = vec![rect("a"), rect("b"), rect("c")];
        let up = restack(&objects, "a", 1).unwrap();
        assert_eq!(frame_of(&up[1]).id, "a");
        assert!(restack(&objects, "c", 1).is_none());
        assert!(restack(&objects, "a", -1).is_none());
        assert!(restack(&objects, "zz", 1).is_none());
    }

    #[test]
    fn arrows_nudge_on_the_frame() {
        let f = frame("x".into(), 0.0, 0.5, 0.4, 0.2);
        assert_eq!(nudge(&f, "left", false), Some((0.0, 0.5)));
        let (x, _) = nudge(&f, "right", true).unwrap();
        assert!((x - 0.05).abs() < 1e-9);
        let near = frame("x".into(), 0.59, 0.79, 0.4, 0.2);
        let (x, y) = nudge(&near, "right", true).unwrap();
        assert!((x - 0.6).abs() < 1e-9);
        assert_eq!(nudge(&near, "down", true).unwrap().1, 0.8);
        assert!(y <= 0.8);
        assert_eq!(nudge(&f, "tab", false), None);
    }

    #[test]
    fn names_colours_and_readouts() {
        assert_eq!(hex_or("#ABCDEF", "#000000"), "#ABCDEF");
        assert_eq!(hex_or("#abc", "#000000"), "#000000");
        assert_eq!(hex_or("red", "#ffffff"), "#ffffff");
        assert!(hex_hsla("#ff0000").is_some());
        assert!(hex_hsla("rgba(0,0,0,1)").is_none());
        let mut t = new_object(Tool::Text, None).unwrap();
        if let PropObject::Text { text, .. } = &mut t {
            text.clear();
        }
        assert_eq!(object_summary(&t), "Empty text");
        let idle = PropTimerView {
            duration_sec: 300.0,
            ends_at: None,
            remaining_sec: 300.0,
            rollover: true,
            now: 0.0,
        };
        assert_eq!(readout(&idle, 0.0), ("5:00".into(), false, false));
        let over = PropTimerView {
            ends_at: Some(0.0),
            ..idle.clone()
        };
        let (text, up, _) = readout(&over, 61_000.0);
        assert!(text.starts_with('-'));
        assert!(up);
        let paused = PropTimerView {
            remaining_sec: 120.0,
            ..idle
        };
        assert!(readout(&paused, 0.0).2, "a paused run is dimmed");
    }
}
