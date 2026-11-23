//! The Imagine page: a gallery of templates and drafts with a floating
//! prompt bar (after the Codex "Images" screen), and a project view with the
//! version strip, compare and restore, refine, and Save to Media.

use std::sync::Arc;

use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::{Disableable, Icon, Selectable, Sizable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, AppContext, Context, Entity, FocusHandle, Focusable, FontWeight,
    InteractiveElement, IntoElement, ObjectFit, ParentElement, Render, Role, SharedString,
    StatefulInteractiveElement, Styled, StyledImage, Subscription, Task, Window, canvas, div, img,
    linear_color_stop, linear_gradient, prelude::FluentBuilder, px, relative,
};
use kairo_imagine::estimate_usd;
use kairo_ui_kit::tokens::{Hue, TextStep, radius, text};
use kairo_ui_kit::{ControlSize, KairoIcon, KairoThemeExt};

use crate::backend::{
    Action, Attachment, ImageModel, ImagineAnswer, ImagineBackend, ImagineCall, ImagineFeed,
    Project, Quality, Slide,
};
use crate::loader;

/// The page's content column, centred in wider windows: four cards across.
const MAX_W: f32 = 1184.;
/// One line of prompt text.
const PROMPT_LINE: f32 = 24.;
/// How long the Save button glows after saving.
const SAVE_FLASH_SECS: f32 = 1.4;
/// The conversation column and the prompt bar.
const CHAT_W: f32 = 860.;
/// An image in the conversation (16:9).
const IMAGE_W: f32 = 620.;
/// A page in the slide preview's strip, and the most the slide grows to.
const THUMB_W: f32 = 136.;
const STAGE_MAX_W: f32 = 1100.;
/// A deck's card in the conversation.
const DECK_CARD_W: f32 = 440.;
/// The conversation column beside the slides: where it starts, and how
/// narrow or wide it can be dragged.
const CHAT_NARROW_W: f32 = 440.;
const CHAT_MIN_W: f32 = 320.;
const CHAT_MAX_W: f32 = 760.;
/// Width of the options panel.
const MENU_W: f32 = 216.;
/// Page margin, grid gap and the narrowest card before a column drops.
const PAD_X: f32 = 32.;
const GAP: f32 = 12.;
const MIN_CARD: f32 = 180.;
/// Cards per row when there is room.
const COLUMNS: usize = 4;
const KEYS_URL: &str = "https://platform.openai.com/api-keys";
const NEW_PROMPT: &str = "Describe an image or slides to create";
const REFINE_PROMPT: &str = "Ask for a change, or something new";

/// Element ids, for tests.
pub mod ids {
    pub const PAGE: &str = "imagine-page";
    pub const CONNECT: &str = "imagine-connect";
    pub const KEY: &str = "imagine-key";
    pub const GET_KEY: &str = "imagine-get-key";
    pub const DISCONNECT: &str = "imagine-disconnect";
    pub const TABS: &str = "imagine-tabs";
    /// The empty gallery's way back to the templates.
    pub const BROWSE_STYLES: &str = "imagine-browse-styles";
    pub const PROMPT: &str = "imagine-prompt";
    pub const SEND: &str = "imagine-send";
    pub const CANCEL: &str = "imagine-cancel";
    /// The model and quality menu in the prompt bar.
    pub const OPTIONS: &str = "imagine-options";
    pub const OPTIONS_PANEL: &str = "imagine-options-panel";
    /// A row of the options panel: `model`, `quality`, `base`.
    pub fn option(key: &str) -> String {
        format!("imagine-option-{key}")
    }
    /// A choice in a page of the options panel, by its key.
    pub fn choice(key: &str) -> String {
        format!("imagine-choice-{key}")
    }
    pub const STYLE_CLEAR: &str = "imagine-style-clear";
    pub const ERROR: &str = "imagine-error";
    pub const STATUS: &str = "imagine-status";
    pub const BACK: &str = "imagine-back";
    pub const PREVIEW: &str = "imagine-preview";
    pub const COMPARE: &str = "imagine-compare";
    pub const FROM_ORIGINAL: &str = "imagine-from-original";
    pub const SAVE_MEDIA: &str = "imagine-save-media";
    pub const DELETE: &str = "imagine-delete";
    pub const DELETE_CONFIRM: &str = "imagine-delete-confirm";
    pub const EMPTY: &str = "imagine-empty";
    /// The conversation view.
    pub const CHAT: &str = "imagine-chat";
    pub const CANVAS: &str = "imagine-canvas";
    /// The edge between the conversation and the slides.
    pub const SPLIT: &str = "imagine-split";
    pub const CANVAS_SLIDE: &str = "imagine-canvas-slide";
    pub const CANVAS_TO_SLIDES: &str = "imagine-canvas-to-slides";
    pub const CANVAS_PPTX: &str = "imagine-canvas-pptx";
    pub const CANVAS_CLOSE: &str = "imagine-canvas-close";
    pub const CANVAS_PREV: &str = "imagine-canvas-prev";
    pub const CANVAS_NEXT: &str = "imagine-canvas-next";
    /// The attach (+) button and the attached file's chip.
    pub const ATTACH: &str = "imagine-attach";
    pub const ATTACH_CLEAR: &str = "imagine-attach-clear";
    /// A page in the canvas strip.
    pub fn page(n: usize) -> String {
        format!("imagine-page-{n}")
    }
    /// A message's Copy button, by message key.
    pub fn copy(key: &str) -> String {
        format!("imagine-copy-{key}")
    }
    /// The assistant's reasoning, shown while it plans.
    pub const THOUGHT: &str = "imagine-thought";
    /// "Thought for 12s": opens the finished reasoning.
    pub const THOUGHT_TOGGLE: &str = "imagine-thought-toggle";
    /// The image preview's Save to Media.
    pub const CANVAS_TO_MEDIA: &str = "imagine-canvas-to-media";
    /// A version's Refine button, by position.
    pub fn refine(index: usize) -> String {
        format!("imagine-refine-{index}")
    }
    /// A version's Save to Media button, by position.
    pub fn save(index: usize) -> String {
        format!("imagine-save-{index}")
    }
    pub fn tab(key: &str) -> String {
        format!("imagine-tab-{key}")
    }
    pub fn template(key: &str) -> String {
        format!("imagine-template-{key}")
    }
    /// A creation card's Delete button, and its confirm and cancel.
    pub fn card_delete(id: &str) -> String {
        format!("imagine-card-delete-{id}")
    }
    pub fn card_confirm(id: &str) -> String {
        format!("imagine-card-confirm-{id}")
    }
    pub fn card_keep(id: &str) -> String {
        format!("imagine-card-keep-{id}")
    }
    pub fn project(id: &str) -> String {
        format!("imagine-project-{id}")
    }
    pub fn job(id: u64) -> String {
        format!("imagine-job-{id}")
    }
    pub fn job_cancel(id: u64) -> String {
        format!("imagine-job-cancel-{id}")
    }
    pub fn version(index: usize) -> String {
        format!("imagine-version-{index}")
    }
}

/// A style preset: a card in Templates and a chip in the prompt bar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Template {
    pub key: &'static str,
    pub title: &'static str,
    /// Added to the operator's prompt.
    pub prompt: &'static str,
    /// What the style does, shown on hover.
    pub description: &'static str,
    /// The colour behind its preview while the art is painted.
    pub hue: Hue,
}

/// Each style's preview is painted by `kairo_imagine::preview`.
pub const TEMPLATES: [Template; 8] = [
    Template {
        key: "worship",
        title: "Worship background",
        description: "Soft, atmospheric scenes that sit behind lyrics without pulling focus.",
        prompt: "A calm, atmospheric worship background with soft light and depth, no text, with clear space for lyrics",
        hue: Hue::Violet,
    },
    Template {
        key: "sermon",
        title: "Sermon series art",
        description: "Bold title art for a sermon or series, with room left for the title.",
        prompt: "Bold cinematic sermon series artwork with a strong composition and room for a title, no text",
        hue: Hue::Orange,
    },
    Template {
        key: "scripture",
        title: "Scripture backdrop",
        description: "Quiet, low-contrast scenes that keep verses easy to read.",
        prompt: "A quiet, low-contrast backdrop that reads well behind scripture text, no text",
        hue: Hue::Blue,
    },
    Template {
        key: "announcement",
        title: "Announcement",
        description: "Bright, friendly backdrops for notices, events and welcome slides.",
        prompt: "A bright, friendly background for church announcements with modern shapes, no text",
        hue: Hue::Coral,
    },
    Template {
        key: "watercolor",
        title: "Watercolor",
        description: "Paints your idea in soft, flowing watercolor.",
        prompt: "In a soft watercolor painting style",
        hue: Hue::Cyan,
    },
    Template {
        key: "stained-glass",
        title: "Stained glass",
        description: "Renders your idea as glowing stained glass.",
        prompt: "In a luminous stained glass style",
        hue: Hue::Amber,
    },
    Template {
        key: "cinematic",
        title: "Cinematic light",
        description: "Adds dramatic light and soft rays for a cinematic feel.",
        prompt: "With dramatic cinematic lighting and soft volumetric rays",
        hue: Hue::Indigo,
    },
    Template {
        key: "gradient",
        title: "Minimal gradient",
        description: "Turns your idea into a calm abstract gradient.",
        prompt: "As a minimal abstract gradient with subtle texture, no text",
        hue: Hue::Green,
    },
];

/// The prompt sent for a new image: the operator's words, then the style.
pub fn compose_prompt(prompt: &str, template: Option<&Template>) -> String {
    let prompt = prompt.trim();
    match template {
        None => prompt.into(),
        Some(t) if prompt.is_empty() => t.prompt.into(),
        Some(t) => format!("{prompt}. {}.", t.prompt),
    }
}

/// Which page of the options panel is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    Root,
    Model,
    Quality,
    /// Refine from the selected version or the original.
    Base,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Templates,
    Images,
}

/// A generation or refinement in flight.
#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub id: u64,
    pub started: std::time::Instant,
    /// The project being changed; `None` for a new creation.
    pub project: Option<String>,
    /// What the operator wrote.
    pub label: String,
    /// The assistant's reply and plan, once it has answered.
    pub reply: Option<String>,
    pub action: Option<Action>,
    pub slides: usize,
    /// The planned slides' titles.
    pub titles: Vec<String>,
    /// The assistant's reasoning summary so far.
    pub thought: String,
    /// How long it thought, once its answer came.
    pub thought_secs: Option<u64>,
    /// The finished reasoning is open under its "Thought for…" line.
    pub show_thought: bool,
    /// Images drawn so far, of the total to draw.
    pub done: usize,
    pub total: usize,
    /// Slides drawn so far, of the deck's slides, once Kairo is designing.
    pub designing: Option<(usize, usize)>,
}

/// An exchange where the assistant only replied (nothing was made). Kept
/// for this session.
#[derive(Clone, Debug, PartialEq)]
pub struct Aside {
    pub project: Option<String>,
    pub user: String,
    pub reply: String,
}

/// The slide preview beside the conversation: a deck version and the
/// slide showing. Changes are asked for in the conversation.
#[derive(Clone, Debug)]
pub struct Canvas {
    pub version: String,
    pub page: usize,
    pub slides: Vec<Slide>,
    /// An image's preview: each version's id, in the strip's order. The
    /// strip pages through versions and the prompt shows as the notes.
    pub image_versions: Option<Vec<String>>,
}

/// What the page tells the app.
#[derive(Clone, Debug, PartialEq)]
pub enum ImagineEvent {
    /// A deck was added to Slides; the Slides page should reload.
    SlidesSaved,
}

fn ask(
    backend: &Arc<dyn ImagineBackend>,
    call: ImagineCall,
) -> impl Future<Output = Result<ImagineAnswer, String>> + use<> {
    let (tx, rx) = futures_channel::oneshot::channel();
    backend.call(
        call,
        Box::new(move |r| {
            let _ = tx.send(r);
        }),
    );
    async move {
        rx.await
            .unwrap_or_else(|_| Err("The request was dropped.".into()))
    }
}

const CANCELLED: &str = "Cancelled.";

