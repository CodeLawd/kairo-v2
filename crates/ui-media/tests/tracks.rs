//! UI integration tests for House audio (prompt 084), driven by a scripted
//! core: the empty state, play, pause, resume after the end, stop, seek,
//! rename, reveal, remove, import, rescan, the folder, errors (including a
//! player with no audio device) and position reads only while playing.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, Entity, TestAppContext, WindowBounds, WindowOptions, px,
    size,
};
use kairo_domain::media::TracksLibrary;
use serde_json::json;
use ui_media::tracks::{
    TrackTime, TracksBackend, TracksCall, TracksEvent, TracksPanel, TracksReply, TracksSink, ids,
};

struct State {
    library: TracksLibrary,
    calls: Vec<TracksCall>,
    sink: Option<TracksSink>,
    time: TrackTime,
    seeks: Vec<f64>,
    fail: Option<String>,
}

struct Core(Mutex<State>);

impl TracksBackend for Core {
    fn library(&self) -> TracksLibrary {
        self.0.lock().unwrap().library.clone()
    }
    fn listen(&self, sink: TracksSink) -> Box<dyn Send> {
        self.0.lock().unwrap().sink = Some(sink);
        Box::new(())
    }
    fn call(&self, call: TracksCall, reply: TracksReply) {
        let mut s = self.0.lock().unwrap();
        s.calls.push(call.clone());
        if let Some(e) = s.fail.clone() {
            drop(s);
            return reply(Err(e));
        }
        let lib = &mut s.library;
        match &call {
            TracksCall::Play(id) => {
                lib.live_id = Some(id.clone());
                lib.live_paused = false;
            }
            TracksCall::SetPaused(p) => lib.live_paused = *p,
            TracksCall::Stop => {
                lib.live_id = None;
                lib.live_paused = false;
            }
            TracksCall::Rename { id, name } => {
                if let Some(i) = lib.items.iter_mut().find(|i| &i.id == id) {
                    i.name = name.clone();
                }
            }
            TracksCall::Remove(id) => lib.items.retain(|i| &i.id != id),
            _ => {}
        }
        let l = lib.clone();
        drop(s);
        reply(Ok(l));
    }
    fn time(&self) -> TrackTime {
        self.0.lock().unwrap().time.clone()
    }
    fn seek(&self, seconds: f64) {
        self.0.lock().unwrap().seeks.push(seconds);
    }
}

impl Core {
    fn new(n: usize) -> Arc<Self> {
        let items: Vec<_> = (0..n)
            .map(|i| json!({"id": format!("t{i}"), "name": format!("Track {i}"), "path": format!("/a/t{i}.mp3"), "ext": "mp3", "size": 1.0}))
            .collect();
        let library = serde_json::from_value(json!({"folder": "/a", "items": items, "liveId": null, "livePaused": false, "error": null})).unwrap();
        Arc::new(Self(Mutex::new(State {
            library,
            calls: Vec::new(),
            sink: None,
            time: TrackTime::default(),
            seeks: Vec::new(),
            fail: None,
        })))
    }
    fn calls(&self) -> Vec<TracksCall> {
        self.0.lock().unwrap().calls.clone()
    }
    fn set_time(&self, t: TrackTime) {
        self.0.lock().unwrap().time = t;
    }
}

struct Opened {
    handle: AnyWindowHandle,
    panel: Entity<TracksPanel>,
    events: Arc<Mutex<Vec<TracksEvent>>>,
}

fn open(cx: &mut TestAppContext, core: Arc<Core>) -> Opened {
    cx.update(|cx| {
        gpui_kit::init(cx);
        kairo_ui_kit::init(kairo_ui_kit::Appearance::Dark, cx);
    });
    let slot: Arc<Mutex<Option<Entity<TracksPanel>>>> = Default::default();
    let out = slot.clone();
    let handle = cx
        .update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        Default::default(),
                        size(px(420.), px(700.)),
                    ))),
                    ..Default::default()
                },
                cx,
                move |window, cx| {
                    let p = cx.new(|cx| TracksPanel::new(core, window, cx));
                    *out.lock().unwrap() = Some(p.clone());
                    p
                },
            )
        })
        .unwrap()
        .0;
    let panel = slot.lock().unwrap().take().unwrap();
    let events: Arc<Mutex<Vec<TracksEvent>>> = Default::default();
    let seen = events.clone();
    cx.update(|cx| {
        cx.subscribe(&panel, move |_, e: &TracksEvent, _| {
            seen.lock().unwrap().push(e.clone())
        })
        .detach()
    });
    let o = Opened {
        handle,
        panel,
        events,
    };
    draw(cx, &o);
    o
}

