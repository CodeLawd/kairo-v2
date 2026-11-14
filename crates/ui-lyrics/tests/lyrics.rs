//! UI integration tests for the Lyrics library (prompt 080), driven by a
//! scripted core: the list, search with full-text hits, the web fallback and
//! its preview, favourites, delete, libraries, setlists with drops and
//! picks, the song usage report, the ⌘F finder and the list at 5,000 songs.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, Entity, TestAppContext, WindowBounds, WindowOptions, px,
    size,
};
use kairo_domain::collections::{
    Library, LibraryState, SetlistCommand, SetlistState, SongSetlist, with_song, without_song,
};
use kairo_domain::lyrics::{LyricsOnlinePreview, LyricsOnlineResult, LyricsSong};
use kairo_domain::usage::SongUsageEntry;
use serde_json::json;
use ui_lyrics::backend::{
    FeedSink, KairoExport, Listening, LyricsAnswer, LyricsBackend, LyricsCall, LyricsFeed,
    LyricsSnapshot, Reply,
};
use ui_lyrics::model::PickGesture;
use ui_lyrics::quick_open::{self, QuickOpen};
use ui_lyrics::rows::{Filter, ListFooter, Sort};
use ui_lyrics::view::ids;
use ui_lyrics::{Lyrics, LyricsEvent, LyricsWorkspace};

// ─── The scripted core ──────────────────────────────────────────────────────

type Script = Box<dyn Fn(&LyricsCall) -> Option<Result<LyricsAnswer, String>> + Send>;

#[derive(Default)]
struct State {
    snapshot: LyricsSnapshot,
    setlists: SetlistState,
    next_list: usize,
    calls: Vec<LyricsCall>,
    sink: Option<FeedSink>,
    script: Option<Script>,
}

struct Core(Mutex<State>);

impl Core {
    fn new(snapshot: LyricsSnapshot) -> Arc<Self> {
        Arc::new(Self(Mutex::new(State {
            setlists: snapshot.setlists.clone(),
            snapshot,
            ..Default::default()
        })))
    }
    fn script(
        &self,
        f: impl Fn(&LyricsCall) -> Option<Result<LyricsAnswer, String>> + Send + 'static,
    ) {
        self.0.lock().unwrap().script = Some(Box::new(f));
    }
    fn push(&self, feed: LyricsFeed) {
        let sink = self.0.lock().unwrap().sink.clone().expect("listening");
        sink(feed);
    }
    fn calls(&self) -> Vec<LyricsCall> {
        self.0.lock().unwrap().calls.clone()
    }
    fn count(&self, f: impl Fn(&LyricsCall) -> bool) -> usize {
        self.calls().iter().filter(|c| f(c)).count()
    }
}

/// A small setlist reducer, as `SetlistService::apply` behaves.
fn reduce(state: &mut State, command: &SetlistCommand) -> SetlistState {
    let lists = &mut state.setlists.lists;
    let target = |lists: &[SongSetlist], id: &Option<String>, active: &Option<String>| {
        let id = id.clone().or(active.clone());
        id.and_then(|id| lists.iter().position(|l| l.id == id))
    };
    match command {
        SetlistCommand::Create { name } => {
            state.next_list += 1;
            let id = format!("l{}", state.next_list);
            lists.insert(0, list(&id, name, &[]));
            state.setlists.active_id = Some(id);
        }
        SetlistCommand::Rename { list_id, name } => {
            if let Some(l) = lists.iter_mut().find(|l| &l.id == list_id) {
                l.name = name.trim().into();
            }
        }
        SetlistCommand::Delete { list_id } => {
            lists.retain(|l| &l.id != list_id);
            if state.setlists.active_id.as_ref() == Some(list_id) {
                state.setlists.active_id = lists.first().map(|l| l.id.clone());
            }
        }
        SetlistCommand::Add {
            song_id,
            index,
            list_id,
        } => {
            if let Some(i) = target(lists, list_id, &state.setlists.active_id) {
                lists[i].song_ids = with_song(&lists[i].song_ids, song_id, *index);
            }
        }
        SetlistCommand::RemoveSong { song_id, list_id } => {
            if let Some(i) = target(lists, list_id, &state.setlists.active_id) {
                lists[i].song_ids = without_song(&lists[i].song_ids, song_id);
            }
        }
        _ => {}
    }
    state.setlists.clone()
}

impl LyricsBackend for Core {
    fn snapshot(&self) -> LyricsSnapshot {
        self.0.lock().unwrap().snapshot.clone()
    }
    fn listen(&self, sink: FeedSink) -> Listening {
        self.0.lock().unwrap().sink = Some(sink);
        Box::new(())
    }
    fn call(&self, call: LyricsCall, reply: Reply) {
        let mut state = self.0.lock().unwrap();
        state.calls.push(call.clone());
        let scripted = state.script.as_ref().and_then(|s| s(&call));
        let answer = scripted.unwrap_or_else(|| {
            Ok(match &call {
                LyricsCall::Search(_) => LyricsAnswer::Songs(Vec::new()),
                LyricsCall::SearchOnline(_) => LyricsAnswer::Online(Vec::new()),
                LyricsCall::ToggleFavorite(_) => LyricsAnswer::Favorite(true),
                LyricsCall::Setlist(command) => LyricsAnswer::Setlists(reduce(&mut state, command)),
                LyricsCall::Libraries(_) => {
                    LyricsAnswer::Libraries(state.snapshot.libraries.clone())
                }
                LyricsCall::UsageList { .. } => LyricsAnswer::Usage(Vec::new()),
                _ => LyricsAnswer::Done,
            })
        });
        drop(state);
        reply(answer);
    }
}

// ─── Fixtures ───────────────────────────────────────────────────────────────

fn song(id: &str, title: &str, artist: &str, at: f64) -> LyricsSong {
    serde_json::from_value(json!({
        "id": id, "title": title, "artist": artist,
        "sections": [
            {"type": "verse", "label": "Verse 1", "lines": [format!("{title} first line"), "Second line"]},
            {"type": "chorus", "label": "Chorus", "lines": ["Hallelujah"]}
        ],
        "createdAt": at, "updatedAt": at,
    }))
    .unwrap()
}

fn list(id: &str, name: &str, songs: &[&str]) -> SongSetlist {
    SongSetlist {
        id: id.into(),
        name: name.into(),
        song_ids: songs.iter().map(|s| s.to_string()).collect(),
        created_at: 1.0,
        updated_at: 1.0,
        extra: Default::default(),
    }
}

