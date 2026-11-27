//! UI integration tests for the media dock (prompt 083), driven by a
//! scripted core: open and size, the persisted UI keys, the folder empty
//! state, push and its failure text, playlists with missing files,
//! rename, copy/cut/paste, reorder, picks, imports and drops, posters,
//! private preview playback, the inspector with trim and colour, and the
//! grid at 2,000 items. Behaviours follow Electron's `media-*` tests at the
//! UI level.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, Entity, TestAppContext, WindowBounds, WindowOptions, px,
    size,
};
use kairo_domain::media::MediaLibrary;
use kairo_domain::media_order::DropPosition;
use serde_json::{Value, json};
use ui_media::backend::{
    FeedSink, Frame, ImportKind, Listening, MediaAnswer, MediaBackend, MediaCall, MediaFeed,
    PreviewSource, Reply,
};
use ui_media::inspector::{self, Inspector};
use ui_media::model::{PickGesture, Poster};
use ui_media::rows::{self, Edge, Selection};
use ui_media::view::ids;
use ui_media::{MediaDock, MediaDockView, MediaEvent};

type Script = Box<dyn Fn(&MediaCall) -> Option<Result<MediaAnswer, String>> + Send>;

struct State {
    library: MediaLibrary,
    calls: Vec<MediaCall>,
    ui: std::collections::HashMap<String, Value>,
    sink: Option<FeedSink>,
    script: Option<Script>,
}

struct Core {
    state: Mutex<State>,
    opens: AtomicUsize,
    seeks: Arc<AtomicUsize>,
}

struct FakeSource {
    t: f64,
    seeks: Arc<AtomicUsize>,
}

impl PreviewSource for FakeSource {
    fn next_frame(&mut self) -> Result<Option<Frame>, String> {
        if self.t >= 2.0 {
            return Ok(None);
        }
        let f = Frame {
            width: 4,
            height: 2,
            pixels: vec![120; 32],
            seconds: self.t,
        };
        self.t += 1. / 30.;
        Ok(Some(f))
    }
    fn seek(&mut self, seconds: f64) -> Result<(), String> {
        self.seeks.fetch_add(1, Ordering::SeqCst);
        self.t = seconds;
        Ok(())
    }
    fn duration(&self) -> Option<f64> {
        Some(2.0)
    }
    fn interval(&self) -> f64 {
        1. / 30.
    }
}

impl MediaBackend for Core {
    fn library(&self) -> MediaLibrary {
        self.state.lock().unwrap().library.clone()
    }
    fn listen(&self, sink: FeedSink) -> Listening {
        self.state.lock().unwrap().sink = Some(sink);
        Box::new(())
    }
    fn call(&self, call: MediaCall, reply: Reply) {
        let mut s = self.state.lock().unwrap();
        s.calls.push(call.clone());
        let scripted = s.script.as_ref().and_then(|f| f(&call));
        let answer = scripted.unwrap_or_else(|| {
            let lib = &mut s.library;
            Ok(match &call {
                MediaCall::Push(id) => {
                    lib.live_item_id = Some(id.clone());
                    MediaAnswer::Pushed {
                        applied: true,
                        reason: None,
                    }
                }
                MediaCall::CreatePlaylist(name) => {
                    let id = format!("p{}", lib.playlists.len() + 1);
                    lib.playlists.push(
                        serde_json::from_value(
                            json!({"id": id, "name": name, "itemIds": [], "createdAt": 0.0}),
                        )
                        .unwrap(),
                    );
                    MediaAnswer::Library(lib.clone())
                }
                MediaCall::RenamePlaylist { id, name } => {
                    if let Some(p) = lib.playlists.iter_mut().find(|p| &p.id == id) {
                        p.name = name.clone();
                    }
                    MediaAnswer::Library(lib.clone())
                }
                MediaCall::DeletePlaylist(id) => {
                    lib.playlists.retain(|p| &p.id != id);
                    MediaAnswer::Library(lib.clone())
                }
                MediaCall::SetPlaylistItems { id, item_ids } => {
                    if let Some(p) = lib.playlists.iter_mut().find(|p| &p.id == id) {
                        p.item_ids = item_ids.clone();
                    }
                    MediaAnswer::Library(lib.clone())
                }
                MediaCall::SetItemOrder(ids) => {
                    let mut items = std::mem::take(&mut lib.items);
                    items
                        .sort_by_key(|i| ids.iter().position(|x| *x == i.id).unwrap_or(usize::MAX));
                    lib.items = items;
                    MediaAnswer::Library(lib.clone())
                }
                MediaCall::DeleteItem(id) => {
                    lib.items.retain(|i| &i.id != id);
                    if lib.live_item_id.as_ref() == Some(id) {
                        lib.live_item_id = None;
                    }
                    MediaAnswer::Library(lib.clone())
                }
                MediaCall::RenameItem { id, name } => {
                    if let Some(i) = lib.items.iter_mut().find(|i| &i.id == id) {
                        i.name = name.clone();
                    }
                    MediaAnswer::Library(lib.clone())
                }
                MediaCall::ClipboardHasFiles => MediaAnswer::Flag(false),
                MediaCall::SetPlayback { id, patch, .. } => {
                    let mut v = serde_json::to_value(lib.playback.get(id)).unwrap_or(Value::Null);
                    if !v.is_object() {
                        v = json!({});
                    }
                    v.as_object_mut().unwrap().extend(patch.clone());
                    lib.playback.insert(
                        id.clone(),
                        kairo_domain::media::normalize_media_playback(&v),
                    );
                    MediaAnswer::Library(lib.clone())
                }
                MediaCall::Poster(id) if id.starts_with("bad") => return_err(),
                MediaCall::Poster(_) => MediaAnswer::Poster {
                    image: "/tmp/poster.png".into(),
                    duration: Some(2.0),
                },
                MediaCall::RevealItem(_) | MediaCall::CopyItems(_) | MediaCall::CutItems { .. } => {
                    MediaAnswer::Done
                }
                _ => MediaAnswer::Library(lib.clone()),
            })
        });
        let answer = match answer {
            Ok(MediaAnswer::Done) if matches!(call, MediaCall::Poster(_)) => Err("no video".into()),
            other => other,
        };
        drop(s);
        reply(answer);
    }
    fn open_preview(&self, _path: &str, _w: u32) -> Result<Box<dyn PreviewSource>, String> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(FakeSource {
            t: 0.,
            seeks: self.seeks.clone(),
        }))
    }
    fn read_ui(&self, key: &str) -> Option<Value> {
        self.state.lock().unwrap().ui.get(key).cloned()
    }
    fn write_ui(&self, key: &str, value: Value) {
        self.state.lock().unwrap().ui.insert(key.into(), value);
    }
}

