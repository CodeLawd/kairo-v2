//! The editor's state and what its controls do (`ThemeEditor.tsx`'s logic).
//! Drawing is in `view.rs`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::{
    AnyView, AppContext, Bounds, Context, Entity, EventEmitter, FocusHandle, Focusable, Pixels,
    RenderImage, Subscription, Task, Window,
};
use kairo_core_gpui::preview::LivePreview;
use kairo_domain::js::now_ms;
use kairo_domain::overlay::{
    CustomOverlayTheme, OverlayBox, OverlayContentKind, OverlayElement, OverlayElementKind,
    OverlayTheme,
};
use kairo_domain::overlay_boxes::{ELEMENT_BOX_MIN, clamp_overlay_box, move_overlay_box};
use kairo_domain::overlay_defaults::DEFAULT_OVERLAY_THEME;
use kairo_domain::overlay_elements::{
    MAX_OVERLAY_ELEMENTS, create_overlay_element, duplicate_overlay_element,
    remove_overlay_element, update_overlay_element,
};
use kairo_domain::overlay_outputs::{OVERLAY_CONTENT_KINDS, content_kind_label};
use kairo_domain::settings::AppSettings;
use kairo_domain::theme_library::{
    create_custom_theme, next_untitled_theme_name, output_uses_theme, theme_baseline,
    themes_for_kind,
};
use kairo_ui_kit::components::{ConfirmRequest, confirm};

use crate::backend::{FeedSink, MediaKind, ThemeBackend, ThemeFeed, ThemePreview};
use crate::canvas::{
    self, Drag, DragEdit, DragMode, Hit, REFERENCE, VERSE, drag_step, hit_test, is_deletable,
    is_text_layer, layer_box, layer_minimum, layers, nudge,
};
use crate::gallery::ShapePick;
use crate::history::ThemeHistory;
use crate::model::{self, SAVE_DELAY_MS, SAVED_FLASH_MS};
use crate::ops::Op;

/// The message under a failed write (`saveOutputSettings`'s catch).
pub const SAVE_FAILED: &str = "Screen changes could not be saved. Please try again.";

/// What the editor tells the application and the inspector.
#[derive(Clone, Debug, PartialEq)]
pub enum ThemeEditorEvent {
    /// The draft changed (an edit, undo, redo, or another theme opened).
    DraftChanged,
    /// The selected layer changed.
    SelectionChanged,
}

/// Shape pictures by shape and colour; `None` where a shape draws nothing.
pub(crate) type ThumbCache = HashMap<(String, u32), Option<Arc<RenderImage>>>;

/// An edit waiting for its save delay.
#[derive(Clone, Debug)]
struct Pending {
    id: String,
    name: String,
    theme: OverlayTheme,
}

/// The canvas drag's token for GPUI's drag events; the state is the editor's.
#[derive(Clone)]
pub(crate) struct CanvasDrag;

impl gpui_kit::Render for CanvasDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl gpui_kit::IntoElement {
        gpui_kit::div()
    }
}

