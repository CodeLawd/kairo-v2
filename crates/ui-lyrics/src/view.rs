//! The Lyrics page's library portion (`Lyrics.tsx` with `LibraryRail.tsx`):
//! the sources rail (libraries, Favorites, playlists), the song list with
//! its header menu, filter and web fallback, the right pane (web preview,
//! the selected song, or the empty state), the delete confirmation and the
//! song usage report. The live rail is the operator's, handed in by the app.
//! The slide grid replaces the song pane's section list in 081.

use gpui_kit::component::FocusTrapElement;
use kairo_ui_kit::components::PopoverTriggerExt;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt, DropdownMenu, PopupMenuItem};
use gpui_kit::component::{
    Disableable, Icon, Sizable, VirtualListScrollHandle, h_flex, v_flex, v_virtual_list,
};
use gpui_kit::{
    AnyElement, AnyView, App, AppContext, Context, DragMoveEvent, Entity, FocusHandle, Focusable,
    FontWeight, InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Render, Role,
    ScrollStrategy, SharedString, StatefulInteractiveElement, Styled, Subscription, Window,
    deferred, div, prelude::FluentBuilder, px, size,
};
use kairo_domain::collections::{LibrariesCommand, LibraryKind as DomainKind};
use kairo_domain::lyrics::{LyricsOnlineResult, LyricsReview};
use kairo_ui_kit::coach::{anchors, coach_anchor};
use kairo_ui_kit::components::{
    ConfirmRequest, LibraryDragItem, LibraryKind, LibraryRow, LibrarySection, LibrarySectionEvent,
    SelectionAction, SelectionBar, confirm, messages,
};
use kairo_ui_kit::tokens::{Hue, TextStep, radius, text};
use kairo_ui_kit::{KairoColors, KairoIcon, KairoThemeExt};

use crate::backend::LyricsBackend;
use crate::editor::{EditorEvent, SongEditor};
use crate::import::{ImportDialog, ImportEvent};
use crate::model::{Lyrics, LyricsEvent, PickGesture};
use crate::quick_open::{QuickOpen, section_block};
use crate::rows::{self, Filter, ListFooter, ReviewNotice, Sort};

mod song_view;
pub use song_view::{SlideDrag, ids as song_ids};

/// Element ids, for tests and the coach tour.
pub mod ids {
    pub const PAGE: &str = "lyrics-page";
    pub const LIST: &str = "lyrics-song-list";
    pub const COUNT: &str = "lyrics-song-count";
    pub const OPTIONS: &str = "lyrics-list-options";
    pub const NEW_SONG: &str = "lyrics-new";
    pub const QUERY: &str = "lyrics-query";
    pub const CLEAR_QUERY: &str = "lyrics-query-clear";
    pub const FAVORITES: &str = "lyrics-favorites";
    pub const PLAYLISTS: &str = "lyrics-playlists";
    pub const PLAYLIST_RENAME: &str = "lyrics-playlist-rename";
    pub const RAIL_ERROR: &str = "lyrics-rail-error";
    pub const ERROR: &str = "lyrics-error";
    pub const NO_SONGS: &str = "lyrics-no-songs";
    pub const SEARCHING: &str = "lyrics-searching";
    pub const NO_MATCH: &str = "lyrics-no-match";
    pub const SEARCH_ONLINE: &str = "lyrics-search-online";
    pub const WEB: &str = "lyrics-web";
    pub const WEB_ERROR: &str = "lyrics-web-error";
    pub const WEB_EMPTY: &str = "lyrics-web-empty";
    pub const EMPTY: &str = "lyrics-empty";
    pub const SONG: &str = "lyrics-song";
    pub const PREVIEW: &str = "lyrics-web-preview";
    pub const PREVIEW_IMPORT: &str = "lyrics-web-preview-import";
    pub const PREVIEW_OPEN_EXISTING: &str = "lyrics-web-preview-open";
    pub const PICKED_REMOVE: &str = "lyrics-picked-remove";
    pub const DROP_END: &str = "lyrics-drop-end";
    pub const USAGE: &str = "lyrics-usage";
    pub const USAGE_FROM: &str = "lyrics-usage-from";
    pub const USAGE_TO: &str = "lyrics-usage-to";
    pub const USAGE_EXPORT: &str = "lyrics-usage-export";
    pub const USAGE_CLOSE: &str = "lyrics-usage-close";
    pub const USAGE_EMPTY: &str = "lyrics-usage-empty";
    pub const USAGE_FOOTER: &str = "lyrics-usage-footer";
    pub const EDIT: &str = "lyrics-edit";
    pub const EDIT_CANCEL: &str = "lyrics-edit-cancel";
    pub const EDIT_SAVE: &str = "lyrics-edit-save";
    pub const TRANSLATE_ERROR: &str = "lyrics-translate-error";

    pub fn song(row: usize) -> String {
        format!("lyrics-song-{row}")
    }
    pub fn favorite(row: usize) -> String {
        format!("lyrics-song-{row}-favorite")
    }
    pub fn remove(row: usize) -> String {
        format!("lyrics-song-{row}-remove")
    }
    pub fn more(row: usize) -> String {
        format!("lyrics-song-{row}-more")
    }
    pub fn playlist(id: &str) -> String {
        format!("lyrics-playlist-{id}")
    }
    pub fn web(index: usize) -> String {
        format!("lyrics-web-{index}")
    }
    pub fn web_action(index: usize) -> String {
        format!("lyrics-web-{index}-action")
    }
}

const ROW_HEIGHT: f32 = 32.;

/// `LyricsReviewNotice`, shared with quick open.
pub(crate) fn review_notice_element(review: Option<&LyricsReview>, cx: &App) -> Option<AnyElement> {
    let k = *cx.kairo();
    Some(match rows::review_notice(review)? {
        ReviewNotice::Reviewed(line) => div()
            .text_size(text::LABEL.size)
            .text_color(k.text(TextStep::T500))
            .child(line)
            .into_any_element(),
        ReviewNotice::Unverified { title, detail } => v_flex()
            .id("lyrics-review-notice")
            .role(Role::Status)
            .rounded(radius::lg())
            .bg(k.surface_tertiary)
            .px_3()
            .py_2p5()
            .text_size(text::XS.size)
            .text_color(k.text(TextStep::T400))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(k.text(TextStep::T200))
                    .child(title),
            )
            .child(div().mt_1().child(detail))
            .into_any_element(),
    })
}

pub struct LyricsWorkspace {
    model: Entity<Lyrics>,
    rail: Option<AnyView>,
    /// The media dock under the page (`BoothWorkspace`'s `MediaDock`).
    media_dock: Option<AnyView>,
    query: Entity<InputState>,
    rename: Entity<InputState>,
    renaming_for: Option<String>,
    usage_from: Entity<InputState>,
    usage_to: Entity<InputState>,
    usage_open: bool,
    library: Entity<LibrarySection>,
    library_rows: Vec<LibraryRow>,
    scroll: VirtualListScrollHandle,
    list_focus: FocusHandle,
    drop_over: Option<String>,
    drop_end: bool,
    quick_open: Option<(Entity<QuickOpen>, Subscription)>,
    song: song_view::SongControls,
    song_focus: FocusHandle,
    editor: Option<(Entity<SongEditor>, Subscription)>,
    import: Option<(Entity<ImportDialog>, Subscription)>,
    import_focus: kairo_ui_kit::components::ModalFocus,
    usage_focus: kairo_ui_kit::components::ModalFocus,
    _subscriptions: Vec<Subscription>,
}

impl LyricsWorkspace {
    pub fn new(
        backend: Arc<dyn LyricsBackend>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let model = cx.new(|cx| Lyrics::new(backend, cx));
        Self::with_model(model, window, cx)
    }

