//! The Media Inspector (`MediaInspector.tsx` with `TrimTimeline.tsx`): a
//! private preview (decoded in this process, never on a screen), the trim
//! timeline with draggable and keyboard handles, the playhead, ±10 s,
//! speed, loop, scaling, colour sliders that re-grade the paused or playing
//! preview without restarting it, details, and Save for the next cue.

use gpui_kit::FocusHandle;
use gpui_kit::component::FocusTrapElement;
use kairo_ui_kit::ControlSize;
use kairo_ui_kit::components::PopoverTriggerExt;
use std::sync::Arc;

use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Disableable, Icon, Sizable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, AppContext, Context, DragMoveEvent, Entity, EventEmitter, FontWeight,
    InteractiveElement, IntoElement, KeyDownEvent, ObjectFit, ParentElement, Render, Role,
    SharedString, StatefulInteractiveElement, Styled, StyledImage, Subscription, Window, deferred,
    div, img, prelude::FluentBuilder, px,
};
use kairo_domain::media::{MediaItem, MediaKind, MediaPlayback, format_media_clock};
use kairo_domain::overlay::MediaFit;
use kairo_ui_kit::tokens::{Step, TextStep, radius, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};
use serde_json::{Map, Value, json};

use crate::backend::MediaBackend;
use crate::player::{Grade, PreviewPlayer, Range};
use crate::rows::{self, Edge, Trim};
use crate::view::grade_of;

/// Element ids, for tests.
pub mod ids {
    pub const DIALOG: &str = "media-inspector";
    pub const SAVE: &str = "media-inspector-save";
    pub const CANCEL: &str = "media-inspector-cancel";
    pub const START: &str = "media-inspector-start";
    pub const END: &str = "media-inspector-end";
    pub const NOTE: &str = "media-inspector-note";
    pub const ERROR: &str = "media-inspector-error";
    pub const PLAY: &str = "media-inspector-play";
    pub const BACK: &str = "media-inspector-back";
    pub const AHEAD: &str = "media-inspector-ahead";
    pub const CLOCK: &str = "media-inspector-clock";
    pub const TRACK: &str = "media-inspector-track";
    pub const RESET: &str = "media-inspector-reset";
    pub const LOOP: &str = "media-inspector-loop";
    pub const USE_START: &str = "media-inspector-use-start";
    pub const USE_END: &str = "media-inspector-use-end";
    pub fn handle(edge: &str) -> String {
        format!("media-inspector-trim-{edge}")
    }
    pub fn panel(name: &str) -> String {
        format!("media-inspector-panel-{}", name.to_lowercase())
    }
}

/// Inspector preview width.
const WIDTH: u32 = 960;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    Playback,
    Appearance,
    Details,
}

