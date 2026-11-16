//! The Documents page (`Documents.tsx` with `DocumentImportModal.tsx`,
//! `DocumentSlidePreview.tsx` and `SlideConverterDialog.tsx`): the slide
//! library with its libraries, the page grid (virtualised by row), pushing a
//! page, the unattended slideshow, clicker and keyboard paging, the live
//! slide's embedded video transport, rename and remove, and the import and
//! converter dialogs. The live rail and media dock are handed in by the app.

use gpui_kit::component::FocusTrapElement;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenuItem};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::{
    Disableable, Icon, Selectable, Sizable, VirtualListScrollHandle, h_flex, v_flex, v_virtual_list,
};
use gpui_kit::{
    AnyElement, AnyView, AppContext, Bounds, Context, Entity, EventEmitter, FocusHandle, Focusable,
    FontWeight, InteractiveElement, IntoElement, KeyDownEvent, ObjectFit, ParentElement, Pixels,
    Render, Role, ScrollStrategy, SharedString, StatefulInteractiveElement, Styled, StyledImage,
    Subscription, Task, Window, canvas, deferred, div, img, prelude::FluentBuilder, px, size,
};
use kairo_domain::collections::{
    DEFAULT_LIBRARY_ID, LibrariesCommand, LibraryKind as DomainKind, LibraryState,
    items_in_library, library_counts,
};
use kairo_domain::documents::{
    DocumentFormat, DocumentNeedsConverter, DocumentPlaybackStatus, DocumentPrepareOptions,
    DocumentVideoCommand, DocumentsSettings, ProjectionDocument, SlideConverterStatus,
    documents_error_message, normalize_document_name, normalize_documents_settings,
};
use kairo_domain::keyboard_shortcuts::{
    ShortcutKeyEvent, command_for_shortcut, shortcut_from_event,
};
use kairo_domain::media::format_media_clock;
use kairo_domain::settings::{ShortcutBindings, ShortcutCommand};
use kairo_ui_kit::components::{
    LibraryDragItem, LibraryKind, LibraryRow, LibrarySection, LibrarySectionEvent,
};
use kairo_ui_kit::tokens::{Hue, TextStep, radius, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};
use serde_json::json;
use ui_media::MediaBackend;
use ui_media::player::{PreviewPlayer, Range as ClipRange};

use crate::backend::{
    DocumentsAnswer, DocumentsBackend, DocumentsCall, DocumentsFeed, ImportKind, ImportOutcome,
    ImportStep,
};
use crate::model::{
    self, Advance, ConverterView, ImportProgress, PLAYBACK_POLL_MS, PUSH_RETRY_MS, THUMB_KEY,
    THUMB_MAX, THUMB_MIN, VideoGate,
};

/// The transport's skip, as `LIVE_VIDEO_SKIP_SECONDS`.
const SKIP: f64 = 5.;
/// The seek bars' resolution.
const SEEK_STEPS: f32 = 1000.;

/// Element ids, for tests.
pub mod ids {
    pub const PAGE: &str = "documents-page";
    pub const IMPORT: &str = "documents-import";
    pub const IMPORT_EMPTY: &str = "documents-import-empty";
    pub const NO_DOCUMENTS: &str = "documents-none";
    pub const EMPTY: &str = "documents-empty";
    pub const ERROR: &str = "documents-error";
    pub const STATUS: &str = "documents-status";
    pub const HEADER: &str = "documents-header";
    pub const COUNT: &str = "documents-count";
    pub const SLIDESHOW: &str = "documents-slideshow";
    pub const SLIDESHOW_SETTINGS: &str = "documents-slideshow-settings";
    pub const SLIDESHOW_MENU: &str = "documents-slideshow-menu";
    pub const SECONDS: &str = "documents-seconds";
    pub const LOOP: &str = "documents-loop";
    pub const AUTO_START: &str = "documents-auto-start";
    pub const SIZE: &str = "documents-size";
    pub const REMOVE: &str = "documents-remove";
    pub const WARNINGS: &str = "documents-warnings";
    pub const GRID: &str = "documents-grid";
    pub const RENAME: &str = "documents-rename";
    pub const PREVIEW: &str = "documents-preview";
    pub const PREVIEW_ONLY: &str = "documents-preview-only";
    pub const PUSH_FOR_VIDEO: &str = "documents-push-for-video";
    pub const IMPORT_DIALOG: &str = "documents-import-dialog";
    pub const IMPORT_LINE: &str = "documents-import-line";
    pub const IMPORT_CANCEL: &str = "documents-import-cancel";
    pub const IMPORT_RETRY: &str = "documents-import-retry";
    pub const IMPORT_CLOSE: &str = "documents-import-close";
    pub const IMPORT_FAILED: &str = "documents-import-failed";
    pub const CONVERTER: &str = "documents-converter";
    pub const CONVERTER_DOWNLOAD: &str = "documents-converter-download";
    pub const CONVERTER_CANCEL: &str = "documents-converter-cancel";
    pub const CONVERTER_SIMPLIFIED: &str = "documents-converter-simplified";
    pub const CONVERTER_PROGRESS: &str = "documents-converter-progress";
    pub fn document(id: &str) -> String {
        format!("documents-doc-{id}")
    }
    pub fn preset(seconds: u32) -> String {
        format!("documents-preset-{seconds}")
    }
    pub fn slide(index: usize) -> String {
        format!("documents-slide-{index}")
    }
    pub fn clip(index: usize) -> String {
        format!("documents-preview-clip-{index}")
    }
    pub fn video(index: usize) -> String {
        format!("documents-video-{index}")
    }
    pub fn video_toggle(index: usize) -> String {
        format!("documents-video-toggle-{index}")
    }
    pub fn video_restart(index: usize) -> String {
        format!("documents-video-restart-{index}")
    }
    pub fn video_back(index: usize) -> String {
        format!("documents-video-back-{index}")
    }
    pub fn video_forward(index: usize) -> String {
        format!("documents-video-forward-{index}")
    }
}

/// What the page tells the app.
#[derive(Clone, Debug, PartialEq)]
pub enum DocumentsEvent {
    /// A page went live (or preview only): the rail's label.
    Pushed { label: String, applied: bool },
}

