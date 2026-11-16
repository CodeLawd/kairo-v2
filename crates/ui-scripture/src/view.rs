//! The Scripture page (`Scripture.tsx` and its parts): the playlist sidebar
//! (`PlaylistSidebar.tsx`), the Bible bar (`ScriptureSearchBar.tsx`,
//! `ScriptureOptions.tsx`), notices, the verse grid (`VerseCardGrid.tsx`,
//! virtualised by line), the selection bar and the queue dock
//! (`QueueDock.tsx`). The live rail is the operator's, handed in by the app.

use kairo_ui_kit::ControlSize;
use kairo_ui_kit::components::PopoverButtonTriggerExt;
use kairo_ui_kit::components::PopoverTriggerExt;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{
    Disableable, Icon, Sizable, VirtualListScrollHandle, h_flex, v_flex, v_virtual_list,
};
use gpui_kit::{
    AnyElement, AnyView, App, AppContext, Context, Entity, FocusHandle, Focusable, FontWeight,
    InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Render, Role, ScrollStrategy,
    SharedString, StatefulInteractiveElement, Styled, Subscription, Window, canvas, deferred, div,
    prelude::FluentBuilder, px, size,
};
use kairo_domain::collections::{DEFAULT_LIBRARY_ID, LibrariesCommand, LibraryKind as DomainKind};
use kairo_domain::media_order::DropPosition;
use kairo_domain::overlay_content::{format_overlay_reference, format_overlay_verse_text};
use kairo_domain::scripture::query::{Direction, is_likely_phrase_query};
use kairo_ui_kit::coach::{anchors, coach_anchor};
use kairo_ui_kit::components::{
    ConfirmRequest, LibraryKind, LibraryRow, LibrarySection, LibrarySectionEvent, SelectionAction,
    SelectionBar, ZoomChanged, ZoomControl, ZoomRange, confirm,
};
use kairo_ui_kit::tokens::{Step, TextStep, radius, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};

use crate::backend::{OptionChange, ScriptureBackend};
use crate::card::VerseCard;
use crate::model::{PickGesture, Scripture, ScriptureEvent, ScrollTarget, Source};
use crate::review::{ReviewEvent, SermonReview};
use crate::rows::{
    self, BarItem, CARD_ZOOM_DEFAULT, CARD_ZOOM_MAX, CARD_ZOOM_MIN, GridKey, GridLine, Scope,
    SendStatus,
};
use kairo_domain::scripture::SermonPlanDraft;

/// Element ids, for tests and the coach tour.
pub mod ids {
    pub const PAGE: &str = "scripture-page";
    pub const QUERY: &str = "scripture-query";
    pub const RECENTS: &str = "scripture-recents";
    pub const SUGGESTIONS: &str = "scripture-search-suggestions";
    pub const TRANSLATION: &str = "scripture-translation";
    pub const OPTIONS: &str = "scripture-options";
    pub const USE_FOR_SERVICE: &str = "scripture-use-for-service";
    pub const ERROR: &str = "scripture-error";
    pub const NOTICE: &str = "scripture-translation-notice";
    pub const EMPTY: &str = "scripture-empty";
    pub const GRID: &str = "scripture-grid";
    pub const DOCK: &str = "scripture-queue";
    pub const PREV: &str = "scripture-prev";
    pub const NEXT: &str = "scripture-next";
    pub const GO_LIVE: &str = "scripture-go-live";
    pub const SAVE: &str = "scripture-save";
    pub const CLEAR: &str = "scripture-clear";
    pub const SCOPE_VERSE: &str = "scripture-scope-verse";
    pub const SCOPE_CHAPTER: &str = "scripture-scope-chapter";
    pub const UPLOAD_NOTES: &str = "scripture-upload-notes";
    pub const IMPORT: &str = "scripture-import";
    pub const NEW_PLAYLIST: &str = "scripture-new-playlist";
    pub const SEARCH_RESULTS: &str = "scripture-search-results";
    pub const RENAME: &str = "scripture-rename";
    pub const RENAME_SAVE: &str = "scripture-rename-save";
    pub const RENAME_CANCEL: &str = "scripture-rename-cancel";
    pub const ITEM_FILTER: &str = "scripture-item-filter";
    pub const PICKED_ADD: &str = "scripture-picked-add";
    pub const PICKED_SAVE: &str = "scripture-picked-save";

    pub fn card(index: usize) -> String {
        format!("scripture-card-{index}")
    }
    pub fn suggestion(index: usize) -> String {
        format!("scripture-suggestion-{index}")
    }
    pub fn suggestion_chapter(index: usize) -> String {
        format!("scripture-suggestion-{index}-chapter")
    }
    pub fn plan(id: &str) -> String {
        format!("scripture-plan-{id}")
    }
    pub fn plan_menu(id: &str) -> String {
        format!("scripture-plan-menu-{id}")
    }
    pub fn delete_confirm(id: &str) -> String {
        format!("scripture-plan-delete-{id}")
    }
    pub fn delete_cancel(id: &str) -> String {
        format!("scripture-plan-delete-cancel-{id}")
    }
    pub fn item(id: &str) -> String {
        format!("scripture-item-{id}")
    }
    pub fn passage(id: &str) -> String {
        format!("scripture-passage-{id}")
    }
    pub fn passage_remove(id: &str) -> String {
        format!("scripture-passage-remove-{id}")
    }
    pub fn header(row: usize) -> String {
        format!("scripture-row-{row}")
    }
}

const GRID_PADDING: f32 = 24.0;

/// The search list's own state (`ScriptureSearchBar`).
#[derive(Default)]
struct Bar {
    focused: bool,
    dismissed: bool,
    recents_open: bool,
    active: Option<usize>,
    suggestions_seen: usize,
}

/// A playlist item being dragged in the sidebar.
#[derive(Clone)]
struct ItemDrag {
    id: String,
    label: SharedString,
}

impl Render for ItemDrag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        kairo_ui_kit::components::DragPill::new(self.label.clone(), "✝").render_pill(cx)
    }
}

pub struct ScriptureWorkspace {
    model: Entity<Scripture>,
    rail: Option<AnyView>,
    /// The media dock under the page (`BoothWorkspace`'s `MediaDock`).
    media_dock: Option<AnyView>,
    query: Entity<InputState>,
    rename: Entity<InputState>,
    renaming_for: Option<String>,
    filter: Entity<InputState>,
    zoom: Entity<ZoomControl>,
    library: Entity<LibrarySection>,
    library_rows: Vec<LibraryRow>,
    scroll: VirtualListScrollHandle,
    grid_width: Rc<Cell<f32>>,
    lines: Rc<Vec<GridLine>>,
    card_size: (f32, f32),
    page_focus: FocusHandle,
    items_focus: FocusHandle,
    passages_focus: FocusHandle,
    bar: Bar,
    drag_over: Option<(String, DropPosition)>,
    backend: Arc<dyn ScriptureBackend>,
    review: Option<(Entity<SermonReview>, Subscription)>,
    _subscriptions: Vec<Subscription>,
}

