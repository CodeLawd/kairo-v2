//! The Lyrics library's state and flows (`Lyrics.tsx`, `LibraryRail.tsx`,
//! `SongUsageModal.tsx`): the library and its sources, the song list with
//! search, favourites, setlists, the opt-in web search with its preview,
//! delete and the song usage report. See
//! `native-migration/notes/lyrics-state-map.md` for the map from Electron.
//!
//! The view reads this model and calls its methods; every request goes
//! through [`LyricsBackend`] and answers on GPUI's executor.

use std::collections::{BTreeSet, HashMap};
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{Context, EventEmitter, Task};
use kairo_domain::collections::{
    DEFAULT_LIBRARY_ID, DEFAULT_SETLIST_NAME, LibrariesCommand, LibraryState, SetlistCommand,
    SetlistState, SongSetlist, library_counts,
};
use kairo_domain::lyrics::online_error::format_online_lyrics_error;
use kairo_domain::lyrics::{LyricsOnlinePreview, LyricsOnlineResult, LyricsSong};
use kairo_domain::usage::{SongUsageEntry, SongUsageSummaryRow, summarize_song_usage};

use crate::backend::{
    FeedSink, KairoExport, Listening, LyricsAnswer, LyricsBackend, LyricsCall, LyricsFeed,
    OnlineSource,
};
use crate::rows::{
    self, FTS_AFTER_MS, Filter, LANDED_MS, ListScope, MIN_FTS_QUERY, MIN_ONLINE_QUERY, Sort,
};

/// What the page asks of the application.
#[derive(Clone, Debug, PartialEq)]
pub enum LyricsEvent {
    /// A short confirmation (`useTransferStore.notify({ tone: 'ok' })`).
    Notify(String),
    /// Unsaved edits would be lost: ask, then call
    /// [`Lyrics::discard_and_continue`] or [`Lyrics::keep_editing`].
    ConfirmDiscard,
    /// A song was opened from quick open; show the Lyrics page.
    Opened(String),
}

/// The ⌘ or ⇧ gesture on a setlist row (`selectGesture`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickGesture {
    Toggle,
    Range,
}

/// `useMultiSelect` over song ids or slide indices.
#[derive(Clone, Debug, PartialEq)]
pub struct Picks<K: Ord + Clone = String> {
    selected: BTreeSet<K>,
    anchor: Option<K>,
}

impl<K: Ord + Clone> Default for Picks<K> {
    fn default() -> Self {
        Self {
            selected: BTreeSet::new(),
            anchor: None,
        }
    }
}

impl<K: Ord + Clone> Picks<K> {
    pub fn pick(&mut self, key: K, gesture: PickGesture, order: &[K]) {
        match gesture {
            PickGesture::Toggle => {
                if !self.selected.remove(&key) {
                    self.selected.insert(key.clone());
                }
                self.anchor = Some(key);
            }
            PickGesture::Range => {
                let from = self
                    .anchor
                    .as_ref()
                    .and_then(|a| order.iter().position(|k| k == a));
                let to = order.iter().position(|k| *k == key);
                if let (Some(a), Some(b)) = (from, to) {
                    let (lo, hi) = (a.min(b), a.max(b));
                    self.selected.extend(order[lo..=hi].iter().cloned());
                } else {
                    self.selected.insert(key.clone());
                    self.anchor = Some(key);
                }
            }
        }
    }
    pub fn select_all(&mut self, order: &[K]) {
        self.selected = order.iter().cloned().collect();
    }
    pub fn clear(&mut self) {
        self.selected.clear();
        self.anchor = None;
    }
    pub fn contains<Q: Ord + ?Sized>(&self, key: &Q) -> bool
    where
        K: std::borrow::Borrow<Q>,
    {
        self.selected.contains(key)
    }
    pub fn selected(&self) -> &BTreeSet<K> {
        &self.selected
    }
    pub fn len(&self) -> usize {
        self.selected.len()
    }
    pub fn is_empty(&self) -> bool {
        self.selected.is_empty()
    }
    fn retain(&mut self, keep: impl Fn(&K) -> bool) {
        self.selected.retain(|k| keep(k));
    }
}

/// The opt-in web search under the song list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OnlineState {
    pub requested: bool,
    pub results: Vec<LyricsOnlineResult>,
    pub searching: bool,
    pub error: Option<String>,
}

/// The right pane's preview of a web hit (`OnlinePreviewPane`).
#[derive(Clone, Debug, PartialEq)]
pub struct WebPreview {
    pub result: LyricsOnlineResult,
    pub data: Option<LyricsOnlinePreview>,
    pub loading: bool,
    pub error: Option<String>,
}

/// The song usage report (`SongUsageModal`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Usage {
    /// `yyyy-mm-dd`.
    pub from: String,
    pub to: String,
    pub entries: Vec<SongUsageEntry>,
    pub rows: Vec<SongUsageSummaryRow>,
    pub busy: bool,
    pub saved: Option<String>,
    pub error: Option<String>,
    request: u64,
}

impl Usage {
    pub fn missing_ccli(&self) -> usize {
        self.rows
            .iter()
            .filter(|r| r.ccli_number.is_empty())
            .count()
    }
}

/// Runs one backend call to completion.
pub(crate) fn ask(
    backend: &Arc<dyn LyricsBackend>,
    call: LyricsCall,
) -> impl Future<Output = Result<LyricsAnswer, String>> + use<> {
    let (tx, rx) = futures_channel::oneshot::channel();
    backend.call(
        call,
        Box::new(move |result| {
            let _ = tx.send(result);
        }),
    );
    async move {
        rx.await
            .unwrap_or_else(|_| Err("The request was dropped.".into()))
    }
}

pub(crate) fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or_default()
}

/// `cleanIpcError`: Electron's IPC wrapper is never shown.
pub(crate) fn clean_error(error: &str) -> String {
    let trimmed = error
        .trim_start_matches("Error invoking remote method ")
        .trim_start_matches("Error: ")
        .trim();
    if trimmed.is_empty() {
        "Something went wrong.".into()
    } else {
        trimmed.into()
    }
}

pub struct Lyrics {
    backend: Arc<dyn LyricsBackend>,
    songs: Vec<LyricsSong>,
    /// Bumped whenever `songs` changes.
    revision: u64,
    libraries: LibraryState,
    setlists: SetlistState,
    /// Indices of `songs` in the order the list shows them.
    shown: Vec<usize>,
    selected: Option<String>,
    filter: Filter,
    sort: Sort,
    viewing_setlist: Option<String>,
    active_library: String,
    pending_open: Option<String>,
    // Search.
    query: String,
    fts: Option<Vec<LyricsSong>>,
    fts_searching: bool,
    fts_seq: u64,
    _fts: Option<Task<()>>,
    online: OnlineState,
    online_seq: u64,
    importing_online: Option<String>,
    web_preview: Option<WebPreview>,
    preview_seq: u64,
    preview_cache: HashMap<String, LyricsOnlinePreview>,
    // Overlays and errors.
    delete_target: Option<String>,
    picks: Picks,
    error: Option<String>,
    rail_error: Option<String>,
    renaming_setlist: Option<String>,
    landed_setlist: Option<String>,
    _landed: Option<Task<()>>,
    usage: Option<Usage>,
    // The song view (081).
    gloss: String,
    card_background: crate::backend::CardBackground,
    live: Option<(String, usize)>,
    pushing: Option<usize>,
    push_seq: u64,
    zoom: f32,
    slide_picks: Picks<usize>,
    selecting: bool,
    /// `None`: paint off; `Some(None)`: clear a line's colour; `Some(c)`: apply.
    paint: Option<Option<String>>,
    busy: bool,
    quick_edit: Option<(usize, String)>,
    // Editing and import (082).
    editing: Option<Editing>,
    pending_leave: Option<Leave>,
    import_open: bool,
    import_tab: crate::import::Tab,
    pending_import_paths: Vec<std::path::PathBuf>,
    translating: bool,
    translate_error: Option<String>,
    translate_language: String,
    _listening: Option<Listening>,
    _feed: Option<Task<()>>,
}

impl EventEmitter<LyricsEvent> for Lyrics {}

