//! The media dock's state and flows (`MediaDock.tsx`, `useMediaDockStore`):
//! the library and its feed, open and size, the sidebar, the playlist on
//! screen, push, playlists, rename, copy/cut/paste, reveal, delete, drag
//! reorder, picks, imports, playback settings and posters.
//!
//! The view reads this model and calls its methods; every request goes
//! through [`MediaBackend`] and answers on GPUI's executor.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{Context, EventEmitter, Task};
use kairo_domain::media::{
    MediaItem, MediaKind, MediaLibrary, MediaPlayback, normalize_media_playback,
};
use kairo_domain::media_order::{DropPosition, reorder_ids};
use serde_json::{Map, Value, json};

use crate::backend::{
    FeedSink, ImportKind, Listening, MediaAnswer, MediaBackend, MediaCall, MediaFeed,
};
use crate::rows::{self, Selection};

/// What the dock asks of the page or the app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaEvent {
    /// The dock opened, closed, or what is live changed: the header's Media
    /// button follows.
    Header,
}

/// `useMultiSelect` over item ids.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Picks {
    selected: BTreeSet<String>,
    anchor: Option<String>,
    marquee_base: BTreeSet<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickGesture {
    Toggle,
    Range,
}

impl Picks {
    pub fn pick(&mut self, id: &str, gesture: PickGesture, order: &[String]) {
        let anchor = self.anchor.clone().filter(|a| order.contains(a));
        match (gesture, anchor) {
            (PickGesture::Range, Some(a)) => {
                let (x, y) = (
                    order.iter().position(|k| *k == a).unwrap_or(0),
                    order.iter().position(|k| k == id).unwrap_or(0),
                );
                let (lo, hi) = (x.min(y), x.max(y));
                self.selected.extend(order[lo..=hi].iter().cloned());
            }
            _ => {
                if !self.selected.remove(id) {
                    self.selected.insert(id.to_owned());
                }
            }
        }
        if gesture == PickGesture::Toggle || self.anchor.is_none() {
            self.anchor = Some(id.to_owned());
        }
    }
    pub fn clear(&mut self) {
        self.selected.clear();
        self.anchor = None;
    }
    pub fn begin_marquee(&mut self, additive: bool) {
        self.marquee_base = if additive {
            self.selected.clone()
        } else {
            BTreeSet::new()
        };
    }
    pub fn update_marquee(&mut self, ids: &[String]) {
        self.selected = self.marquee_base.clone();
        self.selected.extend(ids.iter().cloned());
        if let Some(last) = ids.last() {
            self.anchor = Some(last.clone());
        }
    }
    pub fn contains(&self, id: &str) -> bool {
        self.selected.contains(id)
    }
    pub fn len(&self) -> usize {
        self.selected.len()
    }
    pub fn is_empty(&self) -> bool {
        self.selected.is_empty()
    }
    fn retain(&mut self, keep: &HashSet<&str>) {
        self.selected.retain(|id| keep.contains(id.as_str()));
    }
}

/// A video tile's poster.
#[derive(Clone, Debug, PartialEq)]
pub enum Poster {
    Loading,
    Ready(PathBuf),
    /// FFmpeg could not read it ("Can’t play").
    Failed,
}

/// An item being dragged to rearrange, and where it would land.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Drag {
    pub dragging: Option<String>,
    pub over: Option<(String, DropPosition)>,
}

pub(crate) fn ask(
    backend: &Arc<dyn MediaBackend>,
    call: MediaCall,
) -> impl Future<Output = Result<MediaAnswer, String>> + use<> {
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
    let e = error.trim_start_matches("Error: ").trim();
    if e.is_empty() {
        "Something went wrong.".into()
    } else {
        e.into()
    }
}

pub struct MediaDock {
    backend: Arc<dyn MediaBackend>,
    library: MediaLibrary,
    open: bool,
    height: f32,
    window_height: f32,
    zoom: f32,
    sidebar_open: bool,
    sidebar_width: f32,
    selection: Option<Selection>,
    busy: Option<String>,
    error: Option<String>,
    undecodable: HashSet<String>,
    renaming_playlist: Option<String>,
    renaming_item: Option<String>,
    adding: bool,
    can_paste: bool,
    inspecting: Option<String>,
    selected_item: Option<String>,
    picks: Picks,
    drag: Drag,
    posters: HashMap<String, Poster>,
    _poll: Option<Task<()>>,
    _listening: Option<Listening>,
    _feed: Option<Task<()>>,
}