    pub fn with_model(model: Entity<Lyrics>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query =
            cx.new(|cx| InputState::new(window, cx).placeholder("Filter, or search the web"));
        let rename = cx.new(|cx| InputState::new(window, cx));
        let usage_from = cx.new(|cx| InputState::new(window, cx).placeholder("yyyy-mm-dd"));
        let usage_to = cx.new(|cx| InputState::new(window, cx).placeholder("yyyy-mm-dd"));
        let library = cx.new(|_| {
            LibrarySection::new(LibraryKind::Songs)
                .banded(true)
                .context_selection(true)
                .with_add_items(vec![kairo_ui_kit::components::AddItem {
                    label: "New playlist".into(),
                    icon: Some(KairoIcon::ListMusic),
                }])
        });
        let mut subs = vec![cx.observe(&model, |_, _, cx| cx.notify())];
        subs.push(
            cx.subscribe(&query, |this, _, event: &InputEvent, cx| match event {
                InputEvent::Change => {
                    let value = this.query.read(cx).value().to_string();
                    this.model.update(cx, |m, cx| m.set_query(value, cx));
                }
                InputEvent::PressEnter { .. } => {
                    // ↵ in the filter asks the web when nothing local matches.
                    this.model.update(cx, |m, cx| {
                        if m.footer() == ListFooter::NoMatchSearchOnline {
                            m.search_online(cx)
                        }
                    });
                }
                _ => {}
            }),
        );
        subs.push(
            cx.subscribe(&rename, |this, _, event: &InputEvent, cx| match event {
                InputEvent::PressEnter { .. } | InputEvent::Blur => {
                    let draft = this.rename.read(cx).value().to_string();
                    this.model
                        .update(cx, |m, cx| m.commit_rename_setlist(&draft, cx));
                }
                _ => {}
            }),
        );
        for input in [&usage_from, &usage_to] {
            subs.push(cx.subscribe(input, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change | InputEvent::PressEnter { .. }) {
                    let from = this.usage_from.read(cx).value().to_string();
                    let to = this.usage_to.read(cx).value().to_string();
                    this.model
                        .update(cx, |m, cx| m.set_usage_range(&from, &to, cx));
                }
            }));
        }
        subs.push(
            cx.subscribe(&library, |this, _, event: &LibrarySectionEvent, cx| {
                this.library_event(event.clone(), cx)
            }),
        );
        let weak = cx.entity().downgrade();
        let rename_focus = rename.read(cx).focus_handle(cx);
        subs.push(cx.intercept_keystrokes(move |event, window, cx| {
            let key = event.keystroke.key.as_str();
            if key == "escape" && rename_focus.is_focused(window) {
                let _ = weak.update(cx, |this, cx| {
                    this.model.update(cx, |m, cx| m.cancel_rename_setlist(cx))
                });
                cx.stop_propagation();
                return;
            }
            if key == "escape" {
                let closed = weak
                    .update(cx, |this, cx| {
                        let open = this.model.read(cx).usage().is_some();
                        if open {
                            this.model.update(cx, |m, cx| m.close_usage(cx));
                        }
                        open
                    })
                    .unwrap_or(false);
                if closed {
                    cx.stop_propagation();
                }
            }
        }));
        let song = song_view::SongControls::new(&model, window, cx);
        subs.push(cx.subscribe_in(
            &model,
            window,
            |this, _, event: &LyricsEvent, window, cx| {
                if let LyricsEvent::ConfirmDiscard = event {
                    let model = this.model.clone();
                    confirm(
                        messages::lyrics_discard_changes(),
                        window,
                        cx,
                        move |ok, _, cx| {
                            model.update(cx, |m, cx| {
                                if ok {
                                    m.discard_and_continue(cx)
                                } else {
                                    m.keep_editing(cx)
                                }
                            })
                        },
                    );
                }
            },
        ));
        Self {
            song,
            editor: None,
            import: None,
            import_focus: kairo_ui_kit::components::ModalFocus::new(cx),
            usage_focus: kairo_ui_kit::components::ModalFocus::new(cx),
            song_focus: cx.focus_handle(),
            model,
            rail: None,
            media_dock: None,
            query,
            rename,
            renaming_for: None,
            usage_from,
            usage_to,
            usage_open: false,
            library,
            library_rows: Vec::new(),
            scroll: VirtualListScrollHandle::new(),
            list_focus: cx.focus_handle(),
            drop_over: None,
            drop_end: false,
            quick_open: None,
            _subscriptions: subs,
        }
    }

    pub fn model(&self) -> &Entity<Lyrics> {
        &self.model
    }

    /// The song view's focus: arrows, hotkeys and Esc answer while it has it.
    pub fn song_focus(&self) -> &FocusHandle {
        &self.song_focus
    }

    /// The song list's focus: ↑/↓, ⌘A, Delete and Esc answer while it has it.
    pub fn list_focus(&self) -> &FocusHandle {
        &self.list_focus
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

    /// Opens the ⌘F finder over this page (tests; the app shows it as the
    /// shell's song-search overlay).
    pub fn open_quick_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let model = self.model.clone();
        let finder = cx.new(|cx| QuickOpen::new(model, window, cx));
        let sub = cx.subscribe(
            &finder,
            |this, _, _: &crate::quick_open::QuickOpenEvent, cx| {
                this.quick_open = None;
                cx.notify();
            },
        );
        self.quick_open = Some((finder, sub));
        cx.notify();
    }

    /// The song editor while one is open.
    pub fn editor(&self) -> Option<&Entity<SongEditor>> {
        self.editor.as_ref().map(|(e, _)| e)
    }

    /// The Import Song dialog while open.
    pub fn import_dialog(&self) -> Option<&Entity<ImportDialog>> {
        self.import.as_ref().map(|(d, _)| d)
    }

    /// The editor and the import dialog follow the model.
    fn sync_overlays(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (editing, key, import) = {
            let m = self.model.read(cx);
            (
                m.editing().is_some(),
                m.editing().map(|e| (e.new, e.song_id.clone())),
                m.import_open(),
            )
        };
        let _ = key;
        if editing && self.editor.is_none() {
            let model = self.model.clone();
            let editor = cx.new(|cx| SongEditor::new(model, window, cx));
            let sub = cx.subscribe(&editor, |this, _, event: &EditorEvent, cx| match event {
                EditorEvent::Save => this.model.update(cx, |m, cx| m.save_edit(cx)),
            });
            self.editor = Some((editor, sub));
        } else if !editing {
            self.editor = None;
        }
        // The trigger keeps focus until here, so it gets it back on close.
        self.import_focus.sync(import, window, cx);
        if import && self.import.is_none() {
            let model = self.model.clone();
            let handle = self.import_focus.handle().clone();
            let dialog = cx.new(|cx| {
                let mut dialog = ImportDialog::new(model, window, cx);
                dialog.set_modal_focus(handle);
                dialog
            });
            let sub = cx.subscribe(&dialog, |this, _, event: &ImportEvent, cx| match event {
                ImportEvent::Imported(songs) => {
                    let songs = songs.clone();
                    this.model.update(cx, |m, cx| m.import_finished(songs, cx));
                }
                ImportEvent::Close => this.model.update(cx, |m, cx| m.close_import(cx)),
            });
            self.import = Some((dialog, sub));
        } else if !import {
            self.import = None;
        }
    }

    pub fn quick_open(&self) -> Option<&Entity<QuickOpen>> {
        self.quick_open.as_ref().map(|(q, _)| q)
    }

    fn library_event(&mut self, event: LibrarySectionEvent, cx: &mut Context<Self>) {
        let kind = DomainKind::Songs;
        let command = match event {
            LibrarySectionEvent::Select(id) => {
                self.model
                    .update(cx, |m, cx| m.select_library(id.to_string(), cx));
                return;
            }
            LibrarySectionEvent::AddItem(_) => {
                self.model.update(cx, |m, cx| m.create_setlist(cx));
                return;
            }
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
        let section = self.library.downgrade();
        self.model.update(cx, |m, cx| {
            m.library_command(command, cx, move |m, created, cx| {
                let _ = section.update(cx, |s, cx| {
                    s.set_error(m.rail_error().map(|e| e.to_owned().into()), cx);
                    if let Some(id) = assigned {
                        s.landed(id, cx);
                    }
                });
                if create && let Some(id) = created {
                    let _ = section.update(cx, |s, cx| s.set_active(id, cx));
                }
            })
        });
    }

    /// The model's state into the controls that hold their own.
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let m = self.model.read(cx);
        let query = m.query().to_owned();
        let rows: Vec<LibraryRow> = {
            let counts = m.library_counts();
            m.libraries()
                .libraries
                .iter()
                .map(|l| LibraryRow {
                    id: l.id.clone().into(),
                    name: l.name.clone().into(),
                    count: counts.get(&l.id).copied().unwrap_or(0),
                })
                .collect()
        };
        let default_count = m.songs().len();
        let active: SharedString = if m.viewing_setlist_id().is_some() || m.filter() != Filter::All
        {
            "".into()
        } else {
            m.active_library().to_owned().into()
        };
        let renaming = m.renaming_setlist().map(str::to_owned);
        let rename_name = renaming.as_ref().and_then(|id| {
            m.setlists()
                .lists
                .iter()
                .find(|l| &l.id == id)
                .map(|l| l.name.clone())
        });
        let usage = m.usage().map(|u| (u.from.clone(), u.to.clone()));
        if self.query.read(cx).value().as_ref() != query {
            self.query
                .update(cx, |s, cx| s.set_value(query, window, cx));
        }
        if rows != self.library_rows {
            self.library_rows = rows.clone();
            self.library
                .update(cx, |s, cx| s.set_libraries(rows, default_count, cx));
        }
        self.library.update(cx, |s, cx| s.set_active(active, cx));
        if renaming != self.renaming_for {
            if let Some(name) = rename_name {
                self.rename.update(cx, |s, cx| {
                    s.set_value(name, window, cx);
                    s.focus(window, cx);
                });
            }
            self.renaming_for = renaming;
        }
        match usage {
            Some((from, to)) if !self.usage_open => {
                self.usage_open = true;
                self.usage_from
                    .update(cx, |s, cx| s.set_value(from, window, cx));
                self.usage_to
                    .update(cx, |s, cx| s.set_value(to, window, cx));
            }
            None => self.usage_open = false,
            _ => {}
        }
    }

    // ─── Actions ────────────────────────────────────────────────────────────

    fn request_delete(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(title) = self.model.read(cx).song(&id).map(|s| s.title.clone()) else {
            return;
        };
        self.model.update(cx, |m, cx| m.ask_delete(id, cx));
        let model = self.model.clone();
        confirm(
            ConfirmRequest::new("Delete Song", "Delete")
                .detail(format!("Delete \"{title}\"? This cannot be undone."))
                .destructive(),
            window,
            cx,
            move |ok, _, cx| {
                model.update(cx, |m, cx| {
                    if ok {
                        m.confirm_delete(cx)
                    } else {
                        m.cancel_delete(cx)
                    }
                })
            },
        );
    }

    fn remove_picked(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let m = self.model.read(cx);
        let count = m.picked_ids().len();
        let Some(name) = m.viewing_setlist().map(|l| l.name.clone()) else {
            return;
        };
        if count == 0 {
            return;
        }
        let model = self.model.clone();
        if count == 1 {
            model.update(cx, |m, cx| m.remove_picked(cx));
            return;
        }
        confirm(
            messages::lyrics_remove_songs(count, &name),
            window,
            cx,
            move |ok, _, cx| {
                if ok {
                    model.update(cx, |m, cx| m.remove_picked(cx));
                }
            },
        );
    }

    fn list_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        let in_setlist = self.model.read(cx).viewing_setlist_id().is_some();
        match k.key.as_str() {
            "down" | "up" => {
                let down = k.key == "down";
                if let Some(row) = self.model.update(cx, |m, cx| m.step_selection(down, cx)) {
                    self.scroll.scroll_to_item(row, ScrollStrategy::Center);
                }
            }
            "a" if k.modifiers.platform && in_setlist => {
                self.model.update(cx, |m, cx| m.select_all_picks(cx))
            }
            "backspace" | "delete" if in_setlist => self.remove_picked(window, cx),
            "escape" if in_setlist => self.model.update(cx, |m, cx| m.clear_picks(cx)),
            _ => return,
        }
        cx.stop_propagation();
    }

    // ─── Sidebar ────────────────────────────────────────────────────────────

    fn render_playlists(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let k = *cx.kairo();
        let m = self.model.read(cx);
        let lists = m.setlists().lists.clone();
        let dragging = cx.has_active_drag();
        // With no setlists the section stays out of the way, except while a
        // song is dragged, so dropping one still starts a list.
        if lists.is_empty() && !dragging {
            return None;
        }
        let model = self.model.clone();
        let viewing = m.viewing_setlist_id().map(str::to_owned);
        let renaming = m.renaming_setlist().map(str::to_owned);
        let landed = m.landed_setlist().map(str::to_owned);
        let drop_label = m
            .drop_setlist()
            .map(|l| format!("Add to {}", l.name))
            .unwrap_or_else(|| "Start a playlist".into());
        let section_drop = self.drop_over.as_deref() == Some("");
        let section_model = model.clone();
        let rows: Vec<AnyElement> = if lists.is_empty() {
            vec![
                div()
                    .px_2()
                    .py_2()
                    .text_size(text::LABEL.size)
                    .text_color(k.text(TextStep::T500))
                    .child("Drop here to start a playlist")
                    .into_any_element(),
            ]
        } else {
            lists
                .iter()
                .map(|list| {
                    if renaming.as_deref() == Some(list.id.as_str()) {
                        return div()
                            .id(ids::PLAYLIST_RENAME)
                            .test_support()
                            .mb_0p5()
                            .child(Input::new(&self.rename).small().aria_label("Playlist name"))
                            .into_any_element();
                    }
                    let selected = viewing.as_deref() == Some(list.id.as_str());
                    let over = self.drop_over.as_deref() == Some(list.id.as_str());
                    let landed = landed.as_deref() == Some(list.id.as_str());
                    let id = list.id.clone();
                    let (select, rename, drop, menu_model) =
                        (model.clone(), model.clone(), model.clone(), model.clone());
                    let (sid, rid, did, mid) = (id.clone(), id.clone(), id.clone(), id.clone());
                    let hover_id = id.clone();
                    let count = list.song_ids.len();
                    h_flex()
                        .id(SharedString::from(ids::playlist(&id)))
                        .test_support()
                        .role(Role::Button)
                        .aria_label(SharedString::from(list.name.clone()))
                        .w_full()
                        .gap_2()
                        .rounded(radius::md())
                        .px_2()
                        .py_1p5()
                        .text_size(text::XS.size)
                        .map(|d| {
                            if over {
                                d.bg(k.surface_border).text_color(k.text_primary)
                            } else if landed || selected {
                                d.bg(k.surface_elevated).text_color(k.text_primary)
                            } else {
                                d.text_color(k.text(TextStep::T400))
                                    .hover(|s| s.bg(k.white.opacity(0.05)))
                            }
                        })
                        .on_click(move |event, _, cx| {
                            let id = sid.clone();
                            if event.click_count() >= 2 {
                                rename.update(cx, |m, cx| m.begin_rename_setlist(rid.clone(), cx))
                            } else {
                                select.update(cx, |m, cx| m.select_setlist(id, cx))
                            }
                        })
                        .on_drag_move(cx.listener(
                            move |this, event: &DragMoveEvent<LibraryDragItem>, _, cx| {
                                let inside = event.bounds.contains(&event.event.position);
                                let was = this.drop_over.as_deref() == Some(hover_id.as_str());
                                if inside && !was {
                                    this.drop_over = Some(hover_id.clone());
                                    cx.notify();
                                } else if was && !inside {
                                    this.drop_over = None;
                                    cx.notify();
                                }
                            },
                        ))
                        .can_drop(|dragged, _, _| {
                            dragged
                                .downcast_ref::<LibraryDragItem>()
                                .is_some_and(|i| i.kind == LibraryKind::Songs)
                        })
                        .on_drop(cx.listener(move |this, item: &LibraryDragItem, _, cx| {
                            this.drop_over = None;
                            let (list, song) = (did.clone(), item.item_id.to_string());
                            drop.update(cx, |m, cx| m.drop_on_setlist(list, song, cx));
                            cx.stop_propagation();
                        }))
                        .context_menu(move |menu, _, _| {
                            let (a, b, c) =
                                (menu_model.clone(), menu_model.clone(), menu_model.clone());
                            let (ia, ib, ic) = (mid.clone(), mid.clone(), mid.clone());
                            menu.item(
                                PopupMenuItem::new("Rename")
                                    .icon(Icon::new(KairoIcon::Edit2))
                                    .on_click(move |_, _, cx| {
                                        a.update(cx, |m, cx| m.begin_rename_setlist(ia.clone(), cx))
                                    }),
                            )
                            .item(
                                PopupMenuItem::new("Export as .kairo file…")
                                    .icon(Icon::new(KairoIcon::Upload))
                                    .on_click(move |_, _, cx| {
                                        b.update(cx, |m, cx| m.export_setlist(ib.clone(), cx))
                                    }),
                            )
                            .item(
                                PopupMenuItem::new("Delete")
                                    .icon(Icon::new(KairoIcon::Trash2))
                                    .on_click(move |_, _, cx| {
                                        c.update(cx, |m, cx| m.delete_setlist(ic.clone(), cx))
                                    }),
                            )
                        })
                        .child(
                            Icon::new(KairoIcon::ListMusic.fill())
                                .size_3()
                                .text_color(k.hue(Hue::Violet)),
                        )
                        .child(div().flex_1().min_w_0().truncate().child(list.name.clone()))
                        .child(if over {
                            div()
                                .text_size(text::MICRO.size)
                                .text_color(k.text(TextStep::T300))
                                .child("Add here")
                                .into_any_element()
                        } else if landed {
                            Icon::new(KairoIcon::Check)
                                .size_3()
                                .text_color(k.text(TextStep::T200))
                                .into_any_element()
                        } else {
                            div()
                                .text_size(text::MICRO.size)
                                .text_color(k.text(TextStep::T600))
                                .child(count.to_string())
                                .into_any_element()
                        })
                        .into_any_element()
                })
                .collect()
        };
        Some(
            v_flex()
                .id(ids::PLAYLISTS)
                .test_support()
                .rounded(radius::lg())
                .when(section_drop, |d| d.bg(k.white.opacity(0.05)))
                .on_drag_move(cx.listener(
                    move |this, event: &DragMoveEvent<LibraryDragItem>, _, cx| {
                        let inside = event.bounds.contains(&event.event.position);
                        if inside && this.drop_over.is_none() {
                            this.drop_over = Some(String::new());
                            cx.notify();
                        } else if !inside && this.drop_over.as_deref() == Some("") {
                            this.drop_over = None;
                            cx.notify();
                        }
                    },
                ))
                .can_drop(|dragged, _, _| {
                    dragged
                        .downcast_ref::<LibraryDragItem>()
                        .is_some_and(|i| i.kind == LibraryKind::Songs)
                })
                .on_drop(cx.listener(move |this, item: &LibraryDragItem, _, cx| {
                    this.drop_over = None;
                    let song = item.item_id.to_string();
                    section_model.update(cx, |m, cx| m.drop_on_setlists(song, cx));
                }))
                .child(
                    h_flex()
                        .px_1()
                        .pt_2()
                        .pb_1()
                        .text_size(text::MICRO.size)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(k.text(TextStep::T500))
                        .child(div().flex_1().child("Playlists"))
                        .when(section_drop, |d| {
                            d.child(div().text_color(k.text(TextStep::T300)).child(drop_label))
                        }),
                )
                .children(rows)
                .into_any_element(),
        )
    }

    fn render_sources(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let m = self.model.read(cx);
        let favorites_on = m.viewing_setlist_id().is_none() && m.filter() == Filter::Favorites;
        let favorites = m.favorite_count();
        let model = self.model.clone();
        let rail_error = m.rail_error().map(str::to_owned);
        let favorites_row = h_flex()
            .id(ids::FAVORITES)
            .test_support()
            .role(Role::Button)
            .aria_label("Favorites")
            .gap_2()
            .rounded(radius::md())
            .px_2()
            .py_1p5()
            .text_size(text::XS.size)
            .map(|d| {
                if favorites_on {
                    d.bg(k.surface_elevated).text_color(k.text_primary)
                } else {
                    d.text_color(k.text(TextStep::T400))
                        .hover(|s| s.bg(k.white.opacity(0.05)))
                }
            })
            .on_click(move |_, _, cx| {
                model.update(cx, |m, cx| m.select_filter(Filter::Favorites, cx))
            })
            .child(
                Icon::new(KairoIcon::Star.fill())
                    .size_3()
                    .text_color(k.hue(Hue::Violet)),
            )
            .child(div().flex_1().truncate().child("Favorites"))
            .child(
                div()
                    .text_size(text::MICRO.size)
                    .text_color(k.text(TextStep::T600))
                    .child(favorites.to_string()),
            );
        v_flex()
            .flex_none()
            .gap_1()
            .px_1()
            .pt_2()
            .pb_2()
            .bg(k.surface_secondary)
            .child(self.library.clone())
            .child(favorites_row)
            .children(self.render_playlists(cx))
            .children(rail_error.map(|e| {
                div()
                    .id(ids::RAIL_ERROR)
                    .test_support()
                    .role(Role::Alert)
                    .px_2()
                    .text_size(text::MICRO.size)
                    .text_color(k.danger_text)
                    .child(e)
            }))
            .into_any_element()
    }

    fn render_band(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let m = self.model.read(cx);
        let count = rows::count_label(m.shown().len(), "song");
        let sort = m.sort();
        let viewing = m.viewing_setlist_id().map(str::to_owned);
        let model = self.model.clone();
        let options = Button::new(ids::OPTIONS)
            .ghost()
            .xsmall()
            .icon(Icon::new(KairoIcon::MoreHorizontal))
            .accessibility_label("Song list options")
            .tooltip("Sort, import and export")
            .popover_trigger()
            .dropdown_menu({
                let model = model.clone();
                move |mut menu, _, _| {
                    menu = menu.label("Sort by");
                    for s in Sort::ALL {
                        let m = model.clone();
                        menu = menu.item(
                            PopupMenuItem::new(s.label())
                                .checked(sort == s)
                                .on_click(move |_, _, cx| m.update(cx, |m, cx| m.set_sort(s, cx))),
                        );
                    }
                    let (import, songs, list, all, usage) = (
                        model.clone(),
                        model.clone(),
                        model.clone(),
                        model.clone(),
                        model.clone(),
                    );
                    let list_id = viewing.clone();
                    let songs_import = model.clone();
                    menu = menu
                        .separator()
                        .item(
                            PopupMenuItem::new("Import songs…")
                                .icon(Icon::new(KairoIcon::Download))
                                .on_click(move |_, _, cx| {
                                    songs_import.update(cx, |m, cx| m.open_import(cx))
                                }),
                        )
                        .item(
                            PopupMenuItem::new("Import .kairo file…")
                                .icon(Icon::new(KairoIcon::Download))
                                .on_click(move |_, _, cx| {
                                    import.update(cx, |m, cx| m.import_kairo(cx))
                                }),
                        )
                        .separator()
                        .item(
                            PopupMenuItem::new("Export songs…")
                                .icon(Icon::new(KairoIcon::Upload))
                                .on_click(move |_, _, cx| {
                                    songs.update(cx, |m, cx| m.export_songs(cx))
                                }),
                        );
                    if let Some(id) = list_id {
                        menu = menu.item(
                            PopupMenuItem::new("Export this playlist…")
                                .icon(Icon::new(KairoIcon::Upload))
                                .on_click(move |_, _, cx| {
                                    list.update(cx, |m, cx| m.export_setlist(id.clone(), cx))
                                }),
                        );
                    }
                    menu.item(
                        PopupMenuItem::new("Export whole library…")
                            .icon(Icon::new(KairoIcon::Upload))
                            .on_click(move |_, _, cx| all.update(cx, |m, cx| m.export_library(cx))),
                    )
                    .separator()
                    .item(
                        PopupMenuItem::new("Song usage report…")
                            .icon(Icon::new(KairoIcon::ListMusic))
                            .on_click(move |_, _, cx| usage.update(cx, |m, cx| m.open_usage(cx))),
                    )
                }
            });
        let new = model.clone();
        h_flex()
            .flex_none()
            .h_8()
            .gap_1()
            .pl_3()
            .pr_1()
            .bg(k.surface_tertiary.opacity(0.5))
            .text_size(text::LABEL.size)
            .text_color(k.text(TextStep::T400))
            .child(
                div()
                    .id(ids::COUNT)
                    .test_support()
                    .flex_1()
                    .min_w_0()
                    .child(count),
            )
            .child(options)
            .child(
                div()
                    .relative()
                    .child(
                        Button::new(ids::NEW_SONG)
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(KairoIcon::Plus))
                            .accessibility_label("New song")
                            .tooltip("New song")
                            .on_click(move |_, _, cx| new.update(cx, |m, cx| m.new_song(cx))),
                    )
                    .child(coach_anchor(anchors::LYRICS_NEW)),
            )
            .into_any_element()
    }

    fn render_song_row(&self, row: usize, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let m = self.model.read(cx);
        let Some(song) = m.shown_song(row).cloned() else {
            return div().into_any_element();
        };
        let in_setlist = m.viewing_setlist_id().is_some();
        let selected = m.selected_id() == Some(song.id.as_str());
        let picked = in_setlist && m.picks().contains(&song.id);
        let favorite = song.is_favorite == Some(true);
        let model = self.model.clone();
        let tip = [
            Some(song.title.clone()),
            Some(song.artist.clone()).filter(|a| !a.is_empty()),
            song.ccli_number
                .as_ref()
                .filter(|c| !c.is_empty())
                .map(|c| format!("CCLI #{c}")),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
        let drag = LibraryDragItem {
            item_id: song.id.clone().into(),
            label: song.title.clone().into(),
            kind: LibraryKind::Songs,
        };
        let (click, fav, remove, more_model) =
            (model.clone(), model.clone(), model.clone(), model.clone());
        let (cid, fid, rid) = (song.id.clone(), song.id.clone(), song.id.clone());
        let entity = cx.entity();
        let menu_song = song.clone();
        let half = |before: bool, cx: &mut Context<Self>| {
            let target = if before { row } else { row + 1 };
            let m = model.clone();
            div()
                .absolute()
                .left_0()
                .right_0()
                .h_1_2()
                .when(before, |d| d.top_0())
                .when(!before, |d| d.bottom_0())
                .drag_over::<LibraryDragItem>(move |s, _, _, cx| {
                    let k = cx.kairo();
                    if before {
                        s.border_t_2().border_color(k.white.opacity(0.85))
                    } else {
                        s.border_b_2().border_color(k.white.opacity(0.85))
                    }
                })
                .on_drop(cx.listener(move |_, item: &LibraryDragItem, _, cx| {
                    let song = item.item_id.to_string();
                    m.update(cx, |m, cx| m.drop_into_setlist(song, target, cx));
                    cx.stop_propagation();
                }))
        };
        let (top, bottom) = if in_setlist {
            (Some(half(true, cx)), Some(half(false, cx)))
        } else {
            (None, None)
        };
        h_flex()
            .id(SharedString::from(ids::song(row)))
            .test_support()
            .role(Role::Button)
            .aria_label(SharedString::from(song.title.clone()))
            .tooltip({
                let tip = SharedString::from(tip);
                move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                }
            })
            .group("song-row")
            .relative()
            .h(px(ROW_HEIGHT))
            .w_full()
            .gap_2()
            .px_2()
            .rounded(radius::md())
            .map(|d| {
                if picked {
                    d.bg(k.row_selected())
                } else if selected {
                    d.bg(k.surface_elevated)
                } else {
                    d.hover(|s| s.bg(k.white.opacity(0.05)))
                }
            })
            .on_drag(drag, |drag, _, _, cx| cx.new(|_| drag.clone()))
            .on_click(move |event, window, cx| {
                let id = cid.clone();
                let m = event.modifiers();
                let gesture = if m.platform || m.control {
                    Some(PickGesture::Toggle)
                } else if m.shift {
                    Some(PickGesture::Range)
                } else {
                    None
                };
                click.update(cx, |model, cx| match gesture {
                    Some(g) if in_setlist => model.pick_song(id, g, cx),
                    _ => model.select_song(id, cx),
                });
                let _ = window;
            })
            .context_menu(move |menu, _, _| song_menu(menu, &more_model, &menu_song, &entity))
            .child(if in_setlist {
                div()
                    .w_4()
                    .flex_none()
                    .text_size(text::LABEL.size)
                    .text_color(k.text(TextStep::T500))
                    .child((row + 1).to_string())
                    .into_any_element()
            } else {
                Icon::new(KairoIcon::Music2)
                    .size_3()
                    .text_color(if selected {
                        k.text(TextStep::T300)
                    } else {
                        k.text(TextStep::T600)
                    })
                    .into_any_element()
            })
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1p5()
                    .items_baseline()
                    .child(
                        div()
                            .truncate()
                            .text_size(text::SM.size)
                            .text_color(if selected {
                                k.text_primary
                            } else {
                                k.text(TextStep::T200)
                            })
                            .child(song.title.clone()),
                    )
                    .when(!song.artist.is_empty(), |d| {
                        d.child(
                            div()
                                .truncate()
                                .text_size(text::LABEL.size)
                                .text_color(k.text(TextStep::T500))
                                .child(song.artist.clone()),
                        )
                    }),
            )
            .child(
                div()
                    .id(SharedString::from(ids::favorite(row)))
                    .test_support()
                    .role(Role::Button)
                    .aria_label(if favorite {
                        "Remove from favorites"
                    } else {
                        "Add to favorites"
                    })
                    .flex_none()
                    .p_0p5()
                    .rounded(radius::sm())
                    .map(|d| {
                        if favorite {
                            d.text_color(k.favorite)
                        } else {
                            d.text_color(k.text(TextStep::T600))
                                .opacity(0.)
                                .group_hover("song-row", |s| s.opacity(1.))
                                .hover(|s| s.text_color(k.favorite))
                        }
                    })
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        fav.update(cx, |m, cx| m.toggle_favorite(fid.clone(), cx))
                    })
                    .child(Icon::new(KairoIcon::Star.weight(favorite)).size_3()),
            )
            .when(in_setlist, |d| {
                d.child(
                    div()
                        .id(SharedString::from(ids::remove(row)))
                        .test_support()
                        .role(Role::Button)
                        .aria_label(SharedString::from(format!(
                            "Remove {} from this playlist",
                            song.title
                        )))
                        .flex_none()
                        .p_0p5()
                        .opacity(0.)
                        .group_hover("song-row", |s| s.opacity(1.))
                        .text_color(k.text(TextStep::T600))
                        .hover(|s| s.text_color(k.danger_text))
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            remove.update(cx, |m, cx| m.remove_from_setlist(rid.clone(), cx))
                        })
                        .child(Icon::new(KairoIcon::X).size_3()),
                )
            })
            .children(top)
            .children(bottom)
            .into_any_element()
    }

    fn render_web(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let m = self.model.read(cx);
        let online = m.online().clone();
        let importing = m.importing_online().map(str::to_owned);
        let previewing = m.web_preview().map(|p| p.result.id.clone());
        let model = self.model.clone();
        let rows: Vec<AnyElement> = online
            .results
            .iter()
            .enumerate()
            .map(|(i, r)| {
                web_row(
                    k,
                    i,
                    r,
                    previewing.as_deref() == Some(r.id.as_str()),
                    importing.as_deref() == Some(r.id.as_str()),
                    importing.is_some(),
                    &model,
                )
            })
            .collect();
        v_flex()
            .id(ids::WEB)
            .test_support()
            .pt_3()
            .mt_2()
            .gap_1p5()
            .child(
                h_flex()
                    .gap_1p5()
                    .px_1()
                    .text_size(text::MICRO.size)
                    .font_weight(FontWeight::BOLD)
                    .text_color(k.text(TextStep::T500))
                    .child(Icon::new(KairoIcon::Globe).size_2p5())
                    .child("FROM THE WEB")
                    .when(online.searching, |d| {
                        d.child(
                            Icon::new(KairoIcon::Loader)
                                .size_2p5()
                                .text_color(k.notice_text),
                        )
                    }),
            )
            .children(online.error.clone().map(|e| {
                div()
                    .id(ids::WEB_ERROR)
                    .test_support()
                    .px_1()
                    .py_2()
                    .text_size(text::LABEL.size)
                    .text_color(k.danger_text)
                    .child(e)
            }))
            .when(
                !online.searching && online.error.is_none() && online.results.is_empty(),
                |d| {
                    d.child(
                        div()
                            .id(ids::WEB_EMPTY)
                            .test_support()
                            .px_1()
                            .py_2()
                            .text_size(text::LABEL.size)
                            .text_color(k.text(TextStep::T600))
                            .child("No matches online."),
                    )
                },
            )
            .children(rows)
            .into_any_element()
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let k = *cx.kairo();
        let footer = self.model.read(cx).footer();
        let model = self.model.clone();
        let note = |id: &'static str, text_: &'static str| {
            div()
                .id(id)
                .test_support()
                .py_6()
                .px_4()
                .text_center()
                .text_size(text::XS.size)
                .text_color(k.text(TextStep::T600))
                .child(text_)
        };
        Some(match footer {
            ListFooter::None => return None,
            ListFooter::NoSongs => v_flex()
                .id(ids::NO_SONGS)
                .test_support()
                .items_center()
                .gap_3()
                .py_16()
                .px_4()
                .child(
                    Icon::new(KairoIcon::Music2)
                        .size_5()
                        .text_color(k.text(TextStep::T600)),
                )
                .child(
                    div()
                        .text_size(text::SM.size)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(k.text(TextStep::T400))
                        .child("No songs yet"),
                )
                .child(
                    div()
                        .text_center()
                        .text_size(text::XS.size)
                        .text_color(k.text(TextStep::T600))
                        .child("Use Import to search online, upload a file, or paste lyrics"),
                )
                .into_any_element(),
            ListFooter::Searching => note(ids::SEARCHING, "Searching library…").into_any_element(),
            ListFooter::NoMatch => {
                note(ids::NO_MATCH, "Nothing in your library matches that.").into_any_element()
            }
            ListFooter::NoMatchSearchOnline => v_flex()
                .items_center()
                .gap_3()
                .py_6()
                .px_4()
                .child(note(ids::NO_MATCH, "Nothing in your library matches that.").py_0())
                .child(
                    Button::new(ids::SEARCH_ONLINE)
                        .small()
                        .icon(Icon::new(KairoIcon::Globe))
                        .label("Search online")
                        .on_click(move |_, _, cx| model.update(cx, |m, cx| m.search_online(cx))),
                )
                .into_any_element(),
            ListFooter::SearchOnlineForMore => h_flex()
                .id(ids::SEARCH_ONLINE)
                .test_support()
                .role(Role::Button)
                .aria_label("Search online for more")
                .justify_center()
                .gap_1p5()
                .mt_1()
                .px_2()
                .py_2()
                .text_size(text::LABEL.size)
                .text_color(k.text(TextStep::T500))
                .hover(|s| s.text_color(k.notice_text))
                .on_click(move |_, _, cx| model.update(cx, |m, cx| m.search_online(cx)))
                .child(Icon::new(KairoIcon::Globe).size_2p5())
                .child("Search online for more")
                .into_any_element(),
            ListFooter::Web => self.render_web(cx),
        })
    }

    fn render_list(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let m = self.model.read(cx);
        let count = m.shown().len();
        let in_setlist = m.viewing_setlist_id().is_some();
        let picks = m.picks().len();
        let model = self.model.clone();
        let sizes = Rc::new(vec![
            size(
                px(kairo_ui_kit::components::page_sidebar::PageSidebar::width(
                    None, cx
                )),
                px(ROW_HEIGHT + 1.)
            );
            count
        ]);
        let footer = self.render_footer(cx);
        let selection = (in_setlist && picks > 0).then(|| {
            let weak = cx.entity().downgrade();
            let clear = model.clone();
            div().mx_1p5().mt_1p5().child(
                SelectionBar::new(picks, "song")
                    .action(
                        SelectionAction::new(ids::PICKED_REMOVE, "Remove")
                            .danger(true)
                            .on_click(move |_, window, cx| {
                                let _ = weak.update(cx, |this, cx| this.remove_picked(window, cx));
                            }),
                    )
                    .on_clear(move |_, _, cx| clear.update(cx, |m, cx| m.clear_picks(cx))),
            )
        });
        let end_model = model.clone();
        let list = v_virtual_list(cx.entity(), ids::LIST, sizes, |this, range, _, cx| {
            range
                .map(|i| this.render_song_row(i, cx))
                .collect::<Vec<_>>()
        })
        .track_scroll(&self.scroll)
        .w_full()
        .when(footer.is_none(), |d| d.flex_1())
        .when(footer.is_some() && count > 0, |d| {
            d.flex_1().max_h(px(ROW_HEIGHT * count as f32 + 1.))
        })
        .when(count == 0, |d| d.h_0());
        let _ = window;
        v_flex()
            .flex_1()
            .min_h_0()
            .children(selection)
            .child(
                v_flex()
                    .id("lyrics-list-area")
                    .flex_1()
                    .min_h_0()
                    .px_1p5()
                    .py_1()
                    .track_focus(&self.list_focus)
                    .on_key_down(cx.listener(Self::list_key))
                    .when(self.drop_end && in_setlist, |d| d.bg(k.white.opacity(0.05)))
                    .child(list)
                    .children(footer.map(|f| {
                        div()
                            .id("lyrics-list-footer")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .child(f)
                    }))
                    .when(in_setlist, |d| {
                        d.child(
                            div()
                                .id(ids::DROP_END)
                                .test_support()
                                .flex_1()
                                .min_h_4()
                                .on_drag_move(cx.listener(
                                    |this, event: &DragMoveEvent<LibraryDragItem>, _, cx| {
                                        let inside = event.bounds.contains(&event.event.position);
                                        if inside != this.drop_end {
                                            this.drop_end = inside;
                                            cx.notify();
                                        }
                                    },
                                ))
                                .on_drop(cx.listener(
                                    move |this, item: &LibraryDragItem, _, cx| {
                                        this.drop_end = false;
                                        let song = item.item_id.to_string();
                                        end_model.update(cx, |m, cx| {
                                            let end =
                                                m.viewing_setlist().map_or(0, |l| l.song_ids.len());
                                            m.drop_into_setlist(song, end, cx)
                                        });
                                    },
                                )),
                        )
                    }),
            )
            .into_any_element()
    }

    fn render_filter(&self, cx: &mut Context<Self>) -> AnyElement {
        let has_query = !self.model.read(cx).query().is_empty();
        let model = self.model.clone();
        div()
            .flex_none()
            .p_2()
            .child(
                div().id(ids::QUERY).test_support().child(
                    Input::new(&self.query)
                        .small()
                        .prefix(Icon::new(KairoIcon::ScanSearch).size_3())
                        .when(has_query, |i| {
                            i.suffix(
                                Button::new(ids::CLEAR_QUERY)
                                    .ghost()
                                    .xsmall()
                                    .icon(Icon::new(KairoIcon::X))
                                    .accessibility_label("Clear search")
                                    .on_click(move |_, _, cx| {
                                        model.update(cx, |m, cx| m.set_query(String::new(), cx))
                                    }),
                            )
                        }),
                ),
            )
            .into_any_element()
    }

    fn render_sidebar(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let error = self.model.read(cx).error().map(str::to_owned);
        v_flex()
            .w(px(
                kairo_ui_kit::components::page_sidebar::PageSidebar::width(None, cx),
            ))
            .flex_none()
            .h_full()
            .min_h_0()
            .border_r_1()
            .border_color(k.surface_border)
            .bg(k.surface_secondary.opacity(0.6))
            .child(self.render_sources(cx))
            .child(self.render_band(cx))
            .children(error.map(|e| {
                div()
                    .id(ids::ERROR)
                    .test_support()
                    .role(Role::Alert)
                    .px_3()
                    .py_1()
                    .text_size(text::LABEL.size)
                    .text_color(k.danger_text)
                    .child(e)
            }))
            .child(self.render_list(window, cx))
            .child(self.render_filter(cx))
            .into_any_element()
    }

    // ─── Right pane ─────────────────────────────────────────────────────────

    fn render_preview_pane(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let k = *cx.kairo();
        let m = self.model.read(cx);
        let preview = m.web_preview()?.clone();
        let importing = m.importing_online().is_some();
        let model = self.model.clone();
        let r = preview.result.clone();
        if rows::lyrics_unavailable(&r) {
            let open = model.clone();
            let url = r.url.clone();
            return Some(
                v_flex()
                    .id(ids::PREVIEW)
                    .test_support()
                    .flex_1()
                    .gap_3()
                    .px_4()
                    .py_4()
                    .child(
                        div()
                            .text_size(text::SM.size)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(k.text_primary)
                            .child(r.title.clone()),
                    )
                    .child(
                        div()
                            .text_size(text::XS.size)
                            .text_color(k.text(TextStep::T400))
                            .child(rows::UNAVAILABLE_PANE),
                    )
                    .child(
                        div().child(
                            Button::new(ids::PREVIEW_IMPORT)
                                .small()
                                .label("Open source")
                                .on_click(move |_, _, cx| {
                                    open.update(cx, |m, cx| m.open_url(url.clone(), cx))
                                }),
                        ),
                    )
                    .into_any_element(),
            );
        }
        let body: AnyElement = if preview.loading {
            h_flex()
                .justify_center()
                .gap_2()
                .py_16()
                .text_size(text::SM.size)
                .text_color(k.text(TextStep::T400))
                .child(
                    Icon::new(KairoIcon::Loader)
                        .size_4()
                        .text_color(k.notice_text),
                )
                .child("Loading and reviewing lyrics…")
                .into_any_element()
        } else if let Some(error) = &preview.error {
            v_flex()
                .items_center()
                .gap_3()
                .py_12()
                .px_6()
                .text_center()
                .child(
                    Icon::new(KairoIcon::AlertCircle)
                        .size_4()
                        .text_color(k.text(TextStep::T300)),
                )
                .child(
                    div()
                        .text_size(text::SM.size)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(k.text(TextStep::T200))
                        .child(error.clone()),
                )
                .child(
                    div()
                        .text_size(text::XS.size)
                        .text_color(k.text(TextStep::T400))
                        .child("Try another search result, or paste lyrics under Import → Paste."),
                )
                .into_any_element()
        } else if let Some(data) = &preview.data {
            v_flex()
                .gap_5()
                .children(review_notice_element(data.review.as_ref(), cx))
                .children(data.sections.iter().map(|s| section_block(k, s)))
                .into_any_element()
        } else {
            div().into_any_element()
        };
        let action = match r.existing_song_id.clone() {
            Some(existing) => {
                let open = model.clone();
                Button::new(ids::PREVIEW_OPEN_EXISTING)
                    .primary()
                    .small()
                    .icon(Icon::new(KairoIcon::Library))
                    .label("Open in library")
                    .on_click(move |_, _, cx| {
                        open.update(cx, |m, cx| m.open_existing(existing.clone(), cx))
                    })
            }
            None => {
                let import = model.clone();
                let result = r.clone();
                Button::new(ids::PREVIEW_IMPORT)
                    .primary()
                    .small()
                    .icon(Icon::new(if importing {
                        KairoIcon::Loader
                    } else {
                        KairoIcon::Download
                    }))
                    .label(if importing {
                        "Importing…"
                    } else {
                        "Import song"
                    })
                    .disabled(
                        importing
                            || preview.loading
                            || preview.error.is_some()
                            || preview.data.is_none(),
                    )
                    .on_click(move |_, _, cx| {
                        import.update(cx, |m, cx| m.import_suggestion(result.clone(), cx))
                    })
            }
        };
        Some(
            v_flex()
                .id(ids::PREVIEW)
                .test_support()
                .flex_1()
                .min_h_0()
                .min_w_0()
                .child(
                    v_flex()
                        .flex_none()
                        .px_4()
                        .py_3()
                        .child(
                            div()
                                .truncate()
                                .text_size(text::SM.size)
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(k.text_primary)
                                .child(r.title.clone()),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(text::LABEL.size)
                                .text_color(k.text(TextStep::T400))
                                .child(format!(
                                    "{} · {}",
                                    if r.artist.is_empty() {
                                        "Unknown artist"
                                    } else {
                                        &r.artist
                                    },
                                    rows::provider_label(r.provider)
                                )),
                        ),
                )
                .child(
                    div()
                        .id("lyrics-web-preview-body")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .px_4()
                        .py_3()
                        .child(body),
                )
                .child(
                    h_flex()
                        .flex_none()
                        .justify_end()
                        .px_4()
                        .py_3()
                        .child(action),
                )
                .into_any_element(),
        )
    }

    fn render_song_pane(&self, cx: &mut Context<Self>) -> AnyElement {
        if let Some((editor, _)) = &self.editor {
            return self.render_edit_pane(editor.clone(), cx);
        }
        let k = *cx.kairo();
        let m = self.model.read(cx);
        let Some(song) = m.selected_song().cloned() else {
            let new = self.model.clone();
            return v_flex()
                .id(ids::EMPTY)
                .test_support()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_4()
                .p_8()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .size_16()
                        .rounded(radius::xl2())
                        .bg(k.surface_secondary)
                        .child(
                            Icon::new(KairoIcon::Music2)
                                .size_6()
                                .text_color(k.text(TextStep::T600)),
                        ),
                )
                .child(
                    v_flex()
                        .items_center()
                        .child(
                            div()
                                .text_size(text::SM.size)
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(k.text(TextStep::T400))
                                .child("Select a song from your library"),
                        )
                        .child(
                            div()
                                .mt_1()
                                .text_size(text::XS.size)
                                .text_color(k.text(TextStep::T600))
                                .child("Or add one to get started"),
                        ),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child({
                            let import = self.model.clone();
                            Button::new("lyrics-empty-import")
                                .outline()
                                .small()
                                .icon(Icon::new(KairoIcon::Upload))
                                .label("Import")
                                .on_click(move |_, _, cx| {
                                    import.update(cx, |m, cx| m.open_import(cx))
                                })
                        })
                        .child(
                            Button::new("lyrics-empty-new")
                                .primary()
                                .small()
                                .icon(Icon::new(KairoIcon::Plus))
                                .label("New song")
                                .on_click(move |_, _, cx| new.update(cx, |m, cx| m.new_song(cx))),
                        ),
                )
                .into_any_element();
        };
        self.render_song_view(song, cx)
    }

    // ─── Song usage ─────────────────────────────────────────────────────────

    fn render_usage(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let k = *cx.kairo();
        let usage = self.model.read(cx).usage()?.clone();
        let model = self.model.clone();
        let (close, export, backdrop) = (model.clone(), model.clone(), model);
        let uses = usage.entries.len();
        let songs = usage.rows.len();
        let footer = rows::usage_footer(usage.saved.as_deref(), usage.missing_ccli());
        let table: AnyElement = if usage.rows.is_empty() {
            div()
                .id(ids::USAGE_EMPTY)
                .test_support()
                .px_5()
                .py_8()
                .text_center()
                .text_size(text::XS.size)
                .text_color(k.text(TextStep::T500))
                .child("No songs were pushed in this period.")
                .into_any_element()
        } else {
            v_flex()
                .child(
                    h_flex()
                        .px_5()
                        .py_2()
                        .text_size(text::MICRO.size)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(k.text(TextStep::T500))
                        .child(div().flex_1().child("SONG"))
                        .child(div().w_24().child("CCLI #"))
                        .child(div().w_12().text_right().child("USES")),
                )
                .children(usage.rows.iter().map(|r| {
                    h_flex()
                        .px_5()
                        .py_1p5()
                        .text_size(text::XS.size)
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .child(
                                    div()
                                        .truncate()
                                        .text_color(k.text_primary)
                                        .child(r.title.clone()),
                                )
                                .when(!r.artist.is_empty(), |d| {
                                    d.child(
                                        div()
                                            .truncate()
                                            .text_size(text::MICRO.size)
                                            .text_color(k.text(TextStep::T500))
                                            .child(r.artist.clone()),
                                    )
                                }),
                        )
                        .child(div().w_24().map(|d| {
                            if r.ccli_number.is_empty() {
                                d.text_color(k.warning_text).child("—")
                            } else {
                                d.text_color(k.text(TextStep::T400))
                                    .child(r.ccli_number.clone())
                            }
                        }))
                        .child(
                            div()
                                .w_12()
                                .text_right()
                                .text_color(k.text(TextStep::T300))
                                .child(format!("{}", r.uses)),
                        )
                }))
                .into_any_element()
        };
        Some(
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
                    .on_mouse_down(gpui_kit::MouseButton::Left, move |_, _, cx| {
                        backdrop.update(cx, |m, cx| m.close_usage(cx))
                    })
                    .child(
                        v_flex()
                            .id(ids::USAGE)
                            .test_support()
                            .role(Role::Dialog)
                            .aria_label("Song usage")
                            .w(px(512.))
                            .max_h(px(640.))
                            .overflow_hidden()
                            .rounded(radius::xl2())
                            .bg(k.glass_modal())
                            .on_mouse_down(gpui_kit::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(
                                h_flex()
                                    .items_start()
                                    .gap_3()
                                    .px_5()
                                    .py_4()
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_w_0()
                                            .child(
                                                div()
                                                    .text_size(text::SM.size)
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(k.text_primary)
                                                    .child("Song usage"),
                                            )
                                            .child(
                                                div()
                                                    .mt_0p5()
                                                    .text_size(text::LABEL.size)
                                                    .text_color(k.text(TextStep::T500))
                                                    .child("Every song pushed from Kairo, counted once per service — for your CCLI report."),
                                            ),
                                    )
                                    .child(
                                        Button::new(ids::USAGE_CLOSE)
                                            .ghost()
                                            .xsmall()
                                            .icon(Icon::new(KairoIcon::X))
                                            .accessibility_label("Close")
                                            .on_click(move |_, _, cx| {
                                                close.update(cx, |m, cx| m.close_usage(cx))
                                            }),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .items_end()
                                    .gap_3()
                                    .px_5()
                                    .py_3()
                                    .text_size(text::LABEL.size)
                                    .text_color(k.text(TextStep::T500))
                                    .child(
                                        v_flex()
                                            .w_32()
                                            .gap_1()
                                            .child("From")
                                            .child(div().id(ids::USAGE_FROM).test_support().child(Input::new(&self.usage_from).small().aria_label("From (yyyy-mm-dd)"))),
                                    )
                                    .child(
                                        v_flex()
                                            .w_32()
                                            .gap_1()
                                            .child("To")
                                            .child(div().id(ids::USAGE_TO).test_support().child(Input::new(&self.usage_to).small().aria_label("To (yyyy-mm-dd)"))),
                                    )
                                    .child(div().ml_auto().pb_2().child(format!(
                                        "{} · {}",
                                        rows::count_label(uses, "use"),
                                        rows::count_label(songs, "song")
                                    ))),
                            )
                            .child(
                                div()
                                    .id("lyrics-usage-table")
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_y_scroll()
                                    .child(table),
                            )
                            .child(
                                h_flex()
                                    .gap_3()
                                    .px_5()
                                    .py_3()
                                    .child(
                                        div()
                                            .id(ids::USAGE_FOOTER)
                                            .test_support()
                                            .flex_1()
                                            .min_w_0()
                                            .truncate()
                                            .text_size(text::MICRO.size)
                                            .map(|d| match &usage.error {
                                                Some(e) => d.text_color(k.danger_text).child(e.clone()),
                                                None => d.text_color(k.text(TextStep::T500)).child(footer),
                                            }),
                                    )
                                    .child(
                                        Button::new(ids::USAGE_EXPORT)
                                            .primary()
                                            .small()
                                            .icon(Icon::new(KairoIcon::Download))
                                            .label("Export CSV")
                                            .disabled(usage.busy || usage.entries.is_empty())
                                            .on_click(move |_, _, cx| {
                                                export.update(cx, |m, cx| m.export_usage(cx))
                                            }),
                                    ),
                            )
.focus_trap(ids::USAGE, self.usage_focus.handle()),
                    ),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

/// The song row's menu (`FloatingContextMenu`).
fn song_menu(
    menu: gpui_kit::component::menu::PopupMenu,
    model: &Entity<Lyrics>,
    song: &kairo_domain::lyrics::LyricsSong,
    page: &Entity<LyricsWorkspace>,
) -> gpui_kit::component::menu::PopupMenu {
    let favorite = song.is_favorite == Some(true);
    let (fav, export, edit) = (model.clone(), model.clone(), model.clone());
    let page = page.clone();
    let (fid, eid, did, edid) = (
        song.id.clone(),
        song.id.clone(),
        song.id.clone(),
        song.id.clone(),
    );
    menu.item(
        PopupMenuItem::new("Edit Song")
            .icon(Icon::new(KairoIcon::Edit2))
            .on_click(move |_, _, cx| edit.update(cx, |m, cx| m.edit_song(edid.clone(), cx))),
    )
    .item(
        PopupMenuItem::new(if favorite {
            "Remove Favorite"
        } else {
            "Mark Favorite"
        })
        .icon(Icon::new(KairoIcon::Star.weight(favorite)))
        .on_click(move |_, _, cx| fav.update(cx, |m, cx| m.toggle_favorite(fid.clone(), cx))),
    )
    .item(
        PopupMenuItem::new("Export Song…")
            .icon(Icon::new(KairoIcon::Upload))
            .on_click(move |_, _, cx| export.update(cx, |m, cx| m.export_song(eid.clone(), cx))),
    )
    .separator()
    .item(
        PopupMenuItem::new("Delete")
            .icon(Icon::new(KairoIcon::Trash2))
            .on_click(move |_, window, cx| {
                page.update(cx, |p, cx| p.request_delete(did.clone(), window, cx))
            }),
    )
}

fn web_row(
    k: KairoColors,
    index: usize,
    result: &LyricsOnlineResult,
    selected: bool,
    busy: bool,
    disabled: bool,
    model: &Entity<Lyrics>,
) -> AnyElement {
    let (select, act) = (model.clone(), model.clone());
    let (r1, r2) = (result.clone(), result.clone());
    let action = match result.existing_song_id.clone() {
        Some(existing) => Button::new(SharedString::from(ids::web_action(index)))
            .ghost()
            .xsmall()
            .icon(Icon::new(KairoIcon::Library))
            .label("In library")
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                act.update(cx, |m, cx| m.open_existing(existing.clone(), cx))
            }),
        None => Button::new(SharedString::from(ids::web_action(index)))
            .ghost()
            .xsmall()
            .icon(Icon::new(if busy {
                KairoIcon::Loader
            } else {
                KairoIcon::Download
            }))
            .label(if busy {
                "Importing…"
            } else if rows::lyrics_unavailable(result) {
                "Open source"
            } else {
                "Import"
            })
            .disabled(disabled)
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                act.update(cx, |m, cx| m.import_suggestion(r2.clone(), cx))
            }),
    };
    h_flex()
        .id(SharedString::from(ids::web(index)))
        .test_support()
        .role(Role::Button)
        .aria_label(SharedString::from(result.title.clone()))
        .items_start()
        .gap_2p5()
        .px_3()
        .py_2p5()
        .rounded(radius::lg())
        .map(|d| {
            if selected {
                d.bg(k.row_selected())
            } else {
                d.hover(|s| s.bg(k.surface_tertiary))
            }
        })
        .on_click(move |_, _, cx| select.update(cx, |m, cx| m.preview_suggestion(r1.clone(), cx)))
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size_7()
                .mt_0p5()
                .rounded(radius::lg())
                .bg(k.surface_elevated)
                .child(
                    Icon::new(KairoIcon::Music2)
                        .size_3()
                        .text_color(k.text(TextStep::T400)),
                ),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .text_size(text::SM.size)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(k.text(TextStep::T200))
                        .child(result.title.clone()),
                )
                .child(
                    div()
                        .truncate()
                        .mt_0p5()
                        .text_size(text::LABEL.size)
                        .text_color(k.text(TextStep::T400))
                        .child(rows::online_meta(result)),
                )
                .children(result.snippet.clone().filter(|s| !s.is_empty()).map(|s| {
                    div()
                        .truncate()
                        .mt_1()
                        .pl_2()
                        .border_l_1()
                        .border_color(k.notice_text.opacity(0.4))
                        .italic()
                        .text_size(text::LABEL.size)
                        .text_color(k.text(TextStep::T400))
                        .child(s)
                })),
        )
        .child(action)
        .into_any_element()
}