fn return_err() -> MediaAnswer {
    MediaAnswer::Done
}

impl Core {
    fn new(library: MediaLibrary) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                library,
                calls: Vec::new(),
                ui: Default::default(),
                sink: None,
                script: None,
            }),
            opens: AtomicUsize::new(0),
            seeks: Arc::new(AtomicUsize::new(0)),
        })
    }
    fn script(
        &self,
        f: impl Fn(&MediaCall) -> Option<Result<MediaAnswer, String>> + Send + 'static,
    ) {
        self.state.lock().unwrap().script = Some(Box::new(f));
    }
    fn calls(&self) -> Vec<MediaCall> {
        self.state.lock().unwrap().calls.clone()
    }
    fn ui(&self, key: &str) -> Option<Value> {
        self.state.lock().unwrap().ui.get(key).cloned()
    }
    fn set_ui(&self, key: &str, v: Value) {
        self.state.lock().unwrap().ui.insert(key.into(), v);
    }
    fn push(&self, feed: MediaFeed) {
        let sink = self.state.lock().unwrap().sink.clone().unwrap();
        sink(feed);
    }
}

fn library(n: usize, videos: &[&str]) -> MediaLibrary {
    let mut items: Vec<Value> = (0..n)
        .map(|i| json!({"id": format!("i{i}"), "name": format!("Image {i}"), "path": format!("/m/i{i}.jpg"), "kind": "image", "ext": "jpg", "folder": "", "sizeBytes": 2048.0}))
        .collect();
    for v in videos {
        items.push(json!({"id": v, "name": format!("Video {v}"), "path": format!("/m/{v}.mp4"), "kind": "video", "ext": "mp4", "folder": "", "sizeBytes": 1048576.0}));
    }
    serde_json::from_value(json!({
        "folder": "/m", "folders": [], "items": items,
        "playlists": [{"id": "pl", "name": "Easter", "itemIds": ["i2", "gone", "i0"], "createdAt": 0.0}],
        "playback": {}, "liveItemId": null, "livePaused": false, "error": null,
    }))
    .unwrap()
}

struct Opened {
    handle: AnyWindowHandle,
    view: Entity<MediaDockView>,
    model: Entity<MediaDock>,
    events: Arc<Mutex<Vec<MediaEvent>>>,
}

fn open(cx: &mut TestAppContext, core: Arc<Core>, dock_open: bool) -> Opened {
    cx.update(|cx| {
        gpui_kit::init(cx);
        kairo_ui_kit::init(kairo_ui_kit::Appearance::Dark, cx);
    });
    let slot: Arc<Mutex<Option<Entity<MediaDockView>>>> = Default::default();
    let out = slot.clone();
    let handle = cx
        .update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        Default::default(),
                        size(px(1280.), px(900.)),
                    ))),
                    ..Default::default()
                },
                cx,
                move |window, cx| {
                    let model = cx.new(|cx| MediaDock::new(core, 900., cx));
                    let view = cx.new(|cx| MediaDockView::new(model, window, cx));
                    *out.lock().unwrap() = Some(view.clone());
                    view
                },
            )
        })
        .expect("open")
        .0;
    let view = slot.lock().unwrap().take().unwrap();
    let model = cx.read(|cx| view.read(cx).model().clone());
    let events: Arc<Mutex<Vec<MediaEvent>>> = Default::default();
    let seen = events.clone();
    cx.update(|cx| {
        cx.subscribe(&model, move |_, e: &MediaEvent, _| {
            seen.lock().unwrap().push(e.clone())
        })
        .detach()
    });
    let o = Opened {
        handle,
        view,
        model,
        events,
    };
    if dock_open {
        act(cx, &o, |m, cx| m.set_open(true, cx));
    }
    draw(cx, &o);
    o
}