fn hit(
    id: &str,
    title: &str,
    existing: Option<&str>,
    available: Option<bool>,
) -> LyricsOnlineResult {
    LyricsOnlineResult {
        lyrics_available: available,
        id: id.into(),
        provider: kairo_domain::lyrics::LyricsProvider::Lrclib,
        title: title.into(),
        artist: "Web Artist".into(),
        url: format!("https://example.test/{id}"),
        album: None,
        release_year: Some("2020".into()),
        snippet: Some("a line from the web".into()),
        existing_song_id: existing.map(str::to_owned),
    }
}

fn preview(title: &str) -> LyricsOnlinePreview {
    serde_json::from_value(json!({
        "review": {"status": "reviewed", "removedLines": 1.0},
        "title": title, "artist": "Web Artist", "provider": "lrclib", "url": "https://example.test/x",
        "sections": [{"type": "verse", "label": "Verse 1", "lines": ["From the web"]}]
    }))
    .unwrap()
}

fn base() -> LyricsSnapshot {
    LyricsSnapshot {
        songs: vec![
            song("c", "Cornerstone", "Hillsong", 3.0),
            song("a", "Amazing Grace", "John Newton", 1.0),
            song("b", "Blessed Be Your Name", "Matt Redman", 2.0),
        ],
        ..Default::default()
    }
}

fn library(id: &str, name: &str) -> Library {
    Library {
        id: id.into(),
        name: name.into(),
        created_at: 1.0,
        updated_at: 1.0,
        extra: Default::default(),
    }
}

// ─── Harness ────────────────────────────────────────────────────────────────

struct Opened {
    handle: AnyWindowHandle,
    page: Entity<LyricsWorkspace>,
    model: Entity<Lyrics>,
    events: Arc<Mutex<Vec<LyricsEvent>>>,
}

fn open(cx: &mut TestAppContext, core: Arc<Core>) -> Opened {
    cx.update(|cx| {
        gpui_kit::init(cx);
        kairo_ui_kit::init(kairo_ui_kit::Appearance::Dark, cx);
    });
    let slot: Arc<Mutex<Option<Entity<LyricsWorkspace>>>> = Default::default();
    let out = slot.clone();
    let handle = cx
        .update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        Default::default(),
                        size(px(1200.), px(760.)),
                    ))),
                    ..Default::default()
                },
                cx,
                move |window, cx| {
                    let page = cx.new(|cx| LyricsWorkspace::new(core, window, cx));
                    *out.lock().unwrap() = Some(page.clone());
                    page
                },
            )
        })
        .expect("open")
        .0;
    let page = slot.lock().unwrap().take().unwrap();
    let model = cx.read(|cx| page.read(cx).model().clone());
    let events: Arc<Mutex<Vec<LyricsEvent>>> = Default::default();
    let seen = events.clone();
    cx.update(|cx| {
        cx.subscribe(&model, move |_, e: &LyricsEvent, _| {
            seen.lock().unwrap().push(e.clone())
        })
        .detach()
    });
    let opened = Opened {
        handle,
        page,
        model,
        events,
    };
    draw(cx, &opened);
    opened
}