pub struct ImaginePage {
    backend: Arc<dyn ImagineBackend>,
    focus: FocusHandle,
    loaded: bool,
    connected: bool,
    connecting: bool,
    notice: Option<String>,
    projects: Vec<Project>,
    tab: Tab,
    open: Option<String>,
    /// A new creation's job, shown as a conversation until its project exists.
    pending_open: Option<u64>,
    /// The conversation view is showing (with or without a project yet).
    chat: bool,
    asides: Vec<Aside>,
    _feed: Task<()>,
    chat_scroll: gpui_kit::ScrollHandle,
    /// Scroll the conversation to its end after the next layout.
    scroll_chat: bool,
    /// The version being saved to Media, and the one just saved (for the
    /// button's confirmation flash).
    saving: Option<String>,
    saved_flash: Option<(String, std::time::Instant)>,
    /// The message just copied, for the check on its Copy button.
    copied: Option<(String, std::time::Instant)>,
    /// The gallery card asking "Delete?".
    confirm_card: Option<String>,
    /// A document attached to the next message.
    attachment: Option<Attachment>,
    /// The slide preview beside the conversation.
    canvas: Option<Canvas>,
    /// The preview slide's width, measured each frame.
    stage_width: std::rc::Rc<std::cell::Cell<f32>>,
    /// The conversation's width beside the slides, where it starts on
    /// screen, and whether its edge is being dragged.
    chat_width: f32,
    chat_left: std::rc::Rc<std::cell::Cell<f32>>,
    resizing: bool,
    /// A PowerPoint file is being written, or was just written.
    downloading: bool,
    downloaded: Option<std::time::Instant>,
    /// The prompt's measured height, and the text length at which it went to
    /// several lines (`None`: one line).
    prompt_height: std::rc::Rc<std::cell::Cell<f32>>,
    prompt_multi: Option<usize>,
    /// The prompt shows the refine placeholder.
    placeholder_chat: bool,
    from_original: bool,
    confirm_delete: bool,
    template: Option<usize>,
    model: ImageModel,
    quality: Quality,
    /// Each template's preview art, once painted.
    previews: Vec<Option<Arc<gpui_kit::Image>>>,
    _previews: Task<()>,
    /// The options panel, when open.
    options: Option<Pane>,
    /// Where the options button and the flyout are, so a press on them is
    /// not taken as a press outside the panel.
    options_hits: std::rc::Rc<std::cell::Cell<[Option<gpui_kit::Bounds<gpui_kit::Pixels>>; 2]>>,
    jobs: Vec<Job>,
    next_job: u64,
    error: Option<String>,
    status: Option<String>,
    prompt: Entity<TextareaState>,
    key: Entity<InputState>,
    /// The gallery's width, measured each frame.
    grid_width: std::rc::Rc<std::cell::Cell<f32>>,
    _subscriptions: Vec<Subscription>,
}

impl gpui_kit::EventEmitter<ImagineEvent> for ImaginePage {}