fn draw(cx: &mut TestAppContext, o: &Opened) {
    cx.run_until_parked();
    for _ in 0..2 {
        cx.update_window(o.handle, |_, w, cx| {
            w.render_frame(cx);
            kairo_ui_kit::a11y_audit::check(w);
        })
        .unwrap();
        cx.run_until_parked();
    }
}

fn exists(cx: &mut TestAppContext, o: &Opened, id: &str) -> bool {
    let id: &'static str = Box::leak(id.to_owned().into_boxed_str());
    cx.update_window(o.handle, move |_, w, _| w.try_find(id).is_some())
        .unwrap()
}

fn click(cx: &mut TestAppContext, o: &Opened, id: &str) {
    let id: &'static str = Box::leak(id.to_owned().into_boxed_str());
    cx.update_window(o.handle, move |_, w, cx| w.click(id, cx))
        .unwrap();
    draw(cx, o);
}

fn press(cx: &mut TestAppContext, o: &Opened, key: &'static str) {
    cx.update_window(o.handle, |_, w, cx| w.press(key, cx))
        .unwrap();
    draw(cx, o);
}

fn act(
    cx: &mut TestAppContext,
    o: &Opened,
    f: impl FnOnce(&mut MediaDock, &mut gpui_kit::Context<MediaDock>),
) {
    cx.update(|cx| o.model.update(cx, f));
    draw(cx, o);
}

fn read<R>(cx: &mut TestAppContext, o: &Opened, f: impl FnOnce(&MediaDock) -> R) -> R {
    cx.read(|cx| f(o.model.read(cx)))
}

fn shown(cx: &mut TestAppContext, o: &Opened) -> Vec<String> {
    read(cx, o, |m| {
        m.visible().iter().map(|i| i.id.clone()).collect()
    })
}

// ─── Open, size and stored state ────────────────────────────────────────────

#[gpui_kit::test]
fn the_dock_opens_from_the_header_and_reports_its_state(cx: &mut TestAppContext) {
    let o = open(cx, Core::new(library(3, &[])), false);
    assert!(!exists(cx, &o, ids::GRID));
    act(cx, &o, |m, cx| m.toggle(cx));
    assert!(exists(cx, &o, ids::GRID));
    assert!(o.events.lock().unwrap().contains(&MediaEvent::Header));
    click(cx, &o, ids::CLOSE);
    assert!(!read(cx, &o, |m| m.is_open()));
}

#[gpui_kit::test]
fn stored_ui_state_is_read_and_written(cx: &mut TestAppContext) {
    let core = Core::new(library(3, &[]));
    core.set_ui(rows::HEIGHT_KEY, json!(400));
    core.set_ui(rows::ZOOM_KEY, json!(150));
    core.set_ui(rows::SIDEBAR_HIDDEN_KEY, json!("true"));
    core.set_ui(rows::SIDEBAR_WIDTH_KEY, json!(300));
    let o = open(cx, core.clone(), true);
    assert_eq!(
        read(cx, &o, |m| (
            m.height(),
            m.zoom(),
            m.sidebar_open(),
            m.sidebar_width()
        )),
        (400., 150., false, 300.)
    );
    assert!(!exists(cx, &o, ids::SIDEBAR));
    click(cx, &o, ids::SIDEBAR_TOGGLE);
    assert!(exists(cx, &o, ids::SIDEBAR));
    assert_eq!(core.ui(rows::SIDEBAR_HIDDEN_KEY), Some(json!("false")));
    act(cx, &o, |m, cx| m.set_zoom(90., cx));
    assert_eq!(core.ui(rows::ZOOM_KEY), Some(json!(90.0)));
    act(cx, &o, |m, cx| m.set_sidebar_width(1000., cx));
    assert_eq!(
        core.ui(rows::SIDEBAR_WIDTH_KEY),
        Some(json!(rows::SIDEBAR_MAX))
    );
    act(cx, &o, |m, cx| {
        m.resize_key("up", cx);
    });
    assert_eq!(core.ui(rows::HEIGHT_KEY), Some(json!(424.0)));
}

#[gpui_kit::test]
fn dragging_the_edge_resizes_and_far_down_closes(cx: &mut TestAppContext) {
    let core = Core::new(library(3, &[]));
    let o = open(cx, core.clone(), true);
    cx.update(|cx| o.view.update(cx, |v, cx| v.drag_height(-60., cx)));
    draw(cx, &o);
    assert_eq!(read(cx, &o, |m| m.height()), rows::DEFAULT_HEIGHT + 60.);
    assert_eq!(core.ui(rows::HEIGHT_KEY), Some(json!(348.0)));
    cx.update(|cx| o.view.update(cx, |v, cx| v.drag_height(400., cx)));
    draw(cx, &o);
    assert!(!read(cx, &o, |m| m.is_open()), "dismissed");
    assert_eq!(read(cx, &o, |m| m.height()), 348., "keeps its size");
    // A window too short for the dock puts it away.
    act(cx, &o, |m, cx| m.set_open(true, cx));
    act(cx, &o, |m, cx| m.window_resized(400., cx));
    assert!(!read(cx, &o, |m| m.is_open()));
    // Esc on the edge closes too.
    act(cx, &o, |m, cx| m.window_resized(900., cx));
    act(cx, &o, |m, cx| m.set_open(true, cx));
    act(cx, &o, |m, cx| {
        m.resize_key("escape", cx);
    });
    assert!(!read(cx, &o, |m| m.is_open()));
}