impl EventEmitter<MediaEvent> for MediaDock {}

impl MediaDock {
    pub fn new(backend: Arc<dyn MediaBackend>, window_height: f32, cx: &mut Context<Self>) -> Self {
        let library = backend.library();
        let mut this = Self {
            height: rows::stored_height(backend.read_ui(rows::HEIGHT_KEY).as_ref(), window_height),
            zoom: rows::stored_zoom(backend.read_ui(rows::ZOOM_KEY).as_ref()),
            sidebar_open: rows::stored_sidebar_open(
                backend.read_ui(rows::SIDEBAR_HIDDEN_KEY).as_ref(),
            ),
            sidebar_width: rows::stored_sidebar_width(
                backend.read_ui(rows::SIDEBAR_WIDTH_KEY).as_ref(),
            ),
            backend: backend.clone(),
            selection: (!library.folder.is_empty()).then_some(Selection::All),
            library,
            open: false,
            window_height,
            busy: None,
            error: None,
            undecodable: HashSet::new(),
            renaming_playlist: None,
            renaming_item: None,
            adding: false,
            can_paste: false,
            inspecting: None,
            selected_item: None,
            picks: Picks::default(),
            drag: Drag::default(),
            posters: HashMap::new(),
            _poll: None,
            _listening: None,
            _feed: None,
        };
        let (tx, mut rx) = futures_channel::mpsc::unbounded::<MediaFeed>();
        let sink: FeedSink = Arc::new(move |feed| {
            let _ = tx.unbounded_send(feed);
        });
        this._listening = Some(backend.listen(sink));
        this._feed = Some(cx.spawn(async move |this, cx| {
            use futures_util::StreamExt;
            while let Some(feed) = rx.next().await {
                if this.update(cx, |this, cx| this.apply(feed, cx)).is_err() {
                    break;
                }
            }
        }));
        this.posters_for_library(cx);
        this
    }

    fn apply(&mut self, feed: MediaFeed, cx: &mut Context<Self>) {
        match feed {
            MediaFeed::Library(library) => self.set_library(library, cx),
            MediaFeed::Import(kind) => self.import_files(kind, cx),
        }
    }

    fn set_library(&mut self, library: MediaLibrary, cx: &mut Context<Self>) {
        let live_before = self.live_name();
        self.library = library;
        if self.selection.is_none() && !self.library.folder.is_empty() {
            self.selection = Some(Selection::All);
        }
        if let Some(Selection::Playlist(id)) = &self.selection
            && !self.library.playlists.iter().any(|p| &p.id == id)
        {
            self.selection = Some(Selection::All);
        }
        let owned = self.visible_ids();
        let ids: HashSet<&str> = owned.iter().map(String::as_str).collect();
        self.picks.retain(&ids);
        if self
            .inspecting
            .as_ref()
            .is_some_and(|id| !self.library.items.iter().any(|i| &i.id == id))
        {
            self.inspecting = None;
        }
        self.posters_for_library(cx);
        if live_before != self.live_name() {
            cx.emit(MediaEvent::Header);
        }
        cx.notify();
    }

    /// Asks for a poster for each video not yet asked about. Off the UI thread.
    fn posters_for_library(&mut self, cx: &mut Context<Self>) {
        let wanted: Vec<String> = self
            .library
            .items
            .iter()
            .filter(|i| i.kind == MediaKind::Video && !self.posters.contains_key(&i.id))
            .map(|i| i.id.clone())
            .collect();
        for id in wanted {
            self.posters.insert(id.clone(), Poster::Loading);
            let call = ask(&self.backend, MediaCall::Poster(id.clone()));
            cx.spawn(async move |this, cx| {
                let answer = call.await;
                let _ = this.update(cx, |this, cx| {
                    let poster = match answer {
                        Ok(MediaAnswer::Poster { image, .. }) => Poster::Ready(image),
                        _ => {
                            this.undecodable.insert(id.clone());
                            Poster::Failed
                        }
                    };
                    this.posters.insert(id, poster);
                    cx.notify();
                });
            })
            .detach();
        }
    }

    // ─── Reading ────────────────────────────────────────────────────────────