impl Lyrics {
    pub fn new(backend: Arc<dyn LyricsBackend>, cx: &mut Context<Self>) -> Self {
        let snapshot = backend.snapshot();
        let mut this = Self {
            backend: backend.clone(),
            songs: snapshot.songs,
            revision: 0,
            libraries: snapshot.libraries,
            setlists: snapshot.setlists,
            shown: Vec::new(),
            selected: None,
            filter: Filter::All,
            sort: Sort::Title,
            viewing_setlist: None,
            active_library: DEFAULT_LIBRARY_ID.into(),
            pending_open: None,
            query: String::new(),
            fts: None,
            fts_searching: false,
            fts_seq: 0,
            _fts: None,
            online: OnlineState::default(),
            online_seq: 0,
            importing_online: None,
            web_preview: None,
            preview_seq: 0,
            preview_cache: HashMap::new(),
            delete_target: None,
            picks: Picks::default(),
            error: None,
            rail_error: None,
            renaming_setlist: None,
            landed_setlist: None,
            _landed: None,
            usage: None,
            gloss: snapshot.gloss_color,
            card_background: snapshot.card_background,
            live: None,
            pushing: None,
            push_seq: 0,
            zoom: crate::grid::ZOOM_DEFAULT,
            slide_picks: Picks::default(),
            selecting: false,
            paint: None,
            busy: false,
            quick_edit: None,
            editing: None,
            pending_leave: None,
            import_open: false,
            import_tab: crate::import::Tab::File,
            pending_import_paths: Vec::new(),
            translating: false,
            translate_error: None,
            translate_language: "auto".into(),
            _listening: None,
            _feed: None,
        };
        this.refresh();
        let (tx, mut rx) = futures_channel::mpsc::unbounded::<LyricsFeed>();
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
        this
    }

    fn apply(&mut self, feed: LyricsFeed, cx: &mut Context<Self>) {
        match feed {
            LyricsFeed::Songs(songs) => self.merge_songs(songs),
            LyricsFeed::Libraries(l) => self.libraries = l,
            LyricsFeed::Setlists(s) => self.set_setlists(s),
            LyricsFeed::GlossColor(c) => self.gloss = c,
            LyricsFeed::CardBackground(b) => self.card_background = b,
            LyricsFeed::OpenImport => {
                if self.editing.is_none() {
                    self.import_open = true;
                }
            }
        }
        self.refresh();
        cx.notify();
    }

    /// New songs land first; a newer copy (`updatedAt`) replaces the one
    /// held here; anything not newer is left alone.
    fn merge_songs(&mut self, incoming: Vec<LyricsSong>) {
        self.revision += 1;
        let mut added = Vec::new();
        for song in incoming {
            match self.songs.iter_mut().find(|s| s.id == song.id) {
                Some(held) if song.updated_at > held.updated_at => *held = song,
                Some(_) => {}
                None => added.push(song),
            }
        }
        if !added.is_empty() {
            added.append(&mut self.songs);
            self.songs = added;
        }
    }

    fn set_setlists(&mut self, state: SetlistState) {
        self.setlists = state;
        // A setlist deleted elsewhere falls back to the library.
        if let Some(id) = &self.viewing_setlist
            && !self.setlists.lists.iter().any(|l| &l.id == id)
        {
            self.viewing_setlist = None;
        }
        if let Some(id) = &self.renaming_setlist
            && !self.setlists.lists.iter().any(|l| &l.id == id)
        {
            self.renaming_setlist = None;
        }
    }

    /// Recomputes the list, opens a pending song once it is in the library
    /// and drops a selection that no longer exists.
    fn refresh(&mut self) {
        let setlist = self
            .viewing_setlist()
            .map(|l| l.song_ids.clone())
            .unwrap_or_default();
        self.shown = rows::filtered_songs(
            &self.songs,
            ListScope {
                libraries: &self.libraries,
                active_library: &self.active_library,
                query: &self.query,
                fts: self.fts.as_deref(),
                setlist: self.viewing_setlist.is_some().then_some(setlist.as_slice()),
                filter: self.filter,
                sort: self.sort,
            },
        );
        if let Some(id) = self.pending_open.clone()
            && self.songs.iter().any(|s| s.id == id)
        {
            self.pending_open = None;
            self.selected = Some(id);
            self.web_preview = None;
        }
        if self.pending_open.is_none()
            && let Some(id) = &self.selected
            && !self.songs.iter().any(|s| &s.id == id)
        {
            self.selected = None;
        }
        if self.viewing_setlist.is_some() {
            let shown: Vec<String> = self.shown_ids().into_iter().map(str::to_owned).collect();
            self.picks.retain(|k| shown.iter().any(|s| s == k));
        } else {
            self.picks.clear();
        }
    }

    fn shown_ids(&self) -> Vec<&str> {
        self.shown
            .iter()
            .map(|&i| self.songs[i].id.as_str())
            .collect()
    }

    // ─── Reading ────────────────────────────────────────────────────────────