#[gpui_kit::test]
fn no_folder_shows_the_choose_folder_state(cx: &mut TestAppContext) {
    let mut empty = library(0, &[]);
    empty.folder.clear();
    empty.playlists.clear();
    let core = Core::new(empty);
    core.script(|c| {
        matches!(c, MediaCall::ChooseFolder).then(|| Ok(MediaAnswer::Library(library(2, &[]))))
    });
    let o = open(cx, core.clone(), true);
    assert!(exists(cx, &o, ids::EMPTY));
    click(cx, &o, ids::CHOOSE);
    assert_eq!(
        read(cx, &o, |m| m.selection().cloned()),
        Some(Selection::All)
    );
    assert!(exists(cx, &o, ids::GRID));
}

// ─── Push ───────────────────────────────────────────────────────────────────

#[gpui_kit::test]
fn a_click_pushes_and_a_failed_push_says_why(cx: &mut TestAppContext) {
    let core = Core::new(library(3, &[]));
    let o = open(cx, core.clone(), true);
    click(cx, &o, &ids::card("i1"));
    assert!(core.calls().contains(&MediaCall::Push("i1".into())));
    core.push(MediaFeed::Library(core.library()));
    draw(cx, &o);
    assert_eq!(read(cx, &o, |m| m.live_name()), Some("Image 1".into()));
    core.script(|c| {
        matches!(c, MediaCall::Push(_)).then(|| {
            Ok(MediaAnswer::Pushed {
                applied: false,
                reason: Some("No screen is on".into()),
            })
        })
    });
    click(cx, &o, &ids::card("i2"));
    assert_eq!(
        read(cx, &o, |m| m.error().map(str::to_owned)),
        Some("No screen is on".into())
    );
    assert!(exists(cx, &o, ids::ERROR));
    core.script(|c| {
        matches!(c, MediaCall::Push(_))
            .then(|| Err("That background is no longer in the folder.".into()))
    });
    click(cx, &o, &ids::card("i0"));
    assert_eq!(
        read(cx, &o, |m| m.error().map(str::to_owned)),
        Some("That background is no longer in the folder.".into())
    );
    assert!(read(cx, &o, |m| m.busy().is_none()));
}

#[gpui_kit::test]
fn deleting_the_live_item_takes_it_off(cx: &mut TestAppContext) {
    let core = Core::new(library(3, &[]));
    let o = open(cx, core.clone(), true);
    click(cx, &o, &ids::card("i1"));
    act(cx, &o, |m, cx| m.delete_item("i1".into(), cx));
    assert!(core.calls().contains(&MediaCall::DeleteItem("i1".into())));
    assert_eq!(read(cx, &o, |m| m.live_name()), None);
    assert_eq!(shown(cx, &o), ["i0", "i2"]);
}

// ─── Playlists ──────────────────────────────────────────────────────────────

#[gpui_kit::test]
fn a_playlist_keeps_its_order_and_names_missing_files(cx: &mut TestAppContext) {
    let o = open(cx, Core::new(library(3, &[])), true);
    click(cx, &o, &ids::playlist("pl"));
    assert_eq!(shown(cx, &o), ["i2", "i0"]);
    assert!(exists(cx, &o, ids::MISSING));
    assert!(exists(cx, &o, ids::ADD_TILE));
    assert!(exists(cx, &o, ids::ADD_MEDIA));
}

#[gpui_kit::test]
fn new_playlists_are_named_in_place(cx: &mut TestAppContext) {
    let core = Core::new(library(3, &[]));
    let o = open(cx, core.clone(), true);
    click(cx, &o, ids::NEW_PLAYLIST);
    assert!(
        core.calls()
            .contains(&MediaCall::CreatePlaylist("Untitled playlist".into()))
    );
    assert_eq!(
        read(cx, &o, |m| m.selection().cloned()),
        Some(Selection::Playlist("p2".into()))
    );
    assert!(exists(cx, &o, ids::RENAME_PLAYLIST));
    act(cx, &o, |m, cx| m.commit_rename_playlist("  ", cx));
    assert!(
        !core
            .calls()
            .iter()
            .any(|c| matches!(c, MediaCall::RenamePlaylist { .. })),
        "empty keeps the default"
    );
    act(cx, &o, |m, cx| m.begin_rename_playlist("p2".into(), cx));
    act(cx, &o, |m, cx| m.commit_rename_playlist("Christmas", cx));
    assert_eq!(
        read(cx, &o, |m| m.library().playlists[1].name.clone()),
        "Christmas"
    );
    act(cx, &o, |m, cx| m.delete_playlist("p2".into(), cx));
    assert_eq!(
        read(cx, &o, |m| m.selection().cloned()),
        Some(Selection::All)
    );
}