impl Render for LyricsWorkspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync(window, cx);
        self.sync_overlays(window, cx);
        self.song.sync(&self.model, window, cx);
        let k = *cx.kairo();
        let sidebar = self.render_sidebar(window, cx);
        let pane = match self.render_preview_pane(cx) {
            Some(p) => p,
            None => self.render_song_pane(cx),
        };
        let usage_open = self.model.read(cx).usage().is_some();
        self.usage_focus.sync(usage_open, window, cx);
        let usage = self.render_usage(cx);
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
                            .test_support()
                            .relative()
                            .items_stretch()
                            .flex_1()
                            .h_full()
                            .min_w_0()
                            .min_h_0()
                            .overflow_hidden()
                            .bg(k.surface)
                            .child(sidebar)
                            .child(kairo_ui_kit::components::page_sidebar::handle(
                                "lyrics-sidebar-handle",
                                None,
                                cx,
                            ))
                            .child(v_flex().flex_1().h_full().min_w_0().min_h_0().child(pane))
                            .children(usage),
                    )
                    .children(self.media_dock.clone()),
            )
            .children(self.rail.clone())
            .children(self.quick_open.as_ref().map(|(q, _)| q.clone()))
            .children(self.import.as_ref().map(|(d, _)| d.clone()))
    }
}