impl ScriptureWorkspace {
    pub fn new(
        backend: Arc<dyn ScriptureBackend>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let model = cx.new(|cx| Scripture::new(backend.clone(), cx));
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Scripture Lookup"));
        let rename = cx.new(|cx| InputState::new(window, cx));
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter"));
        let zoom = cx.new(|cx| {
            ZoomControl::new(
                "scripture-zoom",
                "Verse card size",
                ZoomRange {
                    min: CARD_ZOOM_MIN,
                    max: CARD_ZOOM_MAX,
                    default: CARD_ZOOM_DEFAULT,
                    step: 5.0,
                    button_step: 10.0,
                },
                CARD_ZOOM_DEFAULT,
                window,
                cx,
            )
        });
        let library = cx.new(|_| {
            LibrarySection::new(LibraryKind::Scripture)
                .banded(true)
                .context_selection(true)
        });
        let mut subs = Vec::new();
        subs.push(cx.observe(&model, |_, _, cx| cx.notify()));
        subs.push(cx.subscribe_in(
            &model,
            window,
            |this, _, event: &ScriptureEvent, window, cx| match event {
                ScriptureEvent::ReviewSermonNotes(draft) => {
                    this.open_review(draft.clone(), window, cx)
                }
                ScriptureEvent::ImportFinished(true) => {
                    this.review = None;
                    cx.notify();
                }
                ScriptureEvent::ImportFinished(false) => {
                    if let Some((review, _)) = &this.review {
                        review.update(cx, |r, cx| r.set_confirming(false, cx));
                    }
                }
                _ => {}
            },
        ));
        subs.push(
            cx.subscribe_in(&query, window, |this, _, event: &InputEvent, window, cx| {
                let value = this.query.read(cx).value().to_string();
                match event {
                    InputEvent::Change => {
                        this.bar.dismissed = false;
                        this.bar.recents_open = false;
                        this.bar.active = None;
                        this.model.update(cx, |m, cx| m.set_query(value, cx));
                    }
                    InputEvent::PressEnter { .. } => {
                        this.model.update(cx, |m, cx| m.submit(cx));
                    }
                    InputEvent::Focus => {
                        this.bar.focused = true;
                        this.model.update(cx, |m, cx| m.query_focused(cx));
                    }
                    InputEvent::Blur => {
                        this.bar.focused = false;
                        this.bar.recents_open = false;
                    }
                }
                let _ = window;
                cx.notify();
            }),
        );
        subs.push(cx.subscribe(&rename, |this, _, event: &InputEvent, cx| {
            if let InputEvent::PressEnter { .. } = event {
                let draft = this.rename.read(cx).value().to_string();
                this.model.update(cx, |m, cx| m.save_rename(&draft, cx));
            }
        }));
        subs.push(cx.subscribe(&filter, |this, _, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                let value = this.filter.read(cx).value().to_string();
                this.model.update(cx, |m, cx| m.set_item_filter(value, cx));
            }
        }));
        subs.push(
            cx.subscribe(&zoom, |this, _, ZoomChanged(value): &ZoomChanged, cx| {
                let value = *value;
                this.model.update(cx, |m, cx| m.set_zoom(value, cx));
            }),
        );
        subs.push(
            cx.subscribe(&library, |this, _, event: &LibrarySectionEvent, cx| {
                this.library_event(event.clone(), cx)
            }),
        );
        // The search list's keys go before the field's own bindings.
        let weak = cx.entity().downgrade();
        let query_focus = query.read(cx).focus_handle(cx);
        let rename_focus = rename.read(cx).focus_handle(cx);
        subs.push(cx.intercept_keystrokes(move |event, window, cx| {
            let key = event.keystroke.key.clone();
            let shift = event.keystroke.modifiers.shift;
            if rename_focus.is_focused(window) && key == "escape" {
                let _ = weak.update(cx, |this, cx| {
                    this.model.update(cx, |m, cx| m.cancel_rename(cx))
                });
                cx.stop_propagation();
                return;
            }
            if !query_focus.is_focused(window) {
                return;
            }
            let handled = weak
                .update(cx, |this, cx| this.bar_key(&key, shift, cx))
                .unwrap_or(false);
            if handled {
                cx.stop_propagation();
            }
        }));
        Self {
            model,
            rail: None,
            media_dock: None,
            query,
            rename,
            renaming_for: None,
            filter,
            zoom,
            library,
            library_rows: Vec::new(),
            scroll: VirtualListScrollHandle::new(),
            grid_width: Rc::new(Cell::new(800.0)),
            lines: Rc::new(Vec::new()),
            card_size: (180.0, 101.0),
            page_focus: cx.focus_handle(),
            items_focus: cx.focus_handle(),
            passages_focus: cx.focus_handle(),
            bar: Bar::default(),
            drag_over: None,
            backend,
            review: None,
            _subscriptions: subs,
        }
    }

    /// The page's own focus: the grid keys answer while it has it.
    pub fn page_focus(&self) -> &FocusHandle {
        &self.page_focus
    }

    pub fn model(&self) -> &Entity<Scripture> {
        &self.model
    }

    /// The open sermon notes review, if any.
    pub fn review(&self) -> Option<&Entity<SermonReview>> {
        self.review.as_ref().map(|(r, _)| r)
    }

    fn open_review(&mut self, draft: SermonPlanDraft, window: &mut Window, cx: &mut Context<Self>) {
        let review = cx.new(|cx| SermonReview::new(self.backend.clone(), draft, window, cx));
        let sub = cx.subscribe(
            &review,
            |this, review, event: &ReviewEvent, cx| match event {
                ReviewEvent::Cancel => {
                    this.review = None;
                    cx.notify();
                }
                ReviewEvent::Confirm {
                    title,
                    text,
                    analysis,
                } => {
                    let draft = review.read(cx).draft().clone();
                    let (title, text, items) =
                        (title.clone(), text.clone(), analysis.items.clone());
                    this.model
                        .update(cx, |m, cx| m.confirm_import(&draft, title, text, items, cx));
                }
            },
        );
        self.review = Some((review, sub));
        cx.notify();
    }

    /// The live rail beside the page (`BoothWorkspace`'s rail).
    /// The media dock, shown under the page beside the rail.
    pub fn set_dock(&mut self, dock: Option<AnyView>, cx: &mut Context<Self>) {
        self.media_dock = dock;
        cx.notify();
    }

    pub fn set_rail(&mut self, rail: Option<AnyView>, cx: &mut Context<Self>) {
        self.rail = rail;
        cx.notify();
    }

    /// Focus the lookup field.
    pub fn focus_query(&self, window: &mut Window, cx: &mut App) {
        self.query.update(cx, |s, cx| s.focus(window, cx));
    }

    fn library_event(&mut self, event: LibrarySectionEvent, cx: &mut Context<Self>) {
        let kind = DomainKind::Scripture;
        let command = match event {
            LibrarySectionEvent::Select(id) => {
                self.model
                    .update(cx, |m, cx| m.select_library(id.to_string(), cx));
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
        self.model.update(cx, |m, cx| {
            m.library_command(command, cx, move |m, created, _| {
                if create {
                    m.set_library_to_rename(created);
                }
            })
        });
    }

    // ─── Search list ────────────────────────────────────────────────────────

    fn bar_items(&self, cx: &App) -> Vec<BarItem> {
        let m = self.model.read(cx);
        rows::bar_items(
            self.bar.recents_open,
            m.recent_lookups(),
            &m.book_completions(),
            m.suggestions_open(),
            m.suggestions(),
        )
    }

    fn bar_open(&self, items: &[BarItem]) -> bool {
        (self.bar.focused || self.bar.recents_open) && !self.bar.dismissed && !items.is_empty()
    }

    /// ↑/↓, Esc, Tab and Enter while the list is open.
    fn bar_key(&mut self, key: &str, shift: bool, cx: &mut Context<Self>) -> bool {
        let items = self.bar_items(cx);
        if !self.bar_open(&items) {
            return false;
        }
        match key {
            "down" | "up" => {
                self.bar.active = rows::cycle_active(self.bar.active, items.len(), key == "down");
            }
            "escape" => {
                self.bar.dismissed = true;
                self.bar.recents_open = false;
            }
            "tab" if !shift => {
                let picked = self.bar.active.and_then(|i| items.get(i));
                let book = match picked {
                    Some(item @ BarItem::Book(_)) => Some(item.clone()),
                    _ => items
                        .iter()
                        .find(|i| matches!(i, BarItem::Book(_)))
                        .cloned(),
                };
                let Some(book) = book else { return false };
                self.choose(book, false, cx);
            }
            "enter" => {
                let Some(item) = self.bar.active.and_then(|i| items.get(i)).cloned() else {
                    return false;
                };
                self.choose(item, shift, cx);
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    fn choose(&mut self, item: BarItem, whole_chapter: bool, cx: &mut Context<Self>) {
        self.bar.recents_open = false;
        self.bar.active = None;
        self.model.update(cx, |m, cx| match item {
            BarItem::Recent(reference) => m.pick_recent(reference, cx),
            BarItem::Book(book) => m.complete_book(book.value, cx),
            BarItem::Verse(result) if whole_chapter && rows::is_within_chapter(&result) => {
                m.open_chapter(result, cx)
            }
            BarItem::Verse(result) => m.preview_suggestion(result, cx),
        });
        cx.notify();
    }

    // ─── Grid keys ──────────────────────────────────────────────────────────

    fn page_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // Only when the page itself has focus: not a field, menu or the sidebar.
        if !self.page_focus.is_focused(window) {
            return;
        }
        let key = match event.keystroke.key.as_str() {
            "left" => Some(GridKey::Left),
            "right" => Some(GridKey::Right),
            "up" => Some(GridKey::Up),
            "down" => Some(GridKey::Down),
            "enter" | "space" => None,
            _ => return,
        };
        if rows::card_count(self.model.read(cx).rows()) == 0 {
            return;
        }
        self.model.update(cx, |m, cx| m.grid_key(key, cx));
        cx.stop_propagation();
    }

    // ─── Sync ───────────────────────────────────────────────────────────────

    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Focus set from code raises no event, so read it each frame too.
        let focused = self.query.read(cx).focus_handle(cx).is_focused(window);
        if focused != self.bar.focused {
            self.bar.focused = focused;
            if !focused {
                self.bar.recents_open = false;
            }
        }
        let query = self.model.read(cx).query().to_owned();
        if self.query.read(cx).value().as_ref() != query {
            self.query
                .update(cx, |s, cx| s.set_value(query, window, cx));
        }
        let suggestions = self.model.read(cx).suggestions().len();
        if suggestions != self.bar.suggestions_seen {
            self.bar.suggestions_seen = suggestions;
            self.bar.active = None;
        }
        let renaming = self.model.read(cx).renaming().map(str::to_owned);
        if renaming != self.renaming_for {
            if let Some(id) = &renaming {
                let title = self
                    .model
                    .read(cx)
                    .plans()
                    .iter()
                    .find(|p| &p.id == id)
                    .map(|p| p.title.clone())
                    .unwrap_or_default();
                self.rename.update(cx, |s, cx| {
                    s.set_value(title, window, cx);
                    s.focus(window, cx);
                });
            }
            self.renaming_for = renaming;
        }
        if self.model.read(cx).item_filter().is_empty() && !self.filter.read(cx).value().is_empty()
        {
            self.filter.update(cx, |s, cx| s.set_value("", window, cx));
        }
        // The library rows.
        let (libraries, default_count, active, to_rename) = self.model.update(cx, |m, _| {
            m.reconcile_picks();
            let counts = m.library_counts();
            let libraries: Vec<LibraryRow> = m
                .libraries()
                .libraries
                .iter()
                .map(|l| LibraryRow {
                    id: l.id.clone().into(),
                    name: l.name.clone().into(),
                    count: counts.get(&l.id).copied().unwrap_or(0),
                })
                .collect();
            let active = if m.source() == Some(Source::Search) {
                String::new()
            } else {
                m.active_library().to_owned()
            };
            (
                libraries,
                counts.get(DEFAULT_LIBRARY_ID).copied().unwrap_or(0),
                active,
                m.take_library_to_rename(),
            )
        });
        if libraries != self.library_rows {
            self.library_rows = libraries.clone();
            self.library
                .update(cx, |l, cx| l.set_libraries(libraries, default_count, cx));
        }
        self.library.update(cx, |l, cx| l.set_active(active, cx));
        if let Some(id) = to_rename {
            self.library
                .update(cx, |l, cx| l.begin_rename(id, window, cx));
        }
        // Scroll requests.
        let (target, rows) = self
            .model
            .update(cx, |m, _| (m.take_scroll(), m.rows().to_vec()));
        if let Some(target) = target {
            let (line, strategy) = match target {
                ScrollTarget::Card(i) => (
                    rows::line_of_card(&self.lines, &rows, i),
                    ScrollStrategy::Nearest,
                ),
                ScrollTarget::CardCentred(i) => (
                    rows::line_of_card(&self.lines, &rows, i),
                    ScrollStrategy::Center,
                ),
                ScrollTarget::Row(r) => (rows::line_of_row(&self.lines, r), ScrollStrategy::Top),
            };
            if let Some(line) = line {
                self.scroll.scroll_to_item(line, strategy);
            }
        }
    }
}

// ─── Small pieces ───────────────────────────────────────────────────────────

fn quiet_button(id: impl Into<gpui_kit::ElementId>, label: impl Into<SharedString>) -> Button {
    Button::new(id).ghost().xsmall().label(label)
}

fn icon_button(id: impl Into<gpui_kit::ElementId>, icon: KairoIcon, label: &str) -> Button {
    Button::new(id)
        .ghost()
        .xsmall()
        .icon(Icon::new(icon))
        .accessibility_label(SharedString::from(label.to_owned()))
        .tooltip(SharedString::from(label.to_owned()))
}

fn gesture(event: &gpui_kit::ClickEvent) -> Option<PickGesture> {
    let m = event.modifiers();
    if m.shift {
        Some(PickGesture::Range)
    } else if m.platform || m.control {
        Some(PickGesture::Toggle)
    } else {
        None
    }
}

impl ScriptureWorkspace {
    // ─── Sidebar ────────────────────────────────────────────────────────────

    fn render_sidebar(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let passage_count = self.model.read(cx).passage_picks().len();
        let passage_bar = (passage_count > 0).then(|| {
            self.picked_bar(
                "passages",
                passage_count,
                "passage",
                cx,
                |this, window, cx| this.remove_picked_passages(window, cx),
                |m, cx| m.clear_passage_picks(cx),
            )
        });
        let items =
            (self.model.read(cx).source() == Some(Source::Plan)).then(|| self.render_items(cx));
        let model_keys = self.model.clone();
        let passage_keys = cx.listener(move |this, event: &KeyDownEvent, window, cx| {
            let k = &event.keystroke;
            if k.key == "a" && k.modifiers.platform {
                model_keys.update(cx, |m, cx| m.select_all_passages(cx));
                cx.stop_propagation();
            } else if k.key == "backspace" || k.key == "delete" {
                this.remove_picked_passages(window, cx);
                cx.stop_propagation();
            } else if k.key == "escape" {
                model_keys.update(cx, |m, cx| m.clear_passage_picks(cx));
            }
        });
        let m = self.model.read(cx);
        let model = self.model.clone();
        let importing = m.importing();
        let viewing_search = m.source() == Some(Source::Search);
        let search_count = m.search_count();
        let show_items = m.source() == Some(Source::Plan);
        let open_items = m.open_plan_items().len();

        let upload = div().px_3().pt_3().child(
            div()
                .id(ids::UPLOAD_NOTES)
                .test_support()
                .role(Role::Button)
                .aria_label("Upload sermon notes")
                .flex()
                .items_center()
                .gap_3()
                .rounded(radius::lg())
                .bg(k.surface_tertiary)
                .px_3()
                .py_2p5()
                .when(!importing, |d| {
                    d.hover(|s| s.bg(k.surface_elevated)).on_click({
                        let model = model.clone();
                        move |_, _, cx| model.update(cx, |m, cx| m.import_notes(cx))
                    })
                })
                .when(importing, |d| d.opacity(0.7))
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .size_8()
                        .rounded(radius::md())
                        .bg(k.accent(Step::S500))
                        .text_color(k.on_accent)
                        .child(
                            Icon::new(if importing {
                                KairoIcon::Loader
                            } else {
                                KairoIcon::Upload
                            })
                            .size_4(),
                        ),
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
                                    "Reading sermon notes…"
                                } else {
                                    "Upload sermon notes"
                                }),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(text::LABEL.size)
                                .text_color(k.text(TextStep::T400))
                                .child("PDF, Word, TXT, Markdown or RTF"),
                        ),
                ),
        );

        let search_row = div()
            .id(ids::SEARCH_RESULTS)
            .test_support()
            .role(Role::Button)
            .aria_label(format!("Search results, {search_count}"))
            .aria_selected(viewing_search)
            .flex()
            .items_center()
            .gap_2()
            .mx_1()
            .rounded(radius::md())
            .px_2()
            .py_1p5()
            .text_size(text::XS.size)
            .map(|d| {
                if viewing_search {
                    d.bg(k.surface_elevated).text_color(k.text_primary)
                } else {
                    d.text_color(k.text(TextStep::T400))
                        .hover(|s| s.bg(k.white.opacity(0.05)))
                }
            })
            // No results: a press from assistive technology does nothing, as the pointer does.
            .when(search_count == 0, |d| d.opacity(0.4).on_click(|_, _, _| {}))
            .when(search_count > 0, |d| {
                d.on_click({
                    let model = model.clone();
                    move |_, _, cx| model.update(cx, |m, cx| m.show_search_results(cx))
                })
            })
            .child(Icon::new(KairoIcon::ScanSearch).size_3())
            .child(div().flex_1().truncate().child("Search results"))
            .child(
                div()
                    .text_size(text::MICRO.size)
                    .text_color(k.text(TextStep::T600))
                    .child(search_count.to_string()),
            );

        let passages: Vec<_> = m.visible_passages().into_iter().cloned().collect();
        let passage_picks = m.passage_picks().clone();
        let passage_list = (!passages.is_empty()).then(|| {
            div()
                .id("scripture-passages")
                .track_focus(&self.passages_focus)
                .mt_1p5()
                .max_h(px(160.))
                .overflow_y_scroll()
                .on_key_down(passage_keys)
                .children(passages.into_iter().map(|passage| {
                    let picked = passage_picks.contains(&passage.id);
                    let id = passage.id.clone();
                    let open = passage.clone();
                    let model = model.clone();
                    let remove_model = model.clone();
                    let focus = self.passages_focus.clone();
                    h_flex()
                        .id(SharedString::from(ids::passage(&id)))
                        .test_support()
                        .group("passage")
                        .gap_2()
                        .mx_1()
                        .rounded(radius::md())
                        .px_2()
                        .py_1p5()
                        .text_size(text::XS.size)
                        .map(|d| {
                            if picked {
                                d.bg(k.row_selected()).text_color(k.text_primary)
                            } else {
                                d.text_color(k.text(TextStep::T400))
                                    .hover(|s| s.bg(k.surface_tertiary))
                            }
                        })
                        .on_click(move |event, window, cx| {
                            focus.focus(window, cx);
                            match gesture(event) {
                                Some(g) => {
                                    model.update(cx, |m, cx| m.pick_passage(open.id.clone(), g, cx))
                                }
                                None => model.update(cx, |m, cx| m.open_passage(&open, cx)),
                            }
                        })
                        .child(
                            div()
                                .flex_1()
                                .truncate()
                                .child(passage.reference.clone())
                                .child(
                                    div()
                                        .ml_1p5()
                                        .text_size(text::MICRO.size)
                                        .text_color(k.text(TextStep::T600))
                                        .child(passage.translation.to_uppercase()),
                                ),
                        )
                        .child(
                            icon_button(
                                SharedString::from(ids::passage_remove(&id)),
                                KairoIcon::Trash2,
                                &format!("Remove {} from saved passages", passage.reference),
                            )
                            .on_click(move |_, _, cx| {
                                let id = id.clone();
                                remove_model.update(cx, |m, cx| m.remove_passages(vec![id], cx));
                            }),
                        )
                }))
        });

        let heading = h_flex()
            .px_3()
            .pt_3()
            .pb_1()
            .gap_1()
            .child(
                div()
                    .flex_1()
                    .text_size(text::XS.size)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(k.text(TextStep::T300))
                    .child("Playlists"),
            )
            .child(
                icon_button(
                    ids::IMPORT,
                    if importing {
                        KairoIcon::Loader
                    } else {
                        KairoIcon::Download
                    },
                    "Import",
                )
                .disabled(importing)
                .popover_trigger()
                .dropdown_menu({
                    let model = model.clone();
                    move |menu, _, _| {
                        let notes = model.clone();
                        let kairo = model.clone();
                        menu.item(
                            PopupMenuItem::new("Import sermon notes…")
                                .icon(Icon::new(KairoIcon::FileText))
                                .on_click(move |_, _, cx| {
                                    notes.update(cx, |m, cx| m.import_notes(cx))
                                }),
                        )
                        .item(
                            PopupMenuItem::new("Import .kairo playlist…")
                                .icon(Icon::new(KairoIcon::Download))
                                .on_click(move |_, _, cx| {
                                    kairo.update(cx, |m, cx| m.import_kairo(cx))
                                }),
                        )
                    }
                }),
            )
            .child(
                icon_button(
                    ids::NEW_PLAYLIST,
                    if m.creating() {
                        KairoIcon::Loader
                    } else {
                        KairoIcon::Plus
                    },
                    "New playlist",
                )
                .disabled(m.creating())
                .on_click({
                    let model = model.clone();
                    move |_, _, cx| model.update(cx, |m, cx| m.create_playlist(false, cx))
                }),
            );

        let plans_list = div()
            .id("scripture-plans")
            .overflow_y_scroll()
            .pb_2()
            .map(|d| {
                if show_items {
                    d.max_h(px(220.)).flex_none()
                } else {
                    d.flex_1().min_h_0()
                }
            })
            .when(m.plans().is_empty(), |d| {
                d.child(
                    div()
                        .px_3()
                        .pt_4()
                        .pb_6()
                        .text_center()
                        .text_size(text::LABEL.size)
                        .text_color(k.text(TextStep::T600))
                        .child(
                            "No playlists yet. Upload sermon notes above, or create one with ＋.",
                        ),
                )
            })
            .children(
                m.plans()
                    .iter()
                    .map(|plan| self.render_plan_row(plan, window, cx)),
            );

        let filter = (show_items && open_items > 8).then(|| {
            div().flex_none().p_2().child(
                div().id(ids::ITEM_FILTER).test_support().child(
                    Input::new(&self.filter)
                        .kairo_size()
                        .small()
                        .prefix(Icon::new(KairoIcon::ScanSearch).size_3()),
                ),
            )
        });

        v_flex()
            .id("scripture-sidebar")
            .w(px(
                kairo_ui_kit::components::page_sidebar::PageSidebar::width(None, cx),
            ))
            .flex_none()
            .h_full()
            .min_h_0()
            .overflow_hidden()
            .bg(k.sidebar_list)
            .border_r_1()
            .border_color(k.panel_border)
            .child(
                v_flex()
                    .flex_none()
                    .bg(k.sidebar_sources)
                    .child(upload)
                    .child(
                        div()
                            .px_1()
                            .pt_2()
                            .pb_1()
                            .child(self.library.clone())
                            .child(search_row)
                            .children(passage_list)
                            .children(passage_bar),
                    ),
            )
            .child(v_flex().flex_none().bg(k.sidebar_sources).child(heading))
            .child(
                v_flex()
                    .min_h_0()
                    .flex_1()
                    .child(
                        div()
                            .bg(k.sidebar_sources)
                            .flex()
                            .flex_col()
                            .map(|d| {
                                if show_items {
                                    d.flex_none()
                                } else {
                                    d.flex_1().min_h_0()
                                }
                            })
                            .child(plans_list),
                    )
                    .children(items),
            )
            .children(filter)
            .into_any_element()
    }

    fn picked_bar(
        &self,
        key: &'static str,
        count: usize,
        noun: &str,
        cx: &mut Context<Self>,
        remove: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        clear: impl Fn(&mut Scripture, &mut Context<Scripture>) + 'static,
    ) -> AnyElement {
        let k = *cx.kairo();
        let model = self.model.clone();
        h_flex()
            .mx_2()
            .my_1p5()
            .gap_2()
            .rounded(radius::lg())
            .bg(k.surface_elevated)
            .px_2p5()
            .py_1p5()
            .text_size(text::LABEL.size)
            .child(
                div()
                    .flex_1()
                    .truncate()
                    .text_color(k.text(TextStep::T300))
                    .child(format!("{} selected", rows::plural(count, noun))),
            )
            .child(
                quiet_button(
                    SharedString::from(format!("scripture-{key}-remove")),
                    "Remove",
                )
                .tooltip("Remove (Delete)")
                .on_click(cx.listener(move |this, _, window, cx| remove(this, window, cx))),
            )
            .child(
                quiet_button(
                    SharedString::from(format!("scripture-{key}-clear")),
                    "Clear",
                )
                .tooltip("Clear (Esc)")
                .on_click(move |_, _, cx| model.update(cx, |m, cx| clear(m, cx))),
            )
            .into_any_element()
    }

    fn remove_picked_passages(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let m = self.model.read(cx);
        let ids: Vec<String> = m
            .visible_passages()
            .iter()
            .filter(|p| m.passage_picks().contains(&p.id))
            .map(|p| p.id.clone())
            .collect();
        if ids.is_empty() {
            return;
        }
        let model = self.model.clone();
        if ids.len() == 1 {
            model.update(cx, |m, cx| m.remove_passages(ids, cx));
            return;
        }
        confirm(
            ConfirmRequest::new(format!("Remove {} saved passages?", ids.len()), "Remove")
                .destructive(),
            window,
            cx,
            move |ok, _, cx| {
                if ok {
                    model.update(cx, |m, cx| m.remove_passages(ids, cx));
                }
            },
        );
    }

    fn remove_picked_items(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let m = self.model.read(cx);
        if m.source() != Some(Source::Plan) {
            return;
        }
        let ids: Vec<String> = m
            .filtered_items()
            .iter()
            .filter(|(_, i)| m.item_picks().contains(&i.id))
            .map(|(_, i)| i.id.clone())
            .collect();
        if ids.is_empty() {
            return;
        }
        let model = self.model.clone();
        if ids.len() == 1 {
            model.update(cx, |m, cx| m.remove_plan_items(ids, cx));
            return;
        }
        confirm(
            ConfirmRequest::new(
                format!("Remove {} items from this playlist?", ids.len()),
                "Remove",
            )
            .destructive(),
            window,
            cx,
            move |ok, _, cx| {
                if ok {
                    model.update(cx, |m, cx| m.remove_plan_items(ids, cx));
                }
            },
        );
    }

    fn render_plan_row(
        &self,
        plan: &kairo_domain::scripture::SermonPlan,
        _window: &mut Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let m = self.model.read(cx);
        let model = self.model.clone();
        let selected = m.selected_plan_id() == Some(plan.id.as_str());
        let id = plan.id.clone();
        let shell = div()
            .mx_1()
            .rounded(radius::md())
            .when(selected, |d| d.bg(k.surface_elevated));
        if m.renaming() == Some(plan.id.as_str()) {
            let save_model = model.clone();
            let rename = self.rename.clone();
            return shell
                .child(
                    v_flex()
                        .gap_2()
                        .p_2()
                        .child(
                            div().id(ids::RENAME).test_support().child(
                                Input::new(&self.rename)
                                    .kairo_size()
                                    .small()
                                    .aria_label("Playlist name"),
                            ),
                        )
                        .child(
                            h_flex()
                                .gap_1p5()
                                .child(
                                    Button::new(ids::RENAME_SAVE)
                                        .xsmall()
                                        .flex_1()
                                        .icon(Icon::new(KairoIcon::Check))
                                        .label("Save")
                                        .on_click(move |_, _, cx| {
                                            let draft = rename.read(cx).value().to_string();
                                            save_model
                                                .update(cx, |m, cx| m.save_rename(&draft, cx));
                                        }),
                                )
                                .child(quiet_button(ids::RENAME_CANCEL, "Cancel").on_click(
                                    move |_, _, cx| model.update(cx, |m, cx| m.cancel_rename(cx)),
                                )),
                        ),
                )
                .into_any_element();
        }
        if m.pending_delete() == Some(plan.id.as_str()) {
            let cancel = model.clone();
            let del = id.clone();
            return shell
                .child(
                    h_flex()
                        .id(SharedString::from(format!(
                            "{}-group",
                            ids::delete_confirm(&id)
                        )))
                        .gap_1p5()
                        .p_2()
                        .aria_label(SharedString::from(format!(
                            "Confirm deleting {}",
                            plan.title
                        )))
                        .child(
                            quiet_button(SharedString::from(ids::delete_cancel(&id)), "Cancel")
                                .flex_1()
                                .on_click(move |_, _, cx| {
                                    cancel.update(cx, |m, cx| m.request_delete(None, cx))
                                }),
                        )
                        .child(
                            Button::new(SharedString::from(ids::delete_confirm(&id)))
                                .danger()
                                .xsmall()
                                .flex_1()
                                .label("Delete")
                                .on_click(move |_, _, cx| {
                                    let del = del.clone();
                                    model.update(cx, |m, cx| m.delete_plan(del, cx))
                                }),
                        ),
                )
                .into_any_element();
        }
        let open = model.clone();
        let open_id = id.clone();
        shell
            .child(
                h_flex()
                    .group("plan")
                    .gap_0p5()
                    .pr_1()
                    .child(
                        h_flex()
                            .id(SharedString::from(ids::plan(&id)))
                            .test_support()
                            .role(Role::Button)
                            .aria_label(plan.title.clone())
                            .flex_1()
                            .min_w_0()
                            .gap_2()
                            .rounded(radius::md())
                            .px_2()
                            .py_1p5()
                            .text_size(text::XS.size)
                            .text_color(if selected {
                                k.text_primary
                            } else {
                                k.text(TextStep::T300)
                            })
                            .when(!selected, |d| d.hover(|s| s.bg(k.white.opacity(0.05))))
                            .on_click(move |_, _, cx| {
                                open.update(cx, |m, cx| m.open_plan(&open_id, cx))
                            })
                            .child(Icon::new(KairoIcon::FileText).size_3().text_color(
                                if selected {
                                    k.text_primary
                                } else {
                                    k.text(TextStep::T500)
                                },
                            ))
                            .child(div().flex_1().truncate().child(plan.title.clone()))
                            .child(
                                div()
                                    .text_size(text::MICRO.size)
                                    .text_color(k.text(TextStep::T500))
                                    .child(plan.items.len().to_string()),
                            ),
                    )
                    .child(
                        icon_button(
                            SharedString::from(ids::plan_menu(&id)),
                            KairoIcon::MoreHorizontal,
                            &format!("Playlist options for {}", plan.title),
                        )
                        .popover_trigger()
                        .dropdown_menu(move |menu, _, _| {
                            let (a, b, c) = (model.clone(), model.clone(), model.clone());
                            let (ia, ib, ic) = (id.clone(), id.clone(), id.clone());
                            menu.item(
                                PopupMenuItem::new("Rename")
                                    .icon(Icon::new(KairoIcon::Edit2))
                                    .on_click(move |_, _, cx| {
                                        let id = ia.clone();
                                        a.update(cx, |m, cx| m.start_rename(id, cx))
                                    }),
                            )
                            .item(
                                PopupMenuItem::new("Export…")
                                    .icon(Icon::new(KairoIcon::Upload))
                                    .on_click(move |_, _, cx| {
                                        let id = ib.clone();
                                        b.update(cx, |m, cx| m.export_playlist(id, cx))
                                    }),
                            )
                            .item(
                                PopupMenuItem::new("Delete")
                                    .icon(Icon::new(KairoIcon::Trash2))
                                    .on_click(move |_, _, cx| {
                                        let id = ic.clone();
                                        c.update(cx, |m, cx| m.request_delete(Some(id), cx))
                                    }),
                            )
                        }),
                    ),
            )
            .into_any_element()
    }

    fn render_items(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let m = self.model.read(cx);
        let total = m.open_plan_items().len();
        let filtered: Vec<(usize, kairo_domain::scripture::SermonScriptureItem)> = m
            .filtered_items()
            .into_iter()
            .map(|(i, item)| (i, item.clone()))
            .collect();
        let active = m.active_item_id().map(str::to_owned);
        let picks = m.item_picks().clone();
        let can_reorder = m.can_reorder();
        let filter = m.item_filter().trim().to_owned();
        let model = self.model.clone();
        let band = h_flex()
            .flex_none()
            .px_3()
            .py_1p5()
            .bg(k.sidebar_band)
            .text_size(text::LABEL.size)
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(k.text(TextStep::T400))
            .child(format!(
                "{} {}",
                if filtered.len() == total {
                    total.to_string()
                } else {
                    format!("{} of {total}", filtered.len())
                },
                if total == 1 { "item" } else { "items" }
            ));
        let body: AnyElement = if total == 0 {
            v_flex()
                .items_center()
                .px_3()
                .pt_1()
                .pb_4()
                .gap_2()
                .child(
                    Icon::new(KairoIcon::BookOpen)
                        .size_3p5()
                        .text_color(k.text(TextStep::T600)),
                )
                .child(
                    div()
                        .text_size(text::LABEL.size)
                        .text_color(k.text(TextStep::T500))
                        .child("No scriptures yet. Search and add verses."),
                )
                .into_any_element()
        } else if filtered.is_empty() {
            div()
                .px_3()
                .pt_1()
                .pb_4()
                .text_center()
                .text_size(text::LABEL.size)
                .text_color(k.text(TextStep::T500))
                .child(format!("No items match “{filter}”"))
                .into_any_element()
        } else {
            let keys = model.clone();
            div()
                .id("scripture-items")
                .track_focus(&self.items_focus)
                .px_1()
                .pb_2()
                .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                    let k = &event.keystroke;
                    if k.key == "a" && k.modifiers.platform {
                        keys.update(cx, |m, cx| m.select_all_items(cx));
                        cx.stop_propagation();
                    } else if k.key == "backspace" || k.key == "delete" {
                        this.remove_picked_items(window, cx);
                        cx.stop_propagation();
                    } else if k.key == "escape" {
                        keys.update(cx, |m, cx| m.clear_item_picks(cx));
                    }
                }))
                .children(filtered.into_iter().map(|(index, item)| {
                    let picked = picks.contains(&item.id);
                    let current = active.as_deref() == Some(item.id.as_str());
                    let over = self
                        .drag_over
                        .as_ref()
                        .filter(|(id, _)| *id == item.id)
                        .map(|(_, p)| *p);
                    let model = model.clone();
                    let focus = self.items_focus.clone();
                    let item_id = item.id.clone();
                    let drag = ItemDrag {
                        id: item.id.clone(),
                        label: item.reference.clone().into(),
                    };
                    let half = |position: DropPosition, item_id: String, cx: &mut Context<Self>| {
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .h_1_2()
                            .map(|d| match position {
                                DropPosition::Before => d.top_0(),
                                DropPosition::After => d.bottom_0(),
                            })
                            .on_drag_move(cx.listener({
                                let item_id = item_id.clone();
                                move |this, event: &gpui_kit::DragMoveEvent<ItemDrag>, _, cx| {
                                    if event.bounds.contains(&event.event.position) {
                                        let next = Some((item_id.clone(), position));
                                        if this.drag_over != next {
                                            this.drag_over = next;
                                            cx.notify();
                                        }
                                    }
                                }
                            }))
                            .on_drop(cx.listener(move |this, dragged: &ItemDrag, _, cx| {
                                this.drag_over = None;
                                let (from, to) = (dragged.id.clone(), item_id.clone());
                                this.model.update(cx, |m, cx| {
                                    m.reorder_plan_item(&from, &to, position, cx)
                                });
                            }))
                    };
                    h_flex()
                        .id(SharedString::from(ids::item(&item.id)))
                        .test_support()
                        .role(Role::Button)
                        .aria_label(format!(
                            "{}, {}{}",
                            item.reference,
                            item.translation.to_uppercase(),
                            if item.available {
                                ""
                            } else {
                                ", not downloaded"
                            }
                        ))
                        .relative()
                        .gap_1()
                        .rounded(radius::md())
                        .px_1p5()
                        .py_1p5()
                        .text_size(text::XS.size)
                        .when(!item.available, |d| d.opacity(0.55))
                        .map(|d| {
                            if picked {
                                d.bg(k.row_selected()).text_color(k.text_primary)
                            } else if current {
                                d.bg(k.surface_tertiary).text_color(k.text_primary)
                            } else {
                                d.text_color(k.text(TextStep::T300))
                                    .hover(|s| s.bg(k.surface_tertiary))
                            }
                        })
                        .when_some(item.error.clone(), |d, e| {
                            d.tooltip(move |window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new(e.clone())
                                    .build(window, cx)
                            })
                        })
                        .when_some(over, |d, position| {
                            let line = div()
                                .absolute()
                                .left_2()
                                .right_2()
                                .h(px(1.))
                                .bg(k.accent(Step::S400));
                            d.child(match position {
                                DropPosition::Before => line.top_0(),
                                DropPosition::After => line.bottom_0(),
                            })
                        })
                        .on_click(move |event, window, cx| {
                            focus.focus(window, cx);
                            match gesture(event) {
                                Some(g) => {
                                    model.update(cx, |m, cx| m.pick_item(item_id.clone(), g, cx))
                                }
                                None => model.update(cx, |m, cx| m.select_plan_item(&item_id, cx)),
                            }
                        })
                        .when(can_reorder, |d| {
                            d.on_drag(drag, |drag, _, _, cx| cx.new(|_| drag.clone()))
                                .child(half(DropPosition::Before, item.id.clone(), cx))
                                .child(half(DropPosition::After, item.id.clone(), cx))
                        })
                        .child(
                            Icon::new(KairoIcon::GripVertical)
                                .size_3()
                                .text_color(k.text(TextStep::T600))
                                .when(!can_reorder, |i| {
                                    i.text_color(gpui_kit::transparent_black())
                                }),
                        )
                        .child(
                            div()
                                .w_4()
                                .flex_none()
                                .text_size(text::MICRO.size)
                                .text_color(k.text(TextStep::T500))
                                .child((index + 1).to_string()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(item.reference.clone()),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_size(text::MICRO.size)
                                .text_color(if item.available {
                                    k.text(TextStep::T500)
                                } else {
                                    k.text(TextStep::T600)
                                })
                                .when(!item.available, |d| d.line_through())
                                .child(item.translation.to_uppercase()),
                        )
                }))
                .into_any_element()
        };
        let bar = (!picks.is_empty()).then(|| {
            self.picked_bar(
                "items",
                picks.len(),
                "item",
                cx,
                |this, window, cx| this.remove_picked_items(window, cx),
                |m, cx| m.clear_item_picks(cx),
            )
        });
        v_flex()
            .flex_1()
            .min_h_0()
            .child(band)
            .child(
                div()
                    .id("scripture-items-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(body)
                    .children(bar),
            )
            .into_any_element()
    }

    // ─── The Bible bar ──────────────────────────────────────────────────────

    fn render_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let items = self.bar_items(cx);
        let open = self.bar_open(&items);
        let query = self.model.read(cx).query().to_owned();
        let phrase = is_likely_phrase_query(&query);
        let list = open.then(|| self.render_list(items, phrase, &query, cx));
        let m = self.model.read(cx);
        let model = self.model.clone();
        let loading = m.loading();
        let translation = m.translation().to_owned();
        let available: Vec<(String, String)> = m
            .translations()
            .iter()
            .filter(|t| t.available)
            .map(|t| (t.id.clone(), t.name.clone()))
            .collect();
        let current_name = m
            .translations()
            .iter()
            .find(|t| t.id == translation)
            .map(|t| t.name.clone());
        let label = if translation.is_empty() {
            "NKJV".to_owned()
        } else {
            translation.clone()
        };
        // Toolbar controls share the search field's 36 px height.
        let translation_select = Button::new(ids::TRANSLATION)
            .with_size(gpui_kit::component::Size::Medium)
            .h(px(36.))
            .w(px(96.))
            .label(label.clone())
            .dropdown_caret(true)
            .disabled(loading)
            .accessibility_label("Scripture translation")
            .tooltip(SharedString::from(
                current_name.unwrap_or_else(|| "Scripture translation".into()),
            ))
            .popover_trigger()
            .dropdown_menu({
                let model = model.clone();
                move |mut menu, _, _| {
                    menu = menu.max_h(px(416.)).scrollable(true);
                    if available.is_empty() {
                        return menu.item(
                            PopupMenuItem::new(format!("{label} · Loading translations…"))
                                .disabled(true),
                        );
                    }
                    for (id, name) in available.clone() {
                        let model = model.clone();
                        let pick = id.clone();
                        menu = menu.item(
                            PopupMenuItem::new(format!("{id}  {name}"))
                                .checked(id == translation)
                                .on_click(move |_, _, cx| {
                                    let pick = pick.clone();
                                    model.update(cx, |m, cx| m.set_translation(pick, cx))
                                }),
                        );
                    }
                    menu
                }
            });

        let recents_empty = m.recent_lookups().is_empty();
        let has_query = !query.is_empty();
        let field =
            div()
                .relative()
                .min_w(px(192.))
                .flex_1()
                .child(
                    h_flex()
                        .gap_1p5()
                        .child(
                            div()
                                .id(ids::QUERY)
                                .test_support()
                                .relative()
                                .flex_1()
                                .child(
                                    Input::new(&self.query)
                                        .kairo_size()
                                        .small()
                                        .prefix(
                                            Icon::new(if loading {
                                                KairoIcon::Loader
                                            } else {
                                                KairoIcon::BookOpen
                                            })
                                            .size_3()
                                            .text_color(k.text(TextStep::T500)),
                                        )
                                        .when(has_query, |i| {
                                            i.suffix(
                                                icon_button(
                                                    "scripture-clear-query",
                                                    KairoIcon::X,
                                                    "Clear reference",
                                                )
                                                .on_click({
                                                    let model = model.clone();
                                                    move |_, _, cx| {
                                                        model.update(cx, |m, cx| m.clear_query(cx))
                                                    }
                                                }),
                                            )
                                        }),
                                )
                                .child(coach_anchor(anchors::SCRIPTURE_QUERY)),
                        )
                        .child(
                            Button::new(ids::RECENTS)
                                .with_size(gpui_kit::component::Size::Medium)
                                .size(px(36.))
                                .icon(Icon::new(KairoIcon::ChevronDown))
                                .disabled(recents_empty)
                                .accessibility_label("Recent lookups")
                                .tooltip(if recents_empty {
                                    "No recent lookups yet"
                                } else {
                                    "Recent lookups"
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.bar.dismissed = false;
                                    this.bar.recents_open = !this.bar.recents_open;
                                    cx.notify();
                                })),
                        ),
                )
                .children(list);

        let use_for_service = m.selected_plan().map(|plan| {
            let in_use = m.live_plan().and_then(|l| l.plan_id.as_deref()) == Some(plan.id.as_str());
            Button::new(ids::USE_FOR_SERVICE)
                .small()
                .icon(Icon::new(KairoIcon::BookOpen))
                .label(if in_use {
                    "In use for this service"
                } else {
                    "Use for this service"
                })
                .disabled(in_use)
                .tooltip("Use this playlist to guide live scripture detection")
                .on_click({
                    let model = model.clone();
                    move |_, _, cx| model.update(cx, |m, cx| m.use_for_service(cx))
                })
        });

        h_flex()
            .flex_none()
            .flex_wrap()
            .gap_1p5()
            .px_5()
            .pt_3()
            .pb_1()
            .child(translation_select)
            .child(field)
            .child(self.render_options(cx))
            .children(use_for_service)
            .into_any_element()
    }

    fn render_list(
        &self,
        items: Vec<BarItem>,
        phrase: bool,
        query: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let active = self.bar.active;
        let footer = items
            .iter()
            .any(|i| matches!(i, BarItem::Verse(r) if rows::is_within_chapter(r)));
        let query = query.to_owned();
        deferred(
            v_flex()
                .id(ids::SUGGESTIONS)
                .test_support()
                .role(Role::ListBox)
                .absolute()
                .top(px(36.))
                .left_0()
                .w(px(576.))
                .max_h(px(320.))
                .overflow_y_scroll()
                .rounded(radius::lg())
                .bg(k.surface_elevated)
                .p_1()
                .on_mouse_down(gpui_kit::MouseButton::Left, |_, window, _| {
                    window.prevent_default()
                })
                .children(items.into_iter().enumerate().map(|(index, item)| {
                    let on = active == Some(index);
                    let pick = item.clone();
                    let chapter_pick = item.clone();
                    let row = h_flex()
                        .id(SharedString::from(ids::suggestion(index)))
                        .test_support()
                        .role(Role::ListBoxOption)
                        .aria_label(match &item {
                            BarItem::Recent(reference) => reference.clone(),
                            BarItem::Book(book) => book.value.trim().to_owned(),
                            BarItem::Verse(result) => format!(
                                "{} {}: {}",
                                result.reference,
                                result.translation,
                                rows::verse_text(result)
                            ),
                        })
                        .aria_selected(on)
                        .rounded(radius::md())
                        .px_2()
                        .py_1p5()
                        .gap_2p5()
                        .text_size(text::XS.size)
                        .map(|d| {
                            if on {
                                d.bg(k.surface_tertiary)
                            } else {
                                d.hover(|s| s.bg(k.surface_tertiary))
                            }
                        })
                        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                            if *hovered {
                                this.bar.active = Some(index);
                                cx.notify();
                            }
                        }))
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.choose(pick.clone(), false, cx)),
                        );
                    match &item {
                        BarItem::Recent(reference) => row
                            .child(
                                Icon::new(KairoIcon::Clock)
                                    .size_3()
                                    .text_color(k.text(TextStep::T500)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .truncate()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(k.text(TextStep::T200))
                                    .child(reference.clone()),
                            ),
                        BarItem::Book(book) => row
                            .child(
                                Icon::new(KairoIcon::BookOpen)
                                    .size_3()
                                    .text_color(k.text(TextStep::T500)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .truncate()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(k.text(TextStep::T200))
                                    .child(book.value.trim().to_owned()),
                            )
                            .when(index == 0, |d| {
                                d.child(
                                    div()
                                        .text_size(text::MICRO.size)
                                        .text_color(k.text(TextStep::T500))
                                        .child("Tab"),
                                )
                            }),
                        BarItem::Verse(result) => {
                            let text_body = rows::verse_text(result);
                            let row = if phrase {
                                row.items_start().child(
                                    v_flex()
                                        .flex_1()
                                        .min_w_0()
                                        .child(
                                            div()
                                                .text_size(text::SM.size)
                                                .text_color(k.text(TextStep::T300))
                                                .line_clamp(2)
                                                .children(
                                                    rows::highlight(&text_body, &query)
                                                        .into_iter()
                                                        .map(|(part, mark)| {
                                                            div()
                                                                .when(mark, |d| {
                                                                    d.font_weight(
                                                                        FontWeight::SEMIBOLD,
                                                                    )
                                                                    .text_color(k.text_primary)
                                                                })
                                                                .child(part)
                                                        }),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .mt_1()
                                                .text_size(text::LABEL.size)
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(k.text(TextStep::T500))
                                                .child(format!(
                                                    "{} · {}",
                                                    result.reference, result.translation
                                                )),
                                        ),
                                )
                            } else {
                                row.child(
                                    div()
                                        .w(px(112.))
                                        .flex_none()
                                        .truncate()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(k.text_primary)
                                        .child(result.reference.clone()),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .truncate()
                                        .text_color(k.text(TextStep::T400))
                                        .child(text_body),
                                )
                            };
                            row.when(rows::is_within_chapter(result), |d| {
                                let book = result
                                    .reference
                                    .split(':')
                                    .next()
                                    .unwrap_or_default()
                                    .to_owned();
                                d.child(
                                    h_flex()
                                        .flex_none()
                                        .gap_1()
                                        .child(
                                            quiet_button(
                                                SharedString::from(format!(
                                                    "{}-verse",
                                                    ids::suggestion(index)
                                                )),
                                                "Verse",
                                            )
                                            .tooltip(SharedString::from(format!(
                                                "Add {} only (Enter)",
                                                result.reference
                                            )))
                                            .on_click(
                                                cx.listener({
                                                    let pick = item.clone();
                                                    move |this, _, _, cx| {
                                                        cx.stop_propagation();
                                                        this.choose(pick.clone(), false, cx)
                                                    }
                                                }),
                                            ),
                                        )
                                        .child(
                                            quiet_button(
                                                SharedString::from(ids::suggestion_chapter(index)),
                                                "Chapter",
                                            )
                                            .tooltip(SharedString::from(format!(
                                                "Add all of {book} (Shift+Enter)"
                                            )))
                                            .on_click(
                                                cx.listener(move |this, _, _, cx| {
                                                    cx.stop_propagation();
                                                    this.choose(chapter_pick.clone(), true, cx)
                                                }),
                                            ),
                                        ),
                                )
                            })
                        }
                    }
                }))
                .when(footer, |d| {
                    d.child(
                        div()
                            .mt_1()
                            .px_2()
                            .pt_1p5()
                            .pb_0p5()
                            .text_size(text::MICRO.size)
                            .text_color(k.text(TextStep::T500))
                            .child("↵ adds the verse · ⇧↵ adds the whole chapter"),
                    )
                }),
        )
        .with_priority(1)
        .into_any_element()
    }

    fn render_options(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let model = self.model.clone();
        Popover::new(ids::OPTIONS)
            .button_trigger(
                Button::new("scripture-options-trigger")
                    .with_size(gpui_kit::component::Size::Medium)
                    .h(px(36.))
                    .px(px(12.))
                    .icon(Icon::new(KairoIcon::SlidersHorizontal))
                    .label("Options")
                    .accessibility_label("Scripture options"),
            )
            .content(move |_, _, cx| {
                let m = model.read(cx);
                let options = m.options().clone();
                let ready: Vec<(String, String)> = m
                    .translations()
                    .iter()
                    .filter(|t| t.available)
                    .map(|t| (t.id.clone(), t.name.clone()))
                    .collect();
                let toggle = |id: &'static str,
                              label: &'static str,
                              on: bool,
                              change: fn(bool) -> OptionChange| {
                    let model = model.clone();
                    h_flex()
                        .justify_between()
                        .gap_3()
                        .px_1p5()
                        .py_1p5()
                        .rounded(radius::md())
                        .text_size(text::SM.size)
                        .child(label)
                        .child(
                            Switch::new(id)
                                .checked(on)
                                .small()
                                .accessibility_label(label)
                                .on_click(move |on, _, cx| {
                                    let on = *on;
                                    model.update(cx, |m, cx| m.change_option(change(on), cx))
                                }),
                        )
                };
                let current = options.default_translation.clone();
                let installed = ready.iter().any(|(id, _)| *id == current);
                let current_name = ready
                    .iter()
                    .find(|(id, _)| *id == current)
                    .map(|(_, n)| n.clone())
                    .unwrap_or_else(|| format!("{current} (not installed)"));
                let pick_model = model.clone();
                let settings_model = model.clone();
                let heading = |title: &'static str| {
                    div()
                        .text_size(text::LABEL.size)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(k.text(TextStep::T500))
                        .child(title)
                };
                v_flex()
                    .w(px(288.))
                    .gap_4()
                    .p_1()
                    .child(
                        v_flex()
                            .gap_1p5()
                            .child(heading("Slides"))
                            .child(toggle(
                                "scripture-opt-numbers",
                                "Verse numbers",
                                options.show_verse_numbers,
                                OptionChange::VerseNumbers,
                            ))
                            .child(toggle(
                                "scripture-opt-translation",
                                "Translation after reference",
                                options.show_translation,
                                OptionChange::ShowTranslation,
                            ))
                            .child(toggle(
                                "scripture-opt-one",
                                "One verse per slide",
                                options.max_verses == 1.0,
                                |on| OptionChange::MaxVerses(if on { 1.0 } else { 0.0 }),
                            )),
                    )
                    .child(
                        v_flex()
                            .gap_1p5()
                            .child(
                                h_flex().justify_between().child(heading("Bible")).child(
                                    Button::new("scripture-get-more")
                                        .ghost()
                                        .xsmall()
                                        .label("Get more")
                                        .icon(Icon::new(KairoIcon::ChevronRight))
                                        .on_click(move |_, _, cx| {
                                            settings_model.update(cx, |m, cx| m.open_settings(cx))
                                        }),
                                ),
                            )
                            .child(
                                Button::new("scripture-default-bible")
                                    .small()
                                    .w_full()
                                    .label(current_name)
                                    .dropdown_caret(true)
                                    .accessibility_label("Default Bible")
                                    .popover_trigger()
                                    .dropdown_menu(move |mut menu, _, _| {
                                        if !installed {
                                            menu = menu.item(
                                                PopupMenuItem::new(format!(
                                                    "{current} (not installed)"
                                                ))
                                                .disabled(true),
                                            );
                                        }
                                        for (id, name) in ready.clone() {
                                            let model = pick_model.clone();
                                            let pick = id.clone();
                                            menu = menu.item(
                                                PopupMenuItem::new(name)
                                                    .checked(id == current)
                                                    .on_click(move |_, _, cx| {
                                                        let pick = pick.clone();
                                                        model.update(cx, |m, cx| {
                                                            m.change_option(
                                                                OptionChange::DefaultTranslation(
                                                                    pick,
                                                                ),
                                                                cx,
                                                            )
                                                        })
                                                    }),
                                            );
                                        }
                                        menu
                                    }),
                            ),
                    )
            })
            .into_any_element()
    }

    fn render_notices(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let k = *cx.kairo();
        let m = self.model.read(cx);
        let notice = m.translation_notice();
        let error = m.error().map(str::to_owned);
        if notice.is_none() && error.is_none() {
            return None;
        }
        let box_ = |id: &'static str, message: String, role: Option<Role>| {
            h_flex()
                .id(id)
                .test_support()
                .when_some(role, |d, r| d.role(r))
                .items_start()
                .gap_2p5()
                .rounded(radius::lg())
                .bg(k.tints.yellow)
                .px_4()
                .py_3()
                .text_size(text::SM.size)
                .text_color(k.warning_text)
                .child(
                    Icon::new(KairoIcon::AlertCircle)
                        .size_3p5()
                        .mt_0p5()
                        .flex_none(),
                )
                .child(div().flex_1().child(message))
        };
        Some(
            v_flex()
                .flex_none()
                .gap_2()
                .bg(k.surface)
                .px_3()
                .py_2()
                .children(notice.map(|n| box_(ids::NOTICE, n, None)))
                .children(error.map(|e| box_(ids::ERROR, e, Some(Role::Alert))))
                .into_any_element(),
        )
    }

    // ─── The grid ───────────────────────────────────────────────────────────

    fn render_grid(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let m = self.model.read(cx);
        if m.rows().is_empty() {
            if m.loading() || m.error().is_some() {
                return div().flex_1().into_any_element();
            }
            let in_plan = m.source() == Some(Source::Plan);
            let title = if in_plan {
                format!(
                    "{} is empty",
                    m.selected_plan().map_or("Playlist", |p| p.title.as_str())
                )
            } else {
                "Look up a reference or search the text".to_owned()
            };
            return v_flex()
                .id(ids::EMPTY)
                .test_support()
                .flex_1()
                .items_center()
                .py_20()
                .child(
                    div()
                        .text_size(text::SM.size)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(k.text(TextStep::T400))
                        .child(title),
                )
                .child(
                    div()
                        .mt_1()
                        .text_size(text::XS.size)
                        .text_color(k.text(TextStep::T600))
                        .child(if in_plan {
                            "Search a passage, then add it to this playlist"
                        } else {
                            "“jos 1 5 9” works too · Tab completes a book"
                        }),
                )
                .into_any_element();
        }
        let width = (self.grid_width.get() - GRID_PADDING * 2.0).max(1.0);
        let min = rows::card_min_width(m.zoom());
        let cols = rows::columns(width, min);
        let card_w = rows::card_width(width, cols);
        let card_h = rows::card_height(card_w);
        self.card_size = (card_w, card_h - rows::CARD_CHROME);
        let lines = rows::grid_lines(m.rows(), cols);
        let sizes: Vec<_> = lines
            .iter()
            .map(|l| size(px(width), px(rows::line_height(l, m.rows(), card_h))))
            .collect();
        self.lines = Rc::new(lines);
        let measured = self.grid_width.clone();
        let view = cx.entity();
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .child(
                canvas(
                    move |bounds, window, _| {
                        let w = f32::from(bounds.size.width);
                        if (measured.get() - w).abs() > 0.5 {
                            measured.set(w);
                            // Refreshing mid-draw is ignored, and a cached view would not redraw:
                            // notify the view once this frame ends.
                            window.on_next_frame(move |_, cx| view.update(cx, |_, cx| cx.notify()));
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
                    ids::GRID,
                    Rc::new(sizes),
                    |this, range, window, cx| {
                        range
                            .map(|i| this.render_line(i, window, cx))
                            .collect::<Vec<_>>()
                    },
                )
                .track_scroll(&self.scroll)
                .size_full()
                .px(px(GRID_PADDING))
                .py_4(),
            )
            .into_any_element()
    }

    fn render_line(&self, index: usize, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let m = self.model.read(cx);
        let rows = m.rows();
        let Some(line) = self.lines.get(index) else {
            return div().into_any_element();
        };
        let active_row = rows::locate(rows, m.active()).map(|(r, _)| r);
        match line {
            GridLine::Header { row } => {
                let r = &rows[*row];
                let live = m.cue() && active_row == Some(*row);
                h_flex()
                    .id(SharedString::from(ids::header(*row)))
                    .test_support()
                    .h(px(rows::HEADER_HEIGHT))
                    .w_full()
                    .gap_2()
                    .px_5()
                    .mx(px(-20.))
                    .bg(k.surface_tertiary.opacity(0.72))
                    .child(Icon::new(KairoIcon::Bible).size_3().text_color(if live {
                        k.live
                    } else {
                        k.text(TextStep::T500)
                    }))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(text::SM.size)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(if live { k.live } else { k.text_primary })
                            .child(r.reference.clone()),
                    )
                    .children(r.note.clone().map(|n| {
                        div()
                            .text_size(text::MICRO.size)
                            .text_color(k.warning_text.opacity(0.9))
                            .child(n)
                    }))
                    .child(
                        div()
                            .ml_auto()
                            .flex_none()
                            .text_size(text::LABEL.size)
                            .text_color(k.text(TextStep::T500))
                            .child(rows::plural(r.cards.len(), "verse")),
                    )
                    .into_any_element()
            }
            GridLine::Cards {
                row, first, count, ..
            } => {
                let options = m.options().clone();
                let start = rows::flat_index_at(rows, *row, 0);
                let (w, slide_h) = self.card_size;
                let cue = m.cue();
                let picks = m.picks().clone();
                let active = m.active();
                h_flex()
                    .items_start()
                    .gap(px(rows::CARD_GAP))
                    .children(
                        rows[*row].cards[*first..first + count]
                            .iter()
                            .enumerate()
                            .map(|(offset, card)| {
                                let local = first + offset;
                                let flat = start + local;
                                let reference = format_overlay_reference(
                                    &card.result.reference,
                                    &card.result.translation,
                                    options.show_translation,
                                );
                                let body = format_overlay_verse_text(
                                    &card.result.verses,
                                    options.show_verse_numbers,
                                    Some(options.max_verses),
                                );
                                let model = self.model.clone();
                                let focus = self.page_focus.clone();
                                VerseCard::new(
                                    SharedString::from(ids::card(flat)),
                                    card.result.reference.clone(),
                                )
                                .slide(reference, body.map(Into::into))
                                .number(local + 1)
                                .size(w, slide_h)
                                .background(options.background.clone())
                                .live(cue && flat == active)
                                .current(picks.contains(&flat) || flat == active)
                                .sending(card.send_status == SendStatus::Sending)
                                .on_click(
                                    move |event, window, cx| {
                                        focus.focus(window, cx);
                                        match gesture(event) {
                                            Some(g) => {
                                                model.update(cx, |m, cx| m.pick_card(flat, g, cx))
                                            }
                                            None => {
                                                model.update(cx, |m, cx| m.select_card(flat, cx))
                                            }
                                        }
                                    },
                                )
                            }),
                    )
                    .into_any_element()
            }
        }
    }

    fn render_selection(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let m = self.model.read(cx);
        let count = m.picks().len();
        if count == 0 {
            return None;
        }
        let plans: Vec<(String, String)> = m
            .plans()
            .iter()
            .map(|p| (p.id.clone(), p.title.clone()))
            .collect();
        let model = self.model.clone();
        let clear = model.clone();
        let save = model.clone();
        Some(
            div()
                .flex_none()
                .px_5()
                .pb_3()
                .child(
                    SelectionBar::new(count, "verse")
                        .action(
                            Button::new(ids::PICKED_ADD)
                                .xsmall()
                                .icon(Icon::new(KairoIcon::Plus))
                                .label("Add to playlist")
                                .popover_trigger()
                                .dropdown_menu(move |mut menu, _, _| {
                                    menu = menu.max_h(px(288.)).scrollable(true);
                                    if plans.is_empty() {
                                        return menu.item(
                                            PopupMenuItem::new("No playlists yet").disabled(true),
                                        );
                                    }
                                    for (id, title) in plans.clone() {
                                        let model = model.clone();
                                        menu = menu.item(PopupMenuItem::new(title).on_click(
                                            move |_, _, cx| {
                                                model.update(cx, |m, cx| {
                                                    m.add_picked_to_playlist(&id, cx)
                                                })
                                            },
                                        ));
                                    }
                                    menu
                                }),
                        )
                        .action(
                            SelectionAction::new(ids::PICKED_SAVE, "Save to library")
                                .icon(KairoIcon::BookOpen)
                                .on_click(move |_, _, cx| {
                                    save.update(cx, |m, cx| m.save_picked_to_library(cx))
                                }),
                        )
                        .on_clear(move |_, _, cx| clear.update(cx, |m, cx| m.clear_picks(cx))),
                )
                .into_any_element(),
        )
    }

    fn render_dock(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let k = *cx.kairo();
        let m = self.model.read(cx);
        if m.rows().is_empty() {
            return None;
        }
        let model = self.model.clone();
        let count = rows::card_count(m.rows());
        let card = rows::card_at(m.rows(), m.active()).cloned();
        let navigating = m.navigating();
        let scope = m.scope();
        let opening = m.opening_chapter();
        let in_search = m.source() == Some(Source::Search);
        let plans: Vec<(String, String)> = m
            .plans()
            .iter()
            .map(|p| (p.id.clone(), p.title.clone()))
            .collect();
        let added = m.added_all().map(str::to_owned);
        let libraries: Vec<(String, String)> = m
            .libraries()
            .libraries
            .iter()
            .map(|l| (l.id.clone(), l.name.clone()))
            .collect();
        let scope_buttons = scope.map(|scope| {
            let (v, c) = (model.clone(), model.clone());
            h_flex()
                .id("scripture-scope")
                .role(Role::RadioGroup)
                .aria_label("Show the verse or its whole chapter")
                .rounded(radius::md())
                .bg(k.surface)
                .p_0p5()
                .child(
                    Button::new(ids::SCOPE_VERSE)
                        .xsmall()
                        .map(|b| {
                            if scope == Scope::Verse {
                                b.primary()
                            } else {
                                b.ghost()
                            }
                        })
                        .label("Verse")
                        .disabled(opening)
                        .tooltip("Only the selected verse")
                        .on_click(move |_, _, cx| {
                            if scope != Scope::Verse {
                                v.update(cx, |m, cx| m.show_verse_only(cx))
                            }
                        }),
                )
                .child(
                    Button::new(ids::SCOPE_CHAPTER)
                        .xsmall()
                        .map(|b| {
                            if scope == Scope::Chapter {
                                b.primary()
                            } else {
                                b.ghost()
                            }
                        })
                        .when(opening, |b| b.icon(Icon::new(KairoIcon::Loader)))
                        .label("Chapter")
                        .disabled(opening)
                        .tooltip("The whole chapter around it")
                        .on_click(move |_, _, cx| {
                            if scope != Scope::Chapter {
                                c.update(cx, |m, cx| m.show_chapter(cx))
                            }
                        }),
                )
        });
        let (prev, next, live, clear) =
            (model.clone(), model.clone(), model.clone(), model.clone());
        let reference = card.as_ref().map(|c| c.result.reference.clone());
        let save_menu = in_search.then(|| {
            let model = model.clone();
            Button::new(ids::SAVE)
                .ghost()
                .xsmall()
                .label("Save")
                .dropdown_caret(true)
                .disabled(m.creating())
                .popover_trigger()
                .dropdown_menu(move |mut menu, _, _| {
                    if let Some(title) = &added {
                        menu = menu.label(format!("Saved to {title}"));
                    }
                    for (id, title) in plans.clone() {
                        let model = model.clone();
                        menu = menu.item(PopupMenuItem::new(title).on_click(move |_, _, cx| {
                            model.update(cx, |m, cx| m.add_all_to_playlist(&id, cx))
                        }));
                    }
                    if !plans.is_empty() {
                        menu = menu.separator();
                    }
                    let create = model.clone();
                    menu = menu
                        .item(
                            PopupMenuItem::new("New playlist…")
                                .icon(Icon::new(KairoIcon::Plus))
                                .on_click(move |_, _, cx| {
                                    create.update(cx, |m, cx| m.create_playlist(true, cx))
                                }),
                        )
                        .separator()
                        .label("LIBRARY");
                    let default = model.clone();
                    menu = menu.item(
                        PopupMenuItem::new(LibraryKind::Scripture.default_name()).on_click(
                            move |_, _, cx| {
                                default.update(cx, |m, cx| {
                                    m.save_to_library(DEFAULT_LIBRARY_ID.into(), cx)
                                })
                            },
                        ),
                    );
                    for (id, name) in libraries.clone() {
                        let model = model.clone();
                        menu = menu.item(PopupMenuItem::new(name).on_click(move |_, _, cx| {
                            let id = id.clone();
                            model.update(cx, |m, cx| m.save_to_library(id, cx))
                        }));
                    }
                    menu
                })
        });
        Some(
            h_flex()
                .id(ids::DOCK)
                .test_support()
                .flex_none()
                .flex_wrap()
                .gap_2p5()
                .px_4()
                .py_2()
                .bg(k.glass_panel())
                .border_t_1()
                .border_color(k.panel_border)
                .child(
                    h_flex()
                        .min_w_0()
                        .text_size(text::LABEL.size)
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(k.text(TextStep::T200))
                                .child(count.to_string()),
                        )
                        .child(div().mx_1p5().text_color(k.text(TextStep::T600)).child("·"))
                        .child(
                            div()
                                .truncate()
                                .text_color(k.text(TextStep::T300))
                                .child(m.queue_label()),
                        ),
                )
                .child(
                    h_flex()
                        .ml_auto()
                        .flex_wrap()
                        .gap_1p5()
                        .child(self.zoom.clone())
                        .children(scope_buttons)
                        .child(
                            h_flex()
                                .rounded(radius::md())
                                .bg(k.surface)
                                .child(
                                    Button::new(ids::PREV)
                                        .ghost()
                                        .xsmall()
                                        .icon(Icon::new(
                                            if navigating == Some(Direction::Previous) {
                                                KairoIcon::Loader
                                            } else {
                                                KairoIcon::ChevronLeft
                                            },
                                        ))
                                        .label("Prev")
                                        .accessibility_label("Previous verse")
                                        .disabled(navigating.is_some())
                                        .on_click(move |_, _, cx| {
                                            prev.update(cx, |m, cx| {
                                                m.load_adjacent(Direction::Previous, cx)
                                            })
                                        }),
                                )
                                .child(
                                    Button::new(ids::NEXT)
                                        .ghost()
                                        .xsmall()
                                        .label("Next")
                                        .icon(Icon::new(if navigating == Some(Direction::Next) {
                                            KairoIcon::Loader
                                        } else {
                                            KairoIcon::ChevronRight
                                        }))
                                        .accessibility_label("Next verse")
                                        .disabled(navigating.is_some())
                                        .on_click(move |_, _, cx| {
                                            next.update(cx, |m, cx| {
                                                m.load_adjacent(Direction::Next, cx)
                                            })
                                        }),
                                ),
                        )
                        .child(
                            Button::new(ids::GO_LIVE)
                                .primary()
                                .xsmall()
                                .icon(Icon::new(KairoIcon::Send))
                                .label("Go live")
                                .accessibility_label(SharedString::from(format!(
                                    "Go live with {}",
                                    reference.unwrap_or_else(|| "verse".into())
                                )))
                                .disabled(
                                    card.as_ref()
                                        .is_none_or(|c| c.send_status == SendStatus::Sending),
                                )
                                .on_click(move |_, _, cx| {
                                    live.update(cx, |m, cx| {
                                        let index = m.active();
                                        m.send(index, cx)
                                    })
                                }),
                        )
                        .children(save_menu)
                        .child(quiet_button(ids::CLEAR, "Clear").on_click(move |_, _, cx| {
                            clear.update(cx, |m, cx| m.clear_results(cx))
                        })),
                )
                .into_any_element(),
        )
    }
}

impl Render for ScriptureWorkspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync(window, cx);
        let k = *cx.kairo();
        let sidebar = self.render_sidebar(window, cx);
        let bar = self.render_bar(cx);
        let notices = self.render_notices(cx);
        let grid = self.render_grid(cx);
        let selection = self.render_selection(cx);
        let dock = self.render_dock(cx);
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
                        h_flex()
                            .id(ids::PAGE)
                            .items_stretch()
                            .test_support()
                            .track_focus(&self.page_focus)
                            .on_key_down(cx.listener(Self::page_key))
                            .flex_1()
                            .h_full()
                            .min_w_0()
                            .min_h_0()
                            .overflow_hidden()
                            .bg(k.surface)
                            .child(sidebar)
                            .child(kairo_ui_kit::components::page_sidebar::handle(
                                "scripture-sidebar-handle",
                                None,
                                cx,
                            ))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .h_full()
                                    .min_w_0()
                                    .min_h_0()
                                    .child(bar)
                                    .children(notices)
                                    .child(grid)
                                    .children(selection)
                                    .children(dock),
                            ),
                    )
                    .children(self.media_dock.clone()),
            )
            .children(self.rail.clone())
            .children(self.review.as_ref().map(|(r, _)| r.clone()))
    }
}