#[gpui_kit::test]
fn adding_and_removing_playlist_items(cx: &mut TestAppContext) {
    let core = Core::new(library(3, &[]));
    let o = open(cx, core.clone(), true);
    act(cx, &o, |m, cx| {
        m.add_to_playlist("pl".into(), "i1".into(), cx)
    });
    act(cx, &o, |m, cx| {
        m.add_to_playlist("pl".into(), "i1".into(), cx)
    });
    assert_eq!(
        core.calls()
            .iter()
            .filter(|c| matches!(c, MediaCall::SetPlaylistItems { .. }))
            .count(),
        1,
        "already there"
    );
    act(cx, &o, |m, cx| {
        m.select(Selection::Playlist("pl".into()), cx)
    });
    act(cx, &o, |m, cx| m.remove_from_playlist("i2".into(), cx));
    assert_eq!(shown(cx, &o), ["i0", "i1"]);
    click(cx, &o, ids::ADD_TILE);
    assert!(
        core.calls()
            .contains(&MediaCall::AddMediaToPlaylist("pl".into()))
    );
    assert!(!read(cx, &o, |m| m.adding()));
}

// ─── Rename, copy, cut, paste, reveal ───────────────────────────────────────

#[gpui_kit::test]
fn renaming_a_background_inline(cx: &mut TestAppContext) {
    let core = Core::new(library(3, &[]));
    let o = open(cx, core.clone(), true);
    act(cx, &o, |m, cx| m.begin_rename_item("i0".into(), cx));
    assert!(exists(cx, &o, ids::RENAME_ITEM));
    press(cx, &o, "escape");
    assert!(read(cx, &o, |m| m.renaming_item().is_none()));
    act(cx, &o, |m, cx| m.begin_rename_item("i0".into(), cx));
    act(cx, &o, |m, cx| m.commit_rename_item("Sunrise", cx));
    assert_eq!(
        read(cx, &o, |m| m.item("i0").unwrap().name.clone()),
        "Sunrise"
    );
}

#[gpui_kit::test]
fn copy_cut_and_paste_follow_the_target(cx: &mut TestAppContext) {
    let core = Core::new(library(3, &[]));
    let o = open(cx, core.clone(), true);
    // No selection, nothing live: the first item.
    press(cx, &o, "cmd-c");
    assert!(
        core.calls()
            .contains(&MediaCall::CopyItems(vec!["i0".into()]))
    );
    assert!(read(cx, &o, |m| m.can_paste()));
    act(cx, &o, |m, cx| {
        m.select(Selection::Playlist("pl".into()), cx)
    });
    act(cx, &o, |m, cx| m.select_item("i2".into(), cx));
    press(cx, &o, "cmd-x");
    assert!(core.calls().contains(&MediaCall::CutItems {
        ids: vec!["i2".into()],
        from_playlist: Some("pl".into())
    }));
    press(cx, &o, "cmd-v");
    assert!(
        core.calls()
            .contains(&MediaCall::PasteItems(Some("pl".into())))
    );
    assert!(
        !read(cx, &o, |m| m.can_paste()),
        "the clipboard is checked again"
    );
    act(cx, &o, |m, cx| m.reveal("i0".into(), cx));
    assert!(core.calls().contains(&MediaCall::RevealItem("i0".into())));
    // The shortcuts do nothing while closed.
    act(cx, &o, |m, cx| m.set_open(false, cx));
    let n = core.calls().len();
    press(cx, &o, "cmd-c");
    assert_eq!(core.calls().len(), n);
}

#[gpui_kit::test]
fn paste_is_offered_when_the_clipboard_has_files(cx: &mut TestAppContext) {
    let core = Core::new(library(1, &[]));
    core.script(|c| matches!(c, MediaCall::ClipboardHasFiles).then(|| Ok(MediaAnswer::Flag(true))));
    let o = open(cx, core.clone(), true);
    assert!(read(cx, &o, |m| m.can_paste()));
    let polls = core
        .calls()
        .iter()
        .filter(|c| matches!(c, MediaCall::ClipboardHasFiles))
        .count();
    cx.executor()
        .advance_clock(Duration::from_millis(rows::PASTE_POLL_MS + 10));
    draw(cx, &o);
    assert!(
        core.calls()
            .iter()
            .filter(|c| matches!(c, MediaCall::ClipboardHasFiles))
            .count()
            > polls,
        "polled"
    );
}

// ─── Rearranging and picks ──────────────────────────────────────────────────

#[gpui_kit::test]
fn reordering_writes_the_library_or_the_playlist_order(cx: &mut TestAppContext) {
    let core = Core::new(library(3, &[]));
    let o = open(cx, core.clone(), true);
    act(cx, &o, |m, cx| {
        m.reorder("i0".into(), "i2".into(), DropPosition::After, cx)
    });
    assert_eq!(shown(cx, &o), ["i1", "i2", "i0"]);
    assert!(core.calls().contains(&MediaCall::SetItemOrder(vec![
        "i1".into(),
        "i2".into(),
        "i0".into()
    ])));
    act(cx, &o, |m, cx| {
        m.select(Selection::Playlist("pl".into()), cx)
    });
    act(cx, &o, |m, cx| {
        m.reorder("i0".into(), "i2".into(), DropPosition::Before, cx)
    });
    // The missing id stays in the playlist.
    assert!(core.calls().contains(&MediaCall::SetPlaylistItems {
        id: "pl".into(),
        item_ids: vec!["i0".into(), "i2".into(), "gone".into()]
    }));
    // A drop on itself writes nothing; a failed write restores the library.
    let n = core.calls().len();
    act(cx, &o, |m, cx| {
        m.reorder("i0".into(), "i0".into(), DropPosition::Before, cx)
    });
    assert_eq!(core.calls().len(), n);
    core.script(|c| {
        matches!(c, MediaCall::SetPlaylistItems { .. }).then(|| Err("disk full".into()))
    });
    act(cx, &o, |m, cx| {
        m.reorder("i2".into(), "i0".into(), DropPosition::Before, cx)
    });
    assert!(core.calls().contains(&MediaCall::GetLibrary));
    assert_eq!(
        read(cx, &o, |m| m.error().map(str::to_owned)),
        Some("disk full".into())
    );
}