impl LyricsWorkspace {
    /// The right pane while editing: the header with Cancel and Save, then
    /// the editor.
    fn render_edit_pane(&self, editor: Entity<SongEditor>, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let m = self.model.read(cx);
        let saving = m.editing().is_some_and(|e| e.saving);
        let busy = saving || m.translating();
        let (cancel, save) = (self.model.clone(), self.model.clone());
        v_flex()
            .id(ids::SONG)
            .test_support()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .child(
                h_flex()
                    .flex_none()
                    .justify_end()
                    .gap_2()
                    .px_4()
                    .py_3()
                    .bg(k.surface)
                    .child(
                        Button::new(ids::EDIT_CANCEL)
                            .ghost()
                            .small()
                            .label("Cancel")
                            .disabled(busy)
                            .on_click(move |_, _, cx| cancel.update(cx, |m, cx| m.cancel_edit(cx))),
                    )
                    .child(
                        Button::new(ids::EDIT_SAVE)
                            .primary()
                            .small()
                            .icon(Icon::new(if saving {
                                KairoIcon::Loader
                            } else {
                                KairoIcon::Check
                            }))
                            .label(if saving { "Saving…" } else { "Save" })
                            .tooltip("Save (⌘S)")
                            .disabled(busy)
                            .on_click(move |_, _, cx| save.update(cx, |m, cx| m.save_edit(cx))),
                    ),
            )
            .child(editor)
            .into_any_element()
    }
}