pub struct ThemeEditor {
    pub(crate) backend: Arc<dyn ThemeBackend>,
    pub(crate) settings: AppSettings,
    in_flight: usize,
    pub(crate) focus: FocusHandle,
    pub(crate) name_input: Entity<InputState>,
    pub(crate) kind: OverlayContentKind,
    pub(crate) selected_id: Option<String>,
    pub(crate) draft: OverlayTheme,
    pub(crate) name: String,
    pub(crate) selected_layer: Option<String>,
    pub(crate) history: ThemeHistory,
    pub(crate) long_sample: bool,
    pub(crate) saved: bool,
    pub(crate) notice: Option<String>,
    pending: Option<Pending>,
    save_task: Option<Task<()>>,
    saved_task: Option<Task<()>>,
    last_by_kind: [Option<String>; 2],
    pub(crate) drag: Option<Drag>,
    pub(crate) guides: (bool, bool),
    pub(crate) hover: Option<Hit>,
    pub(crate) frame_bounds: Rc<Cell<Bounds<Pixels>>>,
    pub(crate) stage_size: Rc<Cell<(f32, f32)>>,
    preview: Option<ThemePreview>,
    preview_failed: bool,
    pub(crate) preview_view: Option<Entity<LivePreview>>,
    preview_rev: u64,
    preview_sent: Option<u64>,
    pub(crate) shapes_open: bool,
    pub(crate) thumbs: RefCell<ThumbCache>,
    pub(crate) inspector: Option<AnyView>,
    _feed: Task<()>,
    _listening: Box<dyn Send>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ThemeEditorEvent> for ThemeEditor {}

impl Focusable for ThemeEditor {
    fn focus_handle(&self, _: &gpui_kit::App) -> FocusHandle {
        self.focus.clone()
    }
}

fn kind_index(kind: OverlayContentKind) -> usize {
    match kind {
        OverlayContentKind::Scripture => 0,
        OverlayContentKind::Lyrics => 1,
    }
}

impl ThemeEditor {
    pub fn new(
        backend: Arc<dyn ThemeBackend>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let settings = backend.snapshot();
        let (tx, mut rx) = futures_channel::mpsc::unbounded::<ThemeFeed>();
        let sink: FeedSink = Arc::new(move |feed| {
            let _ = tx.unbounded_send(feed);
        });
        let listening = backend.listen(sink);
        let feed = cx.spawn(async move |this, cx| {
            use futures_util::StreamExt;
            while let Some(feed) = rx.next().await {
                if this.update(cx, |this, cx| this.on_feed(feed, cx)).is_err() {
                    break;
                }
            }
        });
        let name_input = cx.new(|cx| InputState::new(window, cx));
        let subscriptions = vec![
            cx.subscribe_in(
                &name_input,
                window,
                |this, state, event: &InputEvent, _, cx| match event {
                    InputEvent::Change => {
                        let value = state.read(cx).value().to_string();
                        this.rename(value, cx);
                    }
                    InputEvent::Blur => this.flush(cx),
                    _ => {}
                },
            ),
            // Anything still waiting is saved when the editor goes.
            cx.on_release(|this, _| {
                if let Some(p) = this.pending.take() {
                    this.backend.edit(this.save_op(p), Box::new(|_| {}));
                }
            }),
        ];
        let mut editor = Self {
            backend,
            settings,
            in_flight: 0,
            focus: cx.focus_handle(),
            name_input,
            kind: OverlayContentKind::Scripture,
            selected_id: None,
            draft: DEFAULT_OVERLAY_THEME.clone(),
            name: String::new(),
            selected_layer: Some(VERSE.into()),
            history: ThemeHistory::new(),
            long_sample: false,
            saved: false,
            notice: None,
            pending: None,
            save_task: None,
            saved_task: None,
            last_by_kind: [None, None],
            drag: None,
            guides: (false, false),
            hover: None,
            frame_bounds: Rc::default(),
            stage_size: Rc::default(),
            preview: None,
            preview_failed: false,
            preview_view: None,
            preview_rev: 0,
            preview_sent: None,
            shapes_open: false,
            thumbs: RefCell::default(),
            inspector: None,
            _feed: feed,
            _listening: listening,
            _subscriptions: subscriptions,
        };
        let first = model::initial_selection(
            &editor.settings.theme_library,
            &editor.settings.overlay.outputs,
            None,
        );
        if let Some(entry) = first.and_then(|id| editor.entry(&id).cloned()) {
            editor.open(&entry, window, cx);
        }
        editor
    }

    // ─── What other panels read and drive ──────────────────────────────────

    pub fn set_inspector(&mut self, inspector: Option<AnyView>, cx: &mut Context<Self>) {
        self.inspector = inspector;
        cx.notify();
    }

    pub fn draft(&self) -> &OverlayTheme {
        &self.draft
    }

    pub fn kind(&self) -> OverlayContentKind {
        self.kind
    }

