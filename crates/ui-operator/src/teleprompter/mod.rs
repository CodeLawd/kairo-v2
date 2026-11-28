//! The Teleprompter section: a library of saved scripts, the script editor,
//! and the live panel that shows and drives what the presenter reads.
//!
//! The primary task is reading a script on stage, so the live panel (what is
//! on the display, where the presenter is, play/pause) stays visible beside
//! the editor. Writing and importing happen in the editor; nothing reaches the
//! display until the operator loads it.

mod library;
mod live;

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::component::slider::{SliderEvent, SliderState};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;
use kairo_domain::teleprompter::{Script, clamp_speed};
use kairo_engine_proto::{StageOperation, StagePosition};
use kairo_ui_kit::KairoThemeExt;
use kairo_ui_kit::components::{ConfirmRequest, confirm};

/// Text size, speed and mirroring sent with a script.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Layout {
    pub font_px: f32,
    /// Shaped lines per second.
    pub speed: f64,
    pub mirror: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            font_px: 64.0,
            speed: kairo_domain::teleprompter::DEFAULT_SPEED,
            mirror: false,
        }
    }
}

pub enum Request {
    List,
    Save {
        id: Option<String>,
        title: String,
        body: String,
    },
    /// Save, load the script for the prompter and, with a target, put it
    /// on that screen.
    Publish {
        id: Option<String>,
        title: String,
        body: String,
        layout: Layout,
        target: Option<String>,
    },
    GoLive {
        target: String,
    },
    /// Take the prompter off its screen.
    TakeOff,
    Delete {
        id: String,
    },
    Import,
    Transport(StageOperation),
    Layout(Layout),
    Status,
}

/// A screen the prompter can be sent to: an audience screen or a stage
/// display in Teleprompter mode.
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub id: String,
    pub name: String,
    pub ready: bool,
    /// Why the screen is not ready; empty when it is.
    pub reason: String,
}

pub enum Reply {
    Scripts(Vec<Script>),
    Saved(Script),
    Published(Script),
    Deleted(String),
    Import(Option<(String, String, Vec<String>)>),
    Transport,
    Status {
        targets: Vec<Target>,
        /// The screen the prompter is on now.
        live: Option<String>,
        position: Option<StagePosition>,
        /// The saved script the display shows.
        script_id: Option<String>,
    },
}

pub trait Backend: Send + Sync {
    fn request(&self, request: Request, done: Box<dyn FnOnce(Result<Reply, String>) + Send>);
}

const DRAFT_KEY: &str = "kairo.teleprompter.draft";
const LAYOUT_KEY: &str = "kairo.teleprompter.layout";
const TARGET_KEY: &str = "kairo.teleprompter.target";
/// The display's text column is laid out 1920 pixels wide.
const DISPLAY_WIDTH: f32 = 1920.0;
/// The display's line height over its text size.
const LINE_HEIGHT: f32 = 1.25;
/// Where the reading line sits, from the top of the display.
const GUIDE: f32 = 0.32;
/// How long a transport click's result is trusted over the polled status.
const OPTIMISTIC: Duration = Duration::from_millis(1500);
/// Speed slider steps: one step is 0.1 lines per second.
const SPEED_STEP: f64 = 0.1;
const SPEED_LEVELS: (f32, f32) = (1.0, 30.0);
const FONT_RANGE: (f32, f32) = (24.0, 160.0);
/// Lines moved by Page Up / Page Down.
const PAGE_LINES: f64 = 5.0;

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Draft {
    id: Option<String>,
    title: String,
    body: String,
}

/// The text the display shows, as this session loaded it.
#[derive(Clone)]
struct Published {
    id: String,
    title: String,
    body: String,
}

/// A step that waits for the operator to discard unsaved changes.
enum Pending {
    Open(Option<Script>),
    Import { title: String, body: String },
}