    pub fn library(&self) -> &MediaLibrary {
        &self.library
    }
    pub fn backend(&self) -> &Arc<dyn MediaBackend> {
        &self.backend
    }
    pub fn is_open(&self) -> bool {
        self.open
    }
    pub fn height(&self) -> f32 {
        self.height
    }
    pub fn max_height(&self) -> f32 {
        rows::max_height(self.window_height)
    }
    pub fn zoom(&self) -> f32 {
        self.zoom
    }
    pub fn sidebar_open(&self) -> bool {
        self.sidebar_open
    }
    pub fn sidebar_width(&self) -> f32 {
        self.sidebar_width
    }
    pub fn selection(&self) -> Option<&Selection> {
        self.selection.as_ref()
    }
    pub fn visible(&self) -> Vec<&MediaItem> {
        rows::visible_items(&self.library, self.selection.as_ref()).0
    }
    pub fn missing(&self) -> Vec<String> {
        rows::visible_items(&self.library, self.selection.as_ref()).1
    }
    pub fn item(&self, id: &str) -> Option<&MediaItem> {
        self.library.items.iter().find(|i| i.id == id)
    }
    pub fn live_item(&self) -> Option<&MediaItem> {
        self.library
            .live_item_id
            .as_deref()
            .and_then(|id| self.item(id))
    }
    /// The live background's name, for the header.
    pub fn live_name(&self) -> Option<String> {
        self.live_item().map(|i| i.name.clone())
    }
    pub fn playback(&self, id: &str) -> MediaPlayback {
        let raw = self
            .library
            .playback
            .get(id)
            .and_then(|p| serde_json::to_value(p).ok())
            .unwrap_or(Value::Null);
        normalize_media_playback(&raw)
    }
    pub fn busy(&self) -> Option<&str> {
        self.busy.as_deref()
    }
    /// The page's error, or the library's own.
    pub fn error(&self) -> Option<&str> {
        self.library.error.as_deref().or(self.error.as_deref())
    }
    pub fn undecodable(&self, id: &str) -> bool {
        self.undecodable.contains(id)
    }
    pub fn poster(&self, id: &str) -> Option<&Poster> {
        self.posters.get(id)
    }
    pub fn renaming_playlist(&self) -> Option<&str> {
        self.renaming_playlist.as_deref()
    }
    pub fn renaming_item(&self) -> Option<&str> {
        self.renaming_item.as_deref()
    }
    pub fn adding(&self) -> bool {
        self.adding
    }
    pub fn can_paste(&self) -> bool {
        self.can_paste
    }
    pub fn inspecting(&self) -> Option<&MediaItem> {
        self.inspecting.as_deref().and_then(|id| self.item(id))
    }
    pub fn selected_item(&self) -> Option<&str> {
        self.selected_item.as_deref()
    }
    pub fn picks(&self) -> &Picks {
        &self.picks
    }
    pub fn drag(&self) -> &Drag {
        &self.drag
    }
    fn visible_ids(&self) -> Vec<String> {
        self.visible().iter().map(|i| i.id.clone()).collect()
    }
    fn playlist_id(&self) -> Option<String> {
        match &self.selection {
            Some(Selection::Playlist(id)) => Some(id.clone()),
            _ => None,
        }
    }

    // ─── Open, size and sidebar ─────────────────────────────────────────────