#[gpui_kit::test]
fn picks_select_without_pushing_and_act_together(cx: &mut TestAppContext) {
    let core = Core::new(library(4, &[]));
    let o = open(cx, core.clone(), true);
    act(cx, &o, |m, cx| m.pick("i0", PickGesture::Toggle, cx));
    act(cx, &o, |m, cx| m.pick("i2", PickGesture::Range, cx));
    assert_eq!(read(cx, &o, |m| m.picked_count()), 3);
    assert!(!core.calls().iter().any(|c| matches!(c, MediaCall::Push(_))));
    assert!(exists(cx, &o, ids::PICKED_DELETE));
    act(cx, &o, |m, cx| m.add_picked_to_playlist("pl".into(), cx));
    assert!(core.calls().contains(&MediaCall::SetPlaylistItems {
        id: "pl".into(),
        item_ids: vec!["i2".into(), "gone".into(), "i0".into(), "i1".into()]
    }));
    assert_eq!(read(cx, &o, |m| m.picked_count()), 0);
    act(cx, &o, |m, cx| m.begin_marquee(false, cx));
    act(cx, &o, |m, cx| {
        m.update_marquee(vec!["i1".into(), "i3".into()], cx)
    });
    act(cx, &o, |m, cx| m.delete_picked(cx));
    let deletes: Vec<MediaCall> = core
        .calls()
        .into_iter()
        .filter(|c| matches!(c, MediaCall::DeleteItem(_)))
        .collect();
    assert_eq!(
        deletes,
        [
            MediaCall::DeleteItem("i1".into()),
            MediaCall::DeleteItem("i3".into())
        ]
    );
    assert_eq!(shown(cx, &o), ["i0", "i2"]);
}

// ─── Imports ────────────────────────────────────────────────────────────────

#[gpui_kit::test]
fn the_import_menu_and_os_drops_bring_files_in(cx: &mut TestAppContext) {
    let core = Core::new(library(2, &[]));
    let o = open(cx, core.clone(), true);
    act(cx, &o, |m, cx| {
        m.select(Selection::Playlist("pl".into()), cx)
    });
    core.push(MediaFeed::Import(ImportKind::Video));
    draw(cx, &o);
    assert!(
        core.calls()
            .contains(&MediaCall::ImportFiles(ImportKind::Video))
    );
    assert_eq!(
        read(cx, &o, |m| m.selection().cloned()),
        Some(Selection::All)
    );
    act(cx, &o, |m, cx| {
        m.select(Selection::Playlist("pl".into()), cx)
    });
    act(cx, &o, |m, cx| m.drop_files(vec!["/x/a.mp4".into()], cx));
    assert!(core.calls().contains(&MediaCall::ImportPaths {
        paths: vec!["/x/a.mp4".into()],
        playlist: Some("pl".into())
    }));
    core.script(|c| {
        matches!(c, MediaCall::ImportPaths { .. }).then(|| Err("That playlist is gone.".into()))
    });
    act(cx, &o, |m, cx| m.drop_files(vec!["/x/b.mp4".into()], cx));
    assert!(!read(cx, &o, |m| m.adding()), "adding clears on failure");
    assert_eq!(
        read(cx, &o, |m| m.error().map(str::to_owned)),
        Some("That playlist is gone.".into())
    );
}

// ─── Posters, preview and the inspector ─────────────────────────────────────

#[gpui_kit::test]
fn videos_get_posters_and_unreadable_ones_say_so(cx: &mut TestAppContext) {
    let core = Core::new(library(1, &["v1", "bad1"]));
    let o = open(cx, core.clone(), true);
    assert_eq!(
        read(cx, &o, |m| m.poster("v1").cloned()),
        Some(Poster::Ready("/tmp/poster.png".into()))
    );
    assert_eq!(
        read(cx, &o, |m| m.poster("bad1").cloned()),
        Some(Poster::Failed)
    );
    assert!(read(cx, &o, |m| m.undecodable("bad1")));
    assert!(
        !core.calls().contains(&MediaCall::Poster("i0".into())),
        "images need none"
    );
}

#[gpui_kit::test]
fn the_selected_video_plays_a_private_preview(cx: &mut TestAppContext) {
    let core = Core::new(library(1, &["v1"]));
    let o = open(cx, core.clone(), true);
    act(cx, &o, |m, cx| m.select_item("v1".into(), cx));
    cx.executor().advance_clock(Duration::from_millis(150));
    draw(cx, &o);
    let player = cx
        .read(|cx| o.view.read(cx).preview().cloned())
        .expect("player");
    assert!(
        cx.read(|cx| player.read(cx).frames()) >= 2,
        "frames arrived"
    );
    assert_eq!(core.opens.load(Ordering::SeqCst), 1);
    assert!(
        !core.calls().iter().any(|c| matches!(c, MediaCall::Push(_))),
        "nothing on screen"
    );
    act(cx, &o, |m, cx| m.select_item("i0".into(), cx));
    assert!(cx.read(|cx| o.view.read(cx).preview().is_none()));
}