pub struct Teleprompter {
    backend: Arc<dyn Backend>,
    state: Arc<dyn crate::UiState>,
    scripts: Vec<Script>,
    selected: Option<String>,
    /// Title and body as last opened or saved, for the unsaved state.
    baseline: (String, String),
    title: Entity<InputState>,
    body: Entity<TextareaState>,
    search: Entity<InputState>,
    preview_body: Entity<TextareaState>,
    import: Option<(String, Vec<String>)>,
    published: Option<Published>,
    position: Option<StagePosition>,
    targets: Vec<Target>,
    /// The screen chosen to receive the prompter.
    target: Option<String>,
    /// The screen the prompter is on now.
    live: Option<String>,
    on_display: Option<String>,
    /// The preview's scroll, in preview pixels from the top of the script.
    offset: f32,
    last_tick: Option<Instant>,
    ticker: Option<Task<()>>,
    /// Play state shown on the controls; the engine's report replaces it
    /// once `optimistic_until` passes.
    playing: bool,
    optimistic_until: Option<Instant>,
    layout: Layout,
    speed_slider: Entity<SliderState>,
    font_slider: Entity<SliderState>,
    preview_scroll: ScrollHandle,
    busy: bool,
    error: Option<String>,
    focus: FocusHandle,
    /// The media dock under the page (as Bible and Lyrics draw it).
    media_dock: Option<AnyView>,
    active: bool,
    draft_save: Option<Task<()>>,
    _poll: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl Teleprompter {
    /// The media dock, drawn under the page and beside its right panel.
    pub fn set_dock(&mut self, dock: Option<AnyView>, cx: &mut Context<Self>) {
        self.media_dock = dock;
        cx.notify();
    }

    pub fn new(
        backend: Arc<dyn Backend>,
        state: Arc<dyn crate::UiState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let draft: Draft = state
            .get(DRAFT_KEY)
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_default();
        let target: Option<String> = state
            .get(TARGET_KEY)
            .and_then(|value| serde_json::from_value(value).ok());
        let layout: Layout = state
            .get(LAYOUT_KEY)
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_default();
        let title = cx.new(|cx| InputState::new(window, cx).placeholder("Untitled script"));
        let body = cx.new(|cx| {
            TextareaState::new(window, cx).placeholder("Type or paste the script, or import one")
        });
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search scripts"));
        title.update(cx, |input, cx| input.set_value(draft.title, window, cx));
        body.update(cx, |input, cx| input.set_value(draft.body, window, cx));
        let speed_slider = cx.new(|_| {
            SliderState::new()
                .min(SPEED_LEVELS.0)
                .max(SPEED_LEVELS.1)
                .step(1.0)
                .default_value(speed_level(layout.speed))
        });
        let font_slider = cx.new(|_| {
            SliderState::new()
                .min(FONT_RANGE.0)
                .max(FONT_RANGE.1)
                .step(4.0)
                .default_value(layout.font_px.clamp(FONT_RANGE.0, FONT_RANGE.1))
        });
        let subscriptions = vec![
            cx.subscribe_in(&title, window, |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.save_draft(window, cx);
                }
            }),
            cx.subscribe_in(&body, window, |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.save_draft(window, cx);
                }
            }),
            cx.subscribe(&search, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe_in(
                &speed_slider,
                window,
                |this, _, event: &SliderEvent, window, cx| match event {
                    SliderEvent::Change(value) => {
                        this.layout.speed = level_speed(value.start());
                        cx.notify();
                    }
                    SliderEvent::Release(value) => {
                        this.set_speed(level_speed(value.start()), window, cx)
                    }
                },
            ),
            cx.subscribe_in(
                &font_slider,
                window,
                |this, _, event: &SliderEvent, window, cx| match event {
                    SliderEvent::Change(value) => {
                        this.layout.font_px = value.start();
                        cx.notify();
                    }
                    SliderEvent::Release(value) => {
                        this.layout.font_px = value.start();
                        this.send_layout(window, cx);
                    }
                },
            ),
        ];
        let mut this = Self {
            backend,
            state,
            scripts: Vec::new(),
            selected: draft.id,
            baseline: (String::new(), String::new()),
            title,
            body,
            search,
            preview_body: cx.new(|cx| TextareaState::new(window, cx)),
            import: None,
            published: None,
            position: None,
            targets: Vec::new(),
            target,
            live: None,
            on_display: None,
            offset: 0.0,
            last_tick: None,
            ticker: None,
            playing: false,
            optimistic_until: None,
            layout,
            speed_slider,
            font_slider,
            preview_scroll: ScrollHandle::new(),
            busy: false,
            error: None,
            focus: cx.focus_handle(),
            media_dock: None,
            active: false,
            draft_save: None,
            _poll: cx.spawn_in(window, async move |entity, cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(250))
                        .await;
                    if entity
                        .update_in(cx, |this, window, cx| {
                            if this.active {
                                this.request(Request::Status, window, cx);
                            }
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            }),
            _subscriptions: subscriptions,
        };
        this.request(Request::List, window, cx);
        this
    }

    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    fn current(&self, cx: &App) -> (String, String) {
        (
            self.title.read(cx).value().to_string(),
            self.body.read(cx).value().to_string(),
        )
    }

    fn dirty(&self, cx: &App) -> bool {
        let (title, body) = self.current(cx);
        (title.trim(), body.as_str()) != (self.baseline.0.trim(), self.baseline.1.as_str())
    }

    fn selected_script(&self) -> Option<&Script> {
        let id = self.selected.as_deref()?;
        self.scripts.iter().find(|script| script.id == id)
    }

    /// What the preview shows: the script on the screen while live,
    /// otherwise the script in the editor, for rehearsal.
    fn preview_text(&self, cx: &App) -> (String, String) {
        if self.live.is_some()
            && let Some(shown) = self.display_text()
        {
            return shown;
        }
        let (title, body) = self.current(cx);
        (title, body)
    }

    /// The text the display shows: what this session loaded, or the saved
    /// script the display reports.
    fn display_text(&self) -> Option<(String, String)> {
        let id = self.on_display.as_deref()?;
        if let Some(published) = self.published.as_ref().filter(|p| p.id == id) {
            return Some((published.title.clone(), published.body.clone()));
        }
        self.scripts
            .iter()
            .find(|script| script.id == id)
            .map(|script| (script.title.clone(), script.body.clone()))
    }

    fn save_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.notify();
        self.draft_save = Some(cx.spawn_in(window, async move |entity, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(350))
                .await;
            let _ = entity.update_in(cx, |this, _, cx| {
                let (title, body) = this.current(cx);
                let draft = Draft {
                    id: this.selected.clone(),
                    title,
                    body,
                };
                if let Ok(value) = serde_json::to_value(draft) {
                    this.state.set(DRAFT_KEY, value);
                }
            });
        }));
    }

    fn save_layout(&self) {
        if let Ok(value) = serde_json::to_value(self.layout) {
            self.state.set(LAYOUT_KEY, value);
        }
    }

    fn request(&mut self, request: Request, window: &mut Window, cx: &mut Context<Self>) {
        // Status, transport and layout changes never block the screen: the
        // operator must be able to nudge and pause while a request is out.
        let quiet = matches!(
            request,
            Request::Status | Request::Transport(_) | Request::Layout(_)
        );
        if !quiet {
            if self.busy {
                return;
            }
            self.busy = true;
            self.error = None;
            cx.notify();
        }
        let status = matches!(request, Request::Status);
        let (tx, rx) = futures_channel::oneshot::channel();
        self.backend.request(
            request,
            Box::new(move |result| {
                let _ = tx.send(result);
            }),
        );
        cx.spawn_in(window, async move |entity, cx| {
            let result = rx
                .await
                .unwrap_or_else(|_| Err("The teleprompter request stopped.".into()));
            let _ = entity.update_in(cx, |this, window, cx| {
                if !quiet {
                    this.busy = false;
                }
                this.reply(result, status, window, cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn reply(
        &mut self,
        result: Result<Reply, String>,
        status: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(Reply::Scripts(scripts)) => {
                self.scripts = scripts;
                if let Some(script) = self.selected_script().cloned() {
                    self.baseline = (script.title, script.body);
                }
            }
            Ok(Reply::Saved(script)) => self.saved(script, window, cx),
            Ok(Reply::Published(script)) => {
                self.published = Some(Published {
                    id: script.id.clone(),
                    title: script.title.clone(),
                    body: script.body.clone(),
                });
                self.on_display = Some(script.id.clone());
                self.saved(script, window, cx);
            }
            Ok(Reply::Deleted(id)) => {
                self.scripts.retain(|script| script.id != id);
                if self.selected.as_deref() == Some(id.as_str()) {
                    self.open_now(None, window, cx);
                }
            }
            Ok(Reply::Import(Some((title, text, warnings)))) => {
                self.preview_body
                    .update(cx, |input, cx| input.set_value(text, window, cx));
                self.import = Some((title, warnings));
            }
            Ok(Reply::Status {
                targets,
                live,
                position,
                script_id,
            }) => {
                if self.target.is_none() {
                    self.target = live
                        .clone()
                        .or_else(|| targets.iter().find(|t| t.ready).map(|t| t.id.clone()));
                }
                let was_live = self.live.is_some();
                self.targets = targets;
                self.live = live;
                self.on_display = script_id.filter(|_| self.live.is_some());
                if self.live.is_some() {
                    let trusted = self
                        .optimistic_until
                        .is_none_or(|until| Instant::now() >= until);
                    if trusted {
                        self.optimistic_until = None;
                        let playing = position.as_ref().is_some_and(|p| p.playing);
                        self.set_playing(playing, window, cx);
                    }
                } else if was_live {
                    // Taken off elsewhere: rehearsal starts paused.
                    self.set_playing(false, window, cx);
                }
                self.position = position;
                self.follow_position();
            }
            Ok(_) => {}
            Err(error) if !status => self.error = Some(error),
            Err(_) => {}
        }
    }

    fn saved(&mut self, script: Script, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = Some(script.id.clone());
        self.baseline = (script.title.clone(), script.body.clone());
        self.scripts.retain(|saved| saved.id != script.id);
        self.scripts.insert(0, script);
        self.save_draft(window, cx);
    }

    /// Open `script` (or a new one) after asking to discard unsaved changes.
    fn open_script(&mut self, script: Option<Script>, window: &mut Window, cx: &mut Context<Self>) {
        if script.as_ref().map(|s| s.id.as_str()) == self.selected.as_deref() && script.is_some() {
            return;
        }
        self.after_discard(Pending::Open(script), window, cx);
    }

    fn after_discard(&mut self, step: Pending, window: &mut Window, cx: &mut Context<Self>) {
        if !self.dirty(cx) {
            self.run(step, window, cx);
            return;
        }
        let (title, _) = self.current(cx);
        let name = if title.trim().is_empty() {
            "Untitled script".to_owned()
        } else {
            title.trim().to_owned()
        };
        let view = cx.entity().downgrade();
        confirm(
            ConfirmRequest::new(
                format!("Discard changes to \u{201c}{name}\u{201d}?"),
                "Discard changes",
            )
            .detail("Your edits since the last save will be lost.")
            .destructive(),
            window,
            cx,
            move |discard, window, cx| {
                if discard {
                    let _ = view.update(cx, |this, cx| this.run(step, window, cx));
                }
            },
        );
    }

    fn run(&mut self, step: Pending, window: &mut Window, cx: &mut Context<Self>) {
        match step {
            Pending::Open(script) => self.open_now(script, window, cx),
            Pending::Import { title, body } => {
                self.selected = None;
                self.baseline = (String::new(), String::new());
                self.title
                    .update(cx, |input, cx| input.set_value(title, window, cx));
                self.body
                    .update(cx, |input, cx| input.set_value(body, window, cx));
                self.import = None;
            }
        }
        cx.notify();
    }

    fn open_now(&mut self, script: Option<Script>, window: &mut Window, cx: &mut Context<Self>) {
        let (id, title, body) = script.map_or((None, String::new(), String::new()), |script| {
            (Some(script.id), script.title, script.body)
        });
        self.selected = id;
        self.baseline = (title.clone(), body.clone());
        self.title
            .update(cx, |input, cx| input.set_value(title, window, cx));
        self.body
            .update(cx, |input, cx| input.set_value(body, window, cx));
        self.error = None;
        cx.notify();
    }

    /// The title and body to save, or why they cannot be saved.
    fn checked(&mut self, cx: &mut Context<Self>) -> Option<(String, String)> {
        let (title, body) = self.current(cx);
        let error = if body.trim().is_empty() {
            Some("Enter some text for the script.")
        } else if body.len() > kairo_domain::teleprompter::MAX_SCRIPT_BYTES {
            Some("Scripts can contain up to 1 MiB of text.")
        } else {
            None
        };
        if let Some(error) = error {
            self.error = Some(error.into());
            cx.notify();
            return None;
        }
        let title = title.trim();
        let title = if title.is_empty() {
            "Untitled script"
        } else {
            title
        };
        Some((title.to_owned(), body))
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((title, body)) = self.checked(cx) {
            let id = self.selected.clone();
            self.request(Request::Save { id, title, body }, window, cx);
        }
    }

    /// Save the script and put it on the chosen screen.
    fn go_live(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.target.clone() else {
            self.error = Some("Choose a screen to send the prompter to.".into());
            cx.notify();
            return;
        };
        if let Some((title, body)) = self.checked(cx) {
            let id = self.selected.clone();
            let layout = self.layout;
            // The screen starts paused where the rehearsal stopped.
            self.set_playing(false, window, cx);
            self.request(
                Request::Publish {
                    id,
                    title,
                    body,
                    layout,
                    target: Some(target),
                },
                window,
                cx,
            );
        }
    }

    /// Send the edited script to the screen it is already on.
    fn publish(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.live.is_some() {
            self.go_live(window, cx);
        }
    }

    fn take_off(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_playing(false, window, cx);
        self.live = None;
        self.position = None;
        self.request(Request::TakeOff, window, cx);
    }

    fn choose_target(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        if let Ok(value) = serde_json::to_value(&id) {
            self.state.set(TARGET_KEY, value);
        }
        let moving = self.live.is_some() && self.live.as_deref() != Some(id.as_str());
        self.target = Some(id.clone());
        if moving {
            self.request(Request::GoLive { target: id }, window, cx);
        }
        cx.notify();
    }

    fn delete(&mut self, script: Script, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity().downgrade();
        let id = script.id.clone();
        confirm(
            ConfirmRequest::new(
                format!("Delete \u{201c}{}\u{201d}?", script.title),
                "Delete",
            )
            .detail("The saved script is removed from this computer. This can\u{2019}t be undone.")
            .destructive(),
            window,
            cx,
            move |delete, window, cx| {
                if delete {
                    let _ = view.update(cx, |this, cx| {
                        this.request(Request::Delete { id }, window, cx)
                    });
                }
            },
        );
    }

    /// The prompter is on a screen and takes commands there.
    fn loaded(&self) -> bool {
        self.live.is_some() && self.position.is_some()
    }

    /// Play, pause and move. Live, the command goes to the screen and the
    /// preview follows; otherwise only the preview moves, for rehearsal.
    fn transport(
        &mut self,
        operation: StageOperation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match operation {
            StageOperation::Play => self.set_playing(true, window, cx),
            StageOperation::Pause => self.set_playing(false, window, cx),
            StageOperation::Scroll { lines } => {
                self.sync_offset();
                self.scroll_preview(self.offset + lines as f32 * self.line_px())
            }
            StageOperation::Seek { anchor } => {
                let (_, text) = self.preview_text(cx);
                self.seek_preview(paragraph_at(&text, anchor));
            }
            StageOperation::Speed { .. } => {}
        }
        if self.loaded() {
            if matches!(operation, StageOperation::Play | StageOperation::Pause) {
                self.optimistic_until = Some(Instant::now() + OPTIMISTIC);
            }
            self.request(Request::Transport(operation), window, cx);
        }
        cx.notify();
    }

    /// The preview's scale: its width over the display's.
    fn preview_scale(&self) -> f32 {
        let width = f32::from(self.preview_scroll.bounds().size.width);
        if width > 0.0 {
            width / DISPLAY_WIDTH
        } else {
            0.18
        }
    }

    fn line_px(&self) -> f32 {
        self.layout.font_px * self.preview_scale() * LINE_HEIGHT
    }

    fn guide_px(&self) -> f32 {
        f32::from(self.preview_scroll.bounds().size.height) * GUIDE
    }

    /// Takes the preview's real position: a mouse wheel moves the view
    /// without telling us, and writing the stale `offset` back made it jump.
    fn sync_offset(&mut self) {
        let max = f32::from(self.preview_scroll.max_offset().y).max(0.0);
        self.offset = f32::from(-self.preview_scroll.offset().y).clamp(0.0, max);
    }

    fn scroll_preview(&mut self, offset: f32) {
        let max = f32::from(self.preview_scroll.max_offset().y).max(0.0);
        self.offset = offset.clamp(0.0, max);
        self.preview_scroll
            .set_offset(point(px(0.0), px(-self.offset)));
    }

    /// Put paragraph `index` on the preview's reading line. The first child
    /// is the spacer that lets the opening line reach the guide.
    fn seek_preview(&mut self, index: usize) {
        let handle = &self.preview_scroll;
        if let Some(item) = handle.bounds_for_item(index + 1) {
            let top = f32::from(item.top() - handle.bounds().top() - handle.offset().y);
            self.scroll_preview(top - self.guide_px());
        }
    }

    /// Start or stop the preview's scroll clock.
    fn set_playing(&mut self, playing: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.playing = playing;
        if !playing {
            self.ticker = None;
            self.last_tick = None;
            return;
        }
        if self.ticker.is_some() {
            return;
        }
        self.last_tick = Some(Instant::now());
        self.ticker = Some(cx.spawn_in(window, async move |entity, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                let going = entity.update(cx, |this, cx| this.tick(cx)).unwrap_or(false);
                if !going {
                    break;
                }
            }
        }));
    }

    /// Advance the preview at the prompter's speed. False once it stops.
    fn tick(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.playing {
            return false;
        }
        let now = Instant::now();
        let elapsed = self
            .last_tick
            .map_or(0.0, |last| now.duration_since(last).as_secs_f32());
        self.last_tick = Some(now);
        // Playing on from wherever the operator scrolled to.
        self.sync_offset();
        let max = f32::from(self.preview_scroll.max_offset().y).max(0.0);
        let rate = self.lines_scale().unwrap_or(self.line_px()) * self.layout.speed as f32;
        self.scroll_preview(self.offset + rate * elapsed);
        cx.notify();
        if max > 0.0 && self.offset >= max {
            // The end of the script: the prompter pauses there too.
            self.playing = false;
            self.ticker = None;
            return false;
        }
        true
    }

    fn toggle_play(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_offset();
        if !self.playing
            && f32::from(self.preview_scroll.max_offset().y) > 0.0
            && self.offset >= f32::from(self.preview_scroll.max_offset().y)
            && self.live.is_none()
        {
            // A finished rehearsal plays again from the top.
            self.scroll_preview(0.0);
        }
        let operation = if self.playing {
            StageOperation::Pause
        } else {
            StageOperation::Play
        };
        self.transport(operation, window, cx);
    }

    fn set_speed(&mut self, speed: f64, window: &mut Window, cx: &mut Context<Self>) {
        let speed = clamp_speed(speed);
        self.layout.speed = speed;
        self.save_layout();
        self.speed_slider.update(cx, |slider, cx| {
            slider.set_value(speed_level(speed), window, cx)
        });
        if self.loaded() {
            self.request(
                Request::Transport(StageOperation::Speed {
                    lines_per_second: speed,
                }),
                window,
                cx,
            );
        }
        cx.notify();
    }

    fn send_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_layout();
        if self.loaded() {
            let layout = self.layout;
            self.request(Request::Layout(layout), window, cx);
        }
        cx.notify();
    }

    fn set_mirror(&mut self, mirror: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.layout.mirror = mirror;
        self.send_layout(window, cx);
    }

    /// Preview pixels per line on the screen while live. The screen and the
    /// preview wrap the script into different numbers of lines, so the
    /// preview is scaled to the screen's length: both reach the end together.
    fn lines_scale(&self) -> Option<f32> {
        self.live.as_ref()?;
        let position = self.position.as_ref()?;
        let max = f32::from(self.preview_scroll.max_offset().y);
        (position.lines > 0.0 && max > 0.0).then(|| max / position.lines as f32)
    }

    /// While live, keep the preview on the screen's reading line. Playing,
    /// the preview runs at the screen's rate and is corrected only when it
    /// drifts, so it moves smoothly between reports.
    fn follow_position(&mut self) {
        let (Some(per_line), Some(position)) = (self.lines_scale(), self.position.as_ref()) else {
            return;
        };
        let target = position.line as f32 * per_line;
        if !self.playing || (self.offset - target).abs() > per_line * 1.5 {
            self.scroll_preview(target);
        }
    }

    /// Shortcuts for the whole section, except while typing.
    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let typing = [
            self.title.focus_handle(cx),
            self.body.focus_handle(cx),
            self.search.focus_handle(cx),
            self.preview_body.focus_handle(cx),
        ]
        .iter()
        .any(|handle| handle.contains_focused(window, cx));
        if typing || self.import.is_some() {
            return;
        }
        let modifiers = event.keystroke.modifiers;
        if modifiers.platform || modifiers.control || modifiers.alt {
            return;
        }
        match event.keystroke.key.as_str() {
            "space" => self.toggle_play(window, cx),
            "up" => self.set_speed(self.layout.speed + SPEED_STEP, window, cx),
            "down" => self.set_speed(self.layout.speed - SPEED_STEP, window, cx),
            "pageup" => self.transport(StageOperation::Scroll { lines: -PAGE_LINES }, window, cx),
            "pagedown" => self.transport(StageOperation::Scroll { lines: PAGE_LINES }, window, cx),
            "left" => self.transport(StageOperation::Scroll { lines: -1.0 }, window, cx),
            "right" => self.transport(StageOperation::Scroll { lines: 1.0 }, window, cx),
            "home" => self.transport(StageOperation::Seek { anchor: 0 }, window, cx),
            _ => return,
        }
        cx.stop_propagation();
    }
}