    pub fn set_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if open && rows::available_room(self.window_height) < rows::MIN_HEIGHT {
            return;
        }
        if self.open == open {
            return;
        }
        self.open = open;
        if open {
            self.poll_paste(cx);
        } else {
            self._poll = None;
        }
        cx.emit(MediaEvent::Header);
        cx.notify();
    }

    pub fn toggle(&mut self, cx: &mut Context<Self>) {
        let open = !self.open;
        self.set_open(open, cx);
    }

    /// The window was resized: the dock keeps the page its floor, and puts
    /// itself away when there is no room at all.
    pub fn window_resized(&mut self, window_height: f32, cx: &mut Context<Self>) {
        if (window_height - self.window_height).abs() < 0.5 {
            return;
        }
        self.window_height = window_height;
        if rows::available_room(window_height) < rows::MIN_HEIGHT {
            self.set_open(false, cx);
        }
        self.height = rows::clamp_height(self.height, window_height);
        cx.notify();
    }

    /// A drag of the top edge (not persisted until [`Self::finish_resize`]).
    pub fn resize_to(&mut self, height: f32, cx: &mut Context<Self>) {
        self.height = rows::clamp_height(height, self.window_height);
        cx.notify();
    }

    /// Release: dragged far enough down puts the dock away (keeping its
    /// size); otherwise the height is remembered.
    pub fn finish_resize(&mut self, start_height: f32, travel: f32, cx: &mut Context<Self>) {
        if travel.abs() < 4. {
            self.height = start_height;
        } else if start_height - travel < rows::MIN_HEIGHT - 40. {
            self.height = start_height;
            self.set_open(false, cx);
        } else {
            self.backend
                .write_ui(rows::HEIGHT_KEY, json!(self.height.round()));
        }
        cx.notify();
    }

    /// Keys on the resize edge: ↑/↓ step, End is the tallest, Home, Esc,
    /// Enter and Space close.
    pub fn resize_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        match key {
            "home" | "escape" | "enter" | "space" => self.set_open(false, cx),
            "up" | "down" | "end" => {
                if rows::available_room(self.window_height) < rows::MIN_HEIGHT {
                    return true;
                }
                let next = if key == "end" {
                    self.max_height()
                } else {
                    rows::clamp_height(
                        self.height
                            + if key == "up" {
                                rows::HEIGHT_STEP
                            } else {
                                -rows::HEIGHT_STEP
                            },
                        self.window_height,
                    )
                };
                self.height = next;
                self.backend.write_ui(rows::HEIGHT_KEY, json!(next.round()));
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    pub fn set_zoom(&mut self, zoom: f32, cx: &mut Context<Self>) {
        self.zoom = zoom.clamp(rows::MIN_ZOOM, rows::MAX_ZOOM);
        self.backend.write_ui(rows::ZOOM_KEY, json!(self.zoom));
        cx.notify();
    }

    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_open = !self.sidebar_open;
        self.backend.write_ui(
            rows::SIDEBAR_HIDDEN_KEY,
            json!((!self.sidebar_open).to_string()),
        );
        cx.notify();
    }

    pub fn set_sidebar_width(&mut self, width: f32, cx: &mut Context<Self>) {
        self.sidebar_width = rows::clamp_sidebar(width);
        self.backend
            .write_ui(rows::SIDEBAR_WIDTH_KEY, json!(self.sidebar_width));
        cx.notify();
    }

    pub fn select(&mut self, selection: Selection, cx: &mut Context<Self>) {
        self.selection = Some(selection);
        self.picks.clear();
        cx.notify();
    }

    // ─── Calls ──────────────────────────────────────────────────────────────

    /// Runs a call that answers the library; errors show in the dock.
    fn library_call(
        &mut self,
        call: MediaCall,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
    ) {
        self.error = None;
        let call = ask(&self.backend, call);
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| {
                match answer {
                    Ok(MediaAnswer::Library(library)) => {
                        this.set_library(library, cx);
                        then(this, cx);
                    }
                    Ok(_) => then(this, cx),
                    Err(e) => this.error = Some(clean(&e)),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn quiet_call(
        &mut self,
        call: MediaCall,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
    ) {
        self.error = None;
        let call = ask(&self.backend, call);
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| {
                match answer {
                    Ok(_) => then(this, cx),
                    Err(e) => this.error = Some(clean(&e)),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// A click is the push: no confirm step, one gesture mid-song. Nothing
    /// showing it says why (`backgroundTargetProblem`).
    pub fn push(&mut self, id: String, cx: &mut Context<Self>) {
        self.selected_item = Some(id.clone());
        self.busy = Some(id.clone());
        self.error = None;
        let call = ask(&self.backend, MediaCall::Push(id));
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = None;
                match answer {
                    Ok(MediaAnswer::Pushed {
                        applied: false,
                        reason: Some(reason),
                    }) => this.error = Some(reason),
                    Ok(_) => {}
                    Err(e) => this.error = Some(clean(&e)),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub fn choose_folder(&mut self, cx: &mut Context<Self>) {
        self.library_call(MediaCall::ChooseFolder, cx, |this, _| {
            this.selection = (!this.library.folder.is_empty()).then_some(Selection::All);
        });
    }

    /// ＋: makes the playlist at once and puts its name in edit.
    pub fn new_playlist(&mut self, cx: &mut Context<Self>) {
        let name = rows::new_playlist_name(&self.library);
        let before: HashSet<String> = self
            .library
            .playlists
            .iter()
            .map(|p| p.id.clone())
            .collect();
        self.library_call(MediaCall::CreatePlaylist(name), cx, move |this, _| {
            if let Some(created) = this
                .library
                .playlists
                .iter()
                .find(|p| !before.contains(&p.id))
            {
                this.selection = Some(Selection::Playlist(created.id.clone()));
                this.renaming_playlist = Some(created.id.clone());
            }
        });
    }

    pub fn begin_rename_playlist(&mut self, id: String, cx: &mut Context<Self>) {
        self.renaming_playlist = Some(id);
        cx.notify();
    }

    pub fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        self.renaming_playlist = None;
        self.renaming_item = None;
        cx.notify();
    }

    /// Enter commits a non-empty name.
    pub fn commit_rename_playlist(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(id) = self.renaming_playlist.take() else {
            return;
        };
        let name = name.trim();
        if name.is_empty() {
            cx.notify();
            return;
        }
        self.library_call(
            MediaCall::RenamePlaylist {
                id,
                name: name.into(),
            },
            cx,
            |_, _| {},
        );
    }

    /// The view asks first; the files stay on disk.
    pub fn delete_playlist(&mut self, id: String, cx: &mut Context<Self>) {
        self.library_call(MediaCall::DeletePlaylist(id), cx, |this, _| {
            this.selection = Some(Selection::All);
        });
    }

    pub fn add_to_playlist(&mut self, playlist: String, item: String, cx: &mut Context<Self>) {
        let Some(p) = self.library.playlists.iter().find(|p| p.id == playlist) else {
            return;
        };
        if p.item_ids.contains(&item) {
            return;
        }
        let mut ids = p.item_ids.clone();
        ids.push(item);
        self.library_call(
            MediaCall::SetPlaylistItems {
                id: playlist,
                item_ids: ids,
            },
            cx,
            |_, _| {},
        );
    }

    /// Drops a dragged card onto a playlist row. When viewing another playlist,
    /// moves the item(s) out of the current playlist into the target; when
    /// viewing All backgrounds, adds them to the target.
    pub fn drop_item_on_playlist(
        &mut self,
        playlist: String,
        item: String,
        cx: &mut Context<Self>,
    ) {
        self.drag.over = None;
        let source_playlist = self.playlist_id();
        if source_playlist.as_deref() == Some(playlist.as_str()) {
            return;
        }
        let Some(target) = self.library.playlists.iter().find(|p| p.id == playlist) else {
            return;
        };
        let multi = self.picks.contains(&item) && self.picked_count() > 1;
        let moving: Vec<String> = if multi { self.picked_ids() } else { vec![item] };
        let mut target_ids = target.item_ids.clone();
        for id in &moving {
            if !target_ids.contains(id) {
                target_ids.push(id.clone());
            }
        }
        let target_changed = target_ids.len() != target.item_ids.len();
        let remove_from_source = move |this: &mut Self, cx: &mut Context<Self>| {
            if multi {
                this.picks.clear();
            }
            let Some(src_id) = source_playlist else {
                return;
            };
            let Some(src) = this.library.playlists.iter().find(|p| p.id == src_id) else {
                return;
            };
            let remaining: Vec<String> = src
                .item_ids
                .iter()
                .filter(|id| !moving.contains(id))
                .cloned()
                .collect();
            if remaining.len() == src.item_ids.len() {
                return;
            }
            this.library_call(
                MediaCall::SetPlaylistItems {
                    id: src_id,
                    item_ids: remaining,
                },
                cx,
                |_, _| {},
            );
        };
        if target_changed {
            self.library_call(
                MediaCall::SetPlaylistItems {
                    id: playlist,
                    item_ids: target_ids,
                },
                cx,
                remove_from_source,
            );
        } else {
            remove_from_source(self, cx);
        }
    }

    pub fn remove_from_playlist(&mut self, item: String, cx: &mut Context<Self>) {
        let Some(playlist) = self.playlist_id() else {
            return;
        };
        let Some(p) = self.library.playlists.iter().find(|p| p.id == playlist) else {
            return;
        };
        let ids = p.item_ids.iter().filter(|i| **i != item).cloned().collect();
        self.library_call(
            MediaCall::SetPlaylistItems {
                id: playlist,
                item_ids: ids,
            },
            cx,
            |_, _| {},
        );
    }

    /// Deletes the file (the view asks first). A live item comes off screen.
    pub fn delete_item(&mut self, id: String, cx: &mut Context<Self>) {
        self.library_call(MediaCall::DeleteItem(id), cx, |_, _| {});
    }

    pub fn begin_rename_item(&mut self, id: String, cx: &mut Context<Self>) {
        self.renaming_item = Some(id);
        cx.notify();
    }

    /// Enter or clicking away; an empty name cancels.
    pub fn commit_rename_item(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(id) = self.renaming_item.take() else {
            return;
        };
        let name = name.trim();
        if name.is_empty() {
            cx.notify();
            return;
        }
        self.library_call(
            MediaCall::RenameItem {
                id,
                name: name.into(),
            },
            cx,
            |_, _| {},
        );
    }

    pub fn reveal(&mut self, id: String, cx: &mut Context<Self>) {
        self.quiet_call(MediaCall::RevealItem(id), cx, |_, _| {});
    }

    pub fn copy(&mut self, id: String, cx: &mut Context<Self>) {
        let held = id.clone();
        self.quiet_call(MediaCall::CopyItems(vec![id]), cx, move |this, _| {
            this.can_paste = true;
            this.selected_item = Some(held);
        });
    }

    pub fn cut(&mut self, id: String, cx: &mut Context<Self>) {
        let held = id.clone();
        let from_playlist = self.playlist_id();
        self.quiet_call(
            MediaCall::CutItems {
                ids: vec![id],
                from_playlist,
            },
            cx,
            move |this, _| {
                this.can_paste = true;
                this.selected_item = Some(held);
            },
        );
    }

    pub fn paste(&mut self, cx: &mut Context<Self>) {
        let playlist = self.playlist_id();
        self.library_call(MediaCall::PasteItems(playlist), cx, |this, cx| {
            this.check_paste(cx)
        });
    }

    fn check_paste(&mut self, cx: &mut Context<Self>) {
        let call = ask(&self.backend, MediaCall::ClipboardHasFiles);
        cx.spawn(async move |this, cx| {
            if let Ok(MediaAnswer::Flag(has)) = call.await {
                let _ = this.update(cx, |this, cx| {
                    if this.can_paste != has {
                        this.can_paste = has;
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    /// While open: Finder or this app may put files on the clipboard.
    fn poll_paste(&mut self, cx: &mut Context<Self>) {
        self.check_paste(cx);
        self._poll = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(rows::PASTE_POLL_MS))
                    .await;
                if this.update(cx, |this, cx| this.check_paste(cx)).is_err() {
                    break;
                }
            }
        }));
    }

    /// The window came to the front.
    pub fn window_focused(&mut self, cx: &mut Context<Self>) {
        if self.open {
            self.check_paste(cx);
        }
    }

    /// ⌘C, ⌘X and ⌘V while the dock is open: the target is the selected
    /// item, else the live one, else the first shown.
    pub fn shortcut(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        if !self.open || self.inspecting.is_some() {
            return false;
        }
        if key == "v" {
            self.paste(cx);
            return true;
        }
        let target = self
            .selected_item
            .clone()
            .or_else(|| self.library.live_item_id.clone())
            .or_else(|| self.visible().first().map(|i| i.id.clone()));
        let Some(id) = target else { return false };
        match key {
            "c" => self.copy(id, cx),
            "x" => self.cut(id, cx),
            _ => return false,
        }
        true
    }

    // ─── Rearranging ────────────────────────────────────────────────────────

    pub fn begin_drag(&mut self, id: String, cx: &mut Context<Self>) {
        self.drag = Drag {
            dragging: Some(id),
            over: None,
        };
        cx.notify();
    }

    pub fn drag_over(&mut self, target: String, position: DropPosition, cx: &mut Context<Self>) {
        if self.drag.dragging.as_deref() == Some(target.as_str()) {
            return;
        }
        let next = Some((target, position));
        if self.drag.over != next {
            self.drag.over = next;
            cx.notify();
        }
    }

    pub fn end_drag(&mut self, cx: &mut Context<Self>) {
        self.drag = Drag::default();
        cx.notify();
    }

    /// Persists a rearrange: a playlist's own ids (missing ones kept), or
    /// the library's item order. The grid moves first; a failed write pulls
    /// the real library back.
    pub fn reorder(
        &mut self,
        dragged: String,
        target: String,
        position: DropPosition,
        cx: &mut Context<Self>,
    ) {
        self.drag = Drag::default();
        if dragged == target {
            cx.notify();
            return;
        }
        let call = if let Some(playlist) = self.playlist_id() {
            let Some(p) = self.library.playlists.iter_mut().find(|p| p.id == playlist) else {
                return;
            };
            let next = reorder_ids(&p.item_ids, &dragged, &target, position);
            if next == p.item_ids {
                cx.notify();
                return;
            }
            p.item_ids = next.clone();
            MediaCall::SetPlaylistItems {
                id: playlist,
                item_ids: next,
            }
        } else {
            let current: Vec<String> = self.library.items.iter().map(|i| i.id.clone()).collect();
            let next = reorder_ids(&current, &dragged, &target, position);
            if next == current {
                cx.notify();
                return;
            }
            let mut items = std::mem::take(&mut self.library.items);
            items.sort_by_key(|i| next.iter().position(|id| *id == i.id).unwrap_or(usize::MAX));
            self.library.items = items;
            MediaCall::SetItemOrder(next)
        };
        self.error = None;
        let backend = self.backend.clone();
        let write = ask(&backend, call);
        cx.spawn(async move |this, cx| match write.await {
            Ok(MediaAnswer::Library(library)) => {
                let _ = this.update(cx, |this, cx| this.set_library(library, cx));
            }
            Ok(_) => {}
            Err(e) => {
                let again = ask(&backend, MediaCall::GetLibrary).await;
                let _ = this.update(cx, |this, cx| {
                    this.error = Some(clean(&e));
                    if let Ok(MediaAnswer::Library(library)) = again {
                        this.set_library(library, cx);
                    }
                    cx.notify();
                });
            }
        })
        .detach();
        cx.notify();
    }

    // ─── Picks ──────────────────────────────────────────────────────────────

    pub fn pick(&mut self, id: &str, gesture: PickGesture, cx: &mut Context<Self>) {
        let order = self.visible_ids();
        self.picks.pick(id, gesture, &order);
        cx.notify();
    }

    pub fn clear_picks(&mut self, cx: &mut Context<Self>) {
        self.picks.clear();
        cx.notify();
    }

    pub fn begin_marquee(&mut self, additive: bool, cx: &mut Context<Self>) {
        self.picks.begin_marquee(additive);
        cx.notify();
    }

    pub fn update_marquee(&mut self, ids: Vec<String>, cx: &mut Context<Self>) {
        self.picks.update_marquee(&ids);
        cx.notify();
    }

    fn picked_ids(&self) -> Vec<String> {
        self.visible_ids()
            .into_iter()
            .filter(|id| self.picks.contains(id))
            .collect()
    }

    pub fn add_picked_to_playlist(&mut self, playlist: String, cx: &mut Context<Self>) {
        let Some(p) = self.library.playlists.iter().find(|p| p.id == playlist) else {
            return;
        };
        let mut ids = p.item_ids.clone();
        ids.extend(
            self.picked_ids()
                .into_iter()
                .filter(|id| !p.item_ids.contains(id)),
        );
        self.library_call(
            MediaCall::SetPlaylistItems {
                id: playlist,
                item_ids: ids,
            },
            cx,
            |this, _| this.picks.clear(),
        );
    }

    pub fn remove_picked_from_playlist(&mut self, cx: &mut Context<Self>) {
        let Some(playlist) = self.playlist_id() else {
            return;
        };
        let Some(p) = self.library.playlists.iter().find(|p| p.id == playlist) else {
            return;
        };
        let ids = p
            .item_ids
            .iter()
            .filter(|id| !self.picks.contains(id))
            .cloned()
            .collect();
        self.library_call(
            MediaCall::SetPlaylistItems {
                id: playlist,
                item_ids: ids,
            },
            cx,
            |this, _| this.picks.clear(),
        );
    }

    /// Deletes every picked file, one after another (the view asks first).
    pub fn delete_picked(&mut self, cx: &mut Context<Self>) {
        let ids = self.picked_ids();
        if ids.is_empty() {
            return;
        }
        self.error = None;
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let mut last = None;
            let mut failure = None;
            for id in ids {
                match ask(&backend, MediaCall::DeleteItem(id)).await {
                    Ok(MediaAnswer::Library(l)) => last = Some(l),
                    Ok(_) => {}
                    Err(e) => {
                        failure = Some(e);
                        break;
                    }
                }
            }
            let _ = this.update(cx, |this, cx| {
                if let Some(l) = last {
                    this.set_library(l, cx);
                }
                match failure {
                    Some(e) => this.error = Some(clean(&e)),
                    None => this.picks.clear(),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn picked_count(&self) -> usize {
        self.picked_ids().len()
    }

    // ─── Imports and playback ───────────────────────────────────────────────

    pub fn add_media_to_playlist(&mut self, cx: &mut Context<Self>) {
        let Some(playlist) = self.playlist_id() else {
            return;
        };
        self.adding_call(MediaCall::AddMediaToPlaylist(playlist), cx, |_| {});
    }

    /// File → Import → Images or Videos.
    pub fn import_files(&mut self, kind: ImportKind, cx: &mut Context<Self>) {
        if self.adding {
            return;
        }
        self.adding_call(MediaCall::ImportFiles(kind), cx, |this| {
            this.selection = Some(Selection::All);
        });
    }

    /// Files dropped from the OS onto the grid.
    pub fn drop_files(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if paths.is_empty() || self.adding {
            return;
        }
        let playlist = self.playlist_id();
        self.adding_call(MediaCall::ImportPaths { paths, playlist }, cx, |_| {});
    }

    /// Files dropped from the OS directly onto a sidebar playlist row.
    pub fn drop_files_on_playlist(
        &mut self,
        playlist: String,
        paths: Vec<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        if paths.is_empty() || self.adding {
            return;
        }
        self.adding_call(
            MediaCall::ImportPaths {
                paths,
                playlist: Some(playlist),
            },
            cx,
            |_| {},
        );
    }

    /// An import: `adding` shows until it answers, either way.
    fn adding_call(
        &mut self,
        call: MediaCall,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self) + 'static,
    ) {
        self.adding = true;
        self.error = None;
        let call = ask(&self.backend, call);
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| {
                this.adding = false;
                match answer {
                    Ok(MediaAnswer::Library(library)) => {
                        this.set_library(library, cx);
                        then(this);
                    }
                    Ok(_) => then(this),
                    Err(e) => this.error = Some(clean(&e)),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Loop on the card menu: applies at once, live included.
    pub fn set_playback(&mut self, id: String, patch: Map<String, Value>, cx: &mut Context<Self>) {
        let mut current = serde_json::to_value(self.playback(&id)).unwrap_or(Value::Null);
        if let Some(obj) = current.as_object_mut() {
            obj.extend(patch.clone());
        }
        self.library
            .playback
            .insert(id.clone(), normalize_media_playback(&current));
        self.library_call(
            MediaCall::SetPlayback {
                id,
                patch,
                next_cue: false,
            },
            cx,
            |_, _| {},
        );
    }

    /// The inspector's Save: applies the next time the item is cued.
    pub fn save_inspector(
        &mut self,
        id: String,
        patch: Map<String, Value>,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, Result<(), String>, &mut Context<Self>) + 'static,
    ) {
        let call = ask(
            &self.backend,
            MediaCall::SetPlayback {
                id,
                patch,
                next_cue: true,
            },
        );
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| {
                match answer {
                    Ok(MediaAnswer::Library(l)) => {
                        this.set_library(l, cx);
                        done(this, Ok(()), cx);
                    }
                    Ok(_) => done(this, Ok(()), cx),
                    Err(e) => done(this, Err(clean(&e)), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn inspect(&mut self, id: String, cx: &mut Context<Self>) {
        self.inspecting = Some(id);
        cx.notify();
    }

    pub fn close_inspector(&mut self, cx: &mut Context<Self>) {
        self.inspecting = None;
        cx.notify();
    }

    pub fn select_item(&mut self, id: String, cx: &mut Context<Self>) {
        self.selected_item = Some(id);
        cx.notify();
    }

    pub fn mark_undecodable(&mut self, id: String, cx: &mut Context<Self>) {
        if self.undecodable.insert(id) {
            cx.notify();
        }
    }
}