fn inspector(cx: &mut TestAppContext, o: &Opened) -> Entity<Inspector> {
    cx.read(|cx| o.view.read(cx).inspector().cloned().expect("inspector"))
}

#[gpui_kit::test]
fn the_inspector_trims_grades_and_saves_for_the_next_cue(cx: &mut TestAppContext) {
    let core = Core::new(library(1, &["v1"]));
    let o = open(cx, core.clone(), true);
    act(cx, &o, |m, cx| m.inspect("v1".into(), cx));
    draw(cx, &o);
    assert!(exists(cx, &o, inspector::ids::DIALOG));
    let i = inspector(cx, &o);
    assert_eq!(cx.read(|cx| i.read(cx).panel()), inspector::Panel::Playback);
    // An end before the start blocks Save.
    cx.update_window(o.handle, |_, w, cx| {
        i.update(cx, |i, cx| i.set_trim_field(Edge::End, "0.2".into(), w, cx))
    })
    .unwrap();
    cx.update_window(o.handle, |_, w, cx| {
        i.update(cx, |i, cx| i.set_trim_field(Edge::Start, "1".into(), w, cx))
    })
    .unwrap();
    draw(cx, &o);
    assert!(!cx.read(|cx| i.read(cx).can_save(cx)));
    assert_eq!(
        cx.read(|cx| i.read(cx).trim(cx).note()),
        "Choose a start before the end, within the video."
    );
    // Dragging the end handle to the clip's end means "file end".
    cx.update_window(o.handle, |_, w, cx| {
        i.update(cx, |i, cx| i.trim_to(Edge::End, 2.0, w, cx))
    })
    .unwrap();
    draw(cx, &o);
    assert!(cx.read(|cx| i.read(cx).can_save(cx)));
    assert_eq!(cx.read(|cx| i.read(cx).trim(cx).end.clone()), "");
    // Colour changes re-grade the preview: no reopen, no seek.
    let (opens, seeks) = (
        core.opens.load(Ordering::SeqCst),
        core.seeks.load(Ordering::SeqCst),
    );
    cx.update_window(o.handle, |_, w, cx| {
        i.update(cx, |i, cx| {
            i.set_grade(
                ui_media::player::Grade {
                    brightness: 1.2,
                    contrast: 1.,
                    saturation: 0.5,
                },
                w,
                cx,
            )
        })
    })
    .unwrap();
    draw(cx, &o);
    assert_eq!(core.opens.load(Ordering::SeqCst), opens);
    assert_eq!(core.seeks.load(Ordering::SeqCst), seeks);
    click(cx, &o, inspector::ids::SAVE);
    let saved = core.calls().into_iter().find_map(|c| match c {
        MediaCall::SetPlayback {
            id,
            patch,
            next_cue,
        } => Some((id, patch, next_cue)),
        _ => None,
    });
    let (id, patch, next_cue) = saved.expect("saved");
    assert_eq!(id, "v1");
    assert!(next_cue, "applies the next time it is cued");
    assert_eq!(patch["startSeconds"], json!(1.0));
    assert_eq!(patch["endSeconds"], Value::Null);
    assert_eq!(patch["brightness"], json!(1.2));
    assert_eq!(patch["saturation"], json!(0.5));
    assert!(
        read(cx, &o, |m| m.inspecting().is_none()),
        "closed after saving"
    );
}

#[gpui_kit::test]
fn a_failed_inspector_save_stays_open(cx: &mut TestAppContext) {
    let core = Core::new(library(1, &[]));
    core.script(|c| {
        matches!(c, MediaCall::SetPlayback { next_cue: true, .. })
            .then(|| Err("This item is no longer in the Media Dock.".into()))
    });
    let o = open(cx, core.clone(), true);
    act(cx, &o, |m, cx| m.inspect("i0".into(), cx));
    let i = inspector(cx, &o);
    assert_eq!(
        cx.read(|cx| i.read(cx).panel()),
        inspector::Panel::Appearance,
        "images start on Appearance"
    );
    click(cx, &o, inspector::ids::SAVE);
    assert_eq!(
        cx.read(|cx| i.read(cx).error().map(str::to_owned)),
        Some("This item is no longer in the Media Dock.".into())
    );
    assert!(exists(cx, &o, inspector::ids::ERROR));
    click(cx, &o, inspector::ids::CANCEL);
    assert!(read(cx, &o, |m| m.inspecting().is_none()));
}

#[gpui_kit::test]
fn an_image_saves_its_motion(cx: &mut TestAppContext) {
    let core = Core::new(library(1, &[]));
    let o = open(cx, core.clone(), true);
    act(cx, &o, |m, cx| m.inspect("i0".into(), cx));
    assert!(exists(cx, &o, inspector::ids::CAMERA));
    let i = inspector(cx, &o);
    cx.update(|cx| {
        i.update(cx, |i, cx| {
            i.set_camera(Some("zoom"), cx);
            i.set_atmosphere(Some("dust"), cx);
        })
    });
    click(cx, &o, inspector::ids::SAVE);
    let patch = core
        .calls()
        .into_iter()
        .find_map(|c| match c {
            MediaCall::SetPlayback { patch, .. } => Some(patch),
            _ => None,
        })
        .expect("saved");
    assert_eq!(
        patch["motion"],
        json!({"camera": "zoom", "atmosphere": "dust"})
    );
}