fn speed_level(speed: f64) -> f32 {
    ((speed / SPEED_STEP).round() as f32).clamp(SPEED_LEVELS.0, SPEED_LEVELS.1)
}

fn level_speed(level: f32) -> f64 {
    clamp_speed(f64::from(level) * SPEED_STEP)
}

/// Byte offsets where each paragraph of `text` starts.
fn paragraph_starts(text: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(text.match_indices('\n').map(|(at, _)| at + 1))
        .collect()
}

/// The paragraph that contains byte offset `anchor`.
fn paragraph_at(text: &str, anchor: usize) -> usize {
    paragraph_starts(text)
        .partition_point(|start| *start <= anchor)
        .saturating_sub(1)
}

impl Focusable for Teleprompter {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Teleprompter {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = *cx.kairo();
        h_flex()
            .id("teleprompter")
            .key_context("Teleprompter")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .size_full()
            .min_h_0()
            .bg(k.surface)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(
                        h_flex()
                            .flex_1()
                            .w_full()
                            .min_h_0()
                            .child(self.library(window, cx))
                            .child(v_flex().flex_1().min_w_0().h_full().child(self.editor(cx))),
                    )
                    .children(self.media_dock.clone()),
            )
            .child(self.live(cx))
    }
}

#[cfg(test)]
mod tests {
    // Not a glob: `gpui_kit::*` would replace the standard `#[test]`.
    use super::{SPEED_LEVELS, level_speed, paragraph_at, paragraph_starts, speed_level};

    #[test]
    fn speed_levels_round_trip_and_stay_in_range() {
        assert_eq!(speed_level(0.4), 4.0);
        assert!((level_speed(4.0) - 0.4).abs() < 1e-9);
        assert_eq!(speed_level(100.0), SPEED_LEVELS.1);
        assert_eq!(speed_level(0.0), SPEED_LEVELS.0);
    }

    #[test]
    fn anchors_map_to_their_paragraph() {
        let text = "One\nTwo two\n\nFour";
        assert_eq!(paragraph_starts(text), vec![0, 4, 12, 13]);
        assert_eq!(paragraph_at(text, 0), 0);
        assert_eq!(paragraph_at(text, 5), 1);
        assert_eq!(paragraph_at(text, 13), 3);
        assert_eq!(paragraph_at(text, 99), 3);
    }
}