impl Panel {
    fn name(self) -> &'static str {
        match self {
            Panel::Playback => "Playback",
            Panel::Appearance => "Appearance",
            Panel::Details => "Details",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum InspectorEvent {
    Close,
    /// The settings to save (`Partial<MediaPlayback>`).
    Save(Map<String, Value>),
}

/// A trim handle being dragged.
#[derive(Clone)]
struct HandleDrag(Edge);
impl Render for HandleDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

const COLOURS: [(&str, &str, f32, f32); 3] = [
    ("brightness", "Brightness", 0.25, 1.75),
    ("contrast", "Contrast", 0.25, 1.75),
    ("saturation", "Saturation", 0., 2.),
];

pub struct Inspector {
    /// The modal's focus scope, set by the dock that opens it
    /// ([`Self::set_modal_focus`]), which gives focus back on close.
    modal_focus: FocusHandle,
    item: MediaItem,
    initial: MediaPlayback,
    panel: Panel,
    grade: Grade,
    fit: Option<MediaFit>,
    player: Option<Entity<PreviewPlayer>>,
    start: Entity<InputState>,
    end: Entity<InputState>,
    rate: f64,
    looped: bool,
    saving: bool,
    error: Option<String>,
    rate_slider: Entity<SliderState>,
    colour_sliders: Vec<Entity<SliderState>>,
    playhead: Entity<SliderState>,
    dimensions: Option<(u32, u32)>,
    /// Drags the dialog by its header (the shared movable dialog's handle).
    frame: Entity<kairo_ui_kit::components::modal::MovableDialog>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<InspectorEvent> for Inspector {}

impl Inspector {
    /// Uses the opener's modal focus scope (`ModalFocus::handle`).
    pub fn set_modal_focus(&mut self, handle: FocusHandle) {
        self.modal_focus = handle;
    }

    pub fn new(
        backend: Arc<dyn MediaBackend>,
        item: MediaItem,
        playback: MediaPlayback,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let video = item.kind == MediaKind::Video;
        let player = video.then(|| {
            let (b, path) = (backend.clone(), item.path.clone());
            cx.new(|cx| PreviewPlayer::new(b, path, WIDTH, false, cx))
        });
        let start_text = rows::number_text(playback.start_seconds.unwrap_or(0.));
        let end_text = playback
            .end_seconds
            .flatten()
            .map(rows::number_text)
            .unwrap_or_default();
        let start = cx.new(|cx| InputState::new(window, cx).default_value(start_text));
        let end = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("File end")
                .default_value(end_text)
        });
        let rate = playback.rate.unwrap_or(1.);
        let rate_slider = cx.new(|_| {
            SliderState::new()
                .min(0.5)
                .max(2.)
                .step(0.05)
                .default_value(rate as f32)
        });
        let grade = grade_of(&playback);
        let colour_sliders: Vec<_> = COLOURS
            .iter()
            .map(|(key, _, min, max)| {
                let v = match *key {
                    "brightness" => grade.brightness,
                    "contrast" => grade.contrast,
                    _ => grade.saturation,
                } as f32;
                cx.new(|_| {
                    SliderState::new()
                        .min(*min)
                        .max(*max)
                        .step(0.05)
                        .default_value(v)
                })
            })
            .collect();
        let playhead = cx.new(|_| SliderState::new().min(0.).max(1000.).step(1.));
        let mut subs = Vec::new();
        for input in [&start, &end] {
            subs.push(cx.subscribe(input, |this, _, e: &InputEvent, cx| {
                if let InputEvent::Change = e {
                    this.range_changed(cx);
                }
            }));
        }
        subs.push(cx.subscribe(&rate_slider, |this, _, e: &SliderEvent, cx| {
            if let SliderEvent::Change(v) = e {
                this.rate = (v.start() as f64 * 100.).round() / 100.;
                this.range_changed(cx);
            }
        }));
        for (i, s) in colour_sliders.iter().enumerate() {
            subs.push(cx.subscribe(s, move |this, _, e: &SliderEvent, cx| {
                if let SliderEvent::Change(v) = e {
                    let v = (v.start() as f64 * 100.).round() / 100.;
                    match i {
                        0 => this.grade.brightness = v,
                        1 => this.grade.contrast = v,
                        _ => this.grade.saturation = v,
                    }
                    this.regrade(cx);
                }
            }));
        }
        subs.push(cx.subscribe(&playhead, |this, _, e: &SliderEvent, cx| {
            if let SliderEvent::Change(v) = e {
                let d = this.duration(cx);
                this.seek(v.start() as f64 / 1000. * d, cx);
            }
        }));
        if let Some(p) = &player {
            subs.push(cx.observe(p, |this, p, cx| {
                if this.dimensions.is_none()
                    && let Some(image) = p.read(cx).image()
                {
                    let s = image.size(0);
                    this.dimensions = Some((s.width.0 as u32, s.height.0 as u32));
                }
                cx.notify();
            }));
            subs.push(
                cx.subscribe(p, |this, _, e: &crate::player::PlayerEvent, cx| {
                    let crate::player::PlayerEvent::Failed(_) = e;
                    this.error = Some("This video could not be opened for preview.".into());
                    cx.notify();
                }),
            );
        }
        let mut this = Self {
            modal_focus: cx.focus_handle(),
            panel: if video {
                Panel::Playback
            } else {
                Panel::Appearance
            },
            fit: playback.fit.flatten(),
            grade,
            initial: playback,
            item,
            player,
            start,
            end,
            rate,
            looped: false,
            saving: false,
            error: None,
            rate_slider,
            colour_sliders,
            playhead,
            dimensions: None,
            frame: {
                let frame = cx.new(|cx| {
                    kairo_ui_kit::components::modal::MovableDialog::new(
                        "media-inspector-frame",
                        "Media Inspector",
                        gpui_kit::size(px(896.), px(800.)),
                        window,
                        cx,
                    )
                });
                subs.push(cx.observe(&frame, |_, _, cx| cx.notify()));
                frame
            },
            _subscriptions: subs,
        };
        this.looped = this.initial.looped;
        this.range_changed(cx);
        this.regrade(cx);
        this
    }

    pub fn item_id(&self) -> &str {
        &self.item.id
    }
    pub fn panel(&self) -> Panel {
        self.panel
    }
    pub fn grade(&self) -> Grade {
        self.grade
    }
    pub fn player(&self) -> Option<&Entity<PreviewPlayer>> {
        self.player.as_ref()
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    fn duration(&self, cx: &gpui_kit::App) -> f64 {
        self.player
            .as_ref()
            .and_then(|p| p.read(cx).duration())
            .unwrap_or(0.)
    }

    pub fn trim(&self, cx: &gpui_kit::App) -> Trim {
        Trim {
            start: self.start.read(cx).value().to_string(),
            end: self.end.read(cx).value().to_string(),
            duration: self.duration(cx),
        }
    }

    fn trim_changed(&self, cx: &gpui_kit::App) -> bool {
        let t = self.trim(cx);
        t.start != rows::number_text(self.initial.start_seconds.unwrap_or(0.))
            || t.end
                != self
                    .initial
                    .end_seconds
                    .flatten()
                    .map(rows::number_text)
                    .unwrap_or_default()
    }

    /// Save is allowed unless a changed trim is invalid.
    pub fn can_save(&self, cx: &gpui_kit::App) -> bool {
        self.item.kind != MediaKind::Video || !self.trim_changed(cx) || self.trim(cx).valid()
    }

    fn range_changed(&mut self, cx: &mut Context<Self>) {
        let t = self.trim(cx);
        if let Some(p) = &self.player {
            let (start, end) = t.track();
            p.read(cx).set_range(Range {
                start: if t.valid() { t.in_point() } else { start },
                end: Some(if t.valid() { t.out_point() } else { end })
                    .filter(|e| *e < t.duration || t.duration == 0.),
                looped: self.looped,
                rate: self.rate,
            });
        }
        cx.notify();
    }

    /// Colour sliders re-grade the frames; the video keeps playing.
    fn regrade(&mut self, cx: &mut Context<Self>) {
        if let Some(p) = &self.player {
            p.read(cx).set_grade(self.grade);
        }
        cx.notify();
    }

    pub fn set_panel(&mut self, panel: Panel, cx: &mut Context<Self>) {
        self.panel = panel;
        cx.notify();
    }

    pub fn set_grade(&mut self, grade: Grade, window: &mut Window, cx: &mut Context<Self>) {
        self.grade = grade;
        for (i, s) in self.colour_sliders.iter().enumerate() {
            let v = [grade.brightness, grade.contrast, grade.saturation][i] as f32;
            s.update(cx, |s, cx| s.set_value(v, window, cx));
        }
        self.regrade(cx);
    }

    pub fn set_fit(&mut self, fit: Option<MediaFit>, cx: &mut Context<Self>) {
        self.fit = fit;
        cx.notify();
    }

    pub fn set_loop(&mut self, looped: bool, cx: &mut Context<Self>) {
        self.looped = looped;
        self.range_changed(cx);
    }

    pub fn set_trim_field(
        &mut self,
        edge: Edge,
        value: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = if edge == Edge::Start {
            &self.start
        } else {
            &self.end
        };
        input.update(cx, |s, cx| s.set_value(value, window, cx));
        self.range_changed(cx);
    }

    /// A handle moved (drag or keys): the field follows and the preview
    /// shows that frame, paused.
    pub fn trim_to(&mut self, edge: Edge, time: f64, window: &mut Window, cx: &mut Context<Self>) {
        let d = self.duration(cx);
        let value = match edge {
            Edge::Start => rows::number_text(time),
            Edge::End => rows::end_field(time, d),
        };
        self.set_trim_field(edge, value, window, cx);
        self.seek(time, cx);
    }

    pub fn seek(&mut self, seconds: f64, cx: &mut Context<Self>) {
        let d = self.duration(cx);
        if d <= 0. {
            return;
        }
        if let Some(p) = &self.player {
            p.update(cx, |p, _| {
                p.pause();
                p.seek(seconds.clamp(0., d));
            });
        }
        cx.notify();
    }

    pub fn toggle_play(&mut self, cx: &mut Context<Self>) {
        let Some(p) = self.player.clone() else { return };
        if p.read(cx).playing() {
            p.read(cx).pause();
            return;
        }
        if !self.trim(cx).valid() {
            return;
        }
        self.range_changed(cx);
        p.read(cx).play();
    }

    fn position(&self, cx: &gpui_kit::App) -> f64 {
        self.player.as_ref().map_or(0., |p| p.read(cx).position())
    }

    /// Use playhead.
    pub fn use_playhead(&mut self, edge: Edge, window: &mut Window, cx: &mut Context<Self>) {
        let (pos, d) = (self.position(cx), self.duration(cx));
        let value = match edge {
            Edge::Start => rows::number_text(pos),
            Edge::End => rows::end_field(pos, d),
        };
        self.set_trim_field(edge, value, window, cx);
    }

    /// The patch Save sends.
    pub fn patch(&self, cx: &gpui_kit::App) -> Map<String, Value> {
        let mut patch = Map::new();
        patch.insert("brightness".into(), json!(self.grade.brightness));
        patch.insert("contrast".into(), json!(self.grade.contrast));
        patch.insert("saturation".into(), json!(self.grade.saturation));
        patch.insert(
            "fit".into(),
            self.fit.map_or(Value::Null, |f| json!(f.as_str())),
        );
        if self.item.kind == MediaKind::Video {
            patch.insert("rate".into(), json!(self.rate));
            patch.insert("loop".into(), json!(self.looped));
            let t = self.trim(cx);
            if t.valid() {
                patch.insert("startSeconds".into(), json!(t.in_point()));
                patch.insert(
                    "endSeconds".into(),
                    if t.end.trim().is_empty() {
                        Value::Null
                    } else {
                        json!(t.out_point())
                    },
                );
            }
        }
        patch
    }

    pub fn save(&mut self, cx: &mut Context<Self>) {
        if !self.can_save(cx) || self.saving {
            return;
        }
        self.saving = true;
        self.error = None;
        cx.emit(InspectorEvent::Save(self.patch(cx)));
        cx.notify();
    }

    pub fn save_failed(&mut self, error: String, cx: &mut Context<Self>) {
        self.saving = false;
        self.error = Some(error);
        cx.notify();
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        if !self.saving {
            cx.emit(InspectorEvent::Close);
        }
    }

    fn render_trim(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let t = self.trim(cx);
        let d = t.duration;
        let (start, end) = t.track();
        let pct = |v: f64| if d > 0. { (v / d * 100.) as f32 } else { 0. };
        let handle = |edge: Edge, cx: &mut Context<Self>| {
            let name = if edge == Edge::Start { "start" } else { "end" };
            let time = if edge == Edge::Start { start } else { end };
            let at = if d > 0. {
                pct(time)
            } else if edge == Edge::Start {
                0.
            } else {
                100.
            };
            div()
                .id(SharedString::from(ids::handle(name)))
                .test_support()
                .role(Role::Slider)
                .aria_label(if edge == Edge::Start {
                    "Trim start"
                } else {
                    "Trim end"
                })
                .tab_index(0)
                .absolute()
                .top_0()
                .bottom_0()
                .w_3()
                .left(gpui_kit::relative(at / 100.))
                .when(edge == Edge::Start, |d| d.ml(px(-12.)))
                .rounded(radius::sm())
                .bg(k.accent(Step::S400))
                .cursor_ew_resize()
                .when(d <= 0., |d| d.opacity(0.4))
                .on_drag(HandleDrag(edge), |h, _, _, cx| cx.new(|_| h.clone()))
                .on_key_down(cx.listener(move |this, e: &KeyDownEvent, window, cx| {
                    let t = this.trim(cx);
                    let (s, en) = t.track();
                    if let Some(v) = rows::trim_key(
                        edge,
                        &e.keystroke.key,
                        e.keystroke.modifiers.shift,
                        s,
                        en,
                        t.duration,
                    ) {
                        this.trim_to(edge, v, window, cx);
                        cx.stop_propagation();
                    }
                }))
        };
        div()
            .px_3()
            .child(
                div()
                    .id(ids::TRACK)
                    .test_support()
                    .relative()
                    .my_3()
                    .h_7()
                    .rounded(radius::sm())
                    .bg(k.surface_tertiary)
                    .on_drag_move(
                        cx.listener(|this, e: &DragMoveEvent<HandleDrag>, window, cx| {
                            let b = e.bounds;
                            let w = f32::from(b.size.width);
                            if w <= 0. {
                                return;
                            }
                            let t = this.trim(cx);
                            let seconds =
                                (f32::from(e.event.position.x - b.left()) / w) as f64 * t.duration;
                            let (s, en) = t.track();
                            let edge = e.drag(cx).0;
                            let v = rows::clamp_trim(
                                edge,
                                rows::snap_trim(seconds, t.duration),
                                s,
                                en,
                                t.duration,
                            );
                            this.trim_to(edge, v, window, cx);
                        }),
                    )
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left(gpui_kit::relative(pct(start) / 100.))
                            .right(gpui_kit::relative(if d > 0. {
                                1. - pct(end) / 100.
                            } else {
                                0.
                            }))
                            .border_t_2()
                            .border_b_2()
                            .border_color(k.accent(Step::S400))
                            .bg(k.accent(Step::S400).opacity(0.2)),
                    )
                    .child(handle(Edge::Start, cx))
                    .child(handle(Edge::End, cx)),
            )
            .into_any_element()
    }

    fn render_preview(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let fit = match self.fit {
            Some(MediaFit::Cover) => ObjectFit::Cover,
            Some(MediaFit::Fill) => ObjectFit::Fill,
            _ => ObjectFit::Contain,
        };
        let picture: AnyElement = match (&self.player, self.item.kind) {
            (Some(p), _) => match p.read(cx).image() {
                Some(image) => img(image.clone())
                    .size_full()
                    .object_fit(fit)
                    .into_any_element(),
                None => div().into_any_element(),
            },
            // An image preview is the file itself (its grade applies on screen).
            (None, _) => img(std::path::PathBuf::from(&self.item.path))
                .size_full()
                .object_fit(fit)
                .into_any_element(),
        };
        v_flex()
            .id("media-inspector-preview")
            .min_w_0()
            .aria_label("Private preview")
            .child(
                div()
                    .w_full()
                    .h(px(320.))
                    .rounded(radius::md())
                    .bg(k.monitor())
                    .overflow_hidden()
                    .child(picture),
            )
            .when(self.player.is_some(), |d| {
                let pos = self.position(cx);
                let dur = self.duration(cx);
                let playing = self.player.as_ref().is_some_and(|p| p.read(cx).playing());
                let valid = self.trim(cx).valid();
                let (back, play, ahead) = (cx.entity(), cx.entity(), cx.entity());
                d.child(
                    div()
                        .mt_3()
                        .text_size(text::XS.size)
                        .text_color(k.text(TextStep::T400))
                        .child("Drag the handles to set the start and end."),
                )
                .child(self.render_trim(cx))
                .child(
                    div().mt_2().child(
                        Slider::new(&self.playhead)
                            .disabled(dur <= 0.)
                            .accessibility_label("Preview playhead"),
                    ),
                )
                .child(
                    h_flex()
                        .mt_2()
                        .gap_2()
                        .child(
                            Button::new(ids::BACK)
                                .cursor_pointer()
                                .small()
                                .label("−10s")
                                .disabled(dur <= 0.)
                                .on_click(move |_, _, cx| {
                                    back.update(cx, |i, cx| {
                                        let p = i.position(cx);
                                        i.seek(p - 10., cx)
                                    })
                                }),
                        )
                        .child(
                            Button::new(ids::PLAY)
                                .cursor_pointer()
                                .small()
                                .icon(Icon::new(if playing {
                                    KairoIcon::Pause
                                } else {
                                    KairoIcon::Play
                                }))
                                .accessibility_label(if playing {
                                    "Pause preview"
                                } else {
                                    "Play preview"
                                })
                                .disabled(!valid && !playing)
                                .on_click(move |_, _, cx| {
                                    play.update(cx, |i, cx| i.toggle_play(cx))
                                }),
                        )
                        .child(
                            Button::new(ids::AHEAD)
                                .cursor_pointer()
                                .small()
                                .label("+10s")
                                .disabled(dur <= 0.)
                                .on_click(move |_, _, cx| {
                                    ahead.update(cx, |i, cx| {
                                        let p = i.position(cx);
                                        i.seek(p + 10., cx)
                                    })
                                }),
                        )
                        .child(
                            div()
                                .id(ids::CLOCK)
                                .test_support()
                                .ml_auto()
                                .text_size(text::XS.size)
                                .text_color(k.text(TextStep::T400))
                                .child(format!(
                                    "{} / {}",
                                    format_media_clock(pos),
                                    format_media_clock(dur)
                                )),
                        ),
                )
            })
            .into_any_element()
    }

    fn render_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let label = |t: &'static str| {
            div()
                .text_size(text::XS.size)
                .text_color(k.text(TextStep::T400))
                .child(t)
        };
        match self.panel {
            Panel::Playback => {
                let t = self.trim(cx);
                let dur = t.duration;
                let (us, ue, lp) = (cx.entity(), cx.entity(), cx.entity());
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_size(text::SM.size)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Playback settings"),
                    )
                    .child(label("Start (seconds)"))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                div().id(ids::START).test_support().flex_1().child(
                                    Input::new(&self.start)
                                        .kairo_size()
                                        .small()
                                        .aria_label("Start (seconds)"),
                                ),
                            )
                            .child(
                                Button::new(ids::USE_START)
                                    .cursor_pointer()
                                    .small()
                                    .label("Use playhead")
                                    .accessibility_label("Set start to playhead")
                                    .disabled(dur <= 0.)
                                    .on_click(move |_, w, cx| {
                                        us.update(cx, |i, cx| i.use_playhead(Edge::Start, w, cx))
                                    }),
                            ),
                    )
                    .child(label("End (seconds)"))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                div().id(ids::END).test_support().flex_1().child(
                                    Input::new(&self.end)
                                        .kairo_size()
                                        .small()
                                        .aria_label("End (seconds)"),
                                ),
                            )
                            .child(
                                Button::new(ids::USE_END)
                                    .cursor_pointer()
                                    .small()
                                    .label("Use playhead")
                                    .accessibility_label("Set end to playhead")
                                    .disabled(dur <= 0.)
                                    .on_click(move |_, w, cx| {
                                        ue.update(cx, |i, cx| i.use_playhead(Edge::End, w, cx))
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .id(ids::NOTE)
                            .test_support()
                            .text_size(text::XS.size)
                            .text_color(k.text(TextStep::T400))
                            .child(t.note()),
                    )
                    .child(label("Speed").child(format!(" · {}×", rows::number_text(self.rate))))
                    .child(Slider::new(&self.rate_slider).accessibility_label("Speed"))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Switch::new(ids::LOOP)
                                    .checked(self.looped)
                                    .accessibility_label("Loop selected range")
                                    .on_change(move |v, _, cx| {
                                        lp.update(cx, |i, cx| i.set_loop(*v, cx))
                                    }),
                            )
                            .child(div().text_size(text::SM.size).child("Loop selected range")),
                    )
                    .into_any_element()
            }
            Panel::Appearance => {
                let fit_label = match self.fit {
                    None => "Use output setting",
                    Some(MediaFit::Contain) => "Scale to fit",
                    Some(MediaFit::Cover) => "Fill and crop",
                    Some(MediaFit::Fill) => "Stretch to fill",
                };
                let e = cx.entity();
                let reset = cx.entity();
                let sliders: Vec<AnyElement> = COLOURS
                    .iter()
                    .zip(&self.colour_sliders)
                    .enumerate()
                    .map(|(i, ((_, name, _, _), s))| {
                        let v = [
                            self.grade.brightness,
                            self.grade.contrast,
                            self.grade.saturation,
                        ][i];
                        h_flex()
                            .gap_2()
                            .text_size(text::XS.size)
                            .child(div().w_20().child(*name))
                            .child(
                                div()
                                    .flex_1()
                                    .child(Slider::new(s).accessibility_label(*name)),
                            )
                            .child(
                                div()
                                    .w_11()
                                    .text_right()
                                    .text_color(k.text(TextStep::T400))
                                    .child(format!("{}%", (v * 100.).round())),
                            )
                            .into_any_element()
                    })
                    .collect();
                v_flex()
                    .gap_4()
                    .child(div().text_size(text::SM.size).font_weight(FontWeight::SEMIBOLD).child("Appearance"))
                    .child(label("Scaling"))
                    .child(Button::new("media-inspector-fit").cursor_pointer().small().label(fit_label).dropdown_caret(true).popover_trigger().dropdown_menu(move |mut menu, _, _| {
                        for (fit, name) in [(None, "Use output setting"), (Some(MediaFit::Contain), "Scale to fit"), (Some(MediaFit::Cover), "Fill and crop"), (Some(MediaFit::Fill), "Stretch to fill")] {
                            let e = e.clone();
                            menu = menu.item(PopupMenuItem::new(name).on_click(move |_, _, cx| e.update(cx, |i, cx| i.set_fit(fit, cx))));
                        }
                        menu
                    }))
                    .when(self.fit.is_none(), |d| d.child(div().text_size(text::XS.size).text_color(k.text(TextStep::T400)).child("The preview shows the full source. Each output uses its own scaling.")))
                    .child(div().text_size(text::XS.size).font_weight(FontWeight::SEMIBOLD).child("Adjust color"))
                    .children(sliders)
                    .child(Button::new(ids::RESET).cursor_pointer().small().label("Reset appearance").on_click(move |_, w, cx| reset.update(cx, |i, cx| { i.set_grade(Grade::default(), w, cx); i.set_fit(None, cx) })))
                    .into_any_element()
            }
            Panel::Details => {
                let dims = self
                    .dimensions
                    .map_or_else(|| "Unavailable".to_owned(), |(w, h)| format!("{w} × {h}"));
                let dur = self.duration(cx);
                let row = |a: &str, b: String| {
                    h_flex()
                        .gap_3()
                        .text_size(text::XS.size)
                        .child(
                            div()
                                .w_20()
                                .text_color(k.text(TextStep::T400))
                                .child(a.to_owned()),
                        )
                        .child(div().flex_1().child(b))
                };
                v_flex()
                    .gap_2()
                    .child(row("Format", self.item.ext.to_uppercase()))
                    .child(row("File size", rows::format_size(self.item.size_bytes)))
                    .child(row("Resolution", dims))
                    .when(self.item.kind == MediaKind::Video, |d| {
                        d.child(row(
                            "Duration",
                            if dur > 0. {
                                format!("{dur:.2} s")
                            } else {
                                "Unavailable".into()
                            },
                        ))
                    })
                    .child(
                        div()
                            .mt_2()
                            .text_size(text::XS.size)
                            .text_color(k.text(TextStep::T400))
                            .child("File path"),
                    )
                    .child(div().text_size(text::XS.size).child(self.item.path.clone()))
                    .into_any_element()
            }
        }
    }
}