#[gpui_kit::test]
fn the_loop_toggle_applies_at_once(cx: &mut TestAppContext) {
    let core = Core::new(library(0, &["v1"]));
    let o = open(cx, core.clone(), true);
    let mut patch = serde_json::Map::new();
    patch.insert("loop".into(), json!(true));
    act(cx, &o, |m, cx| m.set_playback("v1".into(), patch, cx));
    assert!(read(cx, &o, |m| m.playback("v1").looped));
    assert!(core.calls().iter().any(|c| matches!(
        c,
        MediaCall::SetPlayback {
            next_cue: false,
            ..
        }
    )));
}

// ─── Scale ──────────────────────────────────────────────────────────────────

#[gpui_kit::test]
fn two_thousand_items_draw_only_visible_rows(cx: &mut TestAppContext) {
    let t = Instant::now();
    let o = open(cx, Core::new(library(2000, &[])), true);
    let first = t.elapsed();
    assert_eq!(shown(cx, &o).len(), 2000);
    assert!(exists(cx, &o, &ids::card("i0")));
    assert!(!exists(cx, &o, &ids::card("i1999")), "virtualised");
    let t = Instant::now();
    act(cx, &o, |m, cx| m.set_zoom(75., cx));
    let relayout = t.elapsed();
    eprintln!(
        "2000 items (test platform): open + first frames {first:?}, zoom relayout {relayout:?}"
    );
}

#[gpui_kit::test]
fn dropping_media_onto_a_playlist_adds_single_and_picked_items(cx: &mut TestAppContext) {
    let mut lib = library(3, &[]);
    lib.playlists.clear();
    lib.playlists.push(kairo_domain::media::MediaPlaylist {
        id: "p1".into(),
        name: "Service".into(),
        item_ids: vec![],
        created_at: 0.,
        extra: Default::default(),
    });
    lib.playlists.push(kairo_domain::media::MediaPlaylist {
        id: "p2".into(),
        name: "Offering".into(),
        item_ids: vec![],
        created_at: 0.,
        extra: Default::default(),
    });
    let core = Core::new(lib);
    let o = open(cx, core.clone(), true);

    // From All backgrounds: single card drop adds to p1.
    act(cx, &o, |m, cx| {
        m.drop_item_on_playlist("p1".into(), "i0".into(), cx)
    });
    assert_eq!(
        read(cx, &o, |m| m.library().playlists[0].item_ids.clone()),
        vec!["i0"]
    );

    // From All backgrounds: multi-select drop adds every picked item.
    act(cx, &o, |m, cx| {
        m.pick("i1", PickGesture::Toggle, cx);
        m.pick("i2", PickGesture::Toggle, cx);
        m.drop_item_on_playlist("p1".into(), "i1".into(), cx);
    });
    assert_eq!(
        read(cx, &o, |m| m.library().playlists[0].item_ids.clone()),
        vec!["i0", "i1", "i2"]
    );

    // From playlist p1: dragging onto p2 moves the item out of p1 into p2.
    act(cx, &o, |m, cx| {
        m.select(Selection::Playlist("p1".into()), cx);
        m.drop_item_on_playlist("p2".into(), "i0".into(), cx);
    });
    assert_eq!(
        read(cx, &o, |m| m.library().playlists[0].item_ids.clone()),
        vec!["i1", "i2"]
    );
    assert_eq!(
        read(cx, &o, |m| m.library().playlists[1].item_ids.clone()),
        vec!["i0"]
    );

    // From playlist p1: multi-select drag moves all picked items to p2.
    act(cx, &o, |m, cx| {
        m.pick("i1", PickGesture::Toggle, cx);
        m.pick("i2", PickGesture::Toggle, cx);
        m.drop_item_on_playlist("p2".into(), "i1".into(), cx);
    });
    assert!(read(cx, &o, |m| m.library().playlists[0]
        .item_ids
        .is_empty()));
    assert_eq!(
        read(cx, &o, |m| m.library().playlists[1].item_ids.clone()),
        vec!["i0", "i1", "i2"]
    );
}

#[gpui_kit::test]
fn keys_pick_all_clear_and_remove_from_the_playlist(cx: &mut TestAppContext) {
    let core = Core::new(library(3, &[]));
    let o = open(cx, core.clone(), true);
    act(cx, &o, |m, cx| {
        m.select(Selection::Playlist("pl".into()), cx)
    });
    let before = shown(cx, &o);
    assert!(before.len() >= 2, "{before:?}");
    // A click in the dock gives it the keys.
    click(cx, &o, &ids::card(&before[0]));
    press(cx, &o, "cmd-a");
    assert_eq!(
        read(cx, &o, |m| m.picked_count()),
        before.len(),
        "⌘A picks all"
    );
    press(cx, &o, "escape");
    assert_eq!(read(cx, &o, |m| m.picked_count()), 0, "Esc clears");
    act(cx, &o, |m, cx| m.select_item(before[1].clone(), cx));
    press(cx, &o, "backspace");
    assert!(
        !shown(cx, &o).contains(&before[1]),
        "Backspace takes it out of the playlist"
    );
}