fn draw(cx: &mut TestAppContext, opened: &Opened) {
    cx.run_until_parked();
    cx.update_window(opened.handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    cx.run_until_parked();
}

fn exists(cx: &mut TestAppContext, opened: &Opened, id: &str) -> bool {
    let id: &'static str = Box::leak(id.to_owned().into_boxed_str());
    cx.update_window(opened.handle, move |_, window, _| {
        window.try_find(id).is_some()
    })
    .unwrap()
}

fn click(cx: &mut TestAppContext, opened: &Opened, id: &str) {
    let id: &'static str = Box::leak(id.to_owned().into_boxed_str());
    cx.update_window(opened.handle, move |_, window, cx| window.click(id, cx))
        .unwrap();
    draw(cx, opened);
}

/// Hovers a row so its hover-only buttons show, then clicks one.
fn hover_click(cx: &mut TestAppContext, opened: &Opened, row: &str, id: &str) {
    let row: &'static str = Box::leak(row.to_owned().into_boxed_str());
    cx.update_window(opened.handle, move |_, window, cx| window.hover(row, cx))
        .unwrap();
    draw(cx, opened);
    click(cx, opened, id);
}

fn press(cx: &mut TestAppContext, opened: &Opened, key: &'static str) {
    cx.update_window(opened.handle, |_, window, cx| window.press(key, cx))
        .unwrap();
    draw(cx, opened);
}

fn read<R>(cx: &mut TestAppContext, opened: &Opened, f: impl FnOnce(&Lyrics) -> R) -> R {
    cx.read(|cx| f(opened.model.read(cx)))
}

fn act(
    cx: &mut TestAppContext,
    opened: &Opened,
    f: impl FnOnce(&mut Lyrics, &mut gpui_kit::Context<Lyrics>),
) {
    cx.update(|cx| opened.model.update(cx, f));
    draw(cx, opened);
}

fn titles(cx: &mut TestAppContext, opened: &Opened) -> Vec<String> {
    read(cx, opened, |m| {
        m.shown()
            .iter()
            .map(|&i| m.songs()[i].title.clone())
            .collect()
    })
}

fn query(cx: &mut TestAppContext, opened: &Opened, q: &str) {
    let q = q.to_owned();
    act(cx, opened, |m, cx| m.set_query(q, cx));
}

/// Lets the 180 ms full-text debounce run.
fn after_debounce(cx: &mut TestAppContext, opened: &Opened) {
    cx.executor().advance_clock(Duration::from_millis(200));
    draw(cx, opened);
}

fn focus_list(cx: &mut TestAppContext, opened: &Opened) {
    let page = opened.page.clone();
    cx.update_window(opened.handle, move |_, window, cx| {
        let handle = page.read(cx).list_focus().clone();
        handle.focus(window, cx);
    })
    .unwrap();
}

fn open_finder(cx: &mut TestAppContext, opened: &Opened) -> Entity<QuickOpen> {
    let page = opened.page.clone();
    cx.update_window(opened.handle, move |_, window, cx| {
        page.update(cx, |p, cx| p.open_quick_open(window, cx))
    })
    .unwrap();
    draw(cx, opened);
    cx.read(|cx| opened.page.read(cx).quick_open().cloned().expect("open"))
}

// ─── The list ───────────────────────────────────────────────────────────────

#[gpui_kit::test]
fn the_list_shows_the_library_a_to_z_with_the_empty_pane(cx: &mut TestAppContext) {
    let opened = open(cx, Core::new(base()));
    assert_eq!(
        titles(cx, &opened),
        ["Amazing Grace", "Blessed Be Your Name", "Cornerstone"]
    );
    assert!(exists(cx, &opened, ids::EMPTY));
    assert!(exists(cx, &opened, ids::NEW_SONG));
    assert!(exists(cx, &opened, &ids::song(2)));
    assert!(!exists(cx, &opened, ids::NO_SONGS));
    assert!(
        !exists(cx, &opened, ids::PLAYLISTS),
        "no setlists: section hidden"
    );
}

#[gpui_kit::test]
fn an_empty_library_says_no_songs_yet(cx: &mut TestAppContext) {
    let opened = open(cx, Core::new(LyricsSnapshot::default()));
    assert!(exists(cx, &opened, ids::NO_SONGS));
    assert_eq!(read(cx, &opened, |m| m.footer()), ListFooter::NoSongs);
}

#[gpui_kit::test]
fn clicking_and_arrow_keys_select_songs(cx: &mut TestAppContext) {
    let opened = open(cx, Core::new(base()));
    click(cx, &opened, &ids::song(1));
    assert_eq!(
        read(cx, &opened, |m| m.selected_id().map(str::to_owned)),
        Some("b".into())
    );
    assert!(exists(cx, &opened, ids::SONG));
    focus_list(cx, &opened);
    press(cx, &opened, "down");
    assert_eq!(
        read(cx, &opened, |m| m.selected_id().map(str::to_owned)),
        Some("c".into())
    );
    press(cx, &opened, "down");
    assert_eq!(
        read(cx, &opened, |m| m.selected_id().map(str::to_owned)),
        Some("c".into())
    );
    press(cx, &opened, "up");
    press(cx, &opened, "up");
    assert_eq!(
        read(cx, &opened, |m| m.selected_id().map(str::to_owned)),
        Some("a".into())
    );
}

#[gpui_kit::test]
fn sort_orders_follow_the_menu(cx: &mut TestAppContext) {
    let opened = open(cx, Core::new(base()));
    act(cx, &opened, |m, cx| m.set_sort(Sort::Added, cx));
    assert_eq!(
        titles(cx, &opened),
        ["Cornerstone", "Blessed Be Your Name", "Amazing Grace"]
    );
    act(cx, &opened, |m, cx| m.set_sort(Sort::Artist, cx));
    assert_eq!(
        titles(cx, &opened),
        ["Cornerstone", "Amazing Grace", "Blessed Be Your Name"]
    );
}

#[gpui_kit::test]
fn the_star_toggles_a_favourite_and_favorites_narrows(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let opened = open(cx, core.clone());
    hover_click(cx, &opened, &ids::song(0), &ids::favorite(0));
    assert_eq!(
        core.count(|c| matches!(c, LyricsCall::ToggleFavorite(id) if id == "a")),
        1
    );
    assert_eq!(read(cx, &opened, |m| m.favorite_count()), 1);
    click(cx, &opened, ids::FAVORITES);
    assert_eq!(read(cx, &opened, |m| m.filter()), Filter::Favorites);
    assert_eq!(titles(cx, &opened), ["Amazing Grace"]);
}

#[gpui_kit::test]
fn a_rejected_favourite_shows_the_error(cx: &mut TestAppContext) {
    let core = Core::new(base());
    core.script(|c| {
        matches!(c, LyricsCall::ToggleFavorite(_)).then(|| Err("Error: disk full".into()))
    });
    let opened = open(cx, core);
    hover_click(cx, &opened, &ids::song(0), &ids::favorite(0));
    assert_eq!(
        read(cx, &opened, |m| m.error().map(str::to_owned)),
        Some("disk full".into())
    );
    assert!(exists(cx, &opened, ids::ERROR));
}

#[gpui_kit::test]
fn a_new_song_arrives_from_the_core_and_newer_copies_win(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let opened = open(cx, core.clone());
    let mut newer = song("a", "Amazing Grace (My Chains)", "Tomlin", 9.0);
    newer.updated_at = 9.0;
    let mut older = song("b", "Old copy", "", 0.0);
    older.updated_at = 0.0;
    core.push(LyricsFeed::Songs(vec![
        newer,
        older,
        song("d", "Doxology", "", 4.0),
    ]));
    draw(cx, &opened);
    assert_eq!(
        titles(cx, &opened),
        [
            "Amazing Grace (My Chains)",
            "Blessed Be Your Name",
            "Cornerstone",
            "Doxology"
        ]
    );
}

// ─── Search ─────────────────────────────────────────────────────────────────

#[gpui_kit::test]
fn the_filter_matches_locally_then_adds_full_text_hits(cx: &mut TestAppContext) {
    let core = Core::new(base());
    core.script(|c| match c {
        LyricsCall::Search(q) if q == "hallelujah" => Some(Ok(LyricsAnswer::Songs(vec![song(
            "c",
            "Cornerstone",
            "",
            3.0,
        )]))),
        _ => None,
    });
    let opened = open(cx, core.clone());
    query(cx, &opened, "redman");
    assert_eq!(titles(cx, &opened), ["Blessed Be Your Name"]);
    query(cx, &opened, "hallelujah");
    assert!(titles(cx, &opened).is_empty());
    assert!(exists(cx, &opened, ids::SEARCHING));
    assert_eq!(
        core.count(|c| matches!(c, LyricsCall::Search(_))),
        0,
        "debounced"
    );
    after_debounce(cx, &opened);
    assert_eq!(titles(cx, &opened), ["Cornerstone"]);
    assert_eq!(
        read(cx, &opened, |m| m.footer()),
        ListFooter::SearchOnlineForMore
    );
    // One character: no full-text search at all.
    // "redman" was replaced before its debounce ran; one character never searches.
    query(cx, &opened, "x");
    after_debounce(cx, &opened);
    assert_eq!(core.count(|c| matches!(c, LyricsCall::Search(_))), 1);
}

#[gpui_kit::test]
fn a_stale_full_text_answer_is_ignored(cx: &mut TestAppContext) {
    let core = Core::new(base());
    core.script(|c| match c {
        LyricsCall::Search(_) => Some(Ok(LyricsAnswer::Songs(vec![song(
            "c",
            "Cornerstone",
            "",
            3.0,
        )]))),
        _ => None,
    });
    let opened = open(cx, core.clone());
    query(cx, &opened, "zz");
    cx.executor().advance_clock(Duration::from_millis(100));
    query(cx, &opened, "zzz");
    after_debounce(cx, &opened);
    assert_eq!(
        core.count(|c| matches!(c, LyricsCall::Search(_))),
        1,
        "first was cancelled"
    );
    assert_eq!(titles(cx, &opened), ["Cornerstone"]);
}

#[gpui_kit::test]
fn a_failed_full_text_search_keeps_local_matches(cx: &mut TestAppContext) {
    let core = Core::new(base());
    core.script(|c| matches!(c, LyricsCall::Search(_)).then(|| Err("index busy".into())));
    let opened = open(cx, core);
    query(cx, &opened, "grace");
    after_debounce(cx, &opened);
    assert_eq!(titles(cx, &opened), ["Amazing Grace"]);
    assert!(!read(cx, &opened, |m| m.fts_searching()));
}

#[gpui_kit::test]
fn no_matches_offers_the_web_only_from_three_characters(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let opened = open(cx, core.clone());
    query(cx, &opened, "zq");
    after_debounce(cx, &opened);
    assert!(exists(cx, &opened, ids::NO_MATCH));
    assert!(!exists(cx, &opened, ids::SEARCH_ONLINE));
    query(cx, &opened, "zqx");
    after_debounce(cx, &opened);
    assert!(exists(cx, &opened, ids::SEARCH_ONLINE));
    assert_eq!(
        core.count(|c| matches!(c, LyricsCall::SearchOnline(_))),
        0,
        "never automatic"
    );
    click(cx, &opened, ids::SEARCH_ONLINE);
    assert_eq!(
        core.count(|c| matches!(c, LyricsCall::SearchOnline(q) if q == "zqx")),
        1
    );
    assert!(exists(cx, &opened, ids::WEB_EMPTY));
}

#[gpui_kit::test]
fn a_failed_web_search_shows_its_error_and_a_new_query_resets(cx: &mut TestAppContext) {
    let core = Core::new(base());
    core.script(|c| {
        matches!(c, LyricsCall::SearchOnline(_)).then(|| {
            Err("Error invoking remote method 'lyrics:searchOnline': Error: timeout".into())
        })
    });
    let opened = open(cx, core);
    query(cx, &opened, "zion");
    act(cx, &opened, |m, cx| m.search_online(cx));
    assert!(exists(cx, &opened, ids::WEB_ERROR));
    query(cx, &opened, "zions");
    assert!(!read(cx, &opened, |m| m.online().requested));
    assert!(!exists(cx, &opened, ids::WEB));
}

fn web_core() -> Arc<Core> {
    let core = Core::new(base());
    core.script(|c| match c {
        LyricsCall::SearchOnline(_) => Some(Ok(LyricsAnswer::Online(vec![
            hit("w1", "Way Maker", None, None),
            hit("w2", "Amazing Grace", Some("a"), None),
            hit("w3", "Blocked Song", None, Some(false)),
        ]))),
        LyricsCall::PreviewOnline(s) if s.title == "Way Maker" => {
            Some(Ok(LyricsAnswer::Preview(preview("Way Maker"))))
        }
        LyricsCall::ImportOnline(s) => {
            Some(Ok(LyricsAnswer::Song(song("w", &s.title, "Sinach", 10.0))))
        }
        _ => None,
    });
    core
}

#[gpui_kit::test]
fn a_web_hit_previews_once_and_imports_into_the_library(cx: &mut TestAppContext) {
    let core = web_core();
    let opened = open(cx, core.clone());
    query(cx, &opened, "way maker");
    act(cx, &opened, |m, cx| m.search_online(cx));
    click(cx, &opened, &ids::web(0));
    assert!(exists(cx, &opened, ids::PREVIEW));
    assert!(read(cx, &opened, |m| m
        .web_preview()
        .unwrap()
        .data
        .is_some()));
    // Back to it: from the cache.
    click(cx, &opened, &ids::web(1));
    click(cx, &opened, &ids::web(0));
    assert_eq!(
        core.count(|c| matches!(c, LyricsCall::PreviewOnline(s) if s.title == "Way Maker")),
        1
    );
    click(cx, &opened, ids::PREVIEW_IMPORT);
    assert_eq!(core.count(|c| matches!(c, LyricsCall::ImportOnline(_))), 1);
    assert_eq!(
        read(cx, &opened, |m| m.selected_id().map(str::to_owned)),
        Some("w".into())
    );
    assert_eq!(read(cx, &opened, |m| m.query().to_owned()), "");
    assert!(exists(cx, &opened, ids::SONG));
    assert!(!exists(cx, &opened, ids::PREVIEW));
}

#[gpui_kit::test]
fn a_failed_preview_says_why_and_import_stays_off(cx: &mut TestAppContext) {
    let core = Core::new(base());
    core.script(|c| match c {
        LyricsCall::SearchOnline(_) => Some(Ok(LyricsAnswer::Online(vec![hit(
            "w1", "Gone", None, None,
        )]))),
        LyricsCall::PreviewOnline(_) => Some(Err("Request failed with status code 404".into())),
        _ => None,
    });
    let opened = open(cx, core);
    query(cx, &opened, "gone");
    act(cx, &opened, |m, cx| m.search_online(cx));
    click(cx, &opened, &ids::web(0));
    assert_eq!(
        read(cx, &opened, |m| m.web_preview().unwrap().error.clone()),
        Some("Lyrics weren't found for this result.".into())
    );
}

#[gpui_kit::test]
fn an_unavailable_hit_is_not_fetched_and_opens_its_source(cx: &mut TestAppContext) {
    let core = web_core();
    let opened = open(cx, core.clone());
    query(cx, &opened, "blocked");
    act(cx, &opened, |m, cx| m.search_online(cx));
    click(cx, &opened, &ids::web(2));
    assert_eq!(core.count(|c| matches!(c, LyricsCall::PreviewOnline(_))), 0);
    click(cx, &opened, ids::PREVIEW_IMPORT);
    assert_eq!(
        core.count(|c| matches!(c, LyricsCall::OpenUrl(u) if u.ends_with("/w3"))),
        1
    );
    assert_eq!(core.count(|c| matches!(c, LyricsCall::ImportOnline(_))), 0);
}

#[gpui_kit::test]
fn in_library_opens_the_local_copy(cx: &mut TestAppContext) {
    let core = web_core();
    let opened = open(cx, core.clone());
    query(cx, &opened, "amazing");
    act(cx, &opened, |m, cx| m.search_online(cx));
    click(cx, &opened, &ids::web_action(1));
    assert_eq!(
        read(cx, &opened, |m| m.selected_id().map(str::to_owned)),
        Some("a".into())
    );
    assert_eq!(core.count(|c| matches!(c, LyricsCall::ImportOnline(_))), 0);
}

#[gpui_kit::test]
fn a_failed_import_is_reported_in_the_web_section(cx: &mut TestAppContext) {
    let core = Core::new(base());
    core.script(|c| match c {
        LyricsCall::SearchOnline(_) => {
            Some(Ok(LyricsAnswer::Online(vec![hit("w1", "Way", None, None)])))
        }
        LyricsCall::ImportOnline(_) => Some(Err("socket hang up".into())),
        _ => None,
    });
    let opened = open(cx, core);
    query(cx, &opened, "way");
    act(cx, &opened, |m, cx| m.search_online(cx));
    click(cx, &opened, &ids::web_action(0));
    assert_eq!(
        read(cx, &opened, |m| m.online().error.clone()),
        Some("Couldn't reach the lyrics source.".into())
    );
    assert!(read(cx, &opened, |m| m.importing_online().is_none()));
}

// ─── Delete ─────────────────────────────────────────────────────────────────

#[gpui_kit::test]
fn deleting_the_selected_song_clears_the_pane(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let opened = open(cx, core.clone());
    click(cx, &opened, &ids::song(0));
    act(cx, &opened, |m, cx| m.ask_delete("a".into(), cx));
    assert_eq!(
        read(cx, &opened, |m| m.delete_target().map(|s| s.title.clone())),
        Some("Amazing Grace".into())
    );
    act(cx, &opened, |m, cx| m.confirm_delete(cx));
    assert_eq!(
        core.count(|c| matches!(c, LyricsCall::Delete(id) if id == "a")),
        1
    );
    assert_eq!(titles(cx, &opened).len(), 2);
    assert!(read(cx, &opened, |m| m.selected_id().is_none()));
    assert!(exists(cx, &opened, ids::EMPTY));
}

#[gpui_kit::test]
fn a_cancelled_or_failed_delete_keeps_the_song(cx: &mut TestAppContext) {
    let core = Core::new(base());
    core.script(|c| matches!(c, LyricsCall::Delete(_)).then(|| Err("locked".into())));
    let opened = open(cx, core);
    act(cx, &opened, |m, cx| m.ask_delete("a".into(), cx));
    act(cx, &opened, |m, cx| m.cancel_delete(cx));
    assert!(read(cx, &opened, |m| m.delete_target().is_none()));
    act(cx, &opened, |m, cx| m.ask_delete("a".into(), cx));
    act(cx, &opened, |m, cx| m.confirm_delete(cx));
    assert_eq!(titles(cx, &opened).len(), 3);
    assert_eq!(
        read(cx, &opened, |m| m.error().map(str::to_owned)),
        Some("locked".into())
    );
}

// ─── Libraries ──────────────────────────────────────────────────────────────

#[gpui_kit::test]
fn a_library_narrows_the_list_and_favorites_within_it(cx: &mut TestAppContext) {
    let mut snapshot = base();
    snapshot.songs[1].is_favorite = Some(true);
    snapshot.libraries = LibraryState {
        libraries: vec![library("x", "Christmas")],
        assignments: [("a".to_string(), "x".to_string()), ("b".into(), "x".into())].into(),
        extra: Default::default(),
    };
    let opened = open(cx, Core::new(snapshot));
    act(cx, &opened, |m, cx| m.select_library("x".into(), cx));
    assert_eq!(
        titles(cx, &opened),
        ["Amazing Grace", "Blessed Be Your Name"]
    );
    act(cx, &opened, |m, cx| m.select_filter(Filter::Favorites, cx));
    assert_eq!(titles(cx, &opened), ["Amazing Grace"]);
    assert_eq!(
        read(cx, &opened, |m| m.library_counts().get("x").copied()),
        Some(2)
    );
}

// ─── Setlists ───────────────────────────────────────────────────────────────

fn setlist_snapshot() -> LyricsSnapshot {
    let mut s = base();
    s.setlists = SetlistState {
        lists: vec![list("s1", "Sunday", &["c", "a"]), list("s2", "Youth", &[])],
        active_id: Some("s1".into()),
        extra: Default::default(),
    };
    s
}

#[gpui_kit::test]
fn a_setlist_shows_its_order_and_lands_on_its_first_song(cx: &mut TestAppContext) {
    let opened = open(cx, Core::new(setlist_snapshot()));
    assert!(exists(cx, &opened, ids::PLAYLISTS));
    click(cx, &opened, &ids::playlist("s1"));
    assert_eq!(titles(cx, &opened), ["Cornerstone", "Amazing Grace"]);
    assert_eq!(
        read(cx, &opened, |m| m.selected_id().map(str::to_owned)),
        Some("c".into())
    );
    // Searching within keeps the order.
    query(cx, &opened, "o");
    assert_eq!(titles(cx, &opened), ["Cornerstone", "Amazing Grace"]);
    query(cx, &opened, "grace");
    assert_eq!(titles(cx, &opened), ["Amazing Grace"]);
}

#[gpui_kit::test]
fn new_playlist_creates_shows_and_renames_it(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let opened = open(cx, core.clone());
    act(cx, &opened, |m, cx| m.create_setlist(cx));
    assert_eq!(
        read(cx, &opened, |m| m.viewing_setlist_id().map(str::to_owned)),
        Some("l1".into())
    );
    assert_eq!(
        read(cx, &opened, |m| m.renaming_setlist().map(str::to_owned)),
        Some("l1".into())
    );
    assert!(exists(cx, &opened, ids::PLAYLIST_RENAME));
    // Empty or unchanged names send nothing.
    act(cx, &opened, |m, cx| m.commit_rename_setlist("   ", cx));
    act(cx, &opened, |m, cx| m.begin_rename_setlist("l1".into(), cx));
    act(cx, &opened, |m, cx| {
        m.commit_rename_setlist("Sunday service", cx)
    });
    assert_eq!(
        core.count(|c| matches!(c, LyricsCall::Setlist(SetlistCommand::Rename { .. }))),
        0
    );
    act(cx, &opened, |m, cx| m.begin_rename_setlist("l1".into(), cx));
    act(cx, &opened, |m, cx| m.commit_rename_setlist("Easter", cx));
    assert_eq!(
        read(cx, &opened, |m| m.setlists().lists[0].name.clone()),
        "Easter"
    );
}

#[gpui_kit::test]
fn escape_cancels_a_rename(cx: &mut TestAppContext) {
    let opened = open(cx, Core::new(setlist_snapshot()));
    act(cx, &opened, |m, cx| m.begin_rename_setlist("s1".into(), cx));
    press(cx, &opened, "escape");
    assert!(read(cx, &opened, |m| m.renaming_setlist().is_none()));
}

#[gpui_kit::test]
fn dropping_a_song_on_a_setlist_adds_it_once(cx: &mut TestAppContext) {
    let core = Core::new(setlist_snapshot());
    let opened = open(cx, core.clone());
    act(cx, &opened, |m, cx| {
        m.drop_on_setlist("s2".into(), "b".into(), cx)
    });
    assert_eq!(
        read(cx, &opened, |m| m.setlists().lists[1].song_ids.clone()),
        ["b"]
    );
    assert_eq!(
        read(cx, &opened, |m| m.landed_setlist().map(str::to_owned)),
        Some("s2".into())
    );
    cx.executor().advance_clock(Duration::from_millis(1300));
    draw(cx, &opened);
    assert!(read(cx, &opened, |m| m.landed_setlist().is_none()));
    act(cx, &opened, |m, cx| {
        m.drop_on_setlist("s2".into(), "b".into(), cx)
    });
    let events = opened.events.lock().unwrap().clone();
    assert!(events.contains(&LyricsEvent::Notify(
        "Added “Blessed Be Your Name” to Youth".into()
    )));
    assert!(events.contains(&LyricsEvent::Notify(
        "“Blessed Be Your Name” is already on Youth".into()
    )));
    assert_eq!(
        core.count(|c| matches!(c, LyricsCall::Setlist(SetlistCommand::Add { .. }))),
        1
    );
}

#[gpui_kit::test]
fn a_drop_on_the_section_starts_a_playlist_when_there_is_none(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let opened = open(cx, core.clone());
    act(cx, &opened, |m, cx| m.drop_on_setlists("a".into(), cx));
    let lists = read(cx, &opened, |m| m.setlists().lists.clone());
    assert_eq!(lists.len(), 1);
    assert_eq!(lists[0].name, "Sunday service");
    assert_eq!(lists[0].song_ids, ["a"]);
}

#[gpui_kit::test]
fn dropping_inside_the_setlist_on_screen_inserts_at_the_gap(cx: &mut TestAppContext) {
    let core = Core::new(setlist_snapshot());
    let opened = open(cx, core.clone());
    act(cx, &opened, |m, cx| m.select_setlist("s1".into(), cx));
    act(cx, &opened, |m, cx| m.drop_into_setlist("b".into(), 1, cx));
    assert_eq!(
        titles(cx, &opened),
        ["Cornerstone", "Blessed Be Your Name", "Amazing Grace"]
    );
    // Moving "c" to the end discounts its own slot.
    act(cx, &opened, |m, cx| m.drop_into_setlist("c".into(), 3, cx));
    assert_eq!(
        titles(cx, &opened),
        ["Blessed Be Your Name", "Amazing Grace", "Cornerstone"]
    );
    // A drop where it already is sends nothing.
    let before = core.calls().len();
    act(cx, &opened, |m, cx| m.drop_into_setlist("c".into(), 2, cx));
    assert_eq!(core.calls().len(), before);
}

#[gpui_kit::test]
fn picks_select_all_and_remove_from_the_setlist(cx: &mut TestAppContext) {
    let opened = open(cx, Core::new(setlist_snapshot()));
    act(cx, &opened, |m, cx| m.select_setlist("s1".into(), cx));
    act(cx, &opened, |m, cx| {
        m.pick_song("c".into(), PickGesture::Toggle, cx)
    });
    act(cx, &opened, |m, cx| {
        m.pick_song("a".into(), PickGesture::Range, cx)
    });
    assert_eq!(read(cx, &opened, |m| m.picks().len()), 2);
    assert!(exists(cx, &opened, ids::PICKED_REMOVE));
    press(cx, &opened, "escape");
    focus_list(cx, &opened);
    press(cx, &opened, "escape");
    assert!(read(cx, &opened, |m| m.picks().is_empty()));
    press(cx, &opened, "cmd-a");
    assert_eq!(read(cx, &opened, |m| m.picked_ids()), ["c", "a"]);
    act(cx, &opened, |m, cx| m.remove_picked(cx));
    assert!(titles(cx, &opened).is_empty());
    assert!(read(cx, &opened, |m| m.picks().is_empty()));
}

#[gpui_kit::test]
fn the_row_cross_removes_one_song(cx: &mut TestAppContext) {
    let core = Core::new(setlist_snapshot());
    let opened = open(cx, core.clone());
    act(cx, &opened, |m, cx| m.select_setlist("s1".into(), cx));
    hover_click(cx, &opened, &ids::song(0), &ids::remove(0));
    assert_eq!(titles(cx, &opened), ["Amazing Grace"]);
}

#[gpui_kit::test]
fn a_setlist_deleted_elsewhere_falls_back_to_the_library(cx: &mut TestAppContext) {
    let core = Core::new(setlist_snapshot());
    let opened = open(cx, core.clone());
    act(cx, &opened, |m, cx| m.select_setlist("s1".into(), cx));
    core.push(LyricsFeed::Setlists(SetlistState {
        lists: vec![list("s2", "Youth", &[])],
        active_id: Some("s2".into()),
        extra: Default::default(),
    }));
    draw(cx, &opened);
    assert!(read(cx, &opened, |m| m.viewing_setlist_id().is_none()));
    assert_eq!(titles(cx, &opened).len(), 3);
}

#[gpui_kit::test]
fn a_failed_setlist_command_shows_in_the_rail(cx: &mut TestAppContext) {
    let core = Core::new(setlist_snapshot());
    core.script(|c| {
        matches!(c, LyricsCall::Setlist(SetlistCommand::Delete { .. }))
            .then(|| Err("read-only".into()))
    });
    let opened = open(cx, core);
    act(cx, &opened, |m, cx| m.delete_setlist("s1".into(), cx));
    assert!(exists(cx, &opened, ids::RAIL_ERROR));
    assert_eq!(read(cx, &opened, |m| m.setlists().lists.len()), 2);
}

#[gpui_kit::test]
fn exports_carry_what_is_on_screen(cx: &mut TestAppContext) {
    let core = Core::new(setlist_snapshot());
    let opened = open(cx, core.clone());
    act(cx, &opened, |m, cx| m.export_songs(cx));
    assert_eq!(
        core.count(|c| matches!(c, LyricsCall::ExportKairo(_))),
        0,
        "nothing selected"
    );
    act(cx, &opened, |m, cx| m.select_setlist("s1".into(), cx));
    act(cx, &opened, |m, cx| m.export_songs(cx));
    act(cx, &opened, |m, cx| m.export_setlist("s1".into(), cx));
    act(cx, &opened, |m, cx| m.export_library(cx));
    let exports: Vec<KairoExport> = core
        .calls()
        .into_iter()
        .filter_map(|c| match c {
            LyricsCall::ExportKairo(e) => Some(e),
            _ => None,
        })
        .collect();
    assert_eq!(
        exports,
        [
            KairoExport::Songs(vec!["c".into(), "a".into()]),
            KairoExport::Setlist("s1".into()),
            KairoExport::Library
        ]
    );
}

#[gpui_kit::test]
fn new_song_asks_the_app_for_the_editor(cx: &mut TestAppContext) {
    let opened = open(cx, Core::new(base()));
    click(cx, &opened, ids::NEW_SONG);
    assert!(
        opened
            .events
            .lock()
            .unwrap()
            .contains(&LyricsEvent::NewSong)
    );
}

// ─── Song usage ─────────────────────────────────────────────────────────────

fn usage(id: &str, title: &str, ccli: &str, at: f64) -> SongUsageEntry {
    SongUsageEntry {
        song_id: id.into(),
        title: title.into(),
        artist: "A".into(),
        ccli_number: ccli.into(),
        copyright: String::new(),
        at,
        extra: Default::default(),
    }
}

#[gpui_kit::test]
fn the_usage_report_lists_90_days_and_exports(cx: &mut TestAppContext) {
    let core = Core::new(base());
    core.script(|c| match c {
        LyricsCall::UsageList { .. } => Some(Ok(LyricsAnswer::Usage(vec![
            usage("a", "Amazing Grace", "22025", 10.0),
            usage("a", "Amazing Grace", "22025", 20.0),
            usage("b", "Blessed", "", 15.0),
        ]))),
        LyricsCall::UsageExport { .. } => {
            Some(Ok(LyricsAnswer::Saved(Some("/tmp/usage.csv".into()))))
        }
        _ => None,
    });
    let opened = open(cx, core.clone());
    act(cx, &opened, |m, cx| m.open_usage(cx));
    assert!(exists(cx, &opened, ids::USAGE));
    let (from, to) = core
        .calls()
        .into_iter()
        .find_map(|c| match c {
            LyricsCall::UsageList { from, to } => Some((from, to)),
            _ => None,
        })
        .unwrap();
    let days = ((to - from) / 86_400_000.0).round();
    assert!((90.0..=92.0).contains(&days), "{days}");
    let (rows, missing) = read(cx, &opened, |m| {
        let u = m.usage().unwrap();
        (
            u.rows
                .iter()
                .map(|r| (r.title.clone(), r.uses))
                .collect::<Vec<_>>(),
            u.missing_ccli(),
        )
    });
    assert_eq!(
        rows,
        [("Amazing Grace".into(), 2.0), ("Blessed".into(), 1.0)]
    );
    assert_eq!(missing, 1);
    click(cx, &opened, ids::USAGE_EXPORT);
    assert_eq!(
        read(cx, &opened, |m| m.usage().unwrap().saved.clone()),
        Some("/tmp/usage.csv".into())
    );
    // A reversed or unreadable range is ignored.
    let lists = core.count(|c| matches!(c, LyricsCall::UsageList { .. }));
    act(cx, &opened, |m, cx| {
        m.set_usage_range("2026-11-10", "2026-11-01", cx)
    });
    act(cx, &opened, |m, cx| {
        m.set_usage_range("soon", "2026-11-01", cx)
    });
    assert_eq!(
        core.count(|c| matches!(c, LyricsCall::UsageList { .. })),
        lists
    );
    press(cx, &opened, "escape");
    assert!(read(cx, &opened, |m| m.usage().is_none()));
}

#[gpui_kit::test]
fn an_empty_usage_period_cannot_export(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let opened = open(cx, core.clone());
    act(cx, &opened, |m, cx| m.open_usage(cx));
    assert!(exists(cx, &opened, ids::USAGE_EMPTY));
    act(cx, &opened, |m, cx| m.export_usage(cx));
    assert_eq!(
        core.count(|c| matches!(c, LyricsCall::UsageExport { .. })),
        0
    );
    click(cx, &opened, ids::USAGE_CLOSE);
    assert!(!exists(cx, &opened, ids::USAGE));
}

// ─── Quick open ─────────────────────────────────────────────────────────────

fn finder<R>(cx: &mut TestAppContext, q: &Entity<QuickOpen>, f: impl FnOnce(&QuickOpen) -> R) -> R {
    cx.read(|cx| f(q.read(cx)))
}

fn finder_do(
    cx: &mut TestAppContext,
    opened: &Opened,
    q: &Entity<QuickOpen>,
    f: impl FnOnce(&mut QuickOpen, &mut gpui_kit::Context<QuickOpen>),
) {
    cx.update(|cx| q.update(cx, f));
    draw(cx, opened);
}

#[gpui_kit::test]
fn quick_open_lists_everything_then_ranks_and_opens_with_enter(cx: &mut TestAppContext) {
    let opened = open(cx, Core::new(base()));
    let q = open_finder(cx, &opened);
    assert!(exists(cx, &opened, quick_open::ids::DIALOG));
    assert_eq!(finder(cx, &q, |q| q.hits().len()), 3);
    finder_do(cx, &opened, &q, |q, cx| {
        q.set_query("hallelujah".into(), cx)
    });
    assert_eq!(finder(cx, &q, |q| q.hits().len()), 3, "lyric matches");
    finder_do(cx, &opened, &q, |q, cx| q.set_query("bless".into(), cx));
    assert_eq!(finder(cx, &q, |q| q.hits().len()), 1);
    finder_do(cx, &opened, &q, |q, cx| {
        q.key("enter", false, cx);
    });
    assert_eq!(
        read(cx, &opened, |m| m.selected_id().map(str::to_owned)),
        Some("b".into())
    );
    assert!(
        opened
            .events
            .lock()
            .unwrap()
            .contains(&LyricsEvent::Opened("b".into()))
    );
    assert!(
        cx.read(|cx| opened.page.read(cx).quick_open().is_none()),
        "closed"
    );
}

#[gpui_kit::test]
fn quick_open_arrows_click_preview_and_escape(cx: &mut TestAppContext) {
    let opened = open(cx, Core::new(base()));
    let q = open_finder(cx, &opened);
    finder_do(cx, &opened, &q, |q, cx| {
        q.key("down", false, cx);
        q.key("down", false, cx);
        q.key("down", false, cx);
    });
    assert_eq!(finder(cx, &q, |q| q.active()), 2);
    finder_do(cx, &opened, &q, |q, cx| {
        q.key("up", false, cx);
    });
    assert_eq!(finder(cx, &q, |q| q.active()), 1);
    assert!(
        !exists(cx, &opened, quick_open::ids::PREVIEW),
        "shut until a row is chosen"
    );
    click(cx, &opened, &quick_open::ids::row(0));
    assert!(exists(cx, &opened, quick_open::ids::PREVIEW));
    finder_do(cx, &opened, &q, |q, cx| q.set_query("c".into(), cx));
    assert!(
        !finder(cx, &q, |q| q.preview_open()),
        "a new query closes the preview"
    );
    finder_do(cx, &opened, &q, |q, cx| {
        q.key("escape", false, cx);
    });
    assert!(cx.read(|cx| opened.page.read(cx).quick_open().is_none()));
}

#[gpui_kit::test]
fn quick_open_searches_the_web_only_when_asked(cx: &mut TestAppContext) {
    let core = web_core();
    let opened = open(cx, core.clone());
    let q = open_finder(cx, &opened);
    finder_do(cx, &opened, &q, |q, cx| q.set_query("zz".into(), cx));
    assert!(exists(cx, &opened, quick_open::ids::EMPTY));
    assert!(
        !exists(cx, &opened, quick_open::ids::SEARCH_WEB),
        "under three characters"
    );
    finder_do(cx, &opened, &q, |q, cx| {
        q.key("enter", false, cx);
    });
    assert_eq!(core.count(|c| matches!(c, LyricsCall::SearchOnline(_))), 0);
    finder_do(cx, &opened, &q, |q, cx| q.set_query("zzz".into(), cx));
    assert!(exists(cx, &opened, quick_open::ids::SEARCH_WEB));
    finder_do(cx, &opened, &q, |q, cx| {
        q.key("enter", false, cx);
    });
    assert_eq!(core.count(|c| matches!(c, LyricsCall::SearchOnline(_))), 1);
    assert_eq!(finder(cx, &q, |q| q.online().len()), 3);
    // The first web hit: Enter imports and opens it.
    finder_do(cx, &opened, &q, |q, cx| {
        q.key("enter", true, cx);
    });
    assert_eq!(core.count(|c| matches!(c, LyricsCall::ImportOnline(_))), 1);
    assert_eq!(
        read(cx, &opened, |m| m.selected_id().map(str::to_owned)),
        Some("w".into())
    );
    assert!(
        read(cx, &opened, |m| m.song("w").is_some()),
        "added to the library"
    );
}

#[gpui_kit::test]
fn quick_open_reports_an_empty_or_failed_web_search(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let opened = open(cx, core.clone());
    let q = open_finder(cx, &opened);
    finder_do(cx, &opened, &q, |q, cx| {
        q.set_query("nothing here".into(), cx)
    });
    finder_do(cx, &opened, &q, |q, cx| q.search_web(cx));
    assert_eq!(
        finder(cx, &q, |q| q.online_error().map(str::to_owned)),
        Some(ui_lyrics::rows::NO_LYRICS_FOUND.into())
    );
    core.script(|c| matches!(c, LyricsCall::SearchOnline(_)).then(|| Err("HTTP 429".into())));
    finder_do(cx, &opened, &q, |q, cx| {
        q.set_query("nothing there".into(), cx)
    });
    finder_do(cx, &opened, &q, |q, cx| q.search_web(cx));
    assert_eq!(
        finder(cx, &q, |q| q.online_error().map(str::to_owned)),
        Some("That lyrics page blocked the request.".into())
    );
}

#[gpui_kit::test]
fn command_enter_queues_on_a_new_setlist_when_there_is_none(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let opened = open(cx, core.clone());
    let q = open_finder(cx, &opened);
    finder_do(cx, &opened, &q, |q, cx| {
        q.key("enter", true, cx);
    });
    let lists = read(cx, &opened, |m| m.setlists().lists.clone());
    assert_eq!(lists.len(), 1);
    assert_eq!(lists[0].name, "Sunday service");
    assert_eq!(lists[0].song_ids, ["a"]);
    assert_eq!(
        finder(cx, &q, |q| q.added().map(str::to_owned)),
        Some("a".into())
    );
    assert!(
        cx.read(|cx| opened.page.read(cx).quick_open().is_some()),
        "stays open"
    );
}

// ─── Scale ──────────────────────────────────────────────────────────────────

fn many(n: usize) -> LyricsSnapshot {
    LyricsSnapshot {
        songs: (0..n)
            .map(|i| {
                song(
                    &format!("s{i:05}"),
                    &format!("Song {i:05}"),
                    "Artist",
                    i as f64,
                )
            })
            .collect(),
        ..Default::default()
    }
}

#[gpui_kit::test]
fn five_thousand_songs_draw_only_the_visible_rows(cx: &mut TestAppContext) {
    let started = Instant::now();
    let opened = open(cx, Core::new(many(5000)));
    let first = started.elapsed();
    assert_eq!(titles(cx, &opened).len(), 5000);
    assert!(exists(cx, &opened, &ids::song(0)));
    assert!(!exists(cx, &opened, &ids::song(200)), "virtualised");
    assert!(!exists(cx, &opened, &ids::song(4999)));
    // The keyboard reaches the end, and the list follows.
    act(cx, &opened, |m, cx| m.select_song("s04998".into(), cx));
    focus_list(cx, &opened);
    press(cx, &opened, "down");
    draw(cx, &opened);
    assert_eq!(
        read(cx, &opened, |m| m.selected_id().map(str::to_owned)),
        Some("s04999".into())
    );
    assert!(exists(cx, &opened, &ids::song(4999)));
    // A query over 5,000 songs.
    let t = Instant::now();
    query(cx, &opened, "song 0499");
    let filter = t.elapsed();
    assert_eq!(titles(cx, &opened).len(), 10);
    let q = open_finder(cx, &opened);
    let t = Instant::now();
    finder_do(cx, &opened, &q, |q, cx| q.set_query("song 0499".into(), cx));
    let ranked = t.elapsed();
    assert_eq!(finder(cx, &q, |q| q.hits().len()), 10);
    assert!(
        !exists(cx, &opened, &quick_open::ids::row(200)),
        "finder virtualised"
    );
    eprintln!(
        "5000 songs (debug, test platform): open + first frames {first:?}, filter {filter:?}, quick-open rank {ranked:?}"
    );
}

/// `cargo test --release -p ui-lyrics --test lyrics library_open_bench -- --ignored --nocapture`:
/// page open to the first drawn, selectable row at 1,000 and 5,000 songs,
/// five runs each, on the GPUI test platform.
#[gpui_kit::test]
#[ignore]
fn library_open_bench(cx: &mut TestAppContext) {
    for n in [1000, 5000] {
        let mut runs: Vec<Duration> = (0..5)
            .map(|_| {
                let snapshot = many(n);
                let started = Instant::now();
                let opened = open(cx, Core::new(snapshot));
                assert!(exists(cx, &opened, &ids::song(0)));
                let elapsed = started.elapsed();
                cx.update_window(opened.handle, |_, window, _| window.remove_window())
                    .unwrap();
                cx.run_until_parked();
                elapsed
            })
            .collect();
        runs.sort();
        eprintln!(
            "{n} songs: median {:?}, min {:?}, max {:?}",
            runs[2], runs[0], runs[4]
        );
    }
}
