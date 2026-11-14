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
    /// ＋ New song: the song editor (082).
    NewSong,
    /// A song was opened from quick open; show the Lyrics page.
    Opened(String),
}

/// The ⌘ or ⇧ gesture on a setlist row (`selectGesture`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickGesture {
    Toggle,
    Range,
}

/// `useMultiSelect` over song ids.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Picks {
    selected: BTreeSet<String>,
    anchor: Option<String>,
}

impl Picks {
    pub fn pick(&mut self, key: String, gesture: PickGesture, order: &[String]) {
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
    pub fn select_all(&mut self, order: &[String]) {
        self.selected = order.iter().cloned().collect();
    }
    pub fn clear(&mut self) {
        self.selected.clear();
        self.anchor = None;
    }
    pub fn contains(&self, key: &str) -> bool {
        self.selected.contains(key)
    }
    pub fn len(&self) -> usize {
        self.selected.len()
    }
    pub fn is_empty(&self) -> bool {
        self.selected.is_empty()
    }
    fn retain(&mut self, keep: impl Fn(&str) -> bool) {
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

    pub fn new_song(&mut self, cx: &mut Context<Self>) {
        cx.emit(LyricsEvent::NewSong);
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
    fn imported(&mut self, song: LyricsSong, cx: &mut Context<Self>) {
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
            .filter(|id| self.picks.contains(id))
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
        if !ids.is_empty() {
            self.transfer(LyricsCall::ExportKairo(KairoExport::Songs(ids)), cx);
        }
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