fn ask(
    backend: &Arc<dyn DocumentsBackend>,
    call: DocumentsCall,
) -> impl Future<Output = Result<DocumentsAnswer, String>> + use<> {
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

fn clean(error: &str) -> String {
    documents_error_message(error).to_owned()
}

/// GPUI key names as the browser's `KeyboardEvent.key`.
fn dom_key(key: &str) -> String {
    match key {
        "left" => "ArrowLeft".into(),
        "right" => "ArrowRight".into(),
        "up" => "ArrowUp".into(),
        "down" => "ArrowDown".into(),
        "pageup" => "PageUp".into(),
        "pagedown" => "PageDown".into(),
        "space" => " ".into(),
        "enter" => "Enter".into(),
        "escape" => "Escape".into(),
        "home" => "Home".into(),
        "end" => "End".into(),
        other => other.into(),
    }
}

/// The clicker's step for a key under the operator's bindings.
pub fn clicker_step(event: &KeyDownEvent, bindings: &ShortcutBindings) -> Option<isize> {
    let k = &event.keystroke;
    if k.modifiers.function && k.key.is_empty() {
        return None;
    }
    let key = dom_key(&k.key);
    let shortcut = shortcut_from_event(&ShortcutKeyEvent {
        key: &key,
        meta_key: k.modifiers.platform,
        ctrl_key: k.modifiers.control,
        alt_key: k.modifiers.alt,
        shift_key: k.modifiers.shift,
    })?;
    match command_for_shortcut(&shortcut, bindings)? {
        ShortcutCommand::Next => Some(1),
        ShortcutCommand::Previous => Some(-1),
        _ => None,
    }
}

struct ConverterPrompt {
    needs: DocumentNeedsConverter,
    kind: Option<ImportKind>,
    started: bool,
    done: bool,
}

/// The shown slide's embedded videos in the preview: a muted monitor that
/// follows the outputs' players, or, with no output connected, the players
/// themselves (`DocumentSlidePreview`'s `local`).
struct Clips {
    key: (String, usize),
    players: Vec<Entity<PreviewPlayer>>,
    /// Shown once played or seeked (`data-visible`).
    visible: Vec<bool>,
    _observers: Vec<Subscription>,
}

/// Within this of the outputs' time, a playing monitor is left alone.
const FOLLOW_PLAYING: f64 = 0.4;
const FOLLOW_PAUSED: f64 = 0.04;
/// A clip this close to its end counts as ended.
const END_SLACK: f64 = 0.03;

pub struct DocumentsPage {
    backend: Arc<dyn DocumentsBackend>,
    focus: FocusHandle,
    /// Focus for the import and converter dialogs (Radix dialog behaviour).
    dialog_focus: kairo_ui_kit::components::ModalFocus,
    documents: Vec<ProjectionDocument>,
    loaded: bool,
    libraries: LibraryState,
    active_library: String,
    library: Entity<LibrarySection>,
    library_rows: Vec<LibraryRow>,
    selected: Option<String>,
    page: usize,
    busy: bool,
    status: String,
    error: Option<String>,
    renaming: Option<String>,
    rename: Entity<InputState>,
    cancel_rename: bool,
    settings: DocumentsSettings,
    shortcuts: ShortcutBindings,
    playing: bool,
    countdown: u32,
    slideshow_menu: bool,
    seconds: Entity<InputState>,
    thumb: f32,
    thumb_slider: Entity<SliderState>,
    import: Option<ImportProgress>,
    importing: bool,
    canceling: bool,
    cancel_flag: Option<Arc<AtomicBool>>,
    retry_kind: Option<ImportKind>,
    converter_prompt: Option<ConverterPrompt>,
    converter: Option<SlideConverterStatus>,
    live: Option<(String, usize)>,
    local: Option<(String, usize)>,
    playback: Option<DocumentPlaybackStatus>,
    seek_bars: Vec<Entity<SliderState>>,
    scrubbing: Option<(usize, f64)>,
    grid: VirtualListScrollHandle,
    grid_bounds: Rc<Cell<Bounds<Pixels>>>,
    scroll_to_page: bool,
    rail: Option<AnyView>,
    dock: Option<AnyView>,
    media: Option<Arc<dyn MediaBackend>>,
    clips: Option<Clips>,
    polls: u64,
    _dwell: Option<Task<()>>,
    _poll: Option<Task<()>>,
    _feed: Task<()>,
    _listening: Box<dyn Send>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DocumentsEvent> for DocumentsPage {}

impl Focusable for DocumentsPage {
    fn focus_handle(&self, _: &gpui_kit::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl DocumentsPage {
    pub fn new(
        backend: Arc<dyn DocumentsBackend>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let snapshot = backend.snapshot();
        let (tx, mut rx) = futures_channel::mpsc::unbounded::<DocumentsFeed>();
        let listening = backend.listen(Arc::new(move |f| {
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
        let thumb = model::saved_thumb(backend.read_ui(THUMB_KEY).as_ref());
        let thumb_slider = cx.new(|_| {
            SliderState::new()
                .min(THUMB_MIN)
                .max(THUMB_MAX)
                .step(model::THUMB_STEP)
                .default_value(thumb)
        });
        let rename = cx.new(|cx| InputState::new(window, cx));
        let seconds = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(format_seconds(snapshot.settings.slideshow_sec))
        });
        let library = cx.new(|_| LibrarySection::new(LibraryKind::Documents));
        let mut subs = vec![
            cx.subscribe(&thumb_slider, |this, _, e: &SliderEvent, cx| {
                if let SliderEvent::Change(v) = e {
                    this.set_thumb(v.start(), cx);
                }
            }),
            cx.subscribe(&rename, |this, state, e: &InputEvent, cx| match e {
                InputEvent::PressEnter { .. } | InputEvent::Blur => {
                    let draft = state.read(cx).value().to_string();
                    this.commit_rename(draft, cx);
                }
                _ => {}
            }),
            cx.subscribe(&seconds, |this, state, e: &InputEvent, cx| {
                if let InputEvent::Change = e {
                    let value = state.read(cx).value().trim().parse::<f64>();
                    if let Ok(v) = value {
                        this.save_slideshow(json!({ "slideshowSec": v }), cx);
                    }
                }
            }),
            cx.subscribe(&library, |this, _, e: &LibrarySectionEvent, cx| {
                this.library_event(e.clone(), cx)
            }),
        ];
        let weak = cx.entity().downgrade();
        let rename_focus = rename.read(cx).focus_handle(cx);
        subs.push(cx.intercept_keystrokes(move |e, window, cx| {
            if e.keystroke.key == "escape" && rename_focus.is_focused(window) {
                let _ = weak.update(cx, |this, cx| {
                    this.cancel_rename = true;
                    this.renaming = None;
                    cx.notify();
                });
            }
        }));
        let mut page = Self {
            backend,
            focus: cx.focus_handle(),
            dialog_focus: kairo_ui_kit::components::ModalFocus::new(cx),
            documents: Vec::new(),
            loaded: false,
            libraries: snapshot.libraries,
            active_library: DEFAULT_LIBRARY_ID.into(),
            library,
            library_rows: Vec::new(),
            selected: None,
            page: 0,
            busy: false,
            status: String::new(),
            error: None,
            renaming: None,
            rename,
            cancel_rename: false,
            settings: snapshot.settings,
            shortcuts: snapshot.shortcuts,
            playing: false,
            countdown: 0,
            slideshow_menu: false,
            seconds,
            thumb,
            thumb_slider,
            import: None,
            importing: false,
            canceling: false,
            cancel_flag: None,
            retry_kind: None,
            converter_prompt: None,
            converter: None,
            live: None,
            local: None,
            playback: None,
            seek_bars: Vec::new(),
            scrubbing: None,
            grid: VirtualListScrollHandle::new(),
            grid_bounds: Rc::new(Cell::new(Bounds::default())),
            scroll_to_page: false,
            rail: None,
            dock: None,
            media: None,
            clips: None,
            polls: 0,
            _dwell: None,
            _poll: None,
            _feed: feed,
            _listening: listening,
            _subscriptions: subs,
        };
        page.list(cx);
        page
    }

    pub fn set_rail(&mut self, rail: Option<AnyView>, cx: &mut Context<Self>) {
        self.rail = rail;
        cx.notify();
    }

    pub fn set_dock(&mut self, dock: Option<AnyView>, cx: &mut Context<Self>) {
        self.dock = dock;
        cx.notify();
    }

    /// Opens the slide preview's embedded videos (`DocumentSlidePreview`).
    /// Without it the preview marks where each video sits.
    pub fn set_media(&mut self, media: Option<Arc<dyn MediaBackend>>, cx: &mut Context<Self>) {
        self.media = media;
        self.clips = None;
        cx.notify();
    }

    // ─── Reads, for tests and the app ──────────────────────────────────────

    pub fn documents(&self) -> &[ProjectionDocument] {
        &self.documents
    }
    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }
    pub fn page(&self) -> usize {
        self.page
    }
    pub fn live(&self) -> Option<(&str, usize)> {
        self.live.as_ref().map(|(id, p)| (id.as_str(), *p))
    }
    pub fn playing(&self) -> bool {
        self.playing
    }
    pub fn countdown(&self) -> u32 {
        self.countdown
    }
    pub fn settings(&self) -> &DocumentsSettings {
        &self.settings
    }
    pub fn thumb(&self) -> f32 {
        self.thumb
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    pub fn status(&self) -> &str {
        &self.status
    }
    pub fn import_progress(&self) -> Option<&ImportProgress> {
        self.import.as_ref()
    }
    pub fn converter_open(&self) -> bool {
        self.converter_prompt.is_some()
    }
    pub fn playback(&self) -> Option<&DocumentPlaybackStatus> {
        self.playback.as_ref()
    }
    pub fn renaming(&self) -> Option<&str> {
        self.renaming.as_deref()
    }
    pub fn local_preview(&self) -> bool {
        self.local_mode()
    }
    /// Whether each of the preview's embedded videos is showing.
    pub fn clips_shown(&self) -> Vec<bool> {
        self.clips
            .as_ref()
            .map(|c| c.visible.clone())
            .unwrap_or_default()
    }
    /// Playback reads made, for the poll's cost.
    pub fn polls(&self) -> u64 {
        self.polls
    }

    fn current(&self) -> Option<&ProjectionDocument> {
        let id = self.selected.as_deref()?;
        self.documents.iter().find(|d| d.id == id)
    }

    fn visible(&self) -> Vec<&ProjectionDocument> {
        items_in_library(
            &self.libraries,
            &self.documents,
            &self.active_library,
            |d| d.id.as_str(),
        )
    }

    fn page_videos(&self) -> usize {
        self.current()
            .and_then(|d| d.videos.as_ref())
            .and_then(|v| v.get(self.page))
            .map_or(0, Vec::len)
    }

    fn local_mode(&self) -> bool {
        self.local
            .as_ref()
            .is_some_and(|(id, p)| Some(id.as_str()) == self.selected.as_deref() && *p == self.page)
    }

    fn live_playback(&self) -> Option<&DocumentPlaybackStatus> {
        let id = self.selected.as_deref()?;
        self.playback
            .as_ref()
            .filter(|p| p.id == id && p.page == self.page as f64)
    }

    // ─── Feed ──────────────────────────────────────────────────────────────

    fn on_feed(&mut self, feed: DocumentsFeed, cx: &mut Context<Self>) {
        match feed {
            DocumentsFeed::Documents(documents) => self.set_documents(documents, cx),
            DocumentsFeed::Libraries(state) => {
                self.libraries = state;
                self.check_active_library();
            }
            DocumentsFeed::Settings(settings) => self.settings = settings,
            DocumentsFeed::Shortcuts(bindings) => self.shortcuts = bindings,
            DocumentsFeed::Import(kind) => {
                if !self.busy {
                    self.import(Some(kind), DocumentPrepareOptions::default(), cx);
                }
            }
            DocumentsFeed::Converter(status) => self.set_converter(status, cx),
            DocumentsFeed::LiveCleared => {
                self.live = None;
                self.local = None;
                self.playback = None;
                self.stop_slideshow();
                self.refresh_poll(cx);
            }
        }
        cx.notify();
    }

    fn check_active_library(&mut self) {
        if !self
            .libraries
            .libraries
            .iter()
            .any(|l| l.id == self.active_library)
        {
            self.active_library = DEFAULT_LIBRARY_ID.into();
        }
    }

    fn set_documents(&mut self, documents: Vec<ProjectionDocument>, cx: &mut Context<Self>) {
        self.documents = documents;
        self.loaded = true;
        if let Some(id) = &self.selected
            && !self.documents.iter().any(|d| &d.id == id)
        {
            self.select(None, cx);
        }
        if let Some(d) = self.current()
            && self.page >= d.pages.len()
        {
            self.page = 0;
        }
        cx.notify();
    }

    fn list(&mut self, cx: &mut Context<Self>) {
        let call = ask(&self.backend, DocumentsCall::List);
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| match answer {
                Ok(DocumentsAnswer::Documents(d)) => this.set_documents(d, cx),
                Ok(_) => {}
                Err(e) => {
                    this.loaded = true;
                    this.error = Some(clean(&e));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    // ─── Selection and pushing ─────────────────────────────────────────────

    /// A deck that is gone, or replaced by another, must not keep advancing.
    fn select(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        if self.selected != id {
            self.stop_slideshow();
            self.local = None;
        }
        self.selected = id;
        self.page = 0;
        self.renaming = None;
        self.scroll_to_page = true;
        self.refresh_poll(cx);
        cx.notify();
    }

    pub fn open_document(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.status.clear();
        self.error = None;
        self.select(Some(id.to_owned()), cx);
    }

    pub fn push(&mut self, target: usize, cx: &mut Context<Self>) {
        let Some(doc) = self.current() else {
            return;
        };
        if self.busy || target >= doc.pages.len() {
            return;
        }
        // Electron's live label says `Page` for decks too.
        let (id, name, unit) = (doc.id.clone(), doc.name.clone(), "Page");
        self.busy = true;
        self.error = None;
        self.status = "Pushing…".into();
        cx.notify();
        let call = ask(
            &self.backend,
            DocumentsCall::Push {
                id: id.clone(),
                page: target,
            },
        );
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match answer {
                    Ok(DocumentsAnswer::Pushed { applied }) => {
                        let was_local = this.local.as_ref() == Some(&(id.clone(), target));
                        this.local = (!applied).then(|| (id.clone(), target));
                        this.live = Some((id.clone(), target));
                        this.page = target;
                        this.scroll_to_page = true;
                        if applied || !was_local {
                            this.playback = None;
                        }
                        this.status = if applied {
                            String::new()
                        } else {
                            "Preview only · No output connected".into()
                        };
                        cx.emit(DocumentsEvent::Pushed {
                            label: format!("{name} · {unit} {}", target + 1),
                            applied,
                        });
                        // The first page going live is the whole cue.
                        if this.settings.slideshow_auto_start && !this.playing {
                            this.playing = true;
                        }
                        this.restart_dwell(cx);
                        this.refresh_poll(cx);
                    }
                    Ok(_) => this.status.clear(),
                    Err(e) => {
                        this.status.clear();
                        this.error = Some(clean(&e));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Clicker and keyboard paging.
    pub fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.current().is_none() || self.busy {
            return;
        }
        let target = self.page as isize + delta;
        if target >= 0 {
            self.push(target as usize, cx);
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.current().is_none()
            || event.keystroke.key.is_empty()
            || self.import.is_some()
            || self.converter_prompt.is_some()
            || self.slideshow_menu
            || self.rename.read(cx).focus_handle(cx).is_focused(window)
            || self.seconds.read(cx).focus_handle(cx).is_focused(window)
        {
            return;
        }
        if let Some(delta) = clicker_step(event, &self.shortcuts) {
            cx.stop_propagation();
            self.step(delta, cx);
        }
    }

    // ─── Slideshow ─────────────────────────────────────────────────────────

    pub fn toggle_slideshow(&mut self, cx: &mut Context<Self>) {
        if self.playing {
            self.stop_slideshow();
        } else if self.current().is_some_and(|d| d.pages.len() >= 2) {
            self.playing = true;
            self.restart_dwell(cx);
            self.refresh_poll(cx);
        }
        cx.notify();
    }

    fn stop_slideshow(&mut self) {
        self.playing = false;
        self.countdown = 0;
        self._dwell = None;
    }

    /// The dwell restarts from the page, so a manual push or a clicker press
    /// mid-run restarts the timer. A slide with videos waits for them.
    fn restart_dwell(&mut self, cx: &mut Context<Self>) {
        self._dwell = None;
        self.countdown = 0;
        if !self.playing {
            return;
        }
        if self.current().is_none_or(|d| d.pages.len() < 2) {
            self.stop_slideshow();
            return;
        }
        if self.page_videos() > 0 {
            return;
        }
        let seconds = self.settings.slideshow_sec.max(1.) as u32;
        self.countdown = seconds;
        let timer = cx.background_executor().clone();
        self._dwell = Some(cx.spawn(async move |this, cx| {
            for _ in 0..seconds {
                timer.timer(Duration::from_secs(1)).await;
                let alive = this.update(cx, |this, cx| {
                    this.countdown = this.countdown.saturating_sub(1);
                    cx.notify();
                });
                if alive.is_err() {
                    return;
                }
            }
            // A push in flight: wait for it rather than dropping the turn.
            loop {
                let Ok(busy) = this.update(cx, |this, _| this.busy) else {
                    return;
                };
                if !busy {
                    break;
                }
                timer.timer(Duration::from_millis(PUSH_RETRY_MS)).await;
            }
            let _ = this.update(cx, |this, cx| this.advance(cx));
        }));
    }

    fn advance(&mut self, cx: &mut Context<Self>) {
        let Some(pages) = self.current().map(|d| d.pages.len()) else {
            return;
        };
        match model::advance(self.page, pages, self.settings.slideshow_loop) {
            Advance::Push(next) => self.push(next, cx),
            Advance::Stop => self.stop_slideshow(),
        }
        cx.notify();
    }

    fn save_slideshow(&mut self, patch: serde_json::Value, cx: &mut Context<Self>) {
        let mut raw = serde_json::to_value(&self.settings).unwrap_or_default();
        if let (Some(raw), Some(patch)) = (raw.as_object_mut(), patch.as_object()) {
            for (k, v) in patch {
                raw.insert(k.clone(), v.clone());
            }
        }
        let next = normalize_documents_settings(&raw);
        if next == self.settings {
            return;
        }
        let dwell = next.slideshow_sec != self.settings.slideshow_sec;
        self.settings = next.clone();
        self.backend
            .call(DocumentsCall::SaveSettings(next), Box::new(|_| {}));
        if dwell {
            self.restart_dwell(cx);
        }
        cx.notify();
    }

    pub fn set_slideshow_seconds(
        &mut self,
        seconds: f64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.save_slideshow(json!({ "slideshowSec": seconds }), cx);
        let shown = format_seconds(self.settings.slideshow_sec);
        self.seconds
            .update(cx, |s, cx| s.set_value(shown, window, cx));
    }

    /// The slide size slider (persisted per viewer).
    pub fn set_slide_width(&mut self, value: f32, cx: &mut Context<Self>) {
        self.set_thumb(value, cx);
    }

    fn set_thumb(&mut self, value: f32, cx: &mut Context<Self>) {
        let value = model::snap_thumb(value);
        if value != self.thumb {
            self.thumb = value;
            self.backend.write_ui(THUMB_KEY, json!(value));
            cx.notify();
        }
    }

    // ─── Embedded videos ───────────────────────────────────────────────────

    /// Reads the live slide's videos every 350 ms while it has any.
    fn refresh_poll(&mut self, cx: &mut Context<Self>) {
        self._poll = None;
        let Some(id) = self.selected.clone() else {
            self.playback = None;
            return;
        };
        if self.local_mode() || self.page_videos() == 0 {
            if !self.local_mode() {
                self.playback = None;
            }
            return;
        }
        let page = self.page;
        let backend = self.backend.clone();
        let timer = cx.background_executor().clone();
        self._poll = Some(cx.spawn(async move |this, cx| {
            loop {
                let answer = ask(
                    &backend,
                    DocumentsCall::Playback {
                        id: id.clone(),
                        page,
                    },
                )
                .await;
                let alive = this.update(cx, |this, cx| {
                    this.polls += 1;
                    this.set_playback(
                        answer.ok().and_then(|a| match a {
                            DocumentsAnswer::Playback(p) => p,
                            _ => None,
                        }),
                        cx,
                    );
                });
                if alive.is_err() {
                    return;
                }
                timer.timer(Duration::from_millis(PLAYBACK_POLL_MS)).await;
            }
        }));
    }

    fn set_playback(&mut self, playback: Option<DocumentPlaybackStatus>, cx: &mut Context<Self>) {
        let count = playback.as_ref().map_or(0, |p| p.videos.len());
        while self.seek_bars.len() < count {
            let index = self.seek_bars.len();
            let bar = cx.new(|_| SliderState::new().min(0.).max(SEEK_STEPS).step(1.));
            self._subscriptions.push(cx.subscribe(
                &bar,
                move |this, _, e: &SliderEvent, cx| match e {
                    SliderEvent::Change(v) => {
                        let duration = this
                            .live_playback()
                            .and_then(|p| p.videos.get(index))
                            .map_or(0., |v| v.duration);
                        let seconds = v.start() as f64 / SEEK_STEPS as f64 * duration;
                        this.scrubbing = Some((index, seconds));
                        this.control(index, DocumentVideoCommand::Seek { seconds }, cx);
                    }
                    SliderEvent::Release(_) => {
                        this.scrubbing = None;
                        cx.notify();
                    }
                },
            ));
            self.seek_bars.push(bar);
        }
        self.playback = playback;
        if !self.local_mode() {
            self.follow_clips(cx);
        }
        if self.playing && self.page_videos() > 0 {
            match model::video_gate(self.live_playback()) {
                VideoGate::Stop => self.stop_slideshow(),
                VideoGate::Advance if !self.busy => self.advance(cx),
                _ => {}
            }
        }
        cx.notify();
    }

    // ─── The preview's embedded videos ─────────────────────────────────────

    fn clip_file(doc: &ProjectionDocument, page: usize, file: &str) -> String {
        // Pages and media share the document's directory.
        let path = doc.pages.get(page).map_or("", String::as_str);
        let dir = path.rfind(['/', '\\']).map_or("", |i| &path[..i]);
        format!("{dir}/media/{file}")
    }

    /// Opens a player per video of the slide the preview shows, and drops
    /// them when it shows another (or none).
    fn ensure_clips(&mut self, cx: &mut Context<Self>) {
        let local = self.local_mode();
        let want = match (&self.media, self.current()) {
            (Some(_), Some(doc)) if local || self.page_videos() > 0 => {
                Some((doc.id.clone(), self.page))
            }
            _ => None,
        };
        if self.clips.as_ref().map(|c| &c.key) == want.as_ref() {
            return;
        }
        self.clips = None;
        let (Some(key), Some(media), Some(doc)) =
            (want, self.media.clone(), self.current().cloned())
        else {
            return;
        };
        let videos = doc
            .videos
            .as_ref()
            .and_then(|v| v.get(key.1))
            .cloned()
            .unwrap_or_default();
        let mut players = Vec::new();
        let mut observers = Vec::new();
        for clip in &videos {
            let path = Self::clip_file(&doc, key.1, &clip.file);
            let media = media.clone();
            let player = cx.new(|cx| PreviewPlayer::new(media, path, 960, false, cx));
            player.update(cx, |p, _| {
                p.set_range(ClipRange {
                    start: clip.start,
                    end: clip.end,
                    looped: false,
                    rate: 1.,
                });
                p.seek(clip.start);
            });
            observers.push(cx.observe(&player, |this, _, cx| {
                if this.local_mode() {
                    this.publish_local(cx);
                } else {
                    cx.notify();
                }
            }));
            players.push(player);
        }
        self.clips = Some(Clips {
            key,
            visible: vec![false; players.len()],
            players,
            _observers: observers,
        });
        if local {
            self.publish_local(cx);
        } else {
            self.follow_clips(cx);
        }
    }

    /// The monitor follows the outputs: seeks when it drifts, plays and
    /// pauses with them, and shows a video only while they do.
    fn follow_clips(&mut self, cx: &mut Context<Self>) {
        let Some(doc) = self.current().cloned() else {
            return;
        };
        let states = self.live_playback().map(|p| p.videos.clone());
        let Some(clips) = &mut self.clips else {
            return;
        };
        let starts: Vec<f64> = doc
            .videos
            .as_ref()
            .and_then(|v| v.get(clips.key.1))
            .map(|v| v.iter().map(|c| c.start).collect())
            .unwrap_or_default();
        for (i, player) in clips.players.iter().enumerate() {
            let state = states.as_ref().and_then(|s| s.get(i));
            let start = starts.get(i).copied().unwrap_or(0.);
            let target = start + state.map_or(0., |s| s.current_time);
            let still = state.is_none_or(|s| s.paused || s.ended || s.error.is_some());
            let tolerance = if still { FOLLOW_PAUSED } else { FOLLOW_PLAYING };
            player.update(cx, |p, _| {
                if p.duration().is_some() && (p.position() - target).abs() > tolerance {
                    p.seek(target);
                }
                if still {
                    if p.playing() {
                        p.pause();
                    }
                } else if !p.playing() {
                    p.play();
                }
            });
            clips.visible[i] = state.is_some_and(|s| s.visible && s.error.is_none());
        }
    }

    /// No output: the preview's own players take the transport.
    fn local_control(
        &mut self,
        video: usize,
        command: DocumentVideoCommand,
        cx: &mut Context<Self>,
    ) {
        let Some(clip) = self
            .current()
            .and_then(|d| d.videos.as_ref())
            .and_then(|v| v.get(self.page))
            .and_then(|v| v.get(video))
            .cloned()
        else {
            return;
        };
        let Some(clips) = &mut self.clips else {
            return;
        };
        let Some(player) = clips.players.get(video).cloned() else {
            return;
        };
        if !matches!(command, DocumentVideoCommand::Pause) {
            clips.visible[video] = true;
        }
        player.update(cx, |p, _| {
            let end = clip.end.or(p.duration()).unwrap_or(f64::INFINITY);
            match command {
                DocumentVideoCommand::Pause => p.pause(),
                DocumentVideoCommand::Seek { seconds } => {
                    let to = (clip.start + seconds.max(0.)).min(end);
                    if to.is_finite() {
                        p.seek(to);
                    }
                }
                DocumentVideoCommand::Play | DocumentVideoCommand::Restart => {
                    let ended = p.position() >= end - END_SLACK;
                    if matches!(command, DocumentVideoCommand::Restart) || ended {
                        p.seek(clip.start);
                    }
                    p.play();
                }
            }
        });
        self.publish_local(cx);
    }

    /// The local players' state, as the outputs would report it.
    fn publish_local(&mut self, cx: &mut Context<Self>) {
        let Some(doc) = self.current().cloned() else {
            return;
        };
        let Some(clips) = &self.clips else {
            return;
        };
        let specs = doc
            .videos
            .as_ref()
            .and_then(|v| v.get(clips.key.1))
            .cloned()
            .unwrap_or_default();
        let mut videos = Vec::new();
        for (i, clip) in specs.iter().enumerate() {
            let Some(player) = clips.players.get(i) else {
                break;
            };
            let p = player.read(cx);
            let end = match (clip.end, p.duration()) {
                (Some(a), Some(b)) => a.min(b),
                (a, b) => a.or(b).unwrap_or(f64::INFINITY),
            };
            let at = if p.frames() > 0 {
                p.position()
            } else {
                clip.start
            };
            let ended = p.frames() > 0 && at >= end - END_SLACK;
            videos.push(kairo_domain::documents::DocumentVideoStatus {
                current_time: (at - clip.start).max(0.),
                duration: if end.is_finite() {
                    (end - clip.start).max(0.)
                } else {
                    0.
                },
                paused: !p.playing(),
                ended,
                visible: clips.visible.get(i).copied().unwrap_or(false),
                error: p
                    .error()
                    .map(|_| "This video could not be played.".to_owned()),
            });
        }
        let status = DocumentPlaybackStatus {
            id: doc.id.clone(),
            page: clips.key.1 as f64,
            videos,
        };
        if self.playback.as_ref() != Some(&status) {
            self.set_playback(Some(status), cx);
        }
    }

    pub fn control(&mut self, video: usize, command: DocumentVideoCommand, cx: &mut Context<Self>) {
        if self.local_mode() {
            self.local_control(video, command, cx);
            return;
        }
        let Some(id) = self.selected.clone() else {
            return;
        };
        let page = self.page;
        let control = ask(
            &self.backend,
            DocumentsCall::Control {
                id: id.clone(),
                page,
                video,
                command,
            },
        );
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let result = control.await;
            let read = ask(&backend, DocumentsCall::Playback { id, page }).await;
            let _ = this.update(cx, |this, cx| {
                if let Err(e) = result {
                    this.error = Some(clean(&e));
                }
                if let Ok(DocumentsAnswer::Playback(p)) = read {
                    this.set_playback(p, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    // ─── Rename and remove ─────────────────────────────────────────────────

    pub fn start_rename(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self
            .documents
            .iter()
            .find(|d| d.id == id)
            .map(|d| d.name.clone())
        else {
            return;
        };
        if self.selected.as_deref() != Some(id) {
            self.select(Some(id.to_owned()), cx);
        }
        self.renaming = Some(id.to_owned());
        self.cancel_rename = false;
        self.rename.update(cx, |s, cx| {
            s.set_value(name, window, cx);
            s.focus(window, cx);
        });
        cx.notify();
    }

    fn commit_rename(&mut self, draft: String, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.cancel_rename) {
            self.renaming = None;
            cx.notify();
            return;
        }
        let Some(id) = self.renaming.clone() else {
            return;
        };
        if self.busy {
            return;
        }
        let existing = self
            .documents
            .iter()
            .find(|d| d.id == id)
            .map(|d| d.name.clone());
        if existing.as_deref() == Some(draft.trim()) {
            self.renaming = None;
            cx.notify();
            return;
        }
        // The service's rules, so a bad name is caught before the round trip.
        if let Err(e) = normalize_document_name(&draft) {
            self.error = Some(e.into());
            cx.notify();
            return;
        }
        self.busy = true;
        self.error = None;
        let call = ask(&self.backend, DocumentsCall::Rename { id, name: draft });
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match answer {
                    Ok(DocumentsAnswer::Documents(d)) => {
                        this.renaming = None;
                        this.set_documents(d, cx);
                    }
                    Ok(_) => this.renaming = None,
                    Err(e) => this.error = Some(clean(&e)),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn remove(&mut self, id: String, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        self.renaming = None;
        let call = ask(&self.backend, DocumentsCall::Remove(id.clone()));
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match answer {
                    Ok(DocumentsAnswer::Documents(d)) => {
                        if this.live.as_ref().is_some_and(|(l, _)| *l == id) {
                            this.live = None;
                        }
                        let next = d.first().map(|d| d.id.clone());
                        this.set_documents(d, cx);
                        if this.selected.is_none() || this.selected.as_deref() == Some(&id) {
                            this.select(next, cx);
                        }
                        this.status.clear();
                    }
                    Ok(_) => {}
                    Err(e) => this.error = Some(clean(&e)),
                }
                cx.notify();
            });
        })
        .detach();
    }

    // ─── Import ────────────────────────────────────────────────────────────

    /// No `kind` opens one picker for PDFs and PowerPoints together.
    pub fn import(
        &mut self,
        kind: Option<ImportKind>,
        options: DocumentPrepareOptions,
        cx: &mut Context<Self>,
    ) {
        if self.busy {
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel_flag = Some(cancel.clone());
        self.canceling = false;
        self.retry_kind = kind;
        self.import = None;
        self.importing = true;
        self.busy = true;
        self.error = None;
        self.status.clear();
        cx.notify();
        let (tx, mut rx) = futures_channel::mpsc::unbounded::<ImportStep>();
        let (done_tx, done_rx) = futures_channel::oneshot::channel();
        self.backend.import(
            kind,
            options,
            cancel,
            Arc::new(move |step| {
                let _ = tx.unbounded_send(step);
            }),
            Box::new(move |r| {
                let _ = done_tx.send(r);
            }),
        );
        cx.spawn(async move |this, cx| {
            use futures_util::StreamExt;
            while let Some(step) = rx.next().await {
                let alive = this.update(cx, |this, cx| {
                    // A late step after the import ended changes nothing.
                    if this.importing {
                        this.import
                            .get_or_insert_with(ImportProgress::new)
                            .advance(step);
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    return;
                }
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            let outcome = done_rx
                .await
                .unwrap_or_else(|_| Err("The request was dropped.".into()));
            let _ = this.update(cx, |this, cx| this.import_done(kind, outcome, cx));
        })
        .detach();
    }

    fn import_done(
        &mut self,
        kind: Option<ImportKind>,
        outcome: Result<ImportOutcome, String>,
        cx: &mut Context<Self>,
    ) {
        self.busy = false;
        self.importing = false;
        self.canceling = false;
        self.cancel_flag = None;
        match outcome {
            Ok(ImportOutcome::NotPicked) | Ok(ImportOutcome::Cancelled) => self.import = None,
            Ok(ImportOutcome::NeedsConverter(needs)) => {
                self.import = None;
                self.converter_prompt = Some(ConverterPrompt {
                    needs,
                    kind,
                    started: false,
                    done: false,
                });
                let call = ask(&self.backend, DocumentsCall::ConverterStatus);
                cx.spawn(async move |this, cx| {
                    if let Ok(DocumentsAnswer::Converter(s)) = call.await {
                        let _ = this.update(cx, |this, cx| this.set_converter(s, cx));
                    }
                })
                .detach();
            }
            Ok(ImportOutcome::Imported { id, documents }) => {
                self.import = None;
                self.set_documents(documents, cx);
                self.select(Some(id), cx);
            }
            Err(e) => {
                let progress = self.import.get_or_insert_with(ImportProgress::new);
                progress.error = Some(clean(&e));
            }
        }
        cx.notify();
    }

    pub fn cancel_import(&mut self, cx: &mut Context<Self>) {
        let finishing = self.import.as_ref().is_some_and(ImportProgress::finishing);
        if self.canceling || finishing {
            return;
        }
        if let Some(flag) = &self.cancel_flag {
            flag.store(true, Ordering::SeqCst);
            self.canceling = true;
            cx.notify();
        }
    }

    pub fn close_import(&mut self, cx: &mut Context<Self>) {
        if self.import.as_ref().is_some_and(ImportProgress::failed) {
            self.import = None;
            cx.notify();
        }
    }

    pub fn retry_import(&mut self, cx: &mut Context<Self>) {
        self.import = None;
        let kind = self.retry_kind;
        self.import(kind, DocumentPrepareOptions::default(), cx);
    }

    // ─── Slide converter ───────────────────────────────────────────────────

    fn set_converter(&mut self, status: SlideConverterStatus, cx: &mut Context<Self>) {
        self.converter = Some(status);
        let ready = self
            .converter
            .as_ref()
            .is_some_and(|s| s.state == kairo_domain::documents::SlideConverterState::Ready);
        // Ready — just downloaded or already there — means carry on.
        if ready
            && let Some(prompt) = self.converter_prompt.as_mut()
            && !prompt.done
        {
            prompt.done = true;
            let prompt = self.converter_prompt.take().unwrap();
            self.import(
                prompt.kind,
                DocumentPrepareOptions {
                    resume: Some(prompt.needs.token),
                    simplified: None,
                },
                cx,
            );
        }
        cx.notify();
    }

    pub fn download_converter(&mut self, cx: &mut Context<Self>) {
        if let Some(p) = self.converter_prompt.as_mut() {
            p.started = true;
            self.backend
                .call(DocumentsCall::ConverterInstall, Box::new(|_| {}));
            cx.notify();
        }
    }

    pub fn cancel_converter(&mut self, cx: &mut Context<Self>) {
        let view = model::converter_view(
            self.converter.as_ref(),
            self.converter_prompt.as_ref().is_some_and(|p| p.started),
        );
        if view == ConverterView::Busy {
            self.backend
                .call(DocumentsCall::ConverterCancel, Box::new(|_| {}));
        }
        self.converter_prompt = None;
        cx.notify();
    }

    pub fn import_simplified(&mut self, cx: &mut Context<Self>) {
        let Some(prompt) = self.converter_prompt.take() else {
            return;
        };
        self.import(
            prompt.kind,
            DocumentPrepareOptions {
                resume: Some(prompt.needs.token),
                simplified: Some(true),
            },
            cx,
        );
    }

    // ─── Libraries ─────────────────────────────────────────────────────────

    fn library_event(&mut self, event: LibrarySectionEvent, cx: &mut Context<Self>) {
        let kind = DomainKind::Documents;
        let command = match event {
            LibrarySectionEvent::Select(id) => {
                self.active_library = id.to_string();
                cx.notify();
                return;
            }
            LibrarySectionEvent::AddItem(_) => return,
            LibrarySectionEvent::Create => LibrariesCommand::Create {
                kind,
                name: "New library".into(),
            },
            LibrarySectionEvent::Rename { id, name } => LibrariesCommand::Rename {
                kind,
                library_id: id.to_string(),
                name: name.to_string(),
            },
            LibrarySectionEvent::Delete(id) => LibrariesCommand::Delete {
                kind,
                library_id: id.to_string(),
            },
            LibrarySectionEvent::Assign {
                item_id,
                library_id,
                ..
            } => LibrariesCommand::Assign {
                kind,
                item_ids: vec![item_id.to_string()],
                library_id: library_id.to_string(),
            },
        };
        let create = matches!(command, LibrariesCommand::Create { .. });
        let assigned = match &command {
            LibrariesCommand::Assign { library_id, .. } => Some(library_id.clone()),
            _ => None,
        };
        let before: Vec<String> = self
            .libraries
            .libraries
            .iter()
            .map(|l| l.id.clone())
            .collect();
        let call = ask(&self.backend, DocumentsCall::Libraries(command));
        let section = self.library.downgrade();
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| {
                let error = match answer {
                    Ok(DocumentsAnswer::Libraries(state)) => {
                        let created = state
                            .libraries
                            .iter()
                            .find(|l| !before.contains(&l.id))
                            .map(|l| l.id.clone());
                        this.libraries = state;
                        this.check_active_library();
                        let _ = section.update(cx, |s, cx| {
                            if let Some(id) = assigned {
                                s.landed(id, cx);
                            }
                            if create && let Some(id) = created {
                                s.set_active(id, cx);
                            }
                        });
                        None
                    }
                    Ok(_) => None,
                    Err(e) => Some(SharedString::from(clean(&e))),
                };
                let _ = section.update(cx, |s, cx| s.set_error(error, cx));
                cx.notify();
            });
        })
        .detach();
    }

    /// The seek bars follow playback unless the operator is dragging one.
    fn sync_seek(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = self.live_playback().cloned() else {
            return;
        };
        for (i, v) in p.videos.iter().enumerate() {
            if self.scrubbing.is_some_and(|(s, _)| s == i) {
                continue;
            }
            let at = if v.duration > 0. {
                (v.current_time / v.duration).clamp(0., 1.) as f32 * SEEK_STEPS
            } else {
                0.
            };
            if let Some(bar) = self.seek_bars.get(i)
                && bar.read(cx).value().start() != at
            {
                bar.update(cx, |s, cx| s.set_value(at, window, cx));
            }
        }
    }

    fn sync(&mut self, cx: &mut Context<Self>) {
        let counts = library_counts(&self.libraries, &self.documents, |d| d.id.as_str());
        let rows: Vec<LibraryRow> = self
            .libraries
            .libraries
            .iter()
            .map(|l| LibraryRow {
                id: l.id.clone().into(),
                name: l.name.clone().into(),
                count: counts.get(&l.id).copied().unwrap_or(0),
            })
            .collect();
        let total = self.documents.len();
        if rows != self.library_rows {
            self.library_rows = rows.clone();
            self.library
                .update(cx, |s, cx| s.set_libraries(rows, total, cx));
        }
        let active = self.active_library.clone();
        self.library.update(cx, |s, cx| s.set_active(active, cx));
    }
}

fn format_seconds(seconds: f64) -> String {
    kairo_domain::js::number_to_string(seconds)
}

// ─── Drawing ──────────────────────────────────────────────────────────────

impl Render for DocumentsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync(cx);
        self.ensure_clips(cx);
        if let Some(clips) = &self.clips {
            for p in &clips.players {
                p.update(cx, |p, _| p.release_retired(window));
            }
        }
        self.sync_seek(window, cx);
        let dialogs: Vec<AnyElement> = self
            .render_import_dialog(cx)
            .into_iter()
            .chain(self.render_converter_dialog(cx))
            .collect();
        self.dialog_focus.sync(!dialogs.is_empty(), window, cx);
        let k = *cx.kairo();
        let sidebar = self.render_sidebar(cx);
        let main = match self.current().cloned() {
            None => self.render_empty(cx),
            Some(doc) => self.render_document(&doc, window, cx),
        };
        let error = self.error.clone().map(|e| {
            div()
                .id(ids::ERROR)
                .test_support()
                .role(Role::Alert)
                .flex_none()
                .px_4()
                .py_2()
                .bg(k.tints.red)
                .text_size(text::XS.size)
                .text_color(k.danger_text)
                .child(e)
        });
        h_flex()
            .size_full()
            .min_h_0()
            .items_stretch()
            .child(
                v_flex()
                    .flex_1()
                    .h_full()
                    .min_w_0()
                    .min_h_0()
                    .child(
                        v_flex()
                            .id(ids::PAGE)
                            .test_support()
                            .track_focus(&self.focus)
                            .on_key_down(cx.listener(Self::on_key))
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .min_w_0()
                            .bg(k.surface)
                            .child(self.render_header(cx))
                            .children(error)
                            .child(
                                h_flex()
                                    .flex_1()
                                    .min_h_0()
                                    .items_stretch()
                                    .child(sidebar)
                                    .child(kairo_ui_kit::components::page_sidebar::handle(
                                        "documents-sidebar-handle",
                                        None,
                                        cx,
                                    ))
                                    .child(v_flex().flex_1().min_w_0().min_h_0().child(main)),
                            )
                            .children(dialogs),
                    )
                    .children(self.dock.clone()),
            )
            .children(self.rail.clone())
    }
}

impl DocumentsPage {
    /// The title line (Electron put it in the header's toolbar row).
    fn render_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let title = match self.current() {
            Some(d) => h_flex()
                .gap_1()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .text_color(k.text(TextStep::T200))
                        .child(d.name.clone()),
                )
                .child(div().text_color(k.text(TextStep::T500)).child(format!(
                    "· {}/{}",
                    self.page + 1,
                    d.pages.len()
                )))
                .into_any_element(),
            None => div()
                .text_color(k.text(TextStep::T600))
                .child("No slides selected")
                .into_any_element(),
        };
        let status = (self.busy && self.import.is_none() && !self.status.is_empty()).then(|| {
            div()
                .id(ids::STATUS)
                .test_support()
                .truncate()
                .text_color(k.accent(kairo_ui_kit::tokens::Step::S400))
                .child(if self.canceling {
                    "Canceling…".to_string()
                } else {
                    self.status.clone()
                })
        });
        let preview_only = (!self.busy && !self.status.is_empty()).then(|| {
            div()
                .truncate()
                .text_color(k.text(TextStep::T400))
                .child(self.status.clone())
        });
        h_flex()
            .id(ids::HEADER)
            .test_support()
            .flex_none()
            .h(px(28.))
            .px_3()
            .gap_3()
            .min_w_0()
            .text_size(text::XS.size)
            .child(title)
            .children(status)
            .children(preview_only)
            .into_any_element()
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let page = cx.entity();
        let importing = self.importing;
        let visible: Vec<ProjectionDocument> = self.visible().into_iter().cloned().collect();
        let rows: Vec<AnyElement> = visible
            .iter()
            .map(|d| self.render_document_row(d, cx))
            .collect();
        let empty = visible.is_empty().then(|| {
            div()
                .id(ids::NO_DOCUMENTS)
                .test_support()
                .px_2()
                .py_3()
                .text_size(text::XS.size)
                .text_color(k.text(TextStep::T600))
                .child(if !self.loaded {
                    ""
                } else if self.documents.is_empty() {
                    "No slides yet"
                } else {
                    "Nothing in this library"
                })
        });
        v_flex()
            .flex_none()
            .w(px(
                kairo_ui_kit::components::page_sidebar::PageSidebar::width(None, cx),
            ))
            .h_full()
            .min_h_0()
            .bg(k.surface_secondary)
            .child(
                div().flex_none().px_3().pt_3().child(
                    h_flex()
                        .id(ids::IMPORT)
                        .test_support()
                        .role(Role::Button)
                        .aria_label(if importing {
                            "Importing…"
                        } else {
                            "Import slides"
                        })
                        .w_full()
                        .gap_3()
                        .px_3()
                        .py_2p5()
                        .rounded(radius::lg())
                        .bg(k.surface_tertiary)
                        .when(!self.busy, |d| {
                            d.cursor_pointer()
                                .hover(|s| s.bg(k.surface_elevated))
                                .on_click(move |_, _, cx| {
                                    page.update(cx, |p, cx| {
                                        p.import(None, DocumentPrepareOptions::default(), cx)
                                    })
                                })
                        })
                        .when(self.busy, |d| {
                            // Pressing it from assistive technology while busy
                            // does nothing, as the pointer does.
                            d.opacity(0.7).on_click(|_, _, _| {})
                        })
                        .child(
                            div()
                                .flex()
                                .flex_none()
                                .items_center()
                                .justify_center()
                                .size_8()
                                .rounded(radius::md())
                                .bg(k.control_accent)
                                .text_color(k.on_accent)
                                .child(Icon::new(if importing {
                                    KairoIcon::Loader
                                } else {
                                    KairoIcon::Upload
                                })),
                        )
                        .child(
                            v_flex()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_size(text::SM.size)
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(k.text_primary)
                                        .child(if importing {
                                            "Importing…"
                                        } else {
                                            "Import slides"
                                        }),
                                )
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(text::LABEL.size)
                                        .text_color(k.text(TextStep::T400))
                                        .child("PDF or PowerPoint"),
                                ),
                        ),
                ),
            )
            .child(div().flex_none().p_1p5().child(self.library.clone()))
            .child(
                v_flex()
                    .id("documents-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_2()
                    .children(empty)
                    .children(rows),
            )
            .into_any_element()
    }

    fn render_document_row(&self, d: &ProjectionDocument, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let active = self.selected.as_deref() == Some(d.id.as_str());
        let renaming = self.renaming.as_deref() == Some(d.id.as_str());
        let icon = div()
            .flex_none()
            .opacity(0.6)
            .child(Icon::new(KairoIcon::FileText).size_3p5());
        if renaming {
            return h_flex()
                .gap_2()
                .px_1p5()
                .py_1()
                .mb_0p5()
                .rounded(radius::sm())
                .bg(k.row_selected())
                .child(icon)
                .child(
                    div()
                        .id(ids::RENAME)
                        .test_support()
                        .flex_1()
                        .min_w_0()
                        .child(
                            Input::new(&self.rename)
                                .xsmall()
                                .disabled(self.busy)
                                .aria_label("Document name"),
                        ),
                )
                .into_any_element();
        }
        let count = d.pages.len();
        let (open, rename, rename_menu, remove_menu) =
            (cx.entity(), cx.entity(), cx.entity(), cx.entity());
        let (id, rid, mid, xid) = (d.id.clone(), d.id.clone(), d.id.clone(), d.id.clone());
        let busy = self.busy;
        let drag = LibraryDragItem {
            item_id: d.id.clone().into(),
            label: d.name.clone().into(),
            kind: LibraryKind::Documents,
        };
        h_flex()
            .id(SharedString::from(ids::document(&d.id)))
            .test_support()
            .role(Role::Button)
            .aria_label(d.name.clone())
            .aria_selected(active)
            .gap_2()
            .px_1p5()
            .py_1()
            .mb_0p5()
            .rounded(radius::sm())
            .cursor_pointer()
            .when(active, |e| {
                e.bg(k.row_selected()).text_color(k.text_primary)
            })
            .when(!active, |e| {
                e.text_color(k.text(TextStep::T400))
                    .hover(|s| s.bg(k.white.opacity(0.05)))
            })
            .on_drag(drag, |item, _, _, cx| cx.new(|_| item.clone()))
            .on_click(move |e, window, cx| {
                if busy {
                    return;
                }
                if e.click_count() == 2 {
                    rename.update(cx, |p, cx| p.start_rename(&rid, window, cx));
                } else {
                    open.update(cx, |p, cx| p.open_document(&id, cx));
                }
            })
            .context_menu(move |menu, _, _| {
                let (r, x, mid, xid) = (
                    rename_menu.clone(),
                    remove_menu.clone(),
                    mid.clone(),
                    xid.clone(),
                );
                menu.item(
                    PopupMenuItem::new("Rename")
                        .icon(Icon::new(KairoIcon::Edit2))
                        .disabled(busy)
                        .on_click(move |_, window, cx| {
                            r.update(cx, |p, cx| p.start_rename(&mid, window, cx))
                        }),
                )
                .separator()
                .item(
                    PopupMenuItem::new("Delete")
                        .icon(Icon::new(KairoIcon::Trash2))
                        .disabled(busy)
                        .on_click(move |_, _, cx| x.update(cx, |p, cx| p.remove(xid.clone(), cx))),
                )
            })
            .child(icon)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .truncate()
                            .text_size(text::XS.size)
                            .font_weight(FontWeight::MEDIUM)
                            .child(d.name.clone()),
                    )
                    .child(
                        div()
                            .mt_0p5()
                            .text_size(text::MICRO.size)
                            .text_color(k.text(TextStep::T600))
                            .child(format!(
                                "{count} {}",
                                if count == 1 { "slide" } else { "slides" }
                            )),
                    ),
            )
            .into_any_element()
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let page = cx.entity();
        v_flex()
            .id(ids::EMPTY)
            .test_support()
            .flex_1()
            .items_center()
            .justify_center()
            .gap_3()
            .px_6()
            .child(
                div()
                    .text_size(text::SM.size)
                    .text_color(k.text(TextStep::T500))
                    .child("Import a PowerPoint or PDF to show it slide by slide"),
            )
            .child(
                Button::new(ids::IMPORT_EMPTY)
                    .small()
                    .icon(Icon::new(KairoIcon::Upload))
                    .label("Import slides")
                    .disabled(self.busy)
                    .on_click(move |_, _, cx| {
                        page.update(cx, |p, cx| {
                            p.import(None, DocumentPrepareOptions::default(), cx)
                        })
                    }),
            )
            .into_any_element()
    }

    fn render_document(
        &mut self,
        doc: &ProjectionDocument,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let videos = self.page_videos();
        let local = self.local_mode();
        let warnings = doc.warnings.clone().filter(|w| !w.is_empty()).map(|w| {
            v_flex()
                .id(ids::WARNINGS)
                .test_support()
                .flex_none()
                .px_3()
                .py_2()
                .gap_1()
                .text_size(text::XS.size)
                .text_color(k.text(TextStep::T300))
                .child(format!("Import notes ({})", w.len()))
                .children(w.into_iter().map(|w| div().pl_2().child(format!("• {w}"))))
        });
        let transport = (videos > 0).then(|| self.render_transport(cx));
        let preview = (local || videos > 0).then(|| self.render_preview(doc, local, cx));
        v_flex()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .child(self.render_toolbar(doc, window, cx))
            .children(warnings)
            .children(transport)
            .children(preview)
            .child(self.render_grid(doc, cx))
            .into_any_element()
    }

    fn render_toolbar(
        &self,
        doc: &ProjectionDocument,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let (toggle, menu, remove) = (cx.entity(), cx.entity(), cx.entity());
        let id = doc.id.clone();
        let label = if self.playing {
            if self.page_videos() > 0 {
                "Video".to_string()
            } else {
                format!("{}s", self.countdown)
            }
        } else {
            "Slideshow".to_string()
        };
        h_flex()
            .flex_none()
            .h(px(44.))
            .px_3()
            .gap_3()
            .justify_between()
            .border_b_1()
            .border_color(k.surface_line)
            .child(
                h_flex()
                    .relative()
                    .gap_0p5()
                    .child(
                        Button::new(ids::SLIDESHOW)
                            .small()
                            .ghost()
                            .selected(self.playing)
                            .icon(Icon::new(if self.playing {
                                KairoIcon::Square
                            } else {
                                KairoIcon::Play
                            }))
                            .label(label)
                            .tooltip(if self.playing {
                                "Stop the slideshow".to_string()
                            } else {
                                format!(
                                    "Advance every {}s",
                                    format_seconds(self.settings.slideshow_sec)
                                )
                            })
                            .disabled(doc.pages.len() < 2)
                            .on_click(move |_, _, cx| {
                                toggle.update(cx, |p, cx| p.toggle_slideshow(cx))
                            }),
                    )
                    .child(
                        Button::new(ids::SLIDESHOW_SETTINGS)
                            .small()
                            .ghost()
                            .icon(Icon::new(KairoIcon::SlidersHorizontal))
                            .accessibility_label("Slideshow settings")
                            .tooltip("Slideshow settings")
                            .on_click(move |_, _, cx| {
                                menu.update(cx, |p, cx| {
                                    p.slideshow_menu = !p.slideshow_menu;
                                    cx.notify();
                                })
                            }),
                    )
                    .children(self.slideshow_menu.then(|| self.render_slideshow_menu(cx))),
            )
            .child(
                h_flex()
                    .gap_3()
                    .child(
                        h_flex()
                            .id(ids::SIZE)
                            .test_support()
                            .gap_1p5()
                            .text_color(k.text(TextStep::T500))
                            .child(Icon::new(KairoIcon::Minus).size_3())
                            .child(div().w(px(96.)).child(
                                Slider::new(&self.thumb_slider).accessibility_label("Slide size"),
                            ))
                            .child(Icon::new(KairoIcon::Plus).size_3()),
                    )
                    .child(
                        div()
                            .id(ids::COUNT)
                            .test_support()
                            .text_size(text::XS.size)
                            .text_color(k.text(TextStep::T500))
                            .child(format!("{} / {}", self.page + 1, doc.pages.len())),
                    )
                    .child(
                        Button::new(ids::REMOVE)
                            .small()
                            .ghost()
                            .icon(Icon::new(KairoIcon::Trash2))
                            .accessibility_label("Remove imported document")
                            .tooltip("Remove imported copy")
                            .disabled(self.busy)
                            .on_click(move |_, _, cx| {
                                remove.update(cx, |p, cx| p.remove(id.clone(), cx))
                            }),
                    ),
            )
            .into_any_element()
    }

    fn render_slideshow_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let (looping, auto, close) = (cx.entity(), cx.entity(), cx.entity());
        let presets: Vec<AnyElement> = model::PRESETS
            .iter()
            .map(|&s| {
                let page = cx.entity();
                Button::new(SharedString::from(ids::preset(s as u32)))
                    .xsmall()
                    .ghost()
                    .selected(self.settings.slideshow_sec == s)
                    .label(format!("{s}s"))
                    .on_click(move |_, window, cx| {
                        page.update(cx, |p, cx| p.set_slideshow_seconds(s, window, cx))
                    })
                    .into_any_element()
            })
            .collect();
        deferred(
            v_flex()
                .id(ids::SLIDESHOW_MENU)
                .test_support()
                .role(Role::Dialog)
                .aria_label("Slideshow settings")
                .occlude()
                .absolute()
                .left_0()
                .top(px(34.))
                .w(px(240.))
                .p_3()
                .gap_2()
                .rounded(radius::lg())
                .bg(k.surface_elevated)
                .shadow_lg()
                .on_mouse_down_out(move |_, _, cx| {
                    close.update(cx, |p, cx| {
                        p.slideshow_menu = false;
                        cx.notify();
                    })
                })
                .child(
                    v_flex()
                        .gap_1()
                        .text_size(text::LABEL.size)
                        .text_color(k.text(TextStep::T400))
                        .child("Seconds per page")
                        .child(
                            div().id(ids::SECONDS).test_support().child(
                                Input::new(&self.seconds)
                                    .small()
                                    .aria_label("Seconds per page"),
                            ),
                        ),
                )
                .child(h_flex().flex_wrap().gap_1().children(presets))
                .child(
                    Checkbox::new(ids::LOOP)
                        .label("Loop back to page 1")
                        .checked(self.settings.slideshow_loop)
                        .on_click(move |v, _, cx| {
                            looping.update(cx, |p, cx| {
                                p.save_slideshow(json!({ "slideshowLoop": *v }), cx)
                            })
                        }),
                )
                .child(
                    Checkbox::new(ids::AUTO_START)
                        .label("Start on first push")
                        .checked(self.settings.slideshow_auto_start)
                        .on_click(move |v, _, cx| {
                            auto.update(cx, |p, cx| {
                                p.save_slideshow(json!({ "slideshowAutoStart": *v }), cx)
                            })
                        }),
                )
                .child(
                    div()
                        .text_size(text::MICRO.size)
                        .text_color(k.text(TextStep::T500))
                        .child("Pushing a page by hand or with a clicker restarts the timer."),
                ),
        )
        .with_priority(1)
        .into_any_element()
    }

    fn render_transport(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let Some(p) = self.live_playback().cloned() else {
            return div()
                .id(ids::PUSH_FOR_VIDEO)
                .test_support()
                .flex_none()
                .px_3()
                .py_2()
                .text_size(text::XS.size)
                .text_color(k.text(TextStep::T400))
                .child("Push this slide to play its embedded video.")
                .into_any_element();
        };
        let rows: Vec<AnyElement> = p
            .videos
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let waits = self.playing && !v.ended;
                let head = div()
                    .px_3()
                    .pt_2()
                    .text_size(text::XS.size)
                    .text_color(k.text(TextStep::T400))
                    .child(format!(
                        "Video {}{}",
                        i + 1,
                        if waits {
                            " · Slideshow waits for playback"
                        } else {
                            ""
                        }
                    ));
                let body = match &v.error {
                    Some(e) => div()
                        .id(SharedString::from(format!("documents-video-error-{i}")))
                        .role(Role::Alert)
                        .px_3()
                        .py_2()
                        .text_size(text::XS.size)
                        .text_color(k.warning_text)
                        .child(e.clone())
                        .into_any_element(),
                    None => {
                        let (t, r, b, f) = (cx.entity(), cx.entity(), cx.entity(), cx.entity());
                        let shown = self
                            .scrubbing
                            .filter(|(s, _)| *s == i)
                            .map_or(v.current_time, |(_, s)| s);
                        let total = if v.duration > 0. {
                            format_media_clock(v.duration)
                        } else {
                            "–:––".into()
                        };
                        let playing = !v.paused && !v.ended;
                        let now = v.current_time;
                        h_flex()
                            .px_3()
                            .py_1()
                            .gap_1()
                            .child(
                                Button::new(SharedString::from(ids::video_toggle(i)))
                                    .xsmall()
                                    .ghost()
                                    .icon(Icon::new(if playing {
                                        KairoIcon::Pause
                                    } else {
                                        KairoIcon::Play
                                    }))
                                    .accessibility_label(if playing { "Pause" } else { "Play" })
                                    .on_click(move |_, _, cx| {
                                        let command = if playing {
                                            DocumentVideoCommand::Pause
                                        } else {
                                            DocumentVideoCommand::Play
                                        };
                                        t.update(cx, |p, cx| p.control(i, command, cx))
                                    }),
                            )
                            .child(
                                Button::new(SharedString::from(ids::video_restart(i)))
                                    .xsmall()
                                    .ghost()
                                    .icon(Icon::new(KairoIcon::RotateCcw))
                                    .accessibility_label("Restart")
                                    .on_click(move |_, _, cx| {
                                        r.update(cx, |p, cx| {
                                            p.control(i, DocumentVideoCommand::Restart, cx)
                                        })
                                    }),
                            )
                            .child(
                                Button::new(SharedString::from(ids::video_back(i)))
                                    .xsmall()
                                    .ghost()
                                    .icon(Icon::new(KairoIcon::SkipBack))
                                    .accessibility_label("Back 5 seconds")
                                    .on_click(move |_, _, cx| {
                                        let seconds = (now - SKIP).max(0.);
                                        b.update(cx, |p, cx| {
                                            p.control(i, DocumentVideoCommand::Seek { seconds }, cx)
                                        })
                                    }),
                            )
                            .child(
                                Button::new(SharedString::from(ids::video_forward(i)))
                                    .xsmall()
                                    .ghost()
                                    .icon(Icon::new(KairoIcon::SkipForward))
                                    .accessibility_label("Forward 5 seconds")
                                    .on_click(move |_, _, cx| {
                                        f.update(cx, |p, cx| {
                                            p.control(
                                                i,
                                                DocumentVideoCommand::Seek {
                                                    seconds: now + SKIP,
                                                },
                                                cx,
                                            )
                                        })
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(text::LABEL.size)
                                    .text_color(k.text(TextStep::T400))
                                    .child(format_media_clock(shown)),
                            )
                            .children(self.seek_bars.get(i).map(|bar| {
                                div().flex_1().min_w_0().child(
                                    Slider::new(bar)
                                        .disabled(v.duration <= 0.)
                                        .accessibility_label("Seek"),
                                )
                            }))
                            .child(
                                div()
                                    .text_size(text::LABEL.size)
                                    .text_color(k.text(TextStep::T400))
                                    .child(total),
                            )
                            .into_any_element()
                    }
                };
                v_flex()
                    .id(SharedString::from(ids::video(i)))
                    .test_support()
                    .child(head)
                    .child(body)
                    .into_any_element()
            })
            .collect();
        v_flex().flex_none().children(rows).into_any_element()
    }

    /// The live slide, fitted, with its video boxes (`DocumentSlidePreview`).
    fn render_preview(
        &self,
        doc: &ProjectionDocument,
        local: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let path = doc.pages.get(self.page).cloned().unwrap_or_default();
        let ratio = model::aspect(doc.slide_size.as_ref());
        let boxes: Vec<AnyElement> = doc
            .videos
            .as_ref()
            .and_then(|v| v.get(self.page))
            .map(|videos| {
                videos
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        let frame = div()
                            .absolute()
                            .left(gpui_kit::relative(v.frame.x as f32))
                            .top(gpui_kit::relative(v.frame.y as f32))
                            .w(gpui_kit::relative(v.frame.width as f32))
                            .h(gpui_kit::relative(v.frame.height as f32));
                        match &self.clips {
                            // Hidden until played, as the outputs keep it.
                            Some(clips) => {
                                let shown = clips.visible.get(i).copied().unwrap_or(false);
                                let image = clips
                                    .players
                                    .get(i)
                                    .and_then(|p| p.read(cx).image().cloned());
                                frame
                                    .id(ids::clip(i))
                                    .test_support()
                                    .when_some(image.filter(|_| shown), |f, image| {
                                        f.child(img(image).size_full().object_fit(ObjectFit::Fill))
                                    })
                                    .into_any_element()
                            }
                            None => frame
                                .border_1()
                                .border_color(k.hue(Hue::Cyan))
                                .into_any_element(),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        v_flex()
            .id(ids::PREVIEW)
            .test_support()
            .flex_none()
            .h(px(220.))
            .children(local.then(|| {
                div()
                    .id(ids::PREVIEW_ONLY)
                    .test_support()
                    .px_3()
                    .pt_2()
                    .text_size(text::XS.size)
                    .text_color(k.text(TextStep::T400))
                    .child("Preview only · No output connected")
            }))
            .child(
                h_flex().flex_1().min_h_0().justify_center().p_2().child(
                    div()
                        .relative()
                        .h_full()
                        .w(px(180. * ratio))
                        .max_w_full()
                        .bg(k.surface_display)
                        .child(
                            img(std::path::PathBuf::from(path))
                                .size_full()
                                .object_fit(ObjectFit::Contain),
                        )
                        .children(boxes),
                ),
            )
            .into_any_element()
    }

    fn render_grid(&mut self, doc: &ProjectionDocument, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let width = f32::from(self.grid_bounds.get().size.width);
        let (columns, column) = model::grid_columns(width, self.thumb);
        let ratio = model::aspect(doc.slide_size.as_ref());
        let thumb_h = column / ratio;
        let rows = doc.pages.len().div_ceil(columns);
        let sizes = Rc::new(vec![
            size(
                px(width.max(1.)),
                px(thumb_h + model::CAPTION + model::GAP_Y)
            );
            rows
        ]);
        if std::mem::take(&mut self.scroll_to_page) && rows > 0 {
            self.grid
                .scroll_to_item(self.page / columns, ScrollStrategy::Center);
        }
        let measured = self.grid_bounds.clone();
        let pages = Rc::new(doc.pages.clone());
        let (doc_id, is_pdf) = (doc.id.clone(), doc.format == DocumentFormat::Pdf);
        let has_video: Rc<Vec<bool>> = Rc::new(
            (0..doc.pages.len())
                .map(|i| {
                    doc.videos
                        .as_ref()
                        .and_then(|v| v.get(i))
                        .is_some_and(|v| !v.is_empty())
                })
                .collect(),
        );
        let live = self
            .live
            .as_ref()
            .filter(|(id, _)| *id == doc_id)
            .map(|(_, p)| *p);
        let picked = self.page;
        let busy = self.busy;
        div()
            .id(ids::GRID)
            .test_support()
            .aria_label("Slides")
            .relative()
            .flex_1()
            .min_h_0()
            .child(
                canvas(
                    move |bounds, window, _| {
                        if measured.get() != bounds {
                            let resized =
                                (measured.get().size.width - bounds.size.width).abs() > px(0.5);
                            measured.set(bounds);
                            if resized {
                                window.refresh();
                            }
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(
                v_virtual_list(
                    cx.entity(),
                    "documents-rows",
                    sizes,
                    move |_, range, _, cx| {
                        range
                            .map(|r| {
                                let cells: Vec<AnyElement> = (r * columns
                                    ..((r + 1) * columns).min(pages.len()))
                                    .map(|i| {
                                        let page = cx.entity();
                                        let is_live = live == Some(i);
                                        let border = if is_live {
                                            k.live
                                        } else if picked == i {
                                            k.control_accent
                                        } else {
                                            gpui_kit::transparent_black()
                                        };
                                        v_flex()
                                            .id(SharedString::from(ids::slide(i)))
                                            .test_support()
                                            .role(Role::Button)
                                            .aria_label(format!(
                                                "Push {} {}",
                                                if is_pdf { "page" } else { "slide" },
                                                i + 1
                                            ))
                                            .w(px(column))
                                            .flex_none()
                                            .when(!busy, |d| {
                                                d.cursor_pointer().on_click(move |_, _, cx| {
                                                    page.update(cx, |p, cx| p.push(i, cx))
                                                })
                                            })
                                            .child(
                                                div()
                                                    .w_full()
                                                    .h(px(thumb_h))
                                                    .overflow_hidden()
                                                    .rounded(radius::lg())
                                                    .border_2()
                                                    .border_color(border)
                                                    .when(!is_live && picked != i, |d| {
                                                        d.hover(|s| s.border_color(k.neutral[6]))
                                                    })
                                                    .bg(k.surface_display)
                                                    .child(
                                                        img(std::path::PathBuf::from(&pages[i]))
                                                            .size_full()
                                                            .object_fit(ObjectFit::Contain),
                                                    ),
                                            )
                                            .child(
                                                h_flex()
                                                    .mt_1p5()
                                                    .px_0p5()
                                                    .gap_1p5()
                                                    .text_size(text::LABEL.size)
                                                    .text_color(if is_live {
                                                        k.live
                                                    } else {
                                                        k.text(TextStep::T500)
                                                    })
                                                    .child((i + 1).to_string())
                                                    .when(has_video[i], |d| {
                                                        d.child(Icon::new(KairoIcon::Film).size_3())
                                                    }),
                                            )
                                            .into_any_element()
                                    })
                                    .collect();
                                h_flex()
                                    .items_start()
                                    .gap(px(model::GAP_X))
                                    .children(cells)
                                    .into_any_element()
                            })
                            .collect::<Vec<_>>()
                    },
                )
                .track_scroll(&self.grid)
                .size_full()
                .p(px(model::GRID_PAD)),
            )
            .into_any_element()
    }

    fn render_import_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = self.import.as_ref()?;
        let k = *cx.kairo();
        let failed = p.failed();
        let percent = p.percent();
        let icon = match p.format {
            Some(DocumentFormat::Pdf) | None => KairoIcon::FileText,
            _ => KairoIcon::Film,
        };
        let (cancel, retry, close) = (cx.entity(), cx.entity(), cx.entity());
        let body = if failed {
            div()
                .id(ids::IMPORT_FAILED)
                .test_support()
                .role(Role::Alert)
                .mt_4()
                .text_size(text::SM.size)
                .text_color(k.text(TextStep::T400))
                .child(p.error.clone().unwrap_or_default())
                .into_any_element()
        } else {
            v_flex()
                .mt_5()
                .gap_2()
                .child(
                    div()
                        .id("documents-import-progress")
                        .role(Role::ProgressIndicator)
                        .aria_label("Import progress")
                        .h(px(6.))
                        .w_full()
                        .rounded_full()
                        .bg(k.surface_tertiary)
                        .overflow_hidden()
                        .child(match percent {
                            Some(pc) => div()
                                .h_full()
                                .w(gpui_kit::relative(pc as f32 / 100.))
                                .rounded_full()
                                .bg(k.control_accent),
                            None => div()
                                .h_full()
                                .w_full()
                                .rounded_full()
                                .bg(k.control_accent.opacity(0.6)),
                        }),
                )
                .child(
                    h_flex()
                        .justify_between()
                        .text_size(text::XS.size)
                        .text_color(k.text(TextStep::T400))
                        .child(
                            div()
                                .id(ids::IMPORT_LINE)
                                .test_support()
                                .child(p.line(self.canceling)),
                        )
                        .children(percent.map(|pc| div().child(format!("{pc}%")))),
                )
                .into_any_element()
        };
        let buttons = if failed {
            h_flex()
                .mt_5()
                .gap_2()
                .justify_end()
                .child(
                    Button::new(ids::IMPORT_CLOSE)
                        .small()
                        .label("Close")
                        .on_click(move |_, _, cx| close.update(cx, |p, cx| p.close_import(cx))),
                )
                .child(
                    Button::new(ids::IMPORT_RETRY)
                        .small()
                        .primary()
                        .label("Choose file again")
                        .on_click(move |_, _, cx| retry.update(cx, |p, cx| p.retry_import(cx))),
                )
        } else {
            h_flex().mt_5().justify_end().child(
                Button::new(ids::IMPORT_CANCEL)
                    .small()
                    .label(if self.canceling {
                        "Canceling…"
                    } else {
                        "Cancel"
                    })
                    .disabled(self.canceling || p.finishing())
                    .on_click(move |_, _, cx| cancel.update(cx, |p, cx| p.cancel_import(cx))),
            )
        };
        Some(modal(
            ids::IMPORT_DIALOG,
            p.title(),
            k,
            self.dialog_focus.handle(),
            v_flex()
                .items_center()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .size_10()
                        .rounded(radius::lg())
                        .bg(k.tints.teal)
                        .text_color(k.accent(kairo_ui_kit::tokens::Step::S300))
                        .child(Icon::new(icon)),
                )
                .child(
                    div()
                        .mt_3()
                        .text_size(text::BASE.size)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(k.text_primary)
                        .child(p.title()),
                )
                .child(
                    div()
                        .mt_1()
                        .max_w_full()
                        .truncate()
                        .text_size(text::XS.size)
                        .text_color(k.text(TextStep::T500))
                        .child(p.name_line().to_owned()),
                )
                .child(div().w_full().child(body))
                .child(div().w_full().child(buttons))
                .into_any_element(),
        ))
    }

    fn render_converter_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let prompt = self.converter_prompt.as_ref()?;
        let k = *cx.kairo();
        let status = self.converter.as_ref();
        let view = model::converter_view(status, prompt.started);
        let can_simplify = prompt.needs.format == DocumentFormat::Pptx;
        let busy = view == ConverterView::Busy;
        let (download, cancel, simplified) = (cx.entity(), cx.entity(), cx.entity());
        let line = |s: &'static str| {
            div()
                .text_size(text::XS.size)
                .text_color(k.text(TextStep::T300))
                .child(s)
        };
        let body: AnyElement = match view {
            ConverterView::Unsupported => {
                line("Install LibreOffice from your software center, then import this file again.")
                    .into_any_element()
            }
            ConverterView::Failed => div()
                .id("documents-converter-error")
                .role(Role::Alert)
                .text_size(text::XS.size)
                .text_color(k.text(TextStep::T300))
                .child(status.and_then(|s| s.error.clone()).unwrap_or_default())
                .into_any_element(),
            ConverterView::Busy => {
                let s = status.cloned().unwrap();
                let installing =
                    s.state == kairo_domain::documents::SlideConverterState::Installing;
                let pc = model::converter_percent(&s);
                v_flex()
                    .id(ids::CONVERTER_PROGRESS)
                    .test_support()
                    .gap_2()
                    .child(
                        div()
                            .id("documents-converter-bar")
                            .role(Role::ProgressIndicator)
                            .aria_label("Slide converter download")
                            .h(px(6.))
                            .w_full()
                            .rounded_full()
                            .overflow_hidden()
                            .bg(k.surface_tertiary)
                            .child(
                                div()
                                    .h_full()
                                    .rounded_full()
                                    .bg(k.control_accent)
                                    .when(installing, |d| d.w_full())
                                    .when(!installing, |d| {
                                        d.w(gpui_kit::relative(pc as f32 / 100.))
                                    }),
                            ),
                    )
                    .child(
                        h_flex()
                            .justify_between()
                            .text_size(text::XS.size)
                            .text_color(k.text(TextStep::T400))
                            .child(if installing {
                                "Setting up…".to_string()
                            } else {
                                format!(
                                    "{} of {}",
                                    model::format_mb(s.received_bytes),
                                    model::format_mb(s.size_bytes)
                                )
                            })
                            .when(!installing, |d| d.child(format!("{pc}%"))),
                    )
                    .child(line("Your slides import as soon as this finishes."))
                    .into_any_element()
            }
            ConverterView::Offer => v_flex()
                .gap_2()
                .child(line("Every picture, font and effect kept"))
                .child(
                    div()
                        .text_size(text::XS.size)
                        .text_color(k.text(TextStep::T300))
                        .child(format!(
                            "Free, one-time download · {}",
                            model::converter_size(status)
                        )),
                )
                .child(line("Stays in Kairo and works offline"))
                .into_any_element(),
        };
        Some(modal(
            ids::CONVERTER,
            model::converter_title(view),
            k,
            self.dialog_focus.handle(),
            v_flex()
                .gap_4()
                .child(
                    v_flex()
                        .child(
                            div()
                                .truncate()
                                .text_size(text::XS.size)
                                .text_color(k.text(TextStep::T500))
                                .child(prompt.needs.name.clone()),
                        )
                        .child(
                            div()
                                .mt_1()
                                .text_size(text::BASE.size)
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(k.text_primary)
                                .child(model::converter_title(view)),
                        ),
                )
                .child(body)
                .child(
                    v_flex()
                        .gap_2()
                        .when(!busy && view != ConverterView::Unsupported, |d| {
                            d.child(
                                Button::new(ids::CONVERTER_DOWNLOAD)
                                    .primary()
                                    .w_full()
                                    .icon(Icon::new(KairoIcon::Download))
                                    .label(if view == ConverterView::Failed {
                                        "Try again"
                                    } else {
                                        "Download and import"
                                    })
                                    .on_click(move |_, _, cx| {
                                        download.update(cx, |p, cx| p.download_converter(cx))
                                    }),
                            )
                        })
                        .child(
                            Button::new(ids::CONVERTER_CANCEL)
                                .w_full()
                                .label(if busy { "Stop download" } else { "Cancel" })
                                .on_click(move |_, _, cx| {
                                    cancel.update(cx, |p, cx| p.cancel_converter(cx))
                                }),
                        )
                        .when(can_simplify && !busy, |d| {
                            d.child(
                                Button::new(ids::CONVERTER_SIMPLIFIED)
                                    .ghost()
                                    .w_full()
                                    .label("Import a simplified version instead")
                                    .on_click(move |_, _, cx| {
                                        simplified.update(cx, |p, cx| p.import_simplified(cx))
                                    }),
                            )
                        }),
                )
                .into_any_element(),
        ))
    }
}

fn modal(
    id: &'static str,
    label: &'static str,
    k: kairo_ui_kit::KairoColors,
    focus: &FocusHandle,
    content: AnyElement,
) -> AnyElement {
    deferred(
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .p_4()
            .bg(k.scrim())
            .occlude()
            .child(
                v_flex()
                    .id(id)
                    .test_support()
                    .role(Role::Dialog)
                    .aria_label(label)
                    .w(px(400.))
                    .p_6()
                    .rounded(radius::xl2())
                    .bg(k.glass_modal())
                    .child(content)
                    .focus_trap(id, focus),
            ),
    )
    .with_priority(2)
    .into_any_element()
}