    pub fn selected_theme(&self) -> Option<&CustomOverlayTheme> {
        self.selected_id.as_deref().and_then(|id| self.entry(id))
    }

    pub fn selected_layer(&self) -> Option<&str> {
        self.selected_layer.as_deref()
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    pub fn is_default(&self) -> bool {
        self.selected_theme()
            .is_none_or(|entry| model::is_default(&self.draft, entry))
    }

    pub fn library(&self) -> &[CustomOverlayTheme] {
        &self.settings.theme_library
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    pub(crate) fn entry(&self, id: &str) -> Option<&CustomOverlayTheme> {
        self.settings.theme_library.iter().find(|t| t.id == id)
    }

    pub fn select_layer(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        if self.selected_layer == id {
            return;
        }
        self.selected_layer = id;
        cx.emit(ThemeEditorEvent::SelectionChanged);
        cx.notify();
    }

    // ─── Persistence ───────────────────────────────────────────────────────

    fn on_feed(&mut self, feed: ThemeFeed, cx: &mut Context<Self>) {
        let ThemeFeed::Settings(settings) = feed;
        // Our own writes are in flight: the feed may be older than the
        // screen, and the last reply reads the settled state.
        if self.in_flight == 0 {
            self.settings = *settings;
            cx.notify();
        }
    }

    fn save_op(&self, p: Pending) -> Op {
        let name = p.name.trim();
        Op::Save {
            id: p.id,
            name: if name.is_empty() {
                "Untitled theme".into()
            } else {
                name.into()
            },
            theme: p.theme,
            now: now_ms(),
        }
    }

    /// Applies `op` here at once and queues it with the core. Writes keep
    /// their order; a failure shows under the name and the settled state is
    /// read back.
    fn send(&mut self, op: Op, cx: &mut Context<Self>) {
        // A failed op changes nothing here; the core's reply says why.
        let _ = crate::ops::apply(&mut self.settings, &op);
        self.in_flight += 1;
        self.notice = None;
        let (tx, rx) = futures_channel::oneshot::channel();
        self.backend.edit(
            op,
            Box::new(move |result| {
                let _ = tx.send(result);
            }),
        );
        cx.spawn(async move |this, cx| {
            let result = rx
                .await
                .unwrap_or_else(|_| Err("The write was dropped.".into()));
            let _ = this.update(cx, |this, cx| this.settled(result, cx));
        })
        .detach();
        cx.notify();
    }

    fn settled(&mut self, result: Result<(), String>, cx: &mut Context<Self>) {
        self.in_flight = self.in_flight.saturating_sub(1);
        match result {
            Ok(()) => self.flash_saved(cx),
            Err(error) => {
                tracing_error(&error);
                self.notice = Some(SAVE_FAILED.into());
            }
        }
        if self.in_flight == 0 {
            self.settings = self.backend.snapshot();
        }
        cx.notify();
    }

    fn flash_saved(&mut self, cx: &mut Context<Self>) {
        self.saved = true;
        self.saved_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(SAVED_FLASH_MS))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.saved = false;
                cx.notify();
            });
        }));
    }

    /// Writes the pending edit to the library and to every screen using the theme.
    pub fn flush(&mut self, cx: &mut Context<Self>) {
        self.save_task = None;
        if let Some(p) = self.pending.take() {
            let op = self.save_op(p);
            self.send(op, cx);
        }
    }

    fn schedule_save(
        &mut self,
        id: String,
        name: String,
        theme: OverlayTheme,
        cx: &mut Context<Self>,
    ) {
        self.pending = Some(Pending { id, name, theme });
        self.save_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(SAVE_DELAY_MS))
                .await;
            let _ = this.update(cx, |this, cx| this.flush(cx));
        }));
    }

    // ─── Editing ───────────────────────────────────────────────────────────

    /// Shows `entry` for editing, with a clean history.
    pub fn open(
        &mut self,
        entry: &CustomOverlayTheme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.last_by_kind[kind_index(entry.kind)] = Some(entry.id.clone());
        self.kind = entry.kind;
        self.selected_id = Some(entry.id.clone());
        self.name = entry.name.clone();
        self.draft = model::normalized(&entry.theme);
        self.history.clear();
        self.drag = None;
        self.guides = (false, false);
        let name = entry.name.clone();
        self.name_input
            .update(cx, |input, cx| input.set_value(name, window, cx));
        self.preview_rev += 1;
        cx.emit(ThemeEditorEvent::DraftChanged);
        cx.notify();
    }

    pub fn select_theme(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.flush(cx);
        if let Some(entry) = self.entry(id).cloned() {
            self.open(&entry, window, cx);
        }
    }

    /// Shows `theme` and saves it, without touching undo history.
    fn show_theme(&mut self, theme: OverlayTheme, cx: &mut Context<Self>) {
        let Some(id) = self.selected_id.clone() else {
            return;
        };
        self.draft = theme.clone();
        self.preview_rev += 1;
        self.schedule_save(id, self.name.clone(), theme, cx);
        cx.emit(ThemeEditorEvent::DraftChanged);
        cx.notify();
    }

    /// Applies an edit: recorded for undo, shown, and saved after a pause.
    /// Returns false when nothing changed.
    pub fn edit_theme(
        &mut self,
        edit: impl FnOnce(&OverlayTheme) -> OverlayTheme,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.selected_id.is_none() {
            return false;
        }
        let theme = edit(&self.draft);
        if theme == self.draft {
            return false;
        }
        let before = self.draft.clone();
        self.history.record(&before, now_ms() as u64);
        self.show_theme(theme, cx);
        true
    }

    pub fn undo(&mut self, cx: &mut Context<Self>) {
        if let Some(theme) = self.history.undo(&self.draft) {
            self.show_theme(theme, cx);
        }
    }

    pub fn redo(&mut self, cx: &mut Context<Self>) {
        if let Some(theme) = self.history.redo(&self.draft) {
            self.show_theme(theme, cx);
        }
    }

    /// Back to the theme's default. An edit like any other, so Undo brings
    /// the changes back.
    pub fn reset_to_default(&mut self, cx: &mut Context<Self>) {
        if let Some(entry) = self.selected_theme() {
            let theme = model::normalized(theme_baseline(entry));
            self.edit_theme(|_| theme, cx);
        }
    }

    /// "Save theme": what Reset returns to.
    pub fn set_as_default(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_id.clone() else {
            return;
        };
        self.flush(cx);
        let theme = self.draft.clone();
        self.send(Op::SetBaseline { id, theme }, cx);
    }

    pub fn rename(&mut self, name: String, cx: &mut Context<Self>) {
        if name == self.name {
            return;
        }
        self.name = name;
        if let Some(id) = self.selected_id.clone() {
            self.schedule_save(id, self.name.clone(), self.draft.clone(), cx);
        }
        cx.notify();
    }

    pub fn update_element(
        &mut self,
        id: &str,
        patch: impl Fn(&mut OverlayElement),
        cx: &mut Context<Self>,
    ) -> bool {
        self.edit_theme(
            |t| {
                let mut next = t.clone();
                next.elements = update_overlay_element(&t.elements, id, patch);
                next
            },
            cx,
        )
    }

    /// A layer's box, clamped: text keeps room for a line, elements can be thin.
    pub fn update_box(&mut self, id: &str, frame: OverlayBox, cx: &mut Context<Self>) {
        if is_text_layer(id) {
            let frame = clamp_overlay_box(&frame, kairo_domain::overlay_boxes::TEXT_BOX_MIN);
            self.edit_theme(
                |t| {
                    let mut next = t.clone();
                    if id == VERSE {
                        next.verse.frame = frame;
                    } else {
                        next.reference.text.frame = frame;
                    }
                    next
                },
                cx,
            );
        } else {
            let frame = clamp_overlay_box(&frame, ELEMENT_BOX_MIN);
            self.update_element(id, |el| el.frame = frame.clone(), cx);
        }
    }

    /// The canvas's Delete: hides the reference, deletes an element. The
    /// verse cannot go, so Delete on it does nothing.
    pub fn delete_layer(&mut self, id: &str, cx: &mut Context<Self>) {
        if !is_deletable(id) {
            return;
        }
        if id == REFERENCE {
            self.edit_theme(
                |t| {
                    let mut next = t.clone();
                    next.reference.show = false;
                    next
                },
                cx,
            );
            self.select_layer(Some(VERSE.into()), cx);
            return;
        }
        self.edit_theme(
            |t| {
                let mut next = t.clone();
                next.elements = remove_overlay_element(&t.elements, id);
                next
            },
            cx,
        );
        self.select_layer(None, cx);
    }

    pub fn show_reference(&mut self, cx: &mut Context<Self>) {
        self.edit_theme(
            |t| {
                let mut next = t.clone();
                next.reference.show = true;
                next
            },
            cx,
        );
        self.select_layer(Some(REFERENCE.into()), cx);
    }

    /// Adds a shape, or asks for a picture or video first.
    pub fn add_element(&mut self, pick: ShapePick, cx: &mut Context<Self>) {
        if self.draft.elements.len() >= MAX_OVERLAY_ELEMENTS || self.selected_id.is_none() {
            return;
        }
        let id = model::new_id("el", now_ms());
        let mut element = create_overlay_element(pick.kind, None, &id, pick.shape.as_deref());
        if pick.radius_px > 0.0 {
            element.radius_px = pick.radius_px;
        }
        self.insert_element(element, cx);
    }

    /// Picks a picture or video, measures it, and adds it at its own shape.
    pub fn add_media(&mut self, kind: MediaKind, cx: &mut Context<Self>) {
        if self.draft.elements.len() >= MAX_OVERLAY_ELEMENTS || self.selected_id.is_none() {
            return;
        }
        let backend = self.backend.clone();
        let (tx, rx) = futures_channel::oneshot::channel();
        backend.pick_media(
            kind,
            Box::new(move |r| {
                let _ = tx.send(r);
            }),
        );
        cx.spawn(async move |this, cx| {
            let picked = rx
                .await
                .unwrap_or_else(|_| Err("The picker closed.".into()));
            let path = match picked {
                Ok(Some(path)) => path,
                Ok(None) => return,
                Err(error) => {
                    tracing_error(&error);
                    let _ = this.update(cx, |this, cx| {
                        this.notice = Some("Could not open the file picker. Try again.".into());
                        cx.notify();
                    });
                    return;
                }
            };
            let (tx, rx) = futures_channel::oneshot::channel();
            let measured = this.update(cx, |this, _| {
                this.backend.media_aspect(
                    path.clone(),
                    kind,
                    Box::new(move |r| {
                        let _ = tx.send(r);
                    }),
                );
            });
            if measured.is_err() {
                return;
            }
            let aspect = rx.await.ok().and_then(Result::ok).flatten();
            let _ = this.update(cx, |this, cx| {
                let element_kind = match kind {
                    MediaKind::Image => OverlayElementKind::Image,
                    MediaKind::Video => OverlayElementKind::Video,
                };
                let id = model::new_id("el", now_ms());
                let mut element = create_overlay_element(element_kind, Some(&path), &id, None);
                element.frame = model::media_box(aspect, element_kind);
                this.insert_element(element, cx);
            });
        })
        .detach();
    }

    fn insert_element(&mut self, element: OverlayElement, cx: &mut Context<Self>) {
        // The theme may have filled up while a picker was open.
        if self.draft.elements.len() >= MAX_OVERLAY_ELEMENTS {
            return;
        }
        let id = element.id.clone();
        if self.edit_theme(
            |t| {
                let mut next = t.clone();
                next.elements.push(element);
                next
            },
            cx,
        ) {
            self.select_layer(Some(id), cx);
        }
    }

    pub fn duplicate_element(&mut self, id: &str, cx: &mut Context<Self>) {
        let new_id = model::new_id("el", now_ms());
        let (elements, copy) = duplicate_overlay_element(&self.draft.elements, id, &new_id);
        let Some(copy) = copy else {
            return;
        };
        self.edit_theme(
            |t| {
                let mut next = t.clone();
                next.elements = elements;
                next
            },
            cx,
        );
        self.select_layer(Some(copy), cx);
    }

    /// Asks for a file, then hands its path to `then`. A closed picker does
    /// nothing; a failed one shows a notice.
    fn pick_then(
        &mut self,
        kind: MediaKind,
        then: impl FnOnce(&mut Self, String, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let (tx, rx) = futures_channel::oneshot::channel();
        self.backend.pick_media(
            kind,
            Box::new(move |r| {
                let _ = tx.send(r);
            }),
        );
        cx.spawn(async move |this, cx| {
            let picked = rx
                .await
                .unwrap_or_else(|_| Err("The picker closed.".into()));
            let _ = this.update(cx, |this, cx| match picked {
                Ok(Some(path)) => then(this, path, cx),
                Ok(None) => {}
                Err(error) => {
                    tracing_error(&error);
                    this.notice = Some("Could not open the file picker. Try again.".into());
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The element inspector's picture or video: another file for it.
    pub fn replace_element_media(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(kind) = self
            .draft
            .elements
            .iter()
            .find(|e| e.id == id)
            .and_then(|e| match e.kind {
                OverlayElementKind::Image => Some(MediaKind::Image),
                OverlayElementKind::Video => Some(MediaKind::Video),
                _ => None,
            })
        else {
            return;
        };
        let id = id.to_owned();
        self.pick_then(
            kind,
            move |this, path, cx| {
                this.update_element(&id, |el| el.media_path = Some(path.clone()), cx);
            },
            cx,
        );
    }

    /// The Background tab's picture or video.
    pub fn pick_background_media(&mut self, kind: MediaKind, cx: &mut Context<Self>) {
        if self.selected_id.is_none() {
            return;
        }
        self.pick_then(
            kind,
            move |this, path, cx| {
                this.edit_theme(
                    |t| {
                        let mut next = t.clone();
                        next.background.media_path = Some(path);
                        next
                    },
                    cx,
                );
            },
            cx,
        );
    }

    // ─── The library ───────────────────────────────────────────────────────

    pub fn create(
        &mut self,
        base: &OverlayTheme,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.flush(cx);
        let item = create_custom_theme(
            name,
            &model::normalized(base),
            self.kind,
            now_ms(),
            &model::new_id("theme", now_ms()),
        );
        self.send(Op::Add(item.clone()), cx);
        self.open(&item, window, cx);
    }

    pub fn create_blank(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = next_untitled_theme_name(&self.settings.theme_library, self.kind);
        let base = DEFAULT_OVERLAY_THEME.clone();
        self.create(&base, &name, window, cx);
    }

    pub fn duplicate(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.flush(cx);
        let Some(source) = self.entry(id).cloned() else {
            return;
        };
        let copy = create_custom_theme(
            &format!("{} copy", source.name),
            &source.theme,
            source.kind,
            now_ms(),
            &model::new_id("theme", now_ms()),
        );
        self.send(Op::Add(copy.clone()), cx);
        self.open(&copy, window, cx);
    }

    /// Asks, then deletes. Screens using the theme keep their current look.
    pub fn remove(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.entry(id).cloned() else {
            return;
        };
        let users: Vec<String> = self
            .settings
            .overlay
            .outputs
            .iter()
            .filter(|o| output_uses_theme(o, &item.id, item.kind))
            .map(|o| o.name.clone())
            .collect();
        let mut request =
            ConfirmRequest::new(format!("Delete “{}”?", item.name), "Delete").destructive();
        if !users.is_empty() {
            request = request.detail(format!("{} will keep its current look.", users.join(", ")));
        }
        let editor = cx.entity();
        confirm(request, window, cx, move |yes, window, cx| {
            if yes {
                editor.update(cx, |this, cx| this.delete_confirmed(&item.id, window, cx));
            }
        });
    }

    pub fn delete_confirmed(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.flush(cx);
        let Some(item) = self.entry(id).cloned() else {
            return;
        };
        self.send(Op::Remove { id: id.to_owned() }, cx);
        if self.selected_id.as_deref() == Some(id) {
            let fallback = themes_for_kind(&self.settings.theme_library, item.kind)
                .into_iter()
                .next()
                .cloned();
            match fallback {
                Some(next) => self.open(&next, window, cx),
                None => {
                    self.selected_id = None;
                    self.pending = None;
                    cx.notify();
                }
            }
        }
    }

    pub fn switch_kind(
        &mut self,
        kind: OverlayContentKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if kind == self.kind {
            return;
        }
        self.flush(cx);
        let items = themes_for_kind(&self.settings.theme_library, kind);
        let remembered = self.last_by_kind[kind_index(kind)].clone();
        let target = items
            .iter()
            .find(|t| Some(&t.id) == remembered.as_ref())
            .or(items.first())
            .map(|t| (*t).clone());
        self.kind = kind;
        match target {
            Some(entry) => self.open(&entry, window, cx),
            None => {
                self.selected_id = None;
                cx.notify();
            }
        }
    }

    pub fn toggle_output(&mut self, output_id: &str, cx: &mut Context<Self>) {
        self.flush(cx);
        if let Some(id) = self.selected_id.clone() {
            self.send(
                Op::ToggleOutput {
                    theme_id: id,
                    output_id: output_id.to_owned(),
                },
                cx,
            );
        }
    }

    /// This theme becomes the default for its kind: every screen switches to
    /// it now, and screens added later start with it.
    pub fn make_default(&mut self, id: &str, cx: &mut Context<Self>) {
        self.flush(cx);
        if self.entry(id).is_some() {
            self.send(Op::MakeDefault { id: id.to_owned() }, cx);
        }
    }

    pub fn set_long_sample(&mut self, long: bool, cx: &mut Context<Self>) {
        if self.long_sample != long {
            self.long_sample = long;
            self.preview_rev += 1;
            cx.notify();
        }
    }

    pub fn kinds() -> Vec<(OverlayContentKind, &'static str)> {
        OVERLAY_CONTENT_KINDS
            .iter()
            .map(|k| (*k, content_kind_label(*k)))
            .collect()
    }

    // ─── Pointer and keys on the canvas ────────────────────────────────────

    fn frame_size(&self) -> (f64, f64) {
        let b = self.frame_bounds.get();
        (
            f64::from(f32::from(b.size.width)),
            f64::from(f32::from(b.size.height)),
        )
    }

    pub(crate) fn canvas_layers(&self) -> Vec<canvas::CanvasLayer> {
        layers(&self.draft, self.kind)
    }

    /// A press at `pointer` (pixels from the frame's top left). Returns
    /// whether it started a drag.
    pub fn pointer_down(&mut self, pointer: (f64, f64), cx: &mut Context<Self>) -> bool {
        let frame = self.frame_size();
        let layers = self.canvas_layers();
        let hit = hit_test(&layers, self.selected_layer.as_deref(), pointer, frame);
        match hit {
            Some(Hit::Delete(id)) => {
                self.delete_layer(&id, cx);
                false
            }
            Some(hit) => {
                let mode = match &hit {
                    Hit::Resize(_, handle) => DragMode::Resize(*handle),
                    Hit::Rotate(_) => DragMode::Rotate,
                    _ => DragMode::Move,
                };
                let id = hit.id().to_owned();
                self.drag = Drag::begin(&self.draft, &id, mode, pointer);
                self.select_layer(Some(id), cx);
                self.drag.is_some()
            }
            None => {
                self.drag = None;
                self.select_layer(None, cx);
                false
            }
        }
    }

    /// The pointer moved. While a drag runs this edits the layer.
    pub fn pointer_move(&mut self, pointer: (f64, f64), shift: bool, cx: &mut Context<Self>) {
        let frame = self.frame_size();
        let Some(drag) = self.drag.clone() else {
            let hit = hit_test(
                &self.canvas_layers(),
                self.selected_layer.as_deref(),
                pointer,
                frame,
            );
            if hit != self.hover {
                self.hover = hit;
                cx.notify();
            }
            return;
        };
        let Some(step) = drag_step(&drag, pointer, frame, shift) else {
            return;
        };
        if step.guides != self.guides {
            self.guides = step.guides;
            cx.notify();
        }
        match step.edit {
            DragEdit::Box(frame_box) => self.update_box(&drag.id, frame_box, cx),
            DragEdit::Rotate(deg) => {
                self.update_element(&drag.id, |el| el.rotation_deg = deg, cx);
            }
        }
    }

    pub fn pointer_up(&mut self, cx: &mut Context<Self>) {
        if self.drag.take().is_some() || self.guides != (false, false) {
            self.guides = (false, false);
            cx.notify();
        }
    }

    pub fn pointer_left(&mut self, cx: &mut Context<Self>) {
        if self.hover.take().is_some() {
            cx.notify();
        }
    }

    /// Canvas keys, the way any design tool behaves: Delete hides the
    /// reference, arrows nudge, Esc deselects. Returns whether it was used.
    pub fn canvas_key(&mut self, key: &str, shift: bool, cx: &mut Context<Self>) -> bool {
        let Some(selected) = self.selected_layer.clone() else {
            return false;
        };
        if key == "escape" {
            self.select_layer(None, cx);
            return true;
        }
        if (key == "delete" || key == "backspace") && is_deletable(&selected) {
            self.delete_layer(&selected, cx);
            return true;
        }
        let Some((dx, dy)) = nudge(key, shift) else {
            return false;
        };
        let Some(frame) = layer_box(&self.draft, &selected) else {
            return false;
        };
        let moved = move_overlay_box(&frame, dx, dy, layer_minimum(&selected));
        self.update_box(&selected, moved, cx);
        true
    }

    /// ⌘Z, ⇧⌘Z and ⌘Y. Returns whether it was used.
    pub fn history_key(&mut self, key: &str, shift: bool, cx: &mut Context<Self>) -> bool {
        match key {
            "y" => self.redo(cx),
            "z" if shift => self.redo(cx),
            "z" => self.undo(cx),
            _ => return false,
        }
        true
    }

    // ─── The preview ───────────────────────────────────────────────────────

    /// Opens the private surface the first time the slide is drawn, and
    /// sends the draft to it once per change.
    pub(crate) fn sync_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_id.is_none() {
            return;
        }
        if self.preview.is_none() && !self.preview_failed {
            match self.backend.open_preview() {
                Some(session) => {
                    let hub = session.hub();
                    let output = session.output().to_owned();
                    self.preview_view = Some(cx.new(|cx| {
                        let mut view = LivePreview::new(hub, cx);
                        view.set_output(Some(output), window, cx);
                        view.set_backdrop(true, cx);
                        view
                    }));
                    self.preview = Some(session);
                }
                None => self.preview_failed = true,
            }
        }
        if self.preview_sent == Some(self.preview_rev) {
            return;
        }
        if let Some(session) = &self.preview {
            session.show(
                self.draft.clone(),
                model::sample(self.kind, self.long_sample),
            );
            self.preview_sent = Some(self.preview_rev);
        }
    }

    /// The frame's size in pixels as last drawn, for tests.
    pub fn frame_size_for_tests(&self) -> (f64, f64) {
        self.frame_size()
    }

    pub fn preview_failed(&self) -> bool {
        self.preview_failed
    }
}

fn tracing_error(error: &str) {
    tracing::error!(%error, "[Theme] write failed");
}