fn draw(cx: &mut TestAppContext, o: &Opened) {
    cx.run_until_parked();
    cx.update_window(o.handle, |_, w, cx| {
        w.render_frame(cx);
        kairo_ui_kit::a11y_audit::check(w);
    })
    .unwrap();
    cx.run_until_parked();
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

fn act(
    cx: &mut TestAppContext,
    o: &Opened,
    f: impl FnOnce(&mut TracksPanel, &mut gpui_kit::Context<TracksPanel>),
) {
    cx.update(|cx| o.panel.update(cx, f));
    draw(cx, o);
}

fn read<R>(cx: &mut TestAppContext, o: &Opened, f: impl FnOnce(&TracksPanel) -> R) -> R {
    cx.read(|cx| f(o.panel.read(cx)))
}

fn text_of(cx: &mut TestAppContext, o: &Opened, id: &str) -> bool {
    exists(cx, o, id)
}

#[gpui_kit::test]
fn an_empty_folder_offers_add_tracks(cx: &mut TestAppContext) {
    let core = Core::new(0);
    let o = open(cx, core.clone());
    assert!(exists(cx, &o, ids::EMPTY));
    assert!(!exists(cx, &o, ids::ADD), "Add only once there are tracks");
    click(cx, &o, ids::ADD_FIRST);
    assert_eq!(core.calls(), [TracksCall::Import]);
}

#[gpui_kit::test]
fn play_pause_resume_after_the_end_and_stop(cx: &mut TestAppContext) {
    let core = Core::new(3);
    let o = open(cx, core.clone());
    core.set_time(TrackTime {
        current_time: 0.,
        duration: 125.,
        ended: false,
        error: None,
    });
    click(cx, &o, &ids::play("t1"));
    assert_eq!(core.calls(), [TracksCall::Play("t1".into())]);
    assert!(read(cx, &o, |p| p.playing()));
    assert!(text_of(cx, &o, ids::ELAPSED) && text_of(cx, &o, ids::TOTAL));
    assert!(exists(cx, &o, &ids::stop("t1")));
    // The live play button pauses.
    click(cx, &o, &ids::play("t1"));
    assert_eq!(core.calls().last(), Some(&TracksCall::SetPaused(true)));
    click(cx, &o, &ids::play("t1"));
    assert_eq!(core.calls().last(), Some(&TracksCall::SetPaused(false)));
    // Ended: the button plays it again rather than pausing.
    core.set_time(TrackTime {
        current_time: 125.,
        duration: 125.,
        ended: true,
        error: None,
    });
    cx.executor().advance_clock(Duration::from_millis(50));
    draw(cx, &o);
    assert!(!read(cx, &o, |p| p.playing()));
    click(cx, &o, &ids::play("t1"));
    assert_eq!(core.calls().last(), Some(&TracksCall::SetPaused(false)));
    // The live name does nothing; another name plays that track.
    let n = core.calls().len();
    click(cx, &o, &ids::name("t1"));
    assert_eq!(core.calls().len(), n);
    click(cx, &o, &ids::name("t2"));
    assert_eq!(core.calls().last(), Some(&TracksCall::Play("t2".into())));
    click(cx, &o, &ids::stop("t2"));
    assert_eq!(core.calls().last(), Some(&TracksCall::Stop));
    assert_eq!(read(cx, &o, |p| p.time().clone()), TrackTime::default());
    assert!(!exists(cx, &o, ids::ELAPSED));
}

#[gpui_kit::test]
fn seeking_is_clamped_to_the_track(cx: &mut TestAppContext) {
    let core = Core::new(1);
    let o = open(cx, core.clone());
    core.set_time(TrackTime {
        current_time: 10.,
        duration: 60.,
        ended: false,
        error: None,
    });
    act(cx, &o, |p, cx| p.press_play("t0".into(), cx));
    act(cx, &o, |p, cx| p.seek_to(90., cx));
    act(cx, &o, |p, cx| p.seek_to(-4., cx));
    assert_eq!(core.0.lock().unwrap().seeks, [60., 0.]);
}

#[gpui_kit::test]
fn the_position_is_read_each_frame_only_while_playing(cx: &mut TestAppContext) {
    let core = Core::new(1);
    let o = open(cx, core.clone());
    core.set_time(TrackTime {
        current_time: 0.,
        duration: 300.,
        ended: false,
        error: None,
    });
    act(cx, &o, |p, cx| p.press_play("t0".into(), cx));
    let before = read(cx, &o, |p| p.polls());
    cx.executor().advance_clock(Duration::from_secs(1));
    draw(cx, &o);
    let playing = read(cx, &o, |p| p.polls()) - before;
    assert!(
        (55..=65).contains(&playing),
        "{playing} reads in a second of play"
    );
    act(cx, &o, |p, cx| p.press_play("t0".into(), cx));
    assert!(!read(cx, &o, |p| p.playing()));
    let paused = read(cx, &o, |p| p.polls());
    cx.executor().advance_clock(Duration::from_secs(1));
    draw(cx, &o);
    assert!(
        read(cx, &o, |p| p.polls()) - paused <= 1,
        "no reads while paused"
    );
    eprintln!("position reads: {playing} in 1 s playing, 0 while paused");
}

#[gpui_kit::test]
fn renaming_trims_and_escape_cancels(cx: &mut TestAppContext) {
    let core = Core::new(2);
    let o = open(cx, core.clone());
    cx.update_window(o.handle, |_, w, cx| {
        o.panel
            .update(cx, |p, cx| p.begin_rename("t0".into(), w, cx))
    })
    .unwrap();
    draw(cx, &o);
    assert!(exists(cx, &o, ids::RENAME));
    cx.update_window(o.handle, |_, w, cx| w.press("escape", cx))
        .unwrap();
    draw(cx, &o);
    assert!(read(cx, &o, |p| p.renaming().is_none()));
    assert!(core.calls().is_empty());
    cx.update_window(o.handle, |_, w, cx| {
        o.panel
            .update(cx, |p, cx| p.begin_rename("t0".into(), w, cx))
    })
    .unwrap();
    act(cx, &o, |p, cx| p.commit_rename("  ", cx));
    assert!(core.calls().is_empty(), "empty keeps the name");
    cx.update_window(o.handle, |_, w, cx| {
        o.panel
            .update(cx, |p, cx| p.begin_rename("t0".into(), w, cx))
    })
    .unwrap();
    act(cx, &o, |p, cx| p.commit_rename("  Walk in  ", cx));
    assert_eq!(
        core.calls(),
        [TracksCall::Rename {
            id: "t0".into(),
            name: "Walk in".into()
        }]
    );
    assert_eq!(
        read(cx, &o, |p| p.library().items[0].name.clone()),
        "Walk in"
    );
}

#[gpui_kit::test]
fn reveal_remove_rescan_and_folder(cx: &mut TestAppContext) {
    let core = Core::new(2);
    let o = open(cx, core.clone());
    act(cx, &o, |p, cx| p.reveal("t1".into(), cx));
    act(cx, &o, |p, cx| p.remove("t1".into(), cx));
    click(cx, &o, ids::RESCAN);
    click(cx, &o, ids::FOLDER);
    click(cx, &o, ids::ADD);
    assert_eq!(
        core.calls(),
        [
            TracksCall::Reveal("t1".into()),
            TracksCall::Remove("t1".into()),
            TracksCall::Rescan,
            TracksCall::ChooseFolder,
            TracksCall::Import
        ]
    );
    assert_eq!(read(cx, &o, |p| p.library().items.len()), 1);
}

#[gpui_kit::test]
fn failures_and_a_missing_audio_device_are_shown(cx: &mut TestAppContext) {
    let core = Core::new(1);
    let o = open(cx, core.clone());
    core.0.lock().unwrap().fail =
        Some("Error: That track is no longer in the audio folder.".into());
    act(cx, &o, |p, cx| p.press_play("t0".into(), cx));
    assert_eq!(
        read(cx, &o, |p| p.error().map(str::to_owned)),
        Some("That track is no longer in the audio folder.".into())
    );
    assert!(exists(cx, &o, ids::ERROR));
    assert!(!read(cx, &o, |p| p.busy()));
    core.0.lock().unwrap().fail = None;
    // Plays, but the player has no device to play on.
    core.set_time(TrackTime {
        current_time: 0.,
        duration: 0.,
        ended: false,
        error: Some("No audio output device".into()),
    });
    act(cx, &o, |p, cx| p.press_play("t0".into(), cx));
    assert_eq!(
        read(cx, &o, |p| p.error().map(str::to_owned)),
        Some("No audio output device".into())
    );
    assert!(
        exists(cx, &o, ids::TOTAL),
        "the bar shows –:–– until a length is known"
    );
    // The library's own error wins.
    let mut lib = core.library();
    lib.error = Some("The audio folder could not be read.".into());
    let sink = core.0.lock().unwrap().sink.clone().unwrap();
    sink(lib);
    draw(cx, &o);
    assert_eq!(
        read(cx, &o, |p| p.error().map(str::to_owned)),
        Some("The audio folder could not be read.".into())
    );
}

#[gpui_kit::test]
fn import_audio_shows_the_tab_and_adds(cx: &mut TestAppContext) {
    let core = Core::new(1);
    let o = open(cx, core.clone());
    act(cx, &o, |p, cx| p.import_request(cx));
    assert_eq!(
        o.events.lock().unwrap().clone(),
        [TracksEvent::ShowAudioTab]
    );
    assert_eq!(core.calls(), [TracksCall::Import]);
}