impl Focusable for ImaginePage {
    fn focus_handle(&self, _: &gpui_kit::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl ImaginePage {
    pub fn new(
        backend: Arc<dyn ImagineBackend>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let prompt = cx.new(|cx| {
            let mut s = TextareaState::new(window, cx)
                .placeholder(NEW_PROMPT)
                .submit_on_enter(true);
            s.set_auto_grow(1, 6, cx);
            s
        });
        let key = cx.new(|cx| InputState::new(window, cx).placeholder("sk-…").masked(true));
        let (tx, mut rx) = futures_channel::mpsc::unbounded::<ImagineFeed>();
        backend.listen(Arc::new(move |f| {
            let _ = tx.unbounded_send(f);
        }));
        let feed = cx.spawn(async move |this, cx| {
            use futures_util::StreamExt;
            while let Some(f) = rx.next().await {
                if this.update(cx, |this, cx| this.on_feed(f, cx)).is_err() {
                    break;
                }
            }
        });
        let subs = vec![
            cx.subscribe_in(&prompt, window, |this, _, e: &InputEvent, window, cx| {
                match e {
                    // Shift+Enter is a new line; Enter sends.
                    InputEvent::PressEnter { shift: false, .. } => this.submit(window, cx),
                    // The send button follows whether there is anything to send.
                    InputEvent::Change => cx.notify(),
                    _ => {}
                }
            }),
            cx.subscribe_in(&key, window, |this, _, e: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = e {
                    this.connect(window, cx);
                }
            }),
        ];
        // Paint the template previews off the UI thread.
        let previews = cx.spawn(async move |this, cx| {
            let arts = cx
                .background_executor()
                .spawn(async {
                    TEMPLATES
                        .iter()
                        .map(|t| kairo_imagine::preview::template_preview(t.key, 640, 480))
                        .collect::<Vec<_>>()
                })
                .await;
            let _ = this.update(cx, |p, cx| {
                p.previews = arts
                    .into_iter()
                    .map(|png| {
                        png.map(|b| {
                            Arc::new(gpui_kit::Image::from_bytes(gpui_kit::ImageFormat::Png, b))
                        })
                    })
                    .collect();
                cx.notify();
            });
        });
        let mut page = Self {
            backend,
            focus: cx.focus_handle(),
            loaded: false,
            connected: false,
            connecting: false,
            notice: None,
            projects: Vec::new(),
            tab: Tab::Templates,
            open: None,
            pending_open: None,
            chat: false,
            asides: Vec::new(),
            _feed: feed,
            chat_scroll: gpui_kit::ScrollHandle::new(),
            scroll_chat: false,
            saving: None,
            saved_flash: None,
            copied: None,
            confirm_card: None,
            attachment: None,
            canvas: None,
            stage_width: Default::default(),
            chat_width: CHAT_NARROW_W,
            chat_left: Default::default(),
            resizing: false,
            downloading: false,
            downloaded: None,
            placeholder_chat: false,
            prompt_height: Default::default(),
            prompt_multi: None,
            from_original: false,
            confirm_delete: false,
            template: None,
            model: ImageModel::default(),
            quality: Quality::Medium,
            previews: Vec::new(),
            _previews: previews,
            options: None,
            options_hits: Default::default(),
            jobs: Vec::new(),
            next_job: 1,
            error: None,
            status: None,
            prompt,
            key,
            grid_width: Default::default(),
            _subscriptions: subs,
        };
        page.load(cx);
        page
    }

    // ─── Reads, for tests and the app ──────────────────────────────────────

    pub fn connected(&self) -> bool {
        self.connected
    }
    pub fn projects(&self) -> &[Project] {
        &self.projects
    }
    pub fn jobs(&self) -> &[Job] {
        &self.jobs
    }
    pub fn tab(&self) -> Tab {
        self.tab
    }
    pub fn open_project(&self) -> Option<&Project> {
        let id = self.open.as_ref()?;
        self.projects.iter().find(|p| &p.id == id)
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    pub fn quality(&self) -> Quality {
        self.quality
    }
    pub fn model(&self) -> ImageModel {
        self.model
    }

    pub fn set_model(&mut self, model: ImageModel, cx: &mut Context<Self>) {
        self.model = model;
        cx.notify();
    }

    pub fn set_quality(&mut self, quality: Quality, cx: &mut Context<Self>) {
        self.quality = quality;
        cx.notify();
    }

    pub fn set_from_original(&mut self, on: bool, cx: &mut Context<Self>) {
        self.from_original = on;
        cx.notify();
    }

    /// Fills the prompt bar (the app's import of a prompt, and tests).
    pub fn set_prompt(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt
            .update(cx, |p, cx| p.set_value(text.to_string(), window, cx));
    }

    /// Fills the API key field (tests).
    pub fn set_key(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.key
            .update(cx, |k, cx| k.set_value(text.to_string(), window, cx));
    }

    fn on_feed(&mut self, feed: ImagineFeed, cx: &mut Context<Self>) {
        match feed {
            ImagineFeed::Planned {
                job,
                reply,
                action,
                slides,
                titles,
            } => {
                if let Some(j) = self.jobs.iter_mut().find(|j| j.id == job) {
                    j.reply = Some(reply);
                    j.thought_secs = Some(j.started.elapsed().as_secs());
                    j.action = Some(action);
                    j.slides = slides;
                    j.titles = titles;
                }
            }
            ImagineFeed::Thought { job, text } => {
                if let Some(j) = self.jobs.iter_mut().find(|j| j.id == job) {
                    j.thought.push_str(&text);
                }
            }
            ImagineFeed::Progress { job, done, total } => {
                if let Some(j) = self.jobs.iter_mut().find(|j| j.id == job) {
                    j.done = done;
                    j.total = total;
                }
            }
            ImagineFeed::Designing { job, done, total } => {
                if let Some(j) = self.jobs.iter_mut().find(|j| j.id == job) {
                    j.designing = Some((done, total));
                }
            }
        }
        self.scroll_chat = true;
        cx.notify();
    }

    // ─── Calls ─────────────────────────────────────────────────────────────

    fn load(&mut self, cx: &mut Context<Self>) {
        let call = ask(&self.backend, ImagineCall::Load);
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| this.answered(answer, cx));
        })
        .detach();
    }

    fn answered(&mut self, answer: Result<ImagineAnswer, String>, cx: &mut Context<Self>) {
        match answer {
            Ok(ImagineAnswer::Snapshot(s)) => {
                self.loaded = true;
                self.connected = s.connected;
                self.notice = s.notice;
                self.projects = s.projects;
                if self.open_project().is_none() {
                    self.open = None;
                }
            }
            Ok(ImagineAnswer::Project(p)) => {
                // The preview follows the assistant's changes to its deck.
                if self.open.as_deref() == Some(p.id.as_str())
                    && !p.current().slides.is_empty()
                    && let Some(c) = &mut self.canvas
                    && c.version != p.current().id
                {
                    let v = p.current();
                    c.version = v.id.clone();
                    c.page = c.page.min(v.slides.len().saturating_sub(1));
                    c.slides = v.slides.clone();
                }
                self.upsert(p)
            }
            Ok(ImagineAnswer::Done | ImagineAnswer::Reply(_)) => {}
            Ok(ImagineAnswer::Attachment(a)) => self.attachment = Some(a),
            Ok(ImagineAnswer::File(_)) => {}
            Err(e) if e == CANCELLED => {}
            Err(e) => {
                self.loaded = true;
                self.error = Some(e);
            }
        }
        cx.notify();
    }

    fn upsert(&mut self, project: Project) {
        match self.projects.iter_mut().find(|p| p.id == project.id) {
            Some(slot) => *slot = project,
            None => self.projects.insert(0, project),
        }
    }

    pub fn connect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let key = self.key.read(cx).value().trim().to_string();
        if key.is_empty() || self.connecting {
            return;
        }
        self.key.update(cx, |k, cx| k.set_value("", window, cx));
        self.connecting = true;
        self.error = None;
        let call = ask(&self.backend, ImagineCall::Connect(key));
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| {
                this.connecting = false;
                this.answered(answer, cx);
            });
        })
        .detach();
        cx.notify();
    }

    fn disconnect(&mut self, cx: &mut Context<Self>) {
        for job in std::mem::take(&mut self.jobs) {
            self.backend
                .call(ImagineCall::Cancel(job.id), Box::new(|_| {}));
        }
        let call = ask(&self.backend, ImagineCall::Disconnect);
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| this.answered(answer, cx));
        })
        .detach();
    }

    fn job_for(&self, project: &str) -> Option<&Job> {
        self.jobs
            .iter()
            .find(|j| j.project.as_deref() == Some(project))
    }

    /// Sends the prompt: a refinement in a project, else a new image.
    /// Sends the prompt to the assistant: about the open creation, or to
    /// start a new one.
    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.prompt.read(cx).value().trim().to_string();
        let job = self.next_job;
        let project = self.open_project().cloned();
        if self.chat_job(project.as_ref()).is_some() {
            return;
        }
        let template = project
            .is_none()
            .then_some(self.template)
            .flatten()
            .map(|i| &TEMPLATES[i]);
        let (message, label) = match (text.is_empty(), template) {
            (false, _) => (text.clone(), text),
            (true, Some(t)) => (t.prompt.to_string(), t.title.to_string()),
            (true, None) => return,
        };
        let from = project.as_ref().map(|p| {
            if self.from_original {
                p.original().id.clone()
            } else {
                p.selected.clone()
            }
        });
        let call = ImagineCall::Send {
            job,
            project: project.as_ref().map(|p| p.id.clone()),
            from,
            message,
            style: template
                .filter(|_| !label.is_empty())
                .map(|t| t.prompt.to_string()),
            attachment: self.attachment.take(),
            model: self.model,
            quality: self.quality,
        };
        let about = project.as_ref().map(|p| p.id.clone());
        self.next_job += 1;
        self.error = None;
        self.status = None;
        self.jobs.push(Job {
            id: job,
            started: std::time::Instant::now(),
            project: about.clone(),
            label: label.clone(),
            reply: None,
            action: None,
            slides: 0,
            titles: Vec::new(),
            thought: String::new(),
            thought_secs: None,
            show_thought: false,
            done: 0,
            total: 0,
            designing: None,
        });
        if about.is_none() {
            // A new creation opens its conversation straight away.
            self.pending_open = Some(job);
            self.chat = true;
            self.template = None;
            self.tab = Tab::Images;
        }
        self.scroll_chat = true;
        self.prompt.update(cx, |p, cx| p.set_value("", window, cx));
        let call = ask(&self.backend, call);
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| {
                this.jobs.retain(|j| j.id != job);
                let here = this.pending_open == Some(job) || this.open == about;
                if this.pending_open == Some(job) {
                    this.pending_open = None;
                }
                match &answer {
                    // A new creation, or a different kind made from this one:
                    // follow it if the operator is still here.
                    Ok(ImagineAnswer::Project(p)) if here && this.open.as_ref() != Some(&p.id) => {
                        this.open = Some(p.id.clone());
                        this.chat = true;
                    }
                    Ok(ImagineAnswer::Reply(reply)) => this.asides.push(Aside {
                        project: about,
                        user: label,
                        reply: reply.clone(),
                    }),
                    _ => {}
                }
                this.scroll_chat = true;
                this.answered(answer, cx);
            });
        })
        .detach();
        cx.notify();
    }

    pub fn cancel(&mut self, job: u64, cx: &mut Context<Self>) {
        self.jobs.retain(|j| j.id != job);
        if self.pending_open == Some(job) {
            self.pending_open = None;
        }
        self.backend
            .call(ImagineCall::Cancel(job), Box::new(|_| {}));
        cx.notify();
    }

    fn project_call(&mut self, call: ImagineCall, cx: &mut Context<Self>) {
        self.error = None;
        self.status = None;
        let call = ask(&self.backend, call);
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| this.answered(answer, cx));
        })
        .detach();
    }

    /// Restores a version: it becomes the one shown and refined next.
    pub fn select_version(&mut self, version: String, cx: &mut Context<Self>) {
        let Some(project) = self.open.clone() else {
            return;
        };
        self.project_call(ImagineCall::Select { project, version }, cx);
    }

    /// Saves one version: an image to the media library, a deck to Slides.
    /// Nothing goes live.
    pub fn save_version(&mut self, version: String, cx: &mut Context<Self>) {
        let Some(p) = self.open_project() else {
            return;
        };
        if self.saving.is_some() {
            return;
        }
        let deck = p.version(&version).is_some_and(|v| !v.slides.is_empty());
        let (project, version_id) = (p.id.clone(), version.clone());
        let call = ask(
            &self.backend,
            if deck {
                ImagineCall::SaveToSlides {
                    project,
                    version: version_id,
                }
            } else {
                ImagineCall::SaveToMedia {
                    project,
                    version: version_id,
                }
            },
        );
        self.error = None;
        self.saving = Some(version.clone());
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| {
                this.saving = None;
                if answer.is_ok() {
                    this.saved_flash = Some((version, std::time::Instant::now()));
                    if deck {
                        cx.emit(ImagineEvent::SlidesSaved);
                    }
                }
                this.answered(answer, cx);
            });
        })
        .detach();
        cx.notify();
    }

    fn delete_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.open.clone() else {
            return;
        };
        self.close(window, cx);
        self.delete_project(id, cx);
    }

    /// Deletes a creation and every version of it.
    pub fn delete_project(&mut self, id: String, cx: &mut Context<Self>) {
        if let Some(job) = self.job_for(&id).map(|j| j.id) {
            self.cancel(job, cx);
        }
        self.projects.retain(|p| p.id != id);
        self.confirm_card = None;
        cx.notify();
        let call = ask(&self.backend, ImagineCall::Delete(id));
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| this.answered(answer, cx));
        })
        .detach();
    }

    pub fn open(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.open = Some(id);
        self.chat = true;
        self.pending_open = None;
        self.from_original = false;
        self.confirm_delete = false;
        self.status = None;
        self.scroll_chat = true;
        self.prompt.update(cx, |p, cx| p.focus(window, cx));
        cx.notify();
    }

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let _ = window;
        self.open = None;
        self.canvas = None;
        self.chat = false;
        self.pending_open = None;
        self.asides.retain(|a| a.project.is_some());
        self.confirm_delete = false;
        cx.notify();
    }

    fn pick_template(&mut self, index: usize, cx: &mut Context<Self>) {
        self.template = if self.template == Some(index) {
            None
        } else {
            Some(index)
        };
        cx.notify();
    }

    // ─── The slide preview ─────────────────────────────────────────────────

    /// The conversation's width beside the slides.
    /// Back to the Templates tab, as Imagine opens from the rail.
    pub fn show_templates(&mut self, cx: &mut Context<Self>) {
        if self.tab != Tab::Templates {
            self.tab = Tab::Templates;
            cx.notify();
        }
    }

    pub fn chat_width(&self) -> f32 {
        self.chat_width
    }

    pub fn canvas(&self) -> Option<&Canvas> {
        self.canvas.as_ref()
    }

    /// Opens a deck version beside the conversation.
    pub fn open_canvas(&mut self, version: String, cx: &mut Context<Self>) {
        let Some(v) = self
            .open_project()
            .and_then(|p| p.version(&version))
            .cloned()
        else {
            return;
        };
        if v.slides.is_empty() {
            // An image: its versions are the pages.
            let Some(p) = self.open_project() else { return };
            let page = p.versions.iter().position(|x| x.id == version).unwrap_or(0);
            self.canvas = Some(Canvas {
                version,
                page,
                slides: p
                    .versions
                    .iter()
                    .enumerate()
                    .map(|(n, x)| Slide {
                        title: format!("Version {}", n + 1),
                        file: x.file.clone(),
                        notes: x.instruction.clone(),
                        ..Slide::default()
                    })
                    .collect(),
                image_versions: Some(p.versions.iter().map(|x| x.id.clone()).collect()),
            });
        } else {
            self.canvas = Some(Canvas {
                version,
                page: 0,
                slides: v.slides,
                image_versions: None,
            });
        }
        cx.notify();
    }

    pub fn close_canvas(&mut self, cx: &mut Context<Self>) {
        self.canvas = None;
        cx.notify();
    }

    pub fn select_page(&mut self, page: usize, cx: &mut Context<Self>) {
        if let Some(c) = &mut self.canvas
            && page < c.slides.len()
        {
            c.page = page;
            // An image's Save acts on the version showing.
            if let Some(id) = c.image_versions.as_ref().and_then(|v| v.get(page)) {
                c.version = id.clone();
            }
            cx.notify();
        }
    }

    /// The previous or next slide.
    pub fn step_page(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(c) = &self.canvas {
            let to = (c.page as isize + delta).clamp(0, c.slides.len() as isize - 1);
            self.select_page(to as usize, cx);
        }
    }

    /// Writes the deck in the canvas as a PowerPoint file.
    pub fn download_pptx(&mut self, cx: &mut Context<Self>) {
        let (Some(c), Some(project)) = (&self.canvas, self.open.clone()) else {
            return;
        };
        if self.downloading {
            return;
        }
        let call = ask(
            &self.backend,
            ImagineCall::DownloadPptx {
                project,
                version: c.version.clone(),
            },
        );
        self.downloading = true;
        self.error = None;
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |p, cx| {
                p.downloading = false;
                if answer.is_ok() {
                    p.downloaded = Some(std::time::Instant::now());
                }
                p.answered(answer, cx);
            });
        })
        .detach();
        cx.notify();
    }

    /// Picks a document for the next message.
    pub fn attach(&mut self, cx: &mut Context<Self>) {
        let call = ask(&self.backend, ImagineCall::PickAttachment);
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |p, cx| p.answered(answer, cx));
        })
        .detach();
    }

    pub fn attachment(&self) -> Option<&Attachment> {
        self.attachment.as_ref()
    }

    // ─── Rendering ─────────────────────────────────────────────────────────

    fn render_connect(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let (connect, link) = (cx.entity(), cx.entity());
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .child(
                v_flex()
                    .w(px(440.))
                    .gap_3()
                    .p_6()
                    .rounded(radius::xl())
                    .bg(k.surface_secondary)
                    .border_1()
                    .border_color(k.surface_border)
                    .child(Icon::new(KairoIcon::Key).size_6().text_color(k.hue(Hue::Pink)))
                    .child(
                        div()
                            .text_size(text::LG.size)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Connect your OpenAI account"),
                    )
                    .child(
                        div()
                            .text_size(text::XS.size)
                            .text_color(k.text(TextStep::T500))
                            .child(
                                "Imagine makes images with your own OpenAI API key, so usage is billed \
                                 to your OpenAI account. The key stays in this computer's keychain and \
                                 is only sent to OpenAI.",
                            ),
                    )
                    .child(
                        Input::new(&self.key)
                            .kairo_size()
                            .id(ids::KEY)
                            .aria_label("OpenAI API key")
                            .mask_toggle()
                            .disabled(self.connecting),
                    )
                    .children(self.notice.clone().map(|n| {
                        div()
                            .text_size(text::LABEL.size)
                            .text_color(k.warning_text)
                            .child(n)
                    }))
                    .child(
                        h_flex()
                            .gap_2()
                            .justify_between()
                            .child(
                                Button::new(ids::GET_KEY)
                                    .cursor_pointer()
                                    .small()
                                    .ghost()
                                    .icon(Icon::new(KairoIcon::ExternalLink))
                                    .label("Get an API key")
                                    .on_click(move |_, _, cx| {
                                        let _ = link.read(cx);
                                        cx.open_url(KEYS_URL)
                                    }),
                            )
                            .child(
                                Button::new(ids::CONNECT)
                                    .cursor_pointer()
                                    .small()
                                    .primary()
                                    .label(if self.connecting { "Checking…" } else { "Connect" })
                                    .disabled(self.connecting)
                                    .on_click(move |_, window, cx| {
                                        connect.update(cx, |p, cx| p.connect(window, cx))
                                    }),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// Pill tabs: the picked one sits on a soft fill. "My creations"
    /// carries its count.
    fn render_tabs(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let count = self.projects.len();
        let tabs = [
            (Tab::Templates, "templates", "Templates", None),
            (
                Tab::Images,
                "creations",
                "My creations",
                (count > 0).then_some(count),
            ),
        ];
        let disconnect = cx.entity();
        let list = h_flex()
            .id(ids::TABS)
            .role(Role::TabList)
            .aria_label("Imagine sections")
            .gap_1()
            .children(tabs.into_iter().map(|(tab, key, label, count)| {
                let page = cx.entity();
                let picked = self.tab == tab;
                h_flex()
                    .id(SharedString::from(ids::tab(key)))
                    .test_support()
                    .role(Role::Tab)
                    .aria_label(label)
                    .aria_selected(picked)
                    .gap_1p5()
                    .px_4()
                    .py_1p5()
                    .rounded_full()
                    .text_size(text::SM.size)
                    .font_weight(FontWeight::MEDIUM)
                    .map(|d| {
                        if picked {
                            d.bg(k.surface_tertiary).text_color(k.text_primary)
                        } else {
                            d.text_color(k.text(TextStep::T500))
                                .hover(|s| s.text_color(k.text_primary).bg(k.surface_secondary))
                        }
                    })
                    .on_click(move |_, _, cx| {
                        page.update(cx, |p, cx| {
                            p.tab = tab;
                            cx.notify();
                        })
                    })
                    .child(label)
                    .children(count.map(|n| {
                        div()
                            .text_size(text::XS.size)
                            .text_color(k.text(TextStep::T400))
                            .child(n.to_string())
                    }))
            }));
        h_flex()
            .flex_none()
            .px(px(PAD_X))
            .pb_5()
            .justify_between()
            .child(list)
            .child(
                Button::new(ids::DISCONNECT)
                    .small()
                    .ghost()
                    .icon(Icon::new(KairoIcon::LogOut))
                    .accessibility_label("Disconnect OpenAI")
                    .tooltip("Disconnect OpenAI")
                    .on_click(move |_, _, cx| disconnect.update(cx, |p, cx| p.disconnect(cx))),
            )
            .into_any_element()
    }

    /// The page's heading: what this place is for, in one line, and the
    /// promise that nothing leaves it unasked.
    fn render_heading(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        v_flex()
            .flex_none()
            .px(px(PAD_X))
            .pt_10()
            .pb_6()
            .gap_1p5()
            .child(
                div()
                    .text_size(text::XL3.size)
                    .line_height(text::XL3.line_height)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(k.text_primary)
                    .child("What will you make today?"),
            )
            .child(
                div()
                    .max_w(px(620.))
                    .text_size(text::SM.size)
                    .line_height(text::SM.line_height)
                    .text_color(k.text(TextStep::T500))
                    .child(
                        "Describe an image or a slide deck, or start from a style. \
                         Everything stays a draft here until you save it.",
                    ),
            )
            .into_any_element()
    }

    /// A gallery card: the picture in a rounded frame, its title and one
    /// line of detail under it. `overlays` sit inside the frame. The
    /// frame lifts on hover and wears the accent ring when picked.
    #[allow(clippy::too_many_arguments)]
    fn card(
        id: String,
        label: String,
        title: String,
        meta: Option<String>,
        (w, h): (f32, f32),
        body: AnyElement,
        picked: bool,
        overlays: Vec<AnyElement>,
        group: SharedString,
        k: &kairo_ui_kit::tokens::KairoColors,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        let frame = div()
            .relative()
            .w(px(w))
            .h(px(h))
            .overflow_hidden()
            .rounded(radius::xl2())
            .bg(k.surface_secondary)
            .border_1()
            .border_color(k.surface_border)
            .when(!picked, |d| {
                d.group_hover(group.clone(), |s| {
                    s.border_color(k.text(TextStep::T400)).shadow_lg()
                })
            })
            .child(body)
            .children(overlays)
            .when(picked, |d| {
                d.child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded(radius::xl2())
                        .border_2()
                        .border_color(k.control_accent),
                )
                .child(
                    div()
                        .absolute()
                        .top_2p5()
                        .right_2p5()
                        .size(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(k.control_accent)
                        .text_color(k.on_accent)
                        .child(Icon::new(KairoIcon::Check).size_3p5()),
                )
            });
        v_flex()
            .id(SharedString::from(id))
            .role(Role::Button)
            .aria_label(label)
            .aria_selected(picked)
            .group(group)
            .w(px(w))
            .flex_none()
            .gap_2p5()
            .child(frame)
            .child(
                v_flex()
                    .gap_0p5()
                    .px_0p5()
                    .child(
                        div()
                            .truncate()
                            .text_size(text::SM.size)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if picked {
                                k.text_primary
                            } else {
                                k.text(TextStep::T200)
                            })
                            .child(title),
                    )
                    .children(meta.map(|m| {
                        div()
                            .line_clamp(2)
                            .text_size(text::XS.size)
                            .line_height(text::XS.line_height)
                            .text_color(k.text(TextStep::T500))
                            .child(m)
                    })),
            )
    }

    /// Columns and card width for the measured grid width.
    fn layout(&self) -> (usize, f32) {
        let width = self.grid_width.get().max(MIN_CARD);
        let cols = (((width + GAP) / (MIN_CARD + GAP)).floor() as usize).clamp(1, COLUMNS);
        let card = (width - GAP * (cols as f32 - 1.)) / cols as f32;
        (cols, card.floor())
    }

    /// A template's preview art, faded in once it is painted.
    fn preview(&self, i: usize, cx: &gpui_kit::App) -> Option<AnyElement> {
        let image = self.previews.get(i)?.clone()?;
        Some(kairo_ui_kit::motion::enter(
            img(image).size_full().object_fit(ObjectFit::Cover),
            SharedString::from(format!("imagine-preview-{i}")),
            cx,
        ))
    }

    fn render_templates(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let k = *cx.kairo();
        let (_, w) = self.layout();
        TEMPLATES
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let page = cx.entity();
                let body = self
                    .preview(i, cx)
                    .unwrap_or_else(|| div().size_full().into_any_element());
                Self::card(
                    ids::template(t.key),
                    format!("Use the {} style", t.title),
                    t.title.into(),
                    Some(t.description.into()),
                    (w, (w * 0.75).floor()),
                    body,
                    self.template == Some(i),
                    Vec::new(),
                    SharedString::from(format!("template-{}", t.key)),
                    &k,
                )
                .on_click(move |_, _, cx| page.update(cx, |p, cx| p.pick_template(i, cx)))
                .test_support()
                .into_any_element()
            })
            .collect()
    }

    fn render_images(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let k = *cx.kairo();
        let reduced = cx.reduced_motion();
        let (_, w) = self.layout();
        let h = (w * 9. / 16.).floor();
        // What is on its way comes first, drawn as it will arrive.
        let mut cards: Vec<AnyElement> = self
            .jobs
            .iter()
            .filter(|j| j.project.is_none())
            .map(|j| {
                let page = cx.entity();
                let id = j.id;
                let t = loader::clock(j.started, reduced);
                let deck = matches!(j.action, Some(Action::Deck | Action::EditSlides));
                let overlays = vec![
                    div()
                        .absolute()
                        .top_2()
                        .right_2()
                        .child(
                            Button::new(SharedString::from(ids::job_cancel(id)))
                                .xsmall()
                                .ghost()
                                .icon(Icon::new(KairoIcon::XCircle))
                                .accessibility_label(format!("Cancel {}", j.label))
                                .tooltip("Cancel")
                                .on_click(move |_, _, cx| {
                                    page.update(cx, |p, cx| p.cancel(id, cx))
                                }),
                        )
                        .into_any_element(),
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .px_4()
                        .pb_3()
                        .child(loader::status(
                            t,
                            if deck {
                                "Making your deck"
                            } else {
                                "Making your image"
                            },
                            "",
                            &k,
                        ))
                        .into_any_element(),
                ];
                Self::card(
                    ids::job(id),
                    format!("Making {}", j.label),
                    if j.label.trim().is_empty() {
                        "New creation".into()
                    } else {
                        j.label.clone()
                    },
                    Some("In progress".into()),
                    (w, h),
                    loader::dot_field(t, k.text_primary).into_any_element(),
                    false,
                    overlays,
                    SharedString::from(format!("job-{id}")),
                    &k,
                )
                .test_support()
                .into_any_element()
            })
            .collect();
        for p in &self.projects {
            let page = cx.entity();
            let del = cx.entity();
            let id = p.id.clone();
            let del_id = p.id.clone();
            let confirming = self.confirm_card.as_deref() == Some(p.id.as_str());
            let group = SharedString::from(format!("card-{}", p.id));
            let body = img(self.backend.image_path(&p.current().file))
                .size_full()
                .object_fit(ObjectFit::Cover)
                .into_any_element();
            let slides = p.current().slides.len();
            let versions = p.versions.len();
            let meta = match (slides, versions) {
                (0, 1) => "Image".to_string(),
                (0, _) => format!("Image · v{} of {versions}", version_number(p, &p.selected)),
                (n, 1) => format!("Deck · {n} slides"),
                (n, _) => format!("Deck · {n} slides · v{}", version_number(p, &p.selected)),
            };
            let meta = if p.saved.is_empty() {
                meta
            } else {
                format!("{meta} · Saved")
            };
            let mut overlays = vec![
                // Delete: a round button over the picture on hover.
                div()
                    .absolute()
                    .top_2()
                    .right_2()
                    .opacity(0.)
                    .group_hover(group.clone(), |s| s.opacity(1.))
                    .on_mouse_down(gpui_kit::MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation()
                    })
                    .child(
                        div()
                            .id(SharedString::from(ids::card_delete(&p.id)))
                            .test_support()
                            .role(Role::Button)
                            .aria_label(format!("Delete {}", p.title))
                            .size(px(30.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(k.scrim())
                            .text_color(k.white)
                            .hover(|s| s.bg(k.destructive))
                            .child(Icon::new(KairoIcon::Trash2).size_4())
                            .on_click(move |_, _, cx| {
                                let id = del_id.clone();
                                del.update(cx, |p, cx| {
                                    p.confirm_card = Some(id);
                                    cx.notify();
                                })
                            }),
                    )
                    .into_any_element(),
            ];
            if confirming {
                // Asks over the picture, so it can't be missed.
                let (yes, no) = (cx.entity(), cx.entity());
                let id = p.id.clone();
                overlays.push(
                    v_flex()
                        .absolute()
                        .inset_0()
                        .items_center()
                        .justify_center()
                        .gap_3()
                        .px_3()
                        .bg(k.scrim())
                        .on_mouse_down(gpui_kit::MouseButton::Left, |_, _, cx| {
                            cx.stop_propagation()
                        })
                        .child(
                            div()
                                .text_size(text::SM.size)
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(k.white)
                                .text_center()
                                .child(format!("Delete \u{201c}{}\u{201d}?", p.title)),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    Button::new(SharedString::from(ids::card_keep(&p.id)))
                                        .small()
                                        .ghost()
                                        .rounded_full()
                                        .label("Cancel")
                                        .on_click(move |_, _, cx| {
                                            no.update(cx, |p, cx| {
                                                p.confirm_card = None;
                                                cx.notify();
                                            })
                                        }),
                                )
                                .child(
                                    Button::new(SharedString::from(ids::card_confirm(&p.id)))
                                        .small()
                                        .danger()
                                        .rounded_full()
                                        .icon(Icon::new(KairoIcon::Trash2))
                                        .label("Delete")
                                        .on_click(move |_, _, cx| {
                                            let id = id.clone();
                                            yes.update(cx, |p, cx| p.delete_project(id, cx))
                                        }),
                                ),
                        )
                        .into_any_element(),
                );
            }
            cards.push(
                Self::card(
                    ids::project(&p.id),
                    format!("Open {}", p.title),
                    p.title.clone(),
                    Some(meta),
                    (w, h),
                    body,
                    false,
                    overlays,
                    group,
                    &k,
                )
                .on_click(move |_, window, cx| {
                    page.update(cx, |p, cx| p.open(id.clone(), window, cx))
                })
                .test_support()
                .into_any_element(),
            );
        }
        cards
    }

    /// Nothing made yet: say where creations come from and offer the way in.
    fn render_empty(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let page = cx.entity();
        v_flex()
            .id(ids::EMPTY)
            .test_support()
            .w_full()
            .pt_16()
            .items_center()
            .gap_4()
            .child(
                div()
                    .size(px(56.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(k.surface_secondary)
                    .border_1()
                    .border_color(k.surface_border)
                    .child(
                        Icon::new(KairoIcon::Images)
                            .size_6()
                            .text_color(k.text(TextStep::T400)),
                    ),
            )
            .child(
                v_flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .text_size(text::BASE.size)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(k.text_primary)
                            .child("No creations yet"),
                    )
                    .child(
                        div()
                            .max_w(px(380.))
                            .text_center()
                            .text_size(text::SM.size)
                            .text_color(k.text(TextStep::T500))
                            .child("Describe an image or a deck below, or start from a style. What you make appears here."),
                    ),
            )
            .child(
                Button::new(ids::BROWSE_STYLES)
                    .small()
                    .outline()
                    .label("Browse templates")
                    .on_click(move |_, _, cx| {
                        page.update(cx, |p, cx| {
                            p.tab = Tab::Templates;
                            cx.notify();
                        })
                    }),
            )
            .into_any_element()
    }

    fn render_gallery(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let cards = match self.tab {
            Tab::Templates => self.render_templates(cx),
            Tab::Images => self.render_images(cx),
        };
        let empty = cards.is_empty().then(|| self.render_empty(cx));
        let measured = self.grid_width.clone();
        let view = cx.entity();
        v_flex()
            .relative()
            .size_full()
            .min_h_0()
            .child(
                div()
                    .id("imagine-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .pb(px(148.))
                    .child(self.render_heading(cx))
                    .child(self.render_tabs(cx))
                    .child(
                        div()
                            .relative()
                            .px(px(PAD_X))
                            .w_full()
                            .child(
                                canvas(
                                    move |bounds, window, _| {
                                        let width = f32::from(bounds.size.width);
                                        if (measured.get() - width).abs() > 0.5 {
                                            measured.set(width);
                                            let view = view.clone();
                                            window.on_next_frame(move |_, cx| {
                                                view.update(cx, |_, cx| cx.notify())
                                            });
                                        }
                                    },
                                    |_, _, _, _| {},
                                )
                                .absolute()
                                .top_0()
                                .bottom_0()
                                .left(px(PAD_X))
                                .right(px(PAD_X)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .gap_x(px(GAP))
                                    .gap_y_6()
                                    .children(cards)
                                    .children(empty),
                            ),
                    ),
            )
            // Cards slide under the composer rather than meeting it at an edge.
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(120.))
                    .bg(linear_gradient(
                        180.,
                        linear_color_stop(k.surface.opacity(0.), 0.),
                        linear_color_stop(k.surface, 0.85),
                    )),
            )
            .into_any_element()
    }

    /// The conversation for one creation: each refinement is the operator's
    /// message and the image it made, oldest first, as they were saved.
    /// `project` is `None` while a new image's first answer is on its way.
    fn render_chat(&self, project: Option<&Project>, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let (back, delete, confirm) = (cx.entity(), cx.entity(), cx.entity());
        let toolbar = h_flex()
            .flex_none()
            .h(px(56.))
            .px(px(PAD_X))
            .gap_3()
            .justify_between()
            .border_b_1()
            .border_color(k.surface_line)
            .child(
                Button::new(ids::BACK)
                    .cursor_pointer()
                    .small()
                    .ghost()
                    .icon(Icon::new(KairoIcon::ChevronLeft))
                    .label("Back")
                    .tooltip("My creations")
                    .on_click(move |_, window, cx| back.update(cx, |p, cx| p.close(window, cx))),
            )
            .when(project.is_some(), |d| {
                d.child(if self.confirm_delete {
                    Button::new(ids::DELETE_CONFIRM)
                        .cursor_pointer()
                        .small()
                        .danger()
                        .label("Delete every version")
                        .on_click(move |_, window, cx| {
                            confirm.update(cx, |p, cx| p.delete_open(window, cx))
                        })
                } else {
                    Button::new(ids::DELETE)
                        .cursor_pointer()
                        .small()
                        .ghost()
                        .icon(Icon::new(KairoIcon::Trash2))
                        .accessibility_label("Delete this creation")
                        .tooltip("Delete this creation and its versions")
                        .on_click(move |_, _, cx| {
                            delete.update(cx, |p, cx| {
                                p.confirm_delete = true;
                                cx.notify();
                            })
                        })
                })
            });

        let mut thread: Vec<AnyElement> = Vec::new();
        if let Some(p) = project {
            for (i, version) in p.versions.iter().enumerate() {
                let branched =
                    i > 0 && version.parent.as_deref() != Some(p.versions[i - 1].id.as_str());
                if branched {
                    let from = version
                        .parent
                        .as_deref()
                        .map_or(1, |id| version_number(p, id));
                    thread.push(
                        h_flex()
                            .w_full()
                            .justify_end()
                            .text_size(text::XS.size)
                            .text_color(k.text(TextStep::T500))
                            .child(format!("Changing v{from}"))
                            .into_any_element(),
                    );
                }
                let copy = self.copy_button(
                    format!("{}-you", version.id),
                    version.instruction.clone(),
                    cx,
                );
                thread.push(bubble(&k, version.instruction.clone(), Some(copy)));
                if let Some(reply) = &version.reply {
                    let copy = self.copy_button(format!("{}-reply", version.id), reply.clone(), cx);
                    thread.push(said(&k, reply.clone(), Some(copy)));
                }
                thread.push(self.render_answer(p, i, cx));
            }
        }
        let here = project.map(|p| p.id.clone());
        let asides: Vec<_> = self
            .asides
            .iter()
            .enumerate()
            .filter(|(_, a)| a.project == here)
            .map(|(n, a)| (n, a.clone()))
            .collect();
        for (n, aside) in asides {
            let copy = self.copy_button(format!("aside-{n}-you"), aside.user.clone(), cx);
            thread.push(bubble(&k, aside.user.clone(), Some(copy)));
            let copy = self.copy_button(format!("aside-{n}-reply"), aside.reply.clone(), cx);
            thread.push(said(&k, aside.reply.clone(), Some(copy)));
        }
        if let Some(j) = self.chat_job(project) {
            thread.push(bubble(&k, j.label.clone(), None));
            thread.extend(self.render_job(j, cx));
        }
        let canvas = self.canvas.is_some();
        let scroll = div()
            .id("imagine-thread")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.chat_scroll)
            .child(
                v_flex()
                    .w_full()
                    .max_w(px(CHAT_W))
                    .mx_auto()
                    // Beside the preview the column is narrow: keep the
                    // messages off its edges.
                    .map(|d| if canvas { d.px_8() } else { d.px_4() })
                    .pt_4()
                    .pb(px(150.))
                    .gap_5()
                    .children(thread),
            );
        // With the canvas open the prompt bar sits under the conversation.
        let left = self.chat_left.clone();
        let conversation = div()
            .relative()
            .min_h_0()
            .h_full()
            .map(|d| {
                if canvas {
                    d.flex_none().w(px(self.chat_width))
                } else {
                    d.flex_1()
                }
            })
            .child(
                gpui_kit::canvas(
                    move |bounds, _, _| left.set(f32::from(bounds.origin.x)),
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(scroll)
            .when(canvas, |d| d.child(self.render_prompt_bar(cx)));
        // Drag to share the width between the conversation and the slides.
        let divider = canvas.then(|| {
            let k = *cx.kairo();
            let start = cx.entity();
            div()
                .id(ids::SPLIT)
                .flex_none()
                .w(px(7.))
                .h_full()
                .flex()
                .justify_center()
                .cursor_col_resize()
                .group("imagine-split")
                .on_mouse_down(gpui_kit::MouseButton::Left, move |_, _, cx| {
                    start.update(cx, |p, cx| {
                        p.resizing = true;
                        cx.notify();
                    })
                })
                .child(
                    div()
                        .w(px(1.))
                        .h_full()
                        .bg(if self.resizing {
                            k.control_accent
                        } else {
                            k.surface_line
                        })
                        .group_hover("imagine-split", |s| s.bg(k.control_accent)),
                )
                .test_support()
        });
        v_flex()
            .id(ids::CHAT)
            .test_support()
            .size_full()
            .min_h_0()
            .when(self.resizing, |d| d.cursor_col_resize())
            .on_mouse_move(cx.listener(|p, e: &gpui_kit::MouseMoveEvent, _, cx| {
                if p.resizing {
                    let x = f32::from(e.position.x) - p.chat_left.get();
                    p.chat_width = x.clamp(CHAT_MIN_W, CHAT_MAX_W);
                    cx.notify();
                }
            }))
            .on_mouse_up(
                gpui_kit::MouseButton::Left,
                cx.listener(|p, _, _, cx| {
                    if p.resizing {
                        p.resizing = false;
                        cx.notify();
                    }
                }),
            )
            .child(toolbar)
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_start()
                    .child(conversation)
                    .children(divider)
                    .children(canvas.then(|| self.render_canvas(cx))),
            )
            .into_any_element()
    }

    /// The deck beside the conversation, to look through: the pages down the
    /// left, the slide large in the middle with its speaker notes, and the
    /// deck's actions above. Changes are asked for in the conversation.
    fn render_canvas(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let Some(c) = &self.canvas else {
            return div().into_any_element();
        };
        let slide = c.slides.get(c.page).cloned().unwrap_or_default();
        let image = c.image_versions.is_some();
        let several = c.slides.len() > 1;
        let title = self
            .open_project()
            .map_or("Slides".to_string(), |p| p.title.clone());
        let thing = if image { "version" } else { "slide" };
        let (close, slides_save, prev, next) = (cx.entity(), cx.entity(), cx.entity(), cx.entity());
        let version = c.version.clone();
        let saved = self
            .open_project()
            .is_some_and(|p| p.saved.iter().any(|v| v == &c.version));
        let fresh = self
            .downloaded
            .is_some_and(|at| at.elapsed().as_secs_f32() < 2.5);
        let download = cx.entity();

        let pages = c.slides.iter().enumerate().map(|(n, s)| {
            let page = cx.entity();
            let picked = n == c.page;
            h_flex()
                .id(SharedString::from(ids::page(n)))
                .test_support()
                .role(Role::Button)
                .aria_label(if image {
                    s.title.clone()
                } else {
                    format!("Slide {}: {}", n + 1, s.title)
                })
                .items_start()
                .gap_2()
                .cursor_pointer()
                .on_click(move |_, _, cx| page.update(cx, |p, cx| p.select_page(n, cx)))
                .child(
                    div()
                        .w(px(18.))
                        .pt_0p5()
                        .text_size(text::LABEL.size)
                        .text_color(if picked {
                            k.text_primary
                        } else {
                            k.text(TextStep::T500)
                        })
                        .child((n + 1).to_string()),
                )
                .child(
                    div()
                        .w(px(THUMB_W))
                        .h(px(THUMB_W * 9. / 16.))
                        .overflow_hidden()
                        .rounded(radius::md())
                        .border_2()
                        .border_color(if picked {
                            k.control_accent
                        } else {
                            k.surface_border
                        })
                        .bg(k.surface_display)
                        .when(!picked, |d| d.opacity(0.8).hover(|s| s.opacity(1.)))
                        .child(
                            img(self.backend.image_path(&s.file))
                                .size_full()
                                .object_fit(ObjectFit::Cover),
                        ),
                )
        });

        // The slide fills the width it has, at its own 16:9 shape.
        let width = self.stage_width.get().max(320.);
        let measured = self.stage_width.clone();
        let view = cx.entity();
        let stage = div()
            .id(ids::CANVAS_SLIDE)
            .test_support()
            .relative()
            .w_full()
            .h(px((width * 9. / 16.).floor()))
            .rounded(radius::lg())
            .overflow_hidden()
            .bg(k.surface_display)
            .shadow_lg()
            .child(
                canvas(
                    move |bounds, window, _| {
                        let w = f32::from(bounds.size.width);
                        if (measured.get() - w).abs() > 0.5 {
                            measured.set(w);
                            let view = view.clone();
                            window.on_next_frame(move |_, cx| view.update(cx, |_, cx| cx.notify()));
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(
                img(self.backend.image_path(&slide.file))
                    .size_full()
                    .object_fit(if image {
                        ObjectFit::Contain
                    } else {
                        ObjectFit::Cover
                    }),
            );
        let nav = h_flex()
            .justify_center()
            .gap_3()
            .child(
                Button::new(ids::CANVAS_PREV)
                    .cursor_pointer()
                    .small()
                    .ghost()
                    .rounded_full()
                    .icon(Icon::new(KairoIcon::ChevronLeft))
                    .accessibility_label(format!("Previous {thing}"))
                    .disabled(c.page == 0)
                    .on_click(move |_, _, cx| prev.update(cx, |p, cx| p.step_page(-1, cx))),
            )
            .child(
                div()
                    .min_w(px(48.))
                    .text_center()
                    .text_size(text::SM.size)
                    .text_color(k.text(TextStep::T500))
                    .child(format!("{} / {}", c.page + 1, c.slides.len())),
            )
            .child(
                Button::new(ids::CANVAS_NEXT)
                    .cursor_pointer()
                    .small()
                    .ghost()
                    .rounded_full()
                    .icon(Icon::new(KairoIcon::ChevronRight))
                    .accessibility_label(format!("Next {thing}"))
                    .disabled(c.page + 1 >= c.slides.len())
                    .on_click(move |_, _, cx| next.update(cx, |p, cx| p.step_page(1, cx))),
            );
        let notes = (!slide.notes.trim().is_empty()).then(|| {
            v_flex()
                .w_full()
                .gap_1p5()
                .p_4()
                .rounded(radius::lg())
                .bg(k.surface_secondary)
                .child(
                    h_flex()
                        .gap_1p5()
                        .text_size(text::LABEL.size)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(k.text(TextStep::T500))
                        .child(
                            Icon::new(if image {
                                KairoIcon::MessageSquare
                            } else {
                                KairoIcon::FileText
                            })
                            .size_3p5(),
                        )
                        .child(if image { "Prompt" } else { "Speaker notes" }),
                )
                .child(
                    div()
                        .text_size(text::SM.size)
                        .line_height(text::SM.line_height)
                        .text_color(k.text_primary)
                        .child(slide.notes.clone()),
                )
        });

        v_flex()
            .id(ids::CANVAS)
            .test_support()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(k.surface_stage)
            .border_l_1()
            .border_color(k.surface_line)
            .child(
                h_flex()
                    .flex_none()
                    .h(px(56.))
                    .pl_5()
                    .pr_3()
                    .gap_2()
                    .border_b_1()
                    .border_color(k.surface_line)
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(text::SM.size)
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(k.text_primary)
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_size(text::LABEL.size)
                                    .text_color(k.text(TextStep::T500))
                                    .child(if image {
                                        format!(
                                            "Image · version {} of {}",
                                            c.page + 1,
                                            c.slides.len()
                                        )
                                    } else {
                                        format!("{} slides", c.slides.len())
                                    }),
                            ),
                    )
                    .when(image, |d| {
                        let media_save = cx.entity();
                        let version = c.version.clone();
                        let saving = self.saving.as_deref() == Some(c.version.as_str());
                        d.child(
                            Button::new(ids::CANVAS_TO_MEDIA)
                                .small()
                                .when(!saved, |b| b.primary())
                                .when(saved, |b| b.ghost())
                                .icon(Icon::new(match (saving, saved) {
                                    (true, _) => KairoIcon::Loader,
                                    (_, true) => KairoIcon::Check,
                                    _ => KairoIcon::Download,
                                }))
                                .label(match (saving, saved) {
                                    (true, _) => "Saving…",
                                    (_, true) => "In Media",
                                    _ => "Save to Media",
                                })
                                .tooltip("Add this version to the media library. It won't go live.")
                                .disabled(saved || saving)
                                .on_click(move |_, _, cx| {
                                    let v = version.clone();
                                    media_save.update(cx, |p, cx| p.save_version(v, cx))
                                }),
                        )
                    })
                    .when(!image, |d| {
                        d.child(
                            Button::new(ids::CANVAS_TO_SLIDES)
                                .cursor_pointer()
                                .small()
                                .when(!saved, |b| b.primary())
                                .when(saved, |b| b.ghost())
                                .icon(Icon::new(if saved {
                                    KairoIcon::Check
                                } else {
                                    KairoIcon::PhPresentation
                                }))
                                .label(if saved { "In Slides" } else { "Save to Slides" })
                                .tooltip("Add this deck to Slides. It won't go live.")
                                .disabled(saved)
                                .on_click(move |_, _, cx| {
                                    let v = version.clone();
                                    slides_save.update(cx, |p, cx| p.save_version(v, cx))
                                }),
                        )
                        .child(
                            Button::new(ids::CANVAS_PPTX)
                                .cursor_pointer()
                                .small()
                                .ghost()
                                .icon(Icon::new(match (self.downloading, fresh) {
                                    (true, _) => KairoIcon::Loader,
                                    (_, true) => KairoIcon::Check,
                                    _ => KairoIcon::Download,
                                }))
                                .label(match (self.downloading, fresh) {
                                    (true, _) => "Saving…",
                                    (_, true) => "In Downloads",
                                    _ => "PowerPoint",
                                })
                                .tooltip("Download as a PowerPoint file with editable text")
                                .disabled(self.downloading)
                                .on_click(move |_, _, cx| {
                                    download.update(cx, |p, cx| p.download_pptx(cx))
                                }),
                        )
                    })
                    .child(div().w(px(1.)).h(px(20.)).mx_1().bg(k.surface_line))
                    .child(
                        Button::new(ids::CANVAS_CLOSE)
                            .cursor_pointer()
                            .small()
                            .ghost()
                            .icon(Icon::new(KairoIcon::XCircle))
                            .accessibility_label(if image {
                                "Close the preview"
                            } else {
                                "Close the slides"
                            })
                            .tooltip("Close")
                            .on_click(move |_, _, cx| close.update(cx, |p, cx| p.close_canvas(cx))),
                    ),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_start()
                    .child(
                        v_flex()
                            .id("imagine-canvas-pages")
                            .flex_none()
                            .h_full()
                            .py_4()
                            .px_3()
                            .gap_3()
                            .overflow_y_scroll()
                            .border_r_1()
                            .border_color(k.surface_line)
                            .children(pages)
                            .when(!several, |d| d.hidden()),
                    )
                    .child(
                        div()
                            .id("imagine-canvas-view")
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_y_scroll()
                            .child(
                                v_flex()
                                    .w_full()
                                    .max_w(px(STAGE_MAX_W))
                                    .mx_auto()
                                    .px_8()
                                    .py_6()
                                    .gap_4()
                                    .child(stage)
                                    .when(several, |d| d.child(nav))
                                    .children(notes),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// A Copy button for a message; it shows a check for a moment after.
    fn copy_button(&self, key: String, text: String, cx: &mut Context<Self>) -> AnyElement {
        let copied = self
            .copied
            .as_ref()
            .is_some_and(|(k, at)| k == &key && at.elapsed().as_secs_f32() < 1.5);
        let page = cx.entity();
        Button::new(SharedString::from(ids::copy(&key)))
            .cursor_pointer()
            .xsmall()
            .ghost()
            .icon(Icon::new(if copied {
                KairoIcon::Check
            } else {
                KairoIcon::Copy
            }))
            .accessibility_label("Copy message")
            .tooltip(if copied { "Copied" } else { "Copy" })
            .on_click(move |_, _, cx| {
                cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(text.clone()));
                let key = key.clone();
                page.update(cx, |p, cx| {
                    p.copied = Some((key, std::time::Instant::now()));
                    cx.notify();
                    // Back to the copy icon once the check has been seen.
                    cx.spawn(async move |this, cx| {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(1600))
                            .await;
                        let _ = this.update(cx, |_, cx| cx.notify());
                    })
                    .detach();
                })
            })
            .into_any_element()
    }

    /// The job running in this conversation, if any.
    fn chat_job(&self, project: Option<&Project>) -> Option<&Job> {
        match project {
            Some(p) => self.job_for(&p.id),
            None => self
                .pending_open
                .and_then(|id| self.jobs.iter().find(|j| j.id == id)),
        }
    }

    /// A running job: thinking, then the assistant's reply and what it is
    /// drawing.
    fn render_job(&self, j: &Job, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let k = *cx.kairo();
        let t = loader::clock(j.started, cx.reduced_motion());
        let id = j.id;
        let steps = thought_steps(&j.thought);
        let Some(reply) = &j.reply else {
            // While the assistant plans: the step it is on as the status,
            // the steps it finished above it, and what it is weighing now.
            let current = steps.last();
            let status = match current {
                Some(s) if !s.title.is_empty() => s.title.clone(),
                Some(_) => "Thinking".into(),
                None if j.started.elapsed().as_secs_f32() < 2. => "Reading your request".into(),
                None => "Planning what to make".into(),
            };
            let finished = steps.len().saturating_sub(1);
            return vec![
                v_flex()
                    .id(SharedString::from(ids::job(id)))
                    .test_support()
                    .max_w(px(IMAGE_W))
                    .gap_2()
                    .children(
                        steps
                            .iter()
                            .take(finished)
                            .filter(|s| !s.title.is_empty())
                            .map(|s| {
                                h_flex()
                                    .gap_2()
                                    .text_size(text::XS.size)
                                    .text_color(k.text(TextStep::T500))
                                    .child(
                                        Icon::new(KairoIcon::Check)
                                            .size_3p5()
                                            .text_color(k.text(TextStep::T400)),
                                    )
                                    .child(s.title.clone())
                                    .into_any_element()
                            }),
                    )
                    .child(loader::status(t, &status, "", &k))
                    .children(current.filter(|s| !s.body.is_empty()).map(|s| {
                        div()
                            .id(ids::THOUGHT)
                            .test_support()
                            .pl(px(22.))
                            .line_clamp(3)
                            .text_size(text::XS.size)
                            .line_height(text::XS.line_height)
                            .text_color(k.text(TextStep::T500))
                            .child(s.body.clone())
                    }))
                    .into_any_element(),
            ];
        };
        let mut out = Vec::new();
        if !steps.is_empty() {
            // The finished reasoning folds into one line that opens it.
            let toggle = cx.entity();
            let open = j.show_thought;
            let secs = j.thought_secs.unwrap_or(0).max(1);
            out.push(
                v_flex()
                    .max_w(px(IMAGE_W))
                    .gap_2()
                    .child(
                        h_flex()
                            .id(ids::THOUGHT_TOGGLE)
                            .test_support()
                            .role(Role::Button)
                            .aria_label(if open {
                                "Hide the reasoning"
                            } else {
                                "Show the reasoning"
                            })
                            .aria_expanded(open)
                            .gap_1()
                            .text_size(text::XS.size)
                            .text_color(k.text(TextStep::T500))
                            .hover(|s| s.text_color(k.text_primary))
                            .child(format!("Thought for {secs}s"))
                            .child(
                                Icon::new(if open {
                                    KairoIcon::ChevronDown
                                } else {
                                    KairoIcon::ChevronRight
                                })
                                .size_3p5(),
                            )
                            .on_click(move |_, _, cx| {
                                toggle.update(cx, |p, cx| {
                                    if let Some(j) = p.jobs.iter_mut().find(|j| j.id == id) {
                                        j.show_thought = !j.show_thought;
                                        cx.notify();
                                    }
                                })
                            }),
                    )
                    .when(open, |d| {
                        d.child(
                            v_flex()
                                .id(ids::THOUGHT)
                                .test_support()
                                .gap_3()
                                .pl_3()
                                .border_l_1()
                                .border_color(k.surface_border)
                                .children(steps.iter().map(|s| {
                                    v_flex()
                                        .gap_0p5()
                                        .when(!s.title.is_empty(), |d| {
                                            d.child(
                                                div()
                                                    .text_size(text::XS.size)
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(k.text(TextStep::T300))
                                                    .child(s.title.clone()),
                                            )
                                        })
                                        .child(
                                            div()
                                                .text_size(text::XS.size)
                                                .line_height(text::XS.line_height)
                                                .text_color(k.text(TextStep::T500))
                                                .child(s.body.clone()),
                                        )
                                })),
                        )
                    })
                    .into_any_element(),
            );
        }
        out.push(said(&k, reply.clone(), None));
        let frame = |w: f32, label: String, t: f32| {
            div()
                .relative()
                .w_full()
                .max_w(px(w))
                .aspect_ratio(16. / 9.)
                .overflow_hidden()
                .rounded(radius::xl2())
                .bg(k.surface_secondary)
                .child(loader::dot_field(t, k.text_primary))
                .child(
                    div()
                        .absolute()
                        .left_4()
                        .right_4()
                        .bottom_3()
                        .child(loader::status(t, &label, "", &k)),
                )
        };
        let deck = matches!(j.action, Some(Action::Deck | Action::EditSlides));
        let status = match j.action {
            Some(Action::Deck | Action::EditSlides) if let Some((done, total)) = j.designing => {
                format!("Designing slide {} of {}", (done + 1).min(total), total)
            }
            Some(Action::Deck | Action::EditSlides) if j.total > 0 && j.done < j.total => {
                format!("Painting pictures {} of {}", j.done + 1, j.total)
            }
            Some(Action::Deck | Action::EditSlides) if j.total > 0 || j.slides > 0 => {
                format!("Laying out {} slides", j.slides.max(1))
            }
            Some(Action::Deck | Action::EditSlides) => "Writing the slides".into(),
            _ if j.project.is_some() => "Editing your image".into(),
            _ => "Drawing your image".into(),
        };
        if !deck {
            out.push(
                v_flex()
                    .id(SharedString::from(ids::job(id)))
                    .test_support()
                    .items_start()
                    .gap_2()
                    .child(frame(IMAGE_W, status, t))
                    .into_any_element(),
            );
            return out;
        }
        // A deck fills in as one card, the way it will arrive, with its
        // outline under it.
        let title = j
            .titles
            .first()
            .cloned()
            .unwrap_or_else(|| "New deck".into());
        let card = h_flex()
            .w_full()
            .max_w(px(DECK_CARD_W))
            .gap_3()
            .p_2()
            .pr_4()
            .rounded(radius::xl())
            .border_1()
            .border_color(k.surface_border)
            .bg(k.surface_secondary)
            .child(
                div()
                    .relative()
                    .flex_none()
                    .w(px(160.))
                    .h(px(90.))
                    .overflow_hidden()
                    .rounded(radius::lg())
                    .bg(k.surface_display)
                    .child(loader::dot_field(t, k.text_primary)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .justify_center()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .child(
                                Icon::new(KairoIcon::PhPresentation)
                                    .size_4()
                                    .text_color(k.hue(Hue::Cyan)),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(text::SM.size)
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(k.text_primary)
                                    .child(title),
                            ),
                    )
                    .child(loader::status(t, &status, "", &k)),
            );
        let outline = v_flex()
            .gap_1()
            .pl_1()
            .children(j.titles.iter().enumerate().map(|(n, title)| {
                h_flex()
                    .gap_2()
                    .text_size(text::XS.size)
                    .text_color(k.text(TextStep::T500))
                    .child(div().w(px(16.)).child(format!("{}.", n + 1)))
                    .child(div().truncate().child(title.clone()))
            }));
        out.push(
            v_flex()
                .id(SharedString::from(ids::job(id)))
                .test_support()
                .items_start()
                .gap_3()
                .child(card)
                .child(outline)
                .into_any_element(),
        );
        out
    }

    /// One image in the thread, with Refine and Save on it and its details
    /// under it.
    fn render_answer(&self, p: &Project, i: usize, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let version = &p.versions[i];
        let deck = !version.slides.is_empty();
        let picked = version.id == p.selected;
        let saved = p.saved.iter().any(|v| v == &version.id);
        let saving = self.saving.as_deref() == Some(version.id.as_str());
        let flash = self
            .saved_flash
            .as_ref()
            .filter(|(id, _)| id == &version.id)
            .map(|(_, at)| at.elapsed().as_secs_f32() / SAVE_FLASH_SECS)
            .filter(|t| *t < 1.);
        let (select, refine, save) = (cx.entity(), cx.entity(), cx.entity());
        let (id1, id2, id3) = (version.id.clone(), version.id.clone(), version.id.clone());
        let pill = |b: Button| b.cursor_pointer().small().rounded_full();
        let refine_button = pill(Button::new(SharedString::from(ids::refine(i))))
            .label("Refine")
            .icon(Icon::new(KairoIcon::Edit2))
            .tooltip("Make the next change from this version")
            .on_click(move |_, window, cx| {
                let id = id2.clone();
                refine.update(cx, |p, cx| {
                    p.select_version(id, cx);
                    p.prompt.update(cx, |s, cx| s.focus(window, cx));
                })
            });
        let (place, done_tip, todo_tip) = if deck {
            (
                "Slides",
                "In Slides. Not on screen.",
                "Add this deck to Slides. It won't go live.",
            )
        } else {
            (
                "Media",
                "In the media library. Not on screen.",
                "Add to the media library. It won't go live.",
            )
        };
        let save_button = pill(Button::new(SharedString::from(ids::save(i))))
            .icon(Icon::new(match (saving, saved) {
                (true, _) => KairoIcon::Loader,
                (_, true) => KairoIcon::Check,
                _ => KairoIcon::Download,
            }))
            .label(match (saving, saved) {
                (true, _) => "Saving…".to_string(),
                (_, true) => "Saved".to_string(),
                _ => format!("Save to {place}"),
            })
            .when_some(flash, |b, t| {
                // Green that fades back over the flash.
                b.bg(k.success_text.opacity(0.85 * (1. - t)))
                    .text_color(k.text_primary)
            })
            .tooltip(if saved { done_tip } else { todo_tip })
            .disabled(saved || saving)
            .on_click(move |_, _, cx| {
                let id = id3.clone();
                save.update(cx, |p, cx| p.save_version(id, cx))
            });
        let ring = |d: gpui_kit::Stateful<gpui_kit::Div>| {
            d.when(picked, |d| {
                d.child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded(radius::xl2())
                        .border_2()
                        .border_color(k.control_accent),
                )
            })
        };
        let media = if deck {
            // One file, like an attachment: the cover and what it is.
            let open = cx.entity();
            let id = version.id.clone();
            let open_now = self
                .canvas
                .as_ref()
                .is_some_and(|c| c.version == version.id);
            div()
                .id(SharedString::from(ids::version(i)))
                .role(Role::Button)
                .aria_label(format!(
                    "Open {} ({} slides)",
                    p.title,
                    version.slides.len()
                ))
                .w_full()
                .max_w(px(DECK_CARD_W))
                .flex()
                .gap_3()
                .p_2()
                .pr_4()
                .rounded(radius::xl())
                .border_1()
                .border_color(if picked || open_now {
                    k.control_accent
                } else {
                    k.surface_border
                })
                .bg(k.surface_secondary)
                .cursor_pointer()
                .hover(|s| s.bg(k.surface_tertiary))
                .child(
                    div()
                        .flex_none()
                        .w(px(160.))
                        .h(px(90.))
                        .overflow_hidden()
                        .rounded(radius::lg())
                        .bg(k.surface_display)
                        .child(
                            img(self.backend.image_path(&version.file))
                                .size_full()
                                .object_fit(ObjectFit::Cover),
                        ),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .justify_center()
                        .gap_1()
                        .child(
                            h_flex()
                                .gap_1p5()
                                .child(
                                    Icon::new(KairoIcon::PhPresentation)
                                        .size_4()
                                        .text_color(k.hue(Hue::Cyan)),
                                )
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(text::SM.size)
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(k.text_primary)
                                        .child(p.title.clone()),
                                ),
                        )
                        .child(
                            div()
                                .text_size(text::XS.size)
                                .text_color(k.text(TextStep::T500))
                                .child(format!(
                                    "{} slides · {}",
                                    version.slides.len(),
                                    if open_now {
                                        "Open beside the chat"
                                    } else {
                                        "Click to open"
                                    }
                                )),
                        ),
                )
                .on_click(move |_, _, cx| {
                    let id = id.clone();
                    open.update(cx, |p, cx| p.open_canvas(id, cx))
                })
                .test_support()
                .into_any_element()
        } else {
            ring(
                div()
                    .id(SharedString::from(ids::version(i)))
                    .role(Role::Button)
                    .aria_label(format!("Version {}: {}", i + 1, version.instruction))
                    .relative()
                    .w_full()
                    .max_w(px(IMAGE_W))
                    .aspect_ratio(16. / 9.)
                    .overflow_hidden()
                    .rounded(radius::xl2())
                    .bg(k.surface_display)
                    .cursor_pointer()
                    .group(SharedString::from(format!("answer-{i}")))
                    .child(
                        img(self.backend.image_path(&version.file))
                            .size_full()
                            .object_fit(ObjectFit::Cover),
                    )
                    // What a click does, shown on hover.
                    .child(
                        h_flex()
                            .absolute()
                            .top_2p5()
                            .right_2p5()
                            .gap_1()
                            .px_2()
                            .py_1()
                            .rounded_full()
                            .bg(k.scrim())
                            .text_size(text::XS.size)
                            .text_color(k.white)
                            .opacity(0.)
                            .group_hover(SharedString::from(format!("answer-{i}")), |s| {
                                s.opacity(1.)
                            })
                            .child(Icon::new(KairoIcon::ZoomIn).size_3p5())
                            .child("Preview"),
                    ),
            )
            .on_click(move |_, _, cx| {
                let id = id1.clone();
                select.update(cx, |p, cx| {
                    p.select_version(id.clone(), cx);
                    p.open_canvas(id, cx);
                })
            })
            .test_support()
            .into_any_element()
        };
        let details = [
            Some(format!("v{}", i + 1)),
            deck.then(|| format!("{} slides", version.slides.len())),
            version.model.map(|m| m.label().to_string()),
            Some(version.quality.label().to_string()),
            picked.then(|| "Next change starts here".to_string()),
        ];
        // A deck is one card: Save to Slides lives in its canvas.
        if deck {
            return media;
        }
        v_flex()
            .items_start()
            .gap_2()
            .child(media)
            .child(
                h_flex()
                    .w_full()
                    .max_w(px(IMAGE_W))
                    .flex_wrap()
                    .gap_2()
                    .child(refine_button)
                    .child(save_button)
                    .child(
                        h_flex()
                            .flex_1()
                            .justify_end()
                            .gap_1()
                            .text_size(text::XS.size)
                            .text_color(k.text(TextStep::T500))
                            .children(
                                details
                                    .into_iter()
                                    .flatten()
                                    .enumerate()
                                    .map(|(n, d)| if n == 0 { d } else { format!("· {d}") }),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_prompt_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let (send, clear) = (cx.entity(), cx.entity());
        let project = self.open_project();
        let running = match project {
            Some(p) => self.job_for(&p.id).map(|j| j.id),
            None => self.pending_open,
        };
        let empty = self.prompt.read(cx).value().trim().is_empty()
            && (project.is_some() || self.template.is_none());
        let chip = (project.is_none() && self.pending_open.is_none())
            .then_some(self.template)
            .flatten()
            .map(|i| {
                let thumb = self.previews.get(i).cloned().flatten();
                h_flex()
                    .flex_none()
                    .gap_1p5()
                    .pl_1()
                    .pr_1()
                    .py_0p5()
                    .rounded_full()
                    .bg(k.surface_tertiary)
                    .border_1()
                    .border_color(k.surface_border)
                    .text_size(text::XS.size)
                    .text_color(k.text_primary)
                    .child(
                        div()
                            .size(px(20.))
                            .flex_none()
                            .overflow_hidden()
                            .rounded_full()
                            .bg(k.hue(TEMPLATES[i].hue))
                            .children(
                                thumb.map(|t| img(t).size_full().object_fit(ObjectFit::Cover)),
                            ),
                    )
                    .child(TEMPLATES[i].title)
                    .child(
                        Button::new(ids::STYLE_CLEAR)
                            .cursor_pointer()
                            .xsmall()
                            .ghost()
                            .icon(Icon::new(KairoIcon::XCircle))
                            .accessibility_label("Remove the style")
                            .on_click(move |_, _, cx| {
                                clear.update(cx, |p, cx| {
                                    p.template = None;
                                    cx.notify();
                                })
                            }),
                    )
            });
        let action = match running {
            // A stop square, drawn: the icon set's square reads as "x²".
            Some(job) => Button::new(ids::CANCEL)
                .cursor_pointer()
                .small()
                .accessibility_label("Stop")
                .tooltip("Stop")
                .rounded_full()
                .child(div().size(px(10.)).rounded(px(2.)).bg(k.text_primary))
                .on_click(move |_, _, cx| send.update(cx, |p, cx| p.cancel(job, cx))),
            None => Button::new(ids::SEND)
                .cursor_pointer()
                .small()
                .primary()
                .icon(Icon::new(KairoIcon::ArrowUp))
                .accessibility_label(if project.is_some() {
                    "Refine"
                } else {
                    "Generate"
                })
                .rounded_full()
                .disabled(empty)
                .on_click(move |_, window, cx| send.update(cx, |p, cx| p.submit(window, cx))),
        };
        let attached = self.attachment.as_ref().map(|a| {
            let clear = cx.entity();
            h_flex()
                .id(ids::ATTACH_CLEAR.to_owned() + "-chip")
                .flex_none()
                .gap_1()
                .pl_2()
                .pr_1()
                .py_0p5()
                .rounded_full()
                .bg(k.surface_tertiary)
                .text_size(text::XS.size)
                .text_color(k.text_primary)
                .child(Icon::new(KairoIcon::FileText).size_3p5())
                .child(div().max_w(px(220.)).truncate().child(a.name.clone()))
                .when(a.truncated, |d| {
                    d.child(
                        div()
                            .text_color(k.text(TextStep::T500))
                            .child("· shortened"),
                    )
                })
                .child(
                    Button::new(ids::ATTACH_CLEAR)
                        .cursor_pointer()
                        .xsmall()
                        .ghost()
                        .icon(Icon::new(KairoIcon::XCircle))
                        .accessibility_label("Remove the attachment")
                        .on_click(move |_, _, cx| {
                            clear.update(cx, |p, cx| {
                                p.attachment = None;
                                cx.notify();
                            })
                        }),
                )
        });
        let plus = {
            let attach = cx.entity();
            Button::new(ids::ATTACH)
                .cursor_pointer()
                .small()
                .ghost()
                .rounded_full()
                .icon(Icon::new(KairoIcon::Plus))
                .accessibility_label("Attach a document")
                .tooltip("Attach a document (txt, md, pdf, docx)")
                .on_click(move |_, _, cx| attach.update(cx, |p, cx| p.attach(cx)))
        };
        let chips: Vec<AnyElement> = chip
            .map(|c| c.into_any_element())
            .into_iter()
            .chain(attached.map(|a| a.into_any_element()))
            .collect();
        // One line: text and controls share a row, centred. Longer text takes
        // the full width and the controls drop to a row of their own (as the
        // Codex composer does). The style tag sits above the text.
        let measured = self.prompt_height.clone();
        let view = cx.entity();
        let field = div()
            .id(ids::PROMPT)
            .test_support()
            .relative()
            .flex_1()
            .min_w_0()
            .child(
                Textarea::new(&self.prompt)
                    .aria_label(if project.is_some() {
                        REFINE_PROMPT
                    } else {
                        NEW_PROMPT
                    })
                    .appearance(false)
                    .text_size(text::BASE.size)
                    .max_h(px(180.)),
            )
            .child(
                canvas(
                    move |bounds, window, _| {
                        let height = f32::from(bounds.size.height);
                        if (measured.get() - height).abs() > 0.5 {
                            measured.set(height);
                            let view = view.clone();
                            window.on_next_frame(move |_, cx| view.update(cx, |_, cx| cx.notify()));
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            );
        let controls = h_flex()
            .flex_none()
            .items_center()
            .gap_1()
            .child(self.render_options(cx))
            .child(action);
        let multi = self.prompt_multi.is_some();
        let bar = v_flex()
            .w_full()
            .rounded(px(if multi || !chips.is_empty() {
                22.
            } else {
                999.
            }))
            .bg(k.surface_elevated)
            .border_1()
            .border_color(k.surface_border)
            .shadow_lg()
            .when(!chips.is_empty(), |bar| {
                bar.child(h_flex().gap_2().pl_4().pt_2p5().children(chips))
            })
            .map(|bar| {
                if multi {
                    bar.child(div().flex().pl_5().pr_4().pt_3().child(field))
                        .child(
                            h_flex()
                                .justify_between()
                                .px_2()
                                .pb_2()
                                .pt_1()
                                .child(plus)
                                .child(controls),
                        )
                } else {
                    bar.child(
                        h_flex()
                            .h(px(52.))
                            .items_center()
                            .gap_1()
                            .pl_2()
                            .pr_1p5()
                            .child(plus)
                            .child(field)
                            .child(controls),
                    )
                }
            });
        let panel = self.options.map(|pane| self.render_options_panel(pane, cx));
        div()
            .absolute()
            .bottom(px(24.))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .px_4()
            .child(
                div()
                    .relative()
                    .w_full()
                    .max_w(px(CHAT_W))
                    .child(bar)
                    .children(panel),
            )
            .into_any_element()
    }

    /// The "Medium ⌄" button that opens the options panel.
    fn render_options(&self, cx: &mut Context<Self>) -> AnyElement {
        let page = cx.entity();
        let label = match self.model {
            ImageModel::Flare => self.quality.label().to_string(),
            ImageModel::Sunburst => format!("Best · {}", self.quality.label()),
        };
        let button = Button::new(ids::OPTIONS)
            .cursor_pointer()
            .small()
            .ghost()
            .label(label)
            .dropdown_caret(true)
            .selected(self.options.is_some())
            .accessibility_label("Model and quality")
            .on_click(move |_, _, cx| {
                page.update(cx, |p, cx| {
                    p.options = match p.options {
                        Some(_) => None,
                        None => Some(Pane::Root),
                    };
                    cx.notify();
                })
            });
        div()
            .relative()
            .child(button)
            .child(Self::track(self.options_hits.clone(), 0))
            .into_any_element()
    }

    /// Records an element's bounds in `hits[slot]` each frame.
    fn track(
        hits: std::rc::Rc<std::cell::Cell<[Option<gpui_kit::Bounds<gpui_kit::Pixels>>; 2]>>,
        slot: usize,
    ) -> impl IntoElement {
        canvas(
            move |bounds, _, _| {
                let mut all = hits.get();
                all[slot] = Some(bounds);
                hits.set(all);
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full()
    }

    /// The options panel: compact rows that open their choices in a flyout
    /// to the right on hover or click, like a context menu.
    fn render_options_panel(&self, open: Pane, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        // The flyout records its bounds again when it is drawn.
        let mut hits = self.options_hits.get();
        hits[1] = None;
        self.options_hits.set(hits);
        let (outside, hits) = (cx.entity(), self.options_hits.clone());
        let can_branch = self.open_project().is_some_and(|p| p.versions.len() > 1);
        let mut entries: Vec<(Pane, &'static str, String)> = vec![
            (Pane::Model, "Model", self.model.label().into()),
            (Pane::Quality, "Quality", self.quality.label().into()),
        ];
        if can_branch {
            let base = if self.from_original {
                "Original"
            } else {
                "Selected"
            };
            entries.push((Pane::Base, "Refine from", base.into()));
        }
        let rows = entries.into_iter().map(|(pane, name, value)| {
            let (hover, click) = (cx.entity(), cx.entity());
            let shown = open == pane;
            let key = match pane {
                Pane::Model => "model",
                Pane::Quality => "quality",
                _ => "base",
            };
            h_flex()
                .id(SharedString::from(ids::option(key)))
                .test_support()
                .role(Role::MenuItem)
                .aria_label(format!("{name}: {value}"))
                .aria_expanded(shown)
                .relative()
                .h(px(30.))
                .mx_1()
                .px_2()
                .gap_2()
                .rounded(radius::base())
                .cursor_pointer()
                .text_size(text::SM.size)
                .when(shown, |d| d.bg(k.surface_menu_hover))
                .hover(|s| s.bg(k.surface_menu_hover))
                .on_hover(move |hovered, _, cx| {
                    if *hovered {
                        hover.update(cx, |p, cx| {
                            if p.options != Some(pane) {
                                p.options = Some(pane);
                                cx.notify();
                            }
                        })
                    }
                })
                .on_click(move |_, _, cx| {
                    click.update(cx, |p, cx| {
                        p.options = Some(pane);
                        cx.notify();
                    })
                })
                .child(div().flex_1().text_color(k.text_primary).child(name))
                .child(div().text_color(k.text(TextStep::T500)).child(value))
                .child(
                    Icon::new(KairoIcon::ChevronRight)
                        .size_3p5()
                        .text_color(k.text(TextStep::T500)),
                )
                .children(shown.then(|| self.render_flyout(pane, cx)))
        });
        v_flex()
            .id(ids::OPTIONS_PANEL)
            .test_support()
            .occlude()
            .on_mouse_down_out(move |e, _, cx| {
                let on_menu = hits.get().iter().flatten().any(|b| b.contains(&e.position));
                if !on_menu {
                    outside.update(cx, |p, cx| {
                        p.options = None;
                        cx.notify();
                    })
                }
            })
            .absolute()
            .bottom(px(60.))
            .right_0()
            .w(px(MENU_W))
            .py_1()
            .rounded(radius::lg())
            .bg(k.surface_menu)
            .border_1()
            .border_color(k.surface_border)
            .shadow_lg()
            .children(rows)
            .child(div().h(px(1.)).my_1().bg(k.surface_menu_line))
            .child(
                div()
                    .px_3()
                    .py_1()
                    .text_size(text::LABEL.size)
                    .text_color(k.text(TextStep::T500))
                    .child(format!(
                        "About ${:.2} per image",
                        estimate_usd(self.quality, 1)
                    )),
            )
            .into_any_element()
    }

    /// The choices for one row, beside the panel.
    fn render_flyout(&self, pane: Pane, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let choices: Vec<(String, &'static str, bool)> = match pane {
            Pane::Model => ImageModel::ALL
                .iter()
                .map(|m| (m.id().to_string(), m.label(), *m == self.model))
                .collect(),
            Pane::Quality => Quality::ALL
                .iter()
                .map(|q| (q.as_str().to_string(), q.label(), *q == self.quality))
                .collect(),
            _ => vec![
                ("selected".into(), "Selected version", !self.from_original),
                ("original".into(), "Original (v1)", self.from_original),
            ],
        };
        v_flex()
            .id("imagine-options-flyout")
            .occlude()
            .child(Self::track(self.options_hits.clone(), 1))
            .absolute()
            .top(px(-5.))
            .left(px(MENU_W - 4.))
            .min_w(px(168.))
            .py_1()
            .rounded(radius::lg())
            .bg(k.surface_menu)
            .border_1()
            .border_color(k.surface_border)
            .shadow_lg()
            .children(choices.into_iter().map(|(key, label, picked)| {
                let page = cx.entity();
                let pick = key.clone();
                h_flex()
                    .id(SharedString::from(ids::choice(&key)))
                    .test_support()
                    .role(Role::MenuItemRadio)
                    .aria_label(label)
                    .aria_toggled(if picked {
                        gpui_kit::Toggled::True
                    } else {
                        gpui_kit::Toggled::False
                    })
                    .h(px(30.))
                    .mx_1()
                    .px_2()
                    .gap_2()
                    .rounded(radius::base())
                    .cursor_pointer()
                    .text_size(text::SM.size)
                    .text_color(k.text_primary)
                    .hover(|s| s.bg(k.surface_menu_hover))
                    .child(
                        div()
                            .w(px(14.))
                            .children(picked.then(|| Icon::new(KairoIcon::Check).size_3p5())),
                    )
                    .child(label)
                    .on_click(move |_, _, cx| {
                        let pick = pick.clone();
                        page.update(cx, |p, cx| {
                            match pane {
                                Pane::Model => {
                                    p.model = ImageModel::from_id(&pick).unwrap_or_default()
                                }
                                Pane::Quality => {
                                    p.quality = Quality::ALL
                                        .into_iter()
                                        .find(|q| q.as_str() == pick)
                                        .unwrap_or_default()
                                }
                                _ => p.from_original = pick == "original",
                            }
                            p.options = None;
                            cx.notify();
                        })
                    })
            }))
            .into_any_element()
    }
}

/// One step of the assistant's reasoning summary: its bold title and what
/// it wrote under it.
#[derive(Clone, Debug, PartialEq)]
pub struct ThoughtStep {
    pub title: String,
    pub body: String,
}

/// The reasoning summary as steps. Summaries arrive as sections, each
/// usually opening with a `**Title**`; text before any title, or a section
/// without one, is a step of its own with no title. Whitespace is tidied so
/// a half-streamed word never shows a stray marker.
pub fn thought_steps(text: &str) -> Vec<ThoughtStep> {
    let tidy = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut steps = Vec::new();
    for section in text.split("\n\n").map(str::trim).filter(|s| !s.is_empty()) {
        let (title, body) = match section.strip_prefix("**").and_then(|s| s.split_once("**")) {
            Some((title, body)) => (tidy(title), tidy(body)),
            // A title still streaming in: show it once it closes.
            None if section.starts_with("**") => (String::new(), String::new()),
            None => (String::new(), tidy(section)),
        };
        match steps.last_mut() {
            // Untitled text continues the step before it.
            Some(ThoughtStep { body: last, .. }) if title.is_empty() && !body.is_empty() => {
                if !last.is_empty() {
                    last.push(' ');
                }
                last.push_str(&body);
            }
            _ if title.is_empty() && body.is_empty() => {}
            _ => steps.push(ThoughtStep { title, body }),
        }
    }
    // Stray emphasis inside a body.
    for s in &mut steps {
        s.body = s.body.replace("**", "");
    }
    steps
}

/// The operator's message, on the right, with Copy under it on hover.
fn bubble(k: &kairo_ui_kit::KairoColors, text: String, copy: Option<AnyElement>) -> AnyElement {
    let group = SharedString::from(format!("bubble-{:x}", hash(&text)));
    v_flex()
        .group(group.clone())
        .w_full()
        .items_end()
        .gap_1()
        .child(
            div()
                .max_w(relative(0.78))
                .px_5()
                .py_3()
                .rounded(px(22.))
                .bg(k.surface_tertiary)
                .text_size(text::BASE.size)
                .text_color(k.text_primary)
                .child(text),
        )
        .children(copy.map(|c| {
            div()
                .opacity(0.)
                .group_hover(group, |s| s.opacity(1.))
                .child(c)
        }))
        .into_any_element()
}

/// The assistant's words, on the left like an answer, with Copy under them.
fn said(k: &kairo_ui_kit::KairoColors, text: String, copy: Option<AnyElement>) -> AnyElement {
    let group = SharedString::from(format!("said-{:x}", hash(&text)));
    v_flex()
        .group(group.clone())
        .w_full()
        .pr(px(80.))
        .gap_1()
        .child(
            div()
                .text_size(text::BASE.size)
                .line_height(text::BASE.line_height)
                .text_color(k.text_primary)
                .child(text),
        )
        .children(copy.map(|c| {
            div()
                .opacity(0.)
                .group_hover(group, |s| s.opacity(1.))
                .child(c)
        }))
        .into_any_element()
}

fn hash(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

/// 1-based position of a version in its project.
fn version_number(p: &Project, id: &str) -> usize {
    p.versions
        .iter()
        .position(|v| v.id == id)
        .map_or(0, |i| i + 1)
}

impl Render for ImaginePage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = *cx.kairo();
        let flashing = self
            .saved_flash
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed().as_secs_f32() < SAVE_FLASH_SECS);
        if (!self.jobs.is_empty() || flashing) && !cx.reduced_motion() {
            window.request_animation_frame();
        }
        if std::mem::take(&mut self.scroll_chat) {
            let handle = self.chat_scroll.clone();
            window.on_next_frame(move |_, _| handle.scroll_to_bottom());
        }
        // Several lines once the text wraps or has a line break; back to one
        // only when it is clearly shorter again, so the layout doesn't flicker.
        let value = self.prompt.read(cx).value();
        let (len, breaks) = (value.chars().count(), value.contains('\n'));
        match self.prompt_multi {
            None if breaks || self.prompt_height.get() > PROMPT_LINE * 1.5 => {
                self.prompt_multi = Some(len)
            }
            Some(at) if len == 0 || (!breaks && len + 12 < at) => self.prompt_multi = None,
            _ => {}
        }
        let in_chat = self.open.is_some() || self.chat;
        if in_chat != self.placeholder_chat {
            self.placeholder_chat = in_chat;
            let text = if in_chat { REFINE_PROMPT } else { NEW_PROMPT };
            self.prompt
                .update(cx, |p, cx| p.set_placeholder(text, window, cx));
        }
        let body = if !self.loaded {
            div().into_any_element()
        } else if !self.connected {
            self.render_connect(cx)
        } else {
            let pending = self.chat;
            match self.open_project().cloned() {
                Some(p) => self.render_chat(Some(&p), cx),
                None if pending => self.render_chat(None, cx),
                None => self.render_gallery(cx),
            }
        };
        let banner = self
            .error
            .clone()
            .map(|e| {
                div()
                    .id(ids::ERROR)
                    .test_support()
                    .role(Role::Alert)
                    .mx_6()
                    .mb_3()
                    .px_4()
                    .py_2()
                    .rounded(radius::md())
                    .bg(k.tints.red)
                    .text_size(text::XS.size)
                    .text_color(k.danger_text)
                    .child(e)
                    .into_any_element()
            })
            .or_else(|| {
                self.status.clone().map(|s| {
                    div()
                        .id(ids::STATUS)
                        .test_support()
                        .role(Role::Status)
                        .mx_6()
                        .mb_3()
                        .text_size(text::XS.size)
                        .text_color(k.success_text)
                        .child(s)
                        .into_any_element()
                })
            });
        v_flex()
            .id(ids::PAGE)
            .test_support()
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .min_h_0()
            .bg(k.surface)
            .child(
                v_flex()
                    .size_full()
                    .min_h_0()
                    // The slide preview needs the whole window beside the chat.
                    .when(self.canvas.is_none(), |d| d.max_w(px(MAX_W)))
                    .mx_auto()
                    .children(banner)
                    .child(div().flex_1().min_h_0().child(body)),
            )
            .when(
                self.loaded && self.connected && self.canvas.is_none(),
                |d| d.child(self.render_prompt_bar(cx)),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasoning_reads_as_titled_steps() {
        let text = "**Planning the deck**\n\nI want five slides, one idea each.\n\n**Choosing the look**\n\nWarm light,  calm type.\n\nThen the art.\n\n**Writ";
        let steps = thought_steps(text);
        assert_eq!(steps.len(), 2, "a half-streamed title waits: {steps:?}");
        assert_eq!(steps[0].title, "Planning the deck");
        assert_eq!(steps[1].body, "Warm light, calm type. Then the art.");
        let plain = thought_steps("Looking at the request. It asks for **five** slides.");
        assert_eq!(plain[0].title, "");
        assert_eq!(
            plain[0].body,
            "Looking at the request. It asks for five slides."
        );
        assert!(thought_steps("  ").is_empty());
    }

    #[test]
    fn styles_follow_the_prompt() {
        let t = &TEMPLATES[4];
        assert_eq!(compose_prompt("  a lighthouse ", None), "a lighthouse");
        assert_eq!(compose_prompt("", Some(t)), t.prompt);
        assert_eq!(
            compose_prompt("a lighthouse", Some(t)),
            format!("a lighthouse. {}.", t.prompt)
        );
        assert_eq!(compose_prompt(" ", None), "");
    }
}