    pub fn songs(&self) -> &[LyricsSong] {
        &self.songs
    }
    /// Changes whenever the library does.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// The list's rows: indices into [`Lyrics::songs`].
    pub fn shown(&self) -> &[usize] {
        &self.shown
    }
    pub fn shown_song(&self, row: usize) -> Option<&LyricsSong> {
        self.shown.get(row).map(|&i| &self.songs[i])
    }
    pub fn song(&self, id: &str) -> Option<&LyricsSong> {
        self.songs.iter().find(|s| s.id == id)
    }
    pub fn selected_id(&self) -> Option<&str> {
        self.selected.as_deref()
    }
    pub fn selected_song(&self) -> Option<&LyricsSong> {
        self.selected.as_deref().and_then(|id| self.song(id))
    }
    pub fn filter(&self) -> Filter {
        self.filter
    }
    pub fn sort(&self) -> Sort {
        self.sort
    }
    pub fn libraries(&self) -> &LibraryState {
        &self.libraries
    }
    pub fn active_library(&self) -> &str {
        &self.active_library
    }
    pub fn library_counts(&self) -> std::collections::BTreeMap<String, usize> {
        library_counts(&self.libraries, &self.songs, |s| &s.id)
    }
    pub fn favorite_count(&self) -> usize {
        self.songs
            .iter()
            .filter(|s| s.is_favorite == Some(true))
            .count()
    }
    pub fn setlists(&self) -> &SetlistState {
        &self.setlists
    }
    pub fn viewing_setlist_id(&self) -> Option<&str> {
        self.viewing_setlist.as_deref()
    }
    pub fn viewing_setlist(&self) -> Option<&SongSetlist> {
        let id = self.viewing_setlist.as_deref()?;
        self.setlists.lists.iter().find(|l| l.id == id)
    }
    /// The list drops land in: the one on screen, the active one, or the
    /// first (`LibraryRail`'s `activeList`).
    pub fn drop_setlist(&self) -> Option<&SongSetlist> {
        let lists = &self.setlists.lists;
        let id = self
            .viewing_setlist
            .as_deref()
            .or(self.setlists.active_id.as_deref());
        id.and_then(|id| lists.iter().find(|l| l.id == id))
            .or_else(|| lists.first())
    }
    /// `activeSetlist`: the list quick open adds to.
    pub fn active_setlist(&self) -> Option<&SongSetlist> {
        let id = self.setlists.active_id.as_deref()?;
        self.setlists.lists.iter().find(|l| l.id == id)
    }
    pub fn query(&self) -> &str {
        &self.query
    }
    pub fn fts_searching(&self) -> bool {
        self.fts_searching
    }
    pub fn online(&self) -> &OnlineState {
        &self.online
    }
    pub fn importing_online(&self) -> Option<&str> {
        self.importing_online.as_deref()
    }
    pub fn web_preview(&self) -> Option<&WebPreview> {
        self.web_preview.as_ref()
    }
    pub fn delete_target(&self) -> Option<&LyricsSong> {
        self.delete_target.as_deref().and_then(|id| self.song(id))
    }
    pub fn picks(&self) -> &Picks {
        &self.picks
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    pub fn rail_error(&self) -> Option<&str> {
        self.rail_error.as_deref()
    }
    pub fn renaming_setlist(&self) -> Option<&str> {
        self.renaming_setlist.as_deref()
    }
    pub fn landed_setlist(&self) -> Option<&str> {
        self.landed_setlist.as_deref()
    }
    pub fn usage(&self) -> Option<&Usage> {
        self.usage.as_ref()
    }
    pub fn footer(&self) -> rows::ListFooter {
        rows::list_footer(
            self.shown.len(),
            &self.query,
            self.fts_searching,
            self.online.requested,
        )
    }
    pub fn backend(&self) -> &Arc<dyn LyricsBackend> {
        &self.backend
    }

    // ─── Sources ────────────────────────────────────────────────────────────

    pub fn select_library(&mut self, id: String, cx: &mut Context<Self>) {
        self.viewing_setlist = None;
        self.active_library = id;
        self.filter = Filter::All;
        self.refresh();
        cx.notify();
    }

    pub fn select_filter(&mut self, filter: Filter, cx: &mut Context<Self>) {
        self.viewing_setlist = None;
        self.filter = filter;
        self.refresh();
        cx.notify();
    }

    /// Shows a setlist and lands on its first song unless the open one is
    /// part of it.
    pub fn select_setlist(&mut self, id: String, cx: &mut Context<Self>) {
        let Some(list) = self.setlists.lists.iter().find(|l| l.id == id).cloned() else {
            return;
        };
        self.viewing_setlist = Some(id);
        self.picks.clear();
        let keep = self
            .selected
            .as_ref()
            .is_some_and(|s| list.song_ids.contains(s));
        if !keep && let Some(first) = list.song_ids.first() {
            self.select_song(first.clone(), cx);
        }
        self.refresh();
        cx.notify();
    }

    pub fn set_sort(&mut self, sort: Sort, cx: &mut Context<Self>) {
        self.sort = sort;
        self.refresh();
        cx.notify();
    }

    // ─── Selection ──────────────────────────────────────────────────────────

    /// `handleSelect`.
    pub fn select_song(&mut self, id: String, cx: &mut Context<Self>) {
        if self.editing.is_some() && !self.leave_editor(Leave::Select(id.clone()), cx) {
            return;
        }
        if self.selected.as_deref() != Some(id.as_str()) {
            // A different song starts with nothing picked and no quick edit.
            self.slide_picks.clear();
            self.selecting = false;
            self.quick_edit = None;
        }
        self.selected = Some(id);
        self.web_preview = None;
        self.error = None;
        cx.notify();
    }

    /// Moves the selection by one row (↑/↓ in the list).
    pub fn step_selection(&mut self, down: bool, cx: &mut Context<Self>) -> Option<usize> {
        if self.shown.is_empty() {
            return None;
        }
        let current = self
            .selected
            .as_deref()
            .and_then(|id| self.shown_ids().iter().position(|s| *s == id));
        let next = match current {
            None if down => 0,
            None => self.shown.len() - 1,
            Some(i) if down => (i + 1).min(self.shown.len() - 1),
            Some(i) => i.saturating_sub(1),
        };
        let id = self.songs[self.shown[next]].id.clone();
        self.select_song(id, cx);
        Some(next)
    }

    /// The row the selection is on, if shown.
    pub fn selected_row(&self) -> Option<usize> {
        let id = self.selected.as_deref()?;
        self.shown_ids().iter().position(|s| *s == id)
    }

    /// A song from quick open: added if new, selected, the page shown.
    pub fn open_song(&mut self, song: LyricsSong, cx: &mut Context<Self>) {
        let id = song.id.clone();
        self.upsert(song);
        self.select_song(id.clone(), cx);
        self.refresh();
        cx.emit(LyricsEvent::Opened(id));
        cx.notify();
    }

    fn upsert(&mut self, song: LyricsSong) {
        self.revision += 1;
        match self.songs.iter_mut().find(|s| s.id == song.id) {
            Some(held) => *held = song,
            None => self.songs.insert(0, song),
        }
    }

    /// ＋ New song: an empty editor.
    pub fn new_song(&mut self, cx: &mut Context<Self>) {
        if self.editing.is_some() && !self.leave_editor(Leave::NewSong, cx) {
            return;
        }
        let editing = Editing::new(true, None);
        self.selected = None;
        self.web_preview = None;
        self.editing = Some(editing);
        cx.notify();
    }

    // ─── Search ─────────────────────────────────────────────────────────────

    /// Local matches now, full-text hits 180 ms later (2+ characters); a new
    /// query drops the web results and preview.
    pub fn set_query(&mut self, value: String, cx: &mut Context<Self>) {
        if value == self.query {
            return;
        }
        self.query = value;
        // New query: forget the web (never searched automatically).
        self.online_seq += 1;
        self.online = OnlineState::default();
        self.web_preview = None;
        self.fts_seq += 1;
        let q = self.query.trim().to_owned();
        if q.chars().count() < MIN_FTS_QUERY {
            self.fts = None;
            self.fts_searching = false;
            self._fts = None;
        } else {
            self.fts_searching = true;
            let seq = self.fts_seq;
            let backend = self.backend.clone();
            self._fts = Some(cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(FTS_AFTER_MS))
                    .await;
                let answer = ask(&backend, LyricsCall::Search(q)).await;
                let _ = this.update(cx, |this, cx| {
                    if this.fts_seq != seq {
                        return;
                    }
                    this.fts = match answer {
                        Ok(LyricsAnswer::Songs(hits)) => Some(hits),
                        _ => None,
                    };
                    this.fts_searching = false;
                    this.refresh();
                    cx.notify();
                });
            }));
        }
        self.refresh();
        cx.notify();
    }

    /// "Search online": the operator opts in (3+ characters).
    pub fn search_online(&mut self, cx: &mut Context<Self>) {
        let q = self.query.trim().to_owned();
        if q.chars().count() < MIN_ONLINE_QUERY {
            return;
        }
        self.online_seq += 1;
        let seq = self.online_seq;
        self.online.requested = true;
        self.online.searching = true;
        self.online.error = None;
        let call = ask(&self.backend, LyricsCall::SearchOnline(q));
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| {
                if this.online_seq != seq {
                    return;
                }
                this.online.searching = false;
                match answer {
                    Ok(LyricsAnswer::Online(results)) => {
                        // A preview stays only while its hit is still listed.
                        if let Some(p) = &this.web_preview
                            && !results.iter().any(|r| r.id == p.result.id)
                        {
                            this.web_preview = None;
                        }
                        this.online.results = results;
                        this.online.error = None;
                    }
                    Ok(_) => this.online.results = Vec::new(),
                    Err(e) => {
                        this.online.results = Vec::new();
                        this.online.error = Some(clean_error(&e));
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// A web hit chosen: its lyrics in the right pane (cached per hit).
    pub fn preview_suggestion(&mut self, result: LyricsOnlineResult, cx: &mut Context<Self>) {
        self.selected = None;
        self.online.error = None;
        self.preview_seq += 1;
        let cached = self.preview_cache.get(&result.id).cloned();
        let unavailable = rows::lyrics_unavailable(&result);
        let needs_fetch = cached.is_none() && !unavailable;
        let source = OnlineSource::of(&result);
        self.web_preview = Some(WebPreview {
            result,
            data: cached,
            loading: needs_fetch,
            error: None,
        });
        if needs_fetch {
            let seq = self.preview_seq;
            let call = ask(&self.backend, LyricsCall::PreviewOnline(source));
            cx.spawn(async move |this, cx| {
                let answer = call.await;
                let _ = this.update(cx, |this, cx| {
                    if this.preview_seq != seq {
                        return;
                    }
                    let Some(preview) = this.web_preview.as_mut() else {
                        return;
                    };
                    preview.loading = false;
                    match answer {
                        Ok(LyricsAnswer::Preview(data)) => {
                            this.preview_cache
                                .insert(preview.result.id.clone(), data.clone());
                            preview.data = Some(data);
                        }
                        Ok(_) => preview.error = Some(format_online_lyrics_error("")),
                        Err(e) => {
                            preview.data = None;
                            preview.error = Some(format_online_lyrics_error(&e));
                        }
                    }
                    cx.notify();
                });
            })
            .detach();
        }
        cx.notify();
    }

    /// Import a web hit and open it; a hit without importable lyrics opens
    /// its page instead.
    pub fn import_suggestion(&mut self, result: LyricsOnlineResult, cx: &mut Context<Self>) {
        if rows::lyrics_unavailable(&result) {
            self.open_url(result.url, cx);
            return;
        }
        if self.importing_online.is_some() {
            return;
        }
        self.importing_online = Some(result.id.clone());
        self.online.error = None;
        let call = ask(
            &self.backend,
            LyricsCall::ImportOnline(OnlineSource::of(&result)),
        );
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| {
                this.importing_online = None;
                match answer {
                    Ok(LyricsAnswer::Song(song)) => this.imported(song, cx),
                    Ok(_) => {}
                    Err(e) => this.online.error = Some(format_online_lyrics_error(&e)),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// `handleImported`: the song lands and opens; the query clears.
    pub(crate) fn imported(&mut self, song: LyricsSong, cx: &mut Context<Self>) {
        self.pending_open = Some(song.id.clone());
        self.upsert(song);
        self.error = None;
        let query = std::mem::take(&mut self.query);
        if !query.is_empty() {
            self.online_seq += 1;
            self.online = OnlineState::default();
            self.fts = None;
            self.fts_searching = false;
            self.fts_seq += 1;
        }
        self.refresh();
        cx.notify();
    }

    /// "In library": the local copy instead of a duplicate.
    pub fn open_existing(&mut self, song_id: String, cx: &mut Context<Self>) {
        self.web_preview = None;
        self.set_query(String::new(), cx);
        self.select_song(song_id, cx);
        self.refresh();
    }

    pub fn open_url(&mut self, url: String, _cx: &mut Context<Self>) {
        self.backend
            .call(LyricsCall::OpenUrl(url), Box::new(|_| {}));
    }

    // ─── Favourites and delete ──────────────────────────────────────────────

    pub fn toggle_favorite(&mut self, id: String, cx: &mut Context<Self>) {
        let call = ask(&self.backend, LyricsCall::ToggleFavorite(id.clone()));
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| {
                match answer {
                    Ok(LyricsAnswer::Favorite(on)) => {
                        if let Some(s) = this.songs.iter_mut().find(|s| s.id == id) {
                            s.is_favorite = Some(on);
                            this.revision += 1;
                        }
                        this.refresh();
                    }
                    Ok(_) => {}
                    Err(e) => this.error = Some(clean_error(&e)),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn ask_delete(&mut self, id: String, cx: &mut Context<Self>) {
        if self.song(&id).is_some() {
            self.delete_target = Some(id);
            cx.notify();
        }
    }

    pub fn cancel_delete(&mut self, cx: &mut Context<Self>) {
        self.delete_target = None;
        cx.notify();
    }

    pub fn confirm_delete(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.delete_target.clone() else {
            return;
        };
        let call = ask(&self.backend, LyricsCall::Delete(id.clone()));
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| {
                this.delete_target = None;
                match answer {
                    Ok(_) => {
                        this.songs.retain(|s| s.id != id);
                        this.revision += 1;
                        if this.selected.as_deref() == Some(id.as_str()) {
                            this.selected = None;
                        }
                        this.refresh();
                    }
                    Err(e) => this.error = Some(clean_error(&e)),
                }
                cx.notify();
            });
        })
        .detach();
    }

    // ─── Setlists ───────────────────────────────────────────────────────────

    /// Runs a setlist command; `then` gets the new state on success.
    fn setlist(
        &mut self,
        command: SetlistCommand,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, &SetlistState, &mut Context<Self>) + 'static,
    ) {
        self.rail_error = None;
        let call = ask(&self.backend, LyricsCall::Setlist(command));
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| {
                match answer {
                    Ok(LyricsAnswer::Setlists(state)) => {
                        this.set_setlists(state.clone());
                        then(this, &state, cx);
                    }
                    Ok(_) => {}
                    Err(e) => this.rail_error = Some(clean_error(&e)),
                }
                this.refresh();
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// ＋ → New playlist: create, show it and start renaming.
    pub fn create_setlist(&mut self, cx: &mut Context<Self>) {
        self.setlist(
            SetlistCommand::Create {
                name: DEFAULT_SETLIST_NAME.into(),
            },
            cx,
            |this, state, cx| {
                if let Some(created) = state.lists.first() {
                    let id = created.id.clone();
                    this.select_setlist(id.clone(), cx);
                    this.renaming_setlist = Some(id);
                }
            },
        );
    }

    pub fn begin_rename_setlist(&mut self, id: String, cx: &mut Context<Self>) {
        self.renaming_setlist = Some(id);
        cx.notify();
    }

    pub fn cancel_rename_setlist(&mut self, cx: &mut Context<Self>) {
        self.renaming_setlist = None;
        cx.notify();
    }

    /// Enter or blur: a trimmed, changed name is saved.
    pub fn commit_rename_setlist(&mut self, draft: &str, cx: &mut Context<Self>) {
        let Some(id) = self.renaming_setlist.take() else {
            return;
        };
        let current = self
            .setlists
            .lists
            .iter()
            .find(|l| l.id == id)
            .map(|l| l.name.clone())
            .unwrap_or_default();
        if !draft.trim().is_empty() && draft != current {
            self.setlist(
                SetlistCommand::Rename {
                    list_id: id,
                    name: draft.to_owned(),
                },
                cx,
                |_, _, _| {},
            );
        }
        cx.notify();
    }

    pub fn delete_setlist(&mut self, id: String, cx: &mut Context<Self>) {
        self.setlist(SetlistCommand::Delete { list_id: id }, cx, |_, _, _| {});
    }

    pub fn remove_from_setlist(&mut self, song_id: String, cx: &mut Context<Self>) {
        let Some(list) = self.viewing_setlist.clone() else {
            return;
        };
        self.setlist(
            SetlistCommand::RemoveSong {
                song_id,
                list_id: Some(list),
            },
            cx,
            |_, _, _| {},
        );
    }

    /// A song dropped onto a setlist row: added, or a word that it is
    /// already there (adding would move it to the end mid-service).
    pub fn drop_on_setlist(&mut self, list_id: String, song_id: String, cx: &mut Context<Self>) {
        let Some(list) = self
            .setlists
            .lists
            .iter()
            .find(|l| l.id == list_id)
            .cloned()
        else {
            return;
        };
        let title = self
            .song(&song_id)
            .map(|s| s.title.clone())
            .unwrap_or_else(|| "Song".into());
        if list.song_ids.contains(&song_id) {
            cx.emit(LyricsEvent::Notify(format!(
                "“{title}” is already on {}",
                list.name
            )));
            return;
        }
        self.setlist(
            SetlistCommand::Add {
                song_id,
                index: None,
                list_id: Some(list.id.clone()),
            },
            cx,
            move |this, _, cx| {
                this.mark_landed(list.id.clone(), cx);
                cx.emit(LyricsEvent::Notify(format!(
                    "Added “{title}” to {}",
                    list.name
                )));
            },
        );
    }

    /// A drop on the playlists section but not on a row: the active list,
    /// or a new one.
    pub fn drop_on_setlists(&mut self, song_id: String, cx: &mut Context<Self>) {
        if let Some(list) = self.drop_setlist() {
            let id = list.id.clone();
            self.drop_on_setlist(id, song_id, cx);
            return;
        }
        self.setlist(
            SetlistCommand::Create {
                name: DEFAULT_SETLIST_NAME.into(),
            },
            cx,
            move |this, state, cx| {
                if let Some(created) = state.lists.first() {
                    let id = created.id.clone();
                    this.drop_on_setlist(id, song_id, cx);
                }
            },
        );
    }

    /// A drop into the setlist on screen, at a gap (`insert_at`).
    pub fn drop_into_setlist(&mut self, song_id: String, insert_at: usize, cx: &mut Context<Self>) {
        let Some(list) = self.viewing_setlist().cloned() else {
            return;
        };
        let Some(index) = rows::setlist_drop_index(&list.song_ids, &song_id, insert_at) else {
            return;
        };
        self.setlist(
            SetlistCommand::Add {
                song_id,
                index: Some(index as f64),
                list_id: Some(list.id),
            },
            cx,
            |_, _, _| {},
        );
    }

    /// Queue a song on the active setlist, creating one first if none exists
    /// (quick open's ⌘↵). `done` runs once it is added.
    pub fn add_to_active_setlist(
        &mut self,
        song_id: String,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
    ) {
        let add = move |this: &mut Self, cx: &mut Context<Self>| {
            this.setlist(
                SetlistCommand::Add {
                    song_id,
                    index: None,
                    list_id: None,
                },
                cx,
                move |this, _, cx| done(this, cx),
            );
        };
        if self.active_setlist().is_some() {
            add(self, cx);
        } else {
            self.setlist(
                SetlistCommand::Create {
                    name: DEFAULT_SETLIST_NAME.into(),
                },
                cx,
                move |this, _, cx| add(this, cx),
            );
        }
    }

    fn mark_landed(&mut self, id: String, cx: &mut Context<Self>) {
        self.landed_setlist = Some(id.clone());
        self._landed = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(LANDED_MS))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.landed_setlist.as_deref() == Some(id.as_str()) {
                    this.landed_setlist = None;
                    cx.notify();
                }
            });
        }));
    }

    // ─── Picks in the setlist on screen ─────────────────────────────────────

    pub fn pick_song(&mut self, id: String, gesture: PickGesture, cx: &mut Context<Self>) {
        if self.viewing_setlist.is_none() {
            return;
        }
        let order: Vec<String> = self.shown_ids().into_iter().map(str::to_owned).collect();
        self.picks.pick(id, gesture, &order);
        cx.notify();
    }

    pub fn select_all_picks(&mut self, cx: &mut Context<Self>) {
        if self.viewing_setlist.is_none() {
            return;
        }
        let order: Vec<String> = self.shown_ids().into_iter().map(str::to_owned).collect();
        self.picks.select_all(&order);
        cx.notify();
    }

    pub fn clear_picks(&mut self, cx: &mut Context<Self>) {
        self.picks.clear();
        cx.notify();
    }

    /// The picked songs, in list order.
    pub fn picked_ids(&self) -> Vec<String> {
        self.shown_ids()
            .into_iter()
            .filter(|id| self.picks.contains(*id))
            .map(str::to_owned)
            .collect()
    }

    /// Removes the picked songs from the setlist on screen (the view asks
    /// first when there is more than one).
    pub fn remove_picked(&mut self, cx: &mut Context<Self>) {
        let Some(list) = self.viewing_setlist.clone() else {
            return;
        };
        let ids = self.picked_ids();
        if ids.is_empty() {
            return;
        }
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let mut last = Ok(None);
            for song_id in ids {
                let call = ask(
                    &backend,
                    LyricsCall::Setlist(SetlistCommand::RemoveSong {
                        song_id,
                        list_id: Some(list.clone()),
                    }),
                );
                match call.await {
                    Ok(LyricsAnswer::Setlists(state)) => last = Ok(Some(state)),
                    Ok(_) => {}
                    Err(e) => {
                        last = Err(e);
                        break;
                    }
                }
            }
            let _ = this.update(cx, |this, cx| {
                match last {
                    Ok(state) => {
                        if let Some(state) = state {
                            this.set_setlists(state);
                        }
                        this.picks.clear();
                    }
                    Err(e) => this.error = Some(clean_error(&e)),
                }
                this.refresh();
                cx.notify();
            });
        })
        .detach();
    }

    // ─── Libraries ──────────────────────────────────────────────────────────

    /// Runs a library command; `then` gets whether it created one, with its id.
    pub fn library_command(
        &mut self,
        command: LibrariesCommand,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, Option<String>, &mut Context<Self>) + 'static,
    ) {
        let before: Vec<String> = self
            .libraries
            .libraries
            .iter()
            .map(|l| l.id.clone())
            .collect();
        let call = ask(&self.backend, LyricsCall::Libraries(command));
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| {
                match answer {
                    Ok(LyricsAnswer::Libraries(state)) => {
                        let created = state
                            .libraries
                            .iter()
                            .find(|l| !before.contains(&l.id))
                            .map(|l| l.id.clone());
                        this.libraries = state;
                        if !this
                            .libraries
                            .libraries
                            .iter()
                            .any(|l| l.id == this.active_library)
                        {
                            this.active_library = DEFAULT_LIBRARY_ID.into();
                        }
                        then(this, created, cx);
                    }
                    Ok(_) => {}
                    Err(e) => this.rail_error = Some(clean_error(&e)),
                }
                this.refresh();
                cx.notify();
            });
        })
        .detach();
    }

    // ─── Transfer ───────────────────────────────────────────────────────────

    fn transfer(&mut self, call: LyricsCall, cx: &mut Context<Self>) {
        let call = ask(&self.backend, call);
        cx.spawn(async move |this, cx| {
            if let Err(e) = call.await {
                let _ = this.update(cx, |this, cx| {
                    this.error = Some(clean_error(&e));
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Export songs: the setlist on screen, else the selected song.
    pub fn export_songs(&mut self, cx: &mut Context<Self>) {
        let ids = match self.viewing_setlist() {
            Some(list) => list.song_ids.clone(),
            None => self.selected.iter().cloned().collect(),
        };
        self.transfer(LyricsCall::ExportKairo(KairoExport::Pick(ids)), cx);
    }

    pub fn export_song(&mut self, id: String, cx: &mut Context<Self>) {
        self.transfer(LyricsCall::ExportKairo(KairoExport::Songs(vec![id])), cx);
    }

    pub fn export_setlist(&mut self, id: String, cx: &mut Context<Self>) {
        self.transfer(LyricsCall::ExportKairo(KairoExport::Setlist(id)), cx);
    }

    pub fn export_library(&mut self, cx: &mut Context<Self>) {
        self.transfer(LyricsCall::ExportKairo(KairoExport::Library), cx);
    }

    pub fn import_kairo(&mut self, cx: &mut Context<Self>) {
        self.transfer(LyricsCall::ImportKairo, cx);
    }

    // ─── Song usage ─────────────────────────────────────────────────────────

    /// Opens the report on the last 90 days.
    pub fn open_usage(&mut self, cx: &mut Context<Self>) {
        let today = chrono::Local::now().date_naive();
        let from = today - chrono::Duration::days(rows::USAGE_DAYS);
        self.usage = Some(Usage {
            from: from.format("%Y-%m-%d").to_string(),
            to: today.format("%Y-%m-%d").to_string(),
            ..Default::default()
        });
        self.load_usage(cx);
        cx.notify();
    }

    pub fn close_usage(&mut self, cx: &mut Context<Self>) {
        self.usage = None;
        cx.notify();
    }

    /// A date changed. An empty or unreadable date, or one past the other
    /// end, is ignored, as the date input's `min`/`max` were.
    pub fn set_usage_range(&mut self, from: &str, to: &str, cx: &mut Context<Self>) {
        let (Some(f), Some(t)) = (rows::parse_day(from), rows::parse_day(to)) else {
            return;
        };
        if f > t {
            return;
        }
        let Some(usage) = self.usage.as_mut() else {
            return;
        };
        usage.from = f.format("%Y-%m-%d").to_string();
        usage.to = t.format("%Y-%m-%d").to_string();
        self.load_usage(cx);
        cx.notify();
    }

    fn usage_range(&self) -> Option<(f64, f64)> {
        let usage = self.usage.as_ref()?;
        Some(rows::usage_range(
            rows::parse_day(&usage.from)?,
            rows::parse_day(&usage.to)?,
        ))
    }

    fn load_usage(&mut self, cx: &mut Context<Self>) {
        let Some((from, to)) = self.usage_range() else {
            return;
        };
        let Some(usage) = self.usage.as_mut() else {
            return;
        };
        usage.request += 1;
        let request = usage.request;
        let call = ask(&self.backend, LyricsCall::UsageList { from, to });
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| {
                let Some(usage) = this.usage.as_mut().filter(|u| u.request == request) else {
                    return;
                };
                match answer {
                    Ok(LyricsAnswer::Usage(entries)) => {
                        usage.rows = summarize_song_usage(&entries, rows::locale_compare);
                        usage.entries = entries;
                    }
                    Ok(_) => {}
                    Err(e) => tracing::error!(error = %e, "[Lyrics] song usage list failed"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Export CSV: the save dialog and the file.
    pub fn export_usage(&mut self, cx: &mut Context<Self>) {
        let Some((from, to)) = self.usage_range() else {
            return;
        };
        let Some(usage) = self.usage.as_mut() else {
            return;
        };
        if usage.busy || usage.entries.is_empty() {
            return;
        }
        usage.busy = true;
        usage.error = None;
        let call = ask(&self.backend, LyricsCall::UsageExport { from, to });
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| {
                let Some(usage) = this.usage.as_mut() else {
                    return;
                };
                usage.busy = false;
                match answer {
                    Ok(LyricsAnswer::Saved(Some(path))) => usage.saved = Some(path),
                    Ok(_) => {}
                    Err(e) => usage.error = Some(clean_error(&e)),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

// ─── The song view (081) ────────────────────────────────────────────────────

use kairo_domain::lyrics::LyricsSongSection;
use kairo_domain::lyrics::hotkeys::{
    auto_assign_hotkeys, hotkey_from_event, section_for_hotkey, set_section_hotkey,
};
use kairo_domain::lyrics::reorder::{
    Side, SlideLabel, delete_lyric_slides, label_lyric_slides, lyric_slide_lines, move_lyric_slide,
    replace_lyric_slide,
};

impl Lyrics {
    /// The lyrics theme's background for the cards.
    pub fn card_background(&self) -> &crate::backend::CardBackground {
        &self.card_background
    }

    pub fn gloss(&self) -> &str {
        &self.gloss
    }
    /// The song and slide last put on screen from here.
    pub fn live(&self) -> Option<(&str, usize)> {
        self.live.as_ref().map(|(s, i)| (s.as_str(), *i))
    }
    /// The live slide when it belongs to `song`.
    pub fn live_slide_of(&self, song: &str) -> Option<usize> {
        self.live().filter(|(s, _)| *s == song).map(|(_, i)| i)
    }
    pub fn pushing(&self) -> Option<usize> {
        self.pushing
    }
    pub fn zoom(&self) -> f32 {
        self.zoom
    }
    pub fn slide_picks(&self) -> &Picks<usize> {
        &self.slide_picks
    }
    pub fn selecting(&self) -> bool {
        self.selecting
    }
    pub fn paint(&self) -> Option<Option<&str>> {
        self.paint.as_ref().map(|c| c.as_deref())
    }
    pub fn busy(&self) -> bool {
        self.busy
    }
    pub fn quick_edit(&self) -> Option<(usize, &str)> {
        self.quick_edit.as_ref().map(|(i, t)| (*i, t.as_str()))
    }

    /// The playlist's songs in order, when the open song is in the playlist
    /// on screen and it has more than one (`showsSetlist`).
    pub fn playlist_songs(&self) -> Vec<&LyricsSong> {
        let Some(list) = self.viewing_setlist() else {
            return Vec::new();
        };
        let songs: Vec<&LyricsSong> = list
            .song_ids
            .iter()
            .filter_map(|id| self.song(id))
            .collect();
        let selected = self.selected.as_deref();
        if songs.len() > 1 && selected.is_some_and(|id| songs.iter().any(|s| s.id == id)) {
            songs
        } else {
            Vec::new()
        }
    }

    pub fn set_zoom(&mut self, zoom: f32, cx: &mut Context<Self>) {
        self.zoom = zoom.clamp(crate::grid::ZOOM_MIN, crate::grid::ZOOM_MAX);
        cx.notify();
    }

    /// Puts slide `index` of `song_id` on screen. Another song becomes the
    /// selected one. Errors show where the page shows send errors.
    pub fn push_slide(&mut self, song_id: String, index: usize, cx: &mut Context<Self>) {
        if self.song(&song_id).is_none() {
            return;
        }
        if self.selected.as_deref() != Some(song_id.as_str()) {
            self.select_song(song_id.clone(), cx);
        }
        self.push_seq += 1;
        let seq = self.push_seq;
        self.pushing = Some(index);
        self.error = None;
        let call = ask(
            &self.backend,
            LyricsCall::PushSlide {
                song_id: song_id.clone(),
                index,
            },
        );
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| {
                if this.push_seq == seq {
                    this.pushing = None;
                }
                match answer {
                    Ok(_) => this.live = Some((song_id, index)),
                    Err(e) => this.error = Some(clean_error(&e)),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// A plain click on a card of the open song: go live, or pick while
    /// selecting. A click while painting does nothing (lines take it).
    pub fn click_slide(
        &mut self,
        index: usize,
        gesture: Option<PickGesture>,
        cx: &mut Context<Self>,
    ) {
        let Some(song) = self.selected.clone() else {
            return;
        };
        if self.busy {
            return;
        }
        if let Some(g) = gesture {
            self.pick_slide(index, g, cx);
        } else if self.selecting {
            self.pick_slide(index, PickGesture::Toggle, cx);
        } else if self.paint.is_none() {
            self.push_slide(song, index, cx);
        }
    }

    pub fn pick_slide(&mut self, index: usize, gesture: PickGesture, cx: &mut Context<Self>) {
        let Some(song) = self.selected_song() else {
            return;
        };
        let order: Vec<usize> = (0..crate::grid::slide_count(song)).collect();
        self.slide_picks.pick(index, gesture, &order);
        cx.notify();
    }

    /// ⋯ → Select slides.
    pub fn start_selecting(&mut self, cx: &mut Context<Self>) {
        self.selecting = true;
        cx.notify();
    }

    /// Done, or Esc: picks clear and select mode ends.
    pub fn clear_slide_picks(&mut self, cx: &mut Context<Self>) {
        self.slide_picks.clear();
        self.selecting = false;
        cx.notify();
    }

    /// Esc in the song view: paint off first, then picks, then select mode.
    pub fn escape(&mut self, cx: &mut Context<Self>) -> bool {
        if self.paint.is_some() {
            self.paint = None;
        } else if !self.slide_picks.is_empty() || self.selecting {
            self.slide_picks.clear();
            self.selecting = false;
        } else {
            return false;
        }
        cx.notify();
        true
    }

    /// ← → ↑ ↓: step from what is live, across playlist songs.
    pub fn arrow(&mut self, forward: bool, cx: &mut Context<Self>) -> Option<crate::grid::Step> {
        if self.busy || self.pushing.is_some() || self.quick_edit.is_some() {
            return None;
        }
        let selected = self.selected.clone()?;
        let playlist: Vec<(String, usize)> = self
            .playlist_songs()
            .iter()
            .map(|s| (s.id.clone(), crate::grid::slide_count(s)))
            .collect();
        let live = self.live.clone().filter(|(id, _)| self.song(id).is_some());
        let step = crate::grid::step(
            live.as_ref().map(|(s, i)| (s.as_str(), *i)),
            &selected,
            &playlist,
            |id| self.song(id).map_or(0, crate::grid::slide_count),
            forward,
        )?;
        self.push_slide(step.song.clone(), step.slide, cx);
        Some(step)
    }

    /// A letter or digit: that section's first slide goes live.
    pub fn hotkey(&mut self, key: &str, cx: &mut Context<Self>) -> Option<usize> {
        if self.busy || self.pushing.is_some() || self.quick_edit.is_some() {
            return None;
        }
        let key = hotkey_from_event(key)?;
        let song = self.selected_song()?;
        let section = section_for_hotkey(&song.sections, &key)?;
        let first = *crate::grid::section_starts(song).get(section)?;
        if first >= crate::grid::slide_count(song) {
            return None;
        }
        let id = song.id.clone();
        self.push_slide(id, first, cx);
        Some(first)
    }

    /// Saves new sections for the open song, one write at a time.
    fn save_sections(
        &mut self,
        sections: Vec<LyricsSongSection>,
        failure: &'static str,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
    ) {
        let Some(song) = self.selected_song().cloned() else {
            return;
        };
        if self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        let call = ask(
            &self.backend,
            LyricsCall::Update(LyricsSong { sections, ..song }),
        );
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match answer {
                    Ok(LyricsAnswer::Song(updated)) => {
                        this.upsert(updated);
                        this.refresh();
                        then(this, cx);
                    }
                    Ok(_) => this.error = Some(failure.into()),
                    Err(e) => {
                        this.error = Some(if e.is_empty() {
                            failure.into()
                        } else {
                            clean_error(&e)
                        })
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Drag a slide beside another; it joins that slide's section.
    pub fn move_slide(&mut self, from: usize, target: usize, side: Side, cx: &mut Context<Self>) {
        let Some(song) = self.selected_song() else {
            return;
        };
        if from == target {
            return;
        }
        let sections = move_lyric_slide(&song.sections, from as i64, target as i64, side);
        if sections == song.sections {
            return;
        }
        self.save_sections(sections, "Could not save slide order", cx, |this, _| {
            this.live = None;
        });
    }

    /// The slides a menu action on `slide` applies to: the picks when it is
    /// one of them, else that slide.
    pub fn menu_slides(&self, slide: usize) -> Vec<usize> {
        if self.slide_picks.contains(&slide) {
            self.slide_picks.selected().iter().copied().collect()
        } else {
            vec![slide]
        }
    }

    /// Group: relabel the slides.
    pub fn label_slides(&mut self, slides: Vec<usize>, target: SlideLabel, cx: &mut Context<Self>) {
        let Some(song) = self.selected_song() else {
            return;
        };
        if slides.is_empty() {
            return;
        }
        let sections =
            label_lyric_slides(&song.sections, slides.iter().map(|&i| i as i64), &target);
        self.save_sections(sections, "Could not label those slides", cx, |this, cx| {
            this.clear_slide_picks(cx)
        });
    }

    /// Delete (the view confirms first).
    pub fn delete_slides(&mut self, slides: Vec<usize>, cx: &mut Context<Self>) {
        let Some(song) = self.selected_song() else {
            return;
        };
        if slides.is_empty() {
            return;
        }
        let sections = delete_lyric_slides(&song.sections, slides.iter().map(|&i| i as i64));
        self.save_sections(sections, "Could not delete those slides", cx, |this, cx| {
            this.clear_slide_picks(cx);
            this.live = None;
        });
    }

    /// Hot Key: `None` removes it; a key on another section moves here.
    pub fn set_hotkey(&mut self, section: usize, key: Option<String>, cx: &mut Context<Self>) {
        let Some(song) = self.selected_song() else {
            return;
        };
        let sections = set_section_hotkey(&song.sections, section, key.as_deref());
        self.save_sections(sections, "Could not save the hot key", cx, |_, _| {});
    }

    /// ⋯ → Assign hotkeys, or Clear hotkeys.
    pub fn set_hotkeys(&mut self, assign: bool, cx: &mut Context<Self>) {
        let Some(song) = self.selected_song() else {
            return;
        };
        let sections = if assign {
            auto_assign_hotkeys(&song.sections)
        } else {
            song.sections
                .iter()
                .cloned()
                .map(|s| LyricsSongSection { hotkey: None, ..s })
                .collect()
        };
        self.save_sections(sections, "Could not save the hotkeys", cx, |_, _| {});
    }

    /// The slide's own lines, as Copy text puts them on the clipboard.
    pub fn slide_text(&self, slide: usize) -> Option<String> {
        let song = self.selected_song()?;
        let grid = crate::grid::song_grid(song, &self.gloss);
        grid.into_iter().find_map(|g| match g {
            crate::grid::GridItem::Card(c) if c.index == slide => Some(
                c.lines
                    .iter()
                    .map(|l| l.text.clone())
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            _ => None,
        })
    }

    pub fn start_quick_edit(&mut self, slide: usize, cx: &mut Context<Self>) {
        let Some(song) = self.selected_song() else {
            return;
        };
        if let Some(lines) = lyric_slide_lines(&song.sections, slide as i64) {
            self.quick_edit = Some((slide, lines.join("\n")));
            cx.notify();
        }
    }

    pub fn cancel_quick_edit(&mut self, cx: &mut Context<Self>) {
        self.quick_edit = None;
        cx.notify();
    }

    /// Saves a quick edit; a slide on screen is pushed again with the new
    /// words.
    pub fn save_quick_edit(&mut self, text: String, cx: &mut Context<Self>) {
        let Some((slide, initial)) = self.quick_edit.take() else {
            return;
        };
        cx.notify();
        if text == initial {
            return;
        }
        let Some(song) = self.selected_song() else {
            return;
        };
        let lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
        let sections = replace_lyric_slide(&song.sections, slide as i64, &lines);
        if sections == song.sections {
            return;
        }
        let id = song.id.clone();
        let live = self.live_slide_of(&id) == Some(slide);
        self.save_sections(
            sections,
            "Could not save that slide",
            cx,
            move |this, cx| {
                if live {
                    this.push_slide(id, slide, cx);
                }
            },
        );
    }

    /// Paint mode: `Some(None)` clears a line's colour, `None` turns it off.
    pub fn set_paint(&mut self, paint: Option<Option<String>>, cx: &mut Context<Self>) {
        self.paint = paint;
        cx.notify();
    }

    /// A line clicked while painting.
    pub fn paint_line(&mut self, section: usize, line: usize, cx: &mut Context<Self>) {
        let Some(color) = self.paint.clone() else {
            return;
        };
        let Some(song) = self.selected_song() else {
            return;
        };
        let sections = song
            .sections
            .iter()
            .enumerate()
            .map(|(i, s)| {
                if i != section {
                    return s.clone();
                }
                let mut colors = s
                    .line_colors
                    .clone()
                    .unwrap_or_else(|| vec![None; s.lines.len()]);
                colors.resize(s.lines.len().max(colors.len()), None);
                if let Some(c) = colors.get_mut(line) {
                    *c = color.clone();
                }
                let any = colors
                    .iter()
                    .any(|c| c.as_deref().is_some_and(|c| !c.trim().is_empty()));
                LyricsSongSection {
                    line_colors: any.then_some(colors),
                    ..s.clone()
                }
            })
            .collect();
        self.save_sections(sections, "Could not save the line colour", cx, |_, _| {});
    }
}

// ─── Editing, import and translation (082) ──────────────────────────────────

use crate::reflow_edit::{EditSection, edit_sections, song_sections};
use kairo_domain::lyrics::translate::{sections_have_glosses, strip_line_glosses};

/// What waits on the discard question.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Leave {
    Select(String),
    NewSong,
    Edit(String),
}

/// The editor's fields (`EditState`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditFields {
    pub title: String,
    pub artist: String,
    pub copyright: String,
    pub ccli_number: String,
    pub sections: Vec<EditSection>,
}

/// The song being edited.
#[derive(Clone, Debug, PartialEq)]
pub struct Editing {
    pub new: bool,
    /// The song being edited; `None` for a new one.
    pub song_id: Option<String>,
    pub fields: EditFields,
    baseline: EditFields,
    pub saving: bool,
    pub error: Option<String>,
}

impl Editing {
    fn new(new: bool, song: Option<&LyricsSong>) -> Self {
        let fields = match song {
            Some(s) => EditFields {
                title: s.title.clone(),
                artist: s.artist.clone(),
                copyright: s.copyright.clone().unwrap_or_default(),
                ccli_number: s.ccli_number.clone().unwrap_or_default(),
                sections: edit_sections(s),
            },
            None => EditFields {
                title: String::new(),
                artist: String::new(),
                copyright: String::new(),
                ccli_number: String::new(),
                sections: vec![EditSection {
                    kind: kairo_domain::lyrics::LyricsSectionType::Verse,
                    label: "Verse 1".into(),
                    lines_text: String::new(),
                    hotkey: None,
                }],
            },
        };
        Self {
            new,
            song_id: song.map(|s| s.id.clone()),
            baseline: fields.clone(),
            fields,
            saving: false,
            error: None,
        }
    }

    /// Something changed since the editor opened.
    pub fn dirty(&self) -> bool {
        self.fields != self.baseline
    }
}

/// Which editor field changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Title,
    Artist,
    Copyright,
    Ccli,
}

/// `TRANSLATE_SOURCES`: languages a song can be translated to English from.
pub const TRANSLATE_SOURCES: [(&str, &str); 7] = [
    ("auto", "Auto-detect"),
    ("yo", "Yoruba"),
    ("ig", "Igbo"),
    ("ha", "Hausa"),
    ("fr", "French"),
    ("es", "Spanish"),
    ("pt", "Portuguese"),
];

impl Lyrics {
    pub fn editing(&self) -> Option<&Editing> {
        self.editing.as_ref()
    }
    pub fn import_open(&self) -> bool {
        self.import_open
    }
    pub fn translating(&self) -> bool {
        self.translating
    }
    pub fn translate_error(&self) -> Option<&str> {
        self.translate_error.as_deref()
    }
    pub fn translate_language(&self) -> &str {
        &self.translate_language
    }
    pub fn pending_leave(&self) -> Option<&Leave> {
        self.pending_leave.as_ref()
    }

    /// `leaveEditor`: closes a clean editor; a dirty one asks first.
    /// Returns whether the caller may go on now.
    fn leave_editor(&mut self, then: Leave, cx: &mut Context<Self>) -> bool {
        let Some(editing) = &self.editing else {
            return true;
        };
        if editing.dirty() {
            self.pending_leave = Some(then);
            cx.emit(LyricsEvent::ConfirmDiscard);
            return false;
        }
        self.editing = None;
        true
    }

    /// Discard the edits and do what was asked.
    pub fn discard_and_continue(&mut self, cx: &mut Context<Self>) {
        let Some(then) = self.pending_leave.take() else {
            return;
        };
        self.editing = None;
        match then {
            Leave::Select(id) => self.select_song(id, cx),
            Leave::NewSong => self.new_song(cx),
            Leave::Edit(id) => {
                self.selected = Some(id);
                self.edit(cx)
            }
        }
        cx.notify();
    }

    pub fn keep_editing(&mut self, cx: &mut Context<Self>) {
        self.pending_leave = None;
        cx.notify();
    }

    /// Edit the open song.
    pub fn edit(&mut self, cx: &mut Context<Self>) {
        let Some(song) = self.selected_song().cloned() else {
            return;
        };
        if let Some(e) = &self.editing {
            if e.song_id.as_deref() == Some(song.id.as_str()) {
                return;
            }
            if !self.leave_editor(Leave::Edit(song.id.clone()), cx) {
                return;
            }
        }
        self.quick_edit = None;
        self.translate_error = None;
        self.editing = Some(Editing::new(false, Some(&song)));
        cx.notify();
    }

    /// The row menu's Edit Song.
    pub fn edit_song(&mut self, id: String, cx: &mut Context<Self>) {
        if self.editing.is_some() && !self.leave_editor(Leave::Edit(id.clone()), cx) {
            return;
        }
        self.select_song(id, cx);
        self.edit(cx);
    }

    /// Cancel: a new song leaves nothing selected.
    pub fn cancel_edit(&mut self, cx: &mut Context<Self>) {
        if let Some(e) = self.editing.take()
            && e.new
        {
            self.selected = None;
        }
        cx.notify();
    }

    pub fn set_edit_field(&mut self, field: Field, value: String, cx: &mut Context<Self>) {
        let Some(e) = self.editing.as_mut() else {
            return;
        };
        let slot = match field {
            Field::Title => &mut e.fields.title,
            Field::Artist => &mut e.fields.artist,
            Field::Copyright => &mut e.fields.copyright,
            Field::Ccli => &mut e.fields.ccli_number,
        };
        if *slot != value {
            *slot = value;
            cx.notify();
        }
    }

    pub fn set_edit_sections(&mut self, sections: Vec<EditSection>, cx: &mut Context<Self>) {
        if let Some(e) = self.editing.as_mut()
            && e.fields.sections != sections
        {
            e.fields.sections = sections;
            cx.notify();
        }
    }

    /// Save (⌘S): a new song is imported from its marked text; an edit is
    /// written over the song.
    pub fn save_edit(&mut self, cx: &mut Context<Self>) {
        let held = self
            .editing
            .as_ref()
            .and_then(|e| e.song_id.as_deref())
            .and_then(|id| self.song(id))
            .cloned();
        let translating = self.translating;
        let Some(e) = self.editing.as_mut() else {
            return;
        };
        if e.saving || translating {
            return;
        }
        e.saving = true;
        e.error = None;
        let fields = e.fields.clone();
        let sections = song_sections(&fields.sections);
        let call = if e.new {
            let body = sections
                .iter()
                .map(|s| format!("[{}]\n{}", s.label, s.lines.join("\n")))
                .collect::<Vec<_>>()
                .join("\n\n");
            let title = fields.title.trim();
            LyricsCall::Import(kairo_domain::lyrics::LyricsImportSource::Text {
                title: if title.is_empty() {
                    "Untitled".into()
                } else {
                    title.to_owned()
                },
                artist: fields.artist.trim().to_owned(),
                text: body,
                copyright: Some(fields.copyright.trim().to_owned()).filter(|c| !c.is_empty()),
            })
        } else {
            let Some(song) = held else {
                e.saving = false;
                e.error = Some("This song is no longer in the library.".into());
                cx.notify();
                return;
            };
            let title = fields.title.trim();
            LyricsCall::Update(LyricsSong {
                title: if title.is_empty() {
                    song.title.clone()
                } else {
                    title.to_owned()
                },
                artist: fields.artist.trim().to_owned(),
                copyright: Some(fields.copyright.trim().to_owned()).filter(|c| !c.is_empty()),
                ccli_number: Some(fields.ccli_number.trim().to_owned()).filter(|c| !c.is_empty()),
                sections,
                updated_at: now_ms(),
                ..song
            })
        };
        let call = ask(&self.backend, call);
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| {
                match answer {
                    Ok(LyricsAnswer::Song(song)) => {
                        let id = song.id.clone();
                        this.upsert(song);
                        this.editing = None;
                        this.selected = Some(id);
                        this.refresh();
                    }
                    Ok(_) => this.editing = None,
                    Err(err) => {
                        if let Some(e) = this.editing.as_mut() {
                            e.saving = false;
                            e.error = Some(if err.is_empty() {
                                "Could not save the song.".into()
                            } else {
                                clean_error(&err)
                            });
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    // ─── Import ─────────────────────────────────────────────────────────────

    pub fn open_import(&mut self, cx: &mut Context<Self>) {
        self.import_tab = crate::import::Tab::File;
        self.import_open = true;
        cx.notify();
    }

    pub fn open_import_file(&mut self, cx: &mut Context<Self>) {
        self.import_tab = crate::import::Tab::File;
        self.import_open = true;
        cx.notify();
    }

    pub fn import_dropped_files(&mut self, paths: Vec<std::path::PathBuf>, cx: &mut Context<Self>) {
        self.import_tab = crate::import::Tab::File;
        self.pending_import_paths = paths;
        self.import_open = true;
        cx.notify();
    }

    pub fn import_tab(&self) -> crate::import::Tab {
        self.import_tab
    }

    pub fn take_pending_import_paths(&mut self) -> Vec<std::path::PathBuf> {
        std::mem::take(&mut self.pending_import_paths)
    }

    pub fn close_import(&mut self, cx: &mut Context<Self>) {
        self.import_open = false;
        self.pending_import_paths.clear();
        cx.notify();
    }

    /// Songs the import dialog saved; the last opens.
    pub fn import_finished(&mut self, songs: Vec<LyricsSong>, cx: &mut Context<Self>) {
        self.import_open = false;
        for song in songs {
            self.imported(song, cx);
        }
        cx.notify();
    }

    // ─── Translate ──────────────────────────────────────────────────────────

    /// Undo translation is offered while the song carries glosses.
    pub fn can_undo_translation(&self) -> bool {
        self.selected_song()
            .is_some_and(|s| sections_have_glosses(s.sections.iter().map(|x| x.lines.as_slice())))
    }

    /// Translate to English, sung in `language` (`auto` detects).
    pub fn translate(&mut self, language: &str, cx: &mut Context<Self>) {
        let Some(song) = self.selected_song().cloned() else {
            return;
        };
        if song.sections.is_empty() || self.translating {
            return;
        }
        self.translate_language = language.to_owned();
        self.translating = true;
        self.translate_error = None;
        let sections: Vec<LyricsSongSection> = song
            .sections
            .iter()
            .map(|s| LyricsSongSection {
                lines: strip_line_glosses(&s.lines),
                ..s.clone()
            })
            .collect();
        let backend = self.backend.clone();
        let call = ask(
            &backend,
            LyricsCall::Translate {
                sections,
                source_language: language.to_owned(),
                title: song.title.clone(),
                artist: song.artist.clone(),
            },
        );
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let saved = match answer {
                Ok(LyricsAnswer::Sections(translated)) => {
                    let sections = translated
                        .into_iter()
                        // Fresh glosses use the gloss colour.
                        .map(|s| LyricsSongSection {
                            line_colors: None,
                            ..s
                        })
                        .collect();
                    ask(
                        &backend,
                        LyricsCall::Update(LyricsSong { sections, ..song }),
                    )
                    .await
                }
                Ok(_) => Ok(LyricsAnswer::Done),
                Err(e) => Err(e),
            };
            let _ = this.update(cx, |this, cx| {
                this.translating = false;
                match saved {
                    Ok(LyricsAnswer::Song(s)) => {
                        this.upsert(s);
                        this.refresh();
                    }
                    Ok(_) => {}
                    Err(e) => this.translate_error = Some(clean_error(&e)),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Strips the glosses and their colours.
    pub fn undo_translation(&mut self, cx: &mut Context<Self>) {
        let Some(song) = self.selected_song().cloned() else {
            return;
        };
        if !self.can_undo_translation() {
            return;
        }
        self.translate_error = None;
        let sections = song
            .sections
            .iter()
            .map(|s| LyricsSongSection {
                lines: strip_line_glosses(&s.lines),
                line_colors: None,
                ..s.clone()
            })
            .collect();
        let call = ask(
            &self.backend,
            LyricsCall::Update(LyricsSong { sections, ..song }),
        );
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| {
                match answer {
                    Ok(LyricsAnswer::Song(s)) => {
                        this.upsert(s);
                        this.refresh();
                    }
                    Ok(_) => {}
                    Err(e) => this.translate_error = Some(clean_error(&e)),
                }
                cx.notify();
            });
        })
        .detach();
    }
}