impl Render for Inspector {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(p) = &self.player {
            p.update(cx, |p, _| p.release_retired(window));
            let (pos, dur) = (p.read(cx).position(), p.read(cx).duration().unwrap_or(0.));
            if dur > 0. {
                let v = (pos / dur * 1000.) as f32;
                self.playhead.update(cx, |s, cx| s.set_value(v, window, cx));
            }
        }
        let k = *cx.kairo();
        let video = self.item.kind == MediaKind::Video;
        let panels: &[Panel] = if video {
            &[Panel::Playback, Panel::Appearance, Panel::Details]
        } else {
            &[Panel::Appearance, Panel::Details]
        };
        let tabs: Vec<AnyElement> = panels
            .iter()
            .map(|p| {
                let (e, p) = (cx.entity(), *p);
                let on = self.panel == p;
                div()
                    .id(SharedString::from(ids::panel(p.name())))
                    .test_support()
                    .role(Role::Tab)
                    .aria_label(p.name())
                    .aria_selected(on)
                    .flex_1()
                    .px_2()
                    .py_1p5()
                    .text_center()
                    .rounded(radius::sm())
                    .text_size(text::XS.size)
                    .map(|d| {
                        if on {
                            d.bg(k.surface_elevated).text_color(k.text_primary)
                        } else {
                            d.text_color(k.text(TextStep::T400))
                        }
                    })
                    .cursor_pointer()
                    .on_click(move |_, _, cx| e.update(cx, |i, cx| i.set_panel(p, cx)))
                    .child(p.name())
                    .into_any_element()
            })
            .collect();
        let can_save = self.can_save(cx);
        let (cancel, save, backdrop) = (cx.entity(), cx.entity(), cx.entity());
        // Window-level, so it centres on the app rather than the media dock
        // it opens from; the header drags it.
        let viewport = window.viewport_size();
        let offset = self.frame.read(cx).offset();
        let handle = self
            .frame
            .update(cx, |f, cx| f.drag_handle("media-inspector-drag", cx));
        deferred(
            gpui_kit::anchored()
                .position(gpui_kit::point(px(0.), px(0.)))
                .child(
                    div()
                        .w(viewport.width)
                        .h(viewport.height)
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(k.scrim())
                        .occlude()
                        .on_mouse_down(gpui_kit::MouseButton::Left, move |_, _, cx| {
                            backdrop.update(cx, |i, cx| i.cancel(cx))
                        })
                        .child(kairo_ui_kit::motion::enter(
                            v_flex()
                                .id(ids::DIALOG)
                                .test_support()
                                .role(Role::Dialog)
                                .aria_label("Media Inspector")
                                .relative()
                                .left(px(offset.x))
                                .top(px(offset.y))
                                .w(px(896.))
                                .max_w(viewport.width - px(32.))
                                .max_h((viewport.height - px(40.)).min(px(800.)))
                                .overflow_y_scroll()
                                .rounded(radius::xl())
                                .border_1()
                                .border_color(k.surface_border)
                                .bg(k.surface_elevated)
                                .on_mouse_down(gpui_kit::MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation()
                                })
                                .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                                    if e.keystroke.key == "escape" {
                                        this.cancel(cx);
                                        cx.stop_propagation();
                                    }
                                }))
                                .child(
                                    handle
                                        .cursor_grab()
                                        .flex()
                                        .flex_col()
                                        .px_5()
                                        .py_4()
                                        .border_b_1()
                                        .border_color(k.surface_border)
                                        .child(
                                            div()
                                                .text_size(text::BASE.size)
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .child("Media Inspector"),
                                        )
                                        .child(
                                            div()
                                                .truncate()
                                                .text_size(text::XS.size)
                                                .text_color(k.text(TextStep::T400))
                                                .child(self.item.name.clone()),
                                        ),
                                )
                                .child(
                                    h_flex()
                                        .items_start()
                                        .gap_5()
                                        .p_5()
                                        .child(
                                            div().flex_1().min_w_0().child(self.render_preview(cx)),
                                        )
                                        .child(
                                            v_flex()
                                                .w(px(272.))
                                                .flex_none()
                                                .child(
                                                    h_flex()
                                                        .mb_4()
                                                        .gap_1()
                                                        .rounded(radius::md())
                                                        .bg(k.surface_secondary)
                                                        .p_1()
                                                        .children(tabs),
                                                )
                                                .child(self.render_panel(cx)),
                                        ),
                                )
                                .children(self.error.clone().map(|e| {
                                    div()
                                        .id(ids::ERROR)
                                        .test_support()
                                        .role(Role::Alert)
                                        .px_5()
                                        .pb_3()
                                        .text_size(text::SM.size)
                                        .text_color(k.danger_text)
                                        .child(e)
                                }))
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .px_5()
                                        .py_3()
                                        .border_t_1()
                                        .border_color(k.surface_border)
                                        .child(
                                            div()
                                                .mr_auto()
                                                .text_size(text::XS.size)
                                                .text_color(k.text(TextStep::T400))
                                                .child("Applies the next time you cue this item."),
                                        )
                                        .child(
                                            Button::new(ids::CANCEL)
                                                .cursor_pointer()
                                                .small()
                                                .label("Cancel")
                                                .disabled(self.saving)
                                                .on_click(move |_, _, cx| {
                                                    cancel.update(cx, |i, cx| i.cancel(cx))
                                                }),
                                        )
                                        .child(
                                            Button::new(ids::SAVE)
                                                .cursor_pointer()
                                                .primary()
                                                .small()
                                                .label(if self.saving {
                                                    "Saving…"
                                                } else {
                                                    "Save changes"
                                                })
                                                .disabled(!can_save || self.saving)
                                                .on_click(move |_, _, cx| {
                                                    save.update(cx, |i, cx| i.save(cx))
                                                }),
                                        ),
                                )
                                .focus_trap(ids::DIALOG, &self.modal_focus),
                            "media-inspector-enter",
                            cx,
                        )),
                ),
        )
        .with_priority(2)
    }
}
