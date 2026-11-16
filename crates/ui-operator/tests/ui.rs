//! UI integration tests for the Operator frame: the clear-layer strip and its
//! live clearing, the monitor's empty states, the video transport, the
//! output picker, the persisted panel widths and pinned scrolling.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, Context, Entity, InteractiveElement, IntoElement,
    ParentElement, Render, ScrollHandle, StatefulInteractiveElement, Styled, TestAppContext,
    Window, WindowBounds, WindowOptions, div, px, size,
};
use kairo_core::engine::OutputEngine;
use kairo_core::preview::PreviewHub;
use kairo_domain::media::{MediaItem, MediaKind};
use kairo_domain::overlay_defaults::make_projector_output;
use kairo_engine_proto::VideoTime;
use kairo_output::fake::FakeEngine;
use serde_json::{Value, json};
use ui_operator::backend::{Done, LiveSnapshot, OperatorBackend, OperatorCommand, TimeReply};
use ui_operator::layers::Layer;
use ui_operator::layout::{PREVIEW_WIDTH_KEY, TRANSCRIPT_WIDTH_KEY, UiState};
use ui_operator::scroll::{Edge, PinnedScroll};
use ui_operator::workspace::TICK;
use ui_operator::{OperatorEvent, OperatorWorkspace};

#[derive(Default)]
struct FakeState {
    live: LiveSnapshot,
    commands: Vec<OperatorCommand>,
    /// Requests waiting for the test to finish them.
    waiting: Vec<Done>,
    /// Finish requests at once with this result.
    answer: Option<Result<(), String>>,
    time: Option<VideoTime>,
    time_asks: Vec<String>,
}

struct Fake {
    state: Mutex<FakeState>,
    hub: Option<PreviewHub>,
}

impl Fake {
    fn new(with_hub: bool) -> Arc<Self> {
        let hub = with_hub.then(|| {
            let engine: Arc<dyn OutputEngine> = Arc::new(FakeEngine::new());
            PreviewHub::new(engine).expect("hub")
        });
        Arc::new(Self {
            state: Mutex::new(FakeState {
                answer: Some(Ok(())),
                ..Default::default()
            }),
            hub,
        })
    }
    fn edit(&self, f: impl FnOnce(&mut FakeState)) {
        f(&mut self.state.lock().unwrap());
    }
    fn commands(&self) -> Vec<OperatorCommand> {
        self.state.lock().unwrap().commands.clone()
    }
    fn finish(&self, result: Result<(), String>) {
        let waiting: Vec<Done> = std::mem::take(&mut self.state.lock().unwrap().waiting);
        for done in waiting {
            done(result.clone());
        }
    }
}

impl OperatorBackend for Fake {
    fn snapshot(&self) -> LiveSnapshot {
        self.state.lock().unwrap().live.clone()
    }
    fn run(&self, command: OperatorCommand, done: Done) {
        let mut state = self.state.lock().unwrap();
        state.commands.push(command);
        match state.answer.clone() {
            Some(result) => {
                drop(state);
                done(result);
            }
            None => state.waiting.push(done),
        }
    }
    fn video_time(&self, output: &str, reply: TimeReply) {
        let time = {
            let mut state = self.state.lock().unwrap();
            state.time_asks.push(output.into());
            state.time
        };
        reply(time);
    }
    fn preview_hub(&self) -> Option<PreviewHub> {
        self.hub.clone()
    }
}

#[derive(Default)]
struct Memory(Mutex<HashMap<String, Value>>);
impl UiState for Memory {
    fn get(&self, key: &str) -> Option<Value> {
        self.0.lock().unwrap().get(key).cloned()
    }
    fn set(&self, key: &str, value: Value) {
        self.0.lock().unwrap().insert(key.into(), value);
    }
}

fn setup(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        kairo_ui_kit::init(kairo_ui_kit::Appearance::Dark, cx);
    });
}

struct Opened {
    handle: AnyWindowHandle,
    workspace: Entity<OperatorWorkspace>,
    events: Arc<Mutex<Vec<OperatorEvent>>>,
}

fn open(cx: &mut TestAppContext, backend: Arc<Fake>, state: Arc<Memory>) -> Opened {
    let (handle, workspace) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    size(px(1280.), px(800.)),
                ))),
                ..Default::default()
            },
            cx,
            move |window, cx| cx.new(|cx| OperatorWorkspace::new(backend, state, window, cx)),
        )
        .expect("open")
    });
    let events = Arc::new(Mutex::new(Vec::new()));
    let seen = events.clone();
    cx.update(|cx| {
        cx.subscribe(&workspace, move |_, event: &OperatorEvent, _| {
            seen.lock().unwrap().push(event.clone())
        })
        .detach();
    });
    let opened = Opened {
        handle,
        workspace,
        events,
    };
    settle(cx, &opened);
    opened
}

/// Lets the clock run once and draws.
fn settle(cx: &mut TestAppContext, opened: &Opened) {
    cx.executor().advance_clock(TICK);
    cx.run_until_parked();
    cx.update_window(opened.handle, |_, window, cx| {
        window.render_frame(cx);
        kairo_ui_kit::a11y_audit::check(window);
    })
    .unwrap();
}

fn screen(enabled: bool) -> kairo_domain::overlay::OverlayOutput {
    let mut output = make_projector_output(0.0);
    output.enabled = enabled;
    output
}

fn video() -> MediaItem {
    MediaItem {
        id: "loop-1".into(),
        name: "Loop".into(),
        path: "/media/loop.mp4".into(),
        kind: MediaKind::Video,
        ext: "mp4".into(),
        folder: String::new(),
        size_bytes: 1.0,
    }
}

fn label(cx: &mut TestAppContext, opened: &Opened, id: &'static str) -> Option<String> {
    cx.update_window(opened.handle, |_, window, _| {
        window
            .try_find(id)
            .and_then(|e| e.label().map(str::to_owned))
    })
    .unwrap()
}

fn disabled(cx: &mut TestAppContext, opened: &Opened, id: &'static str) -> bool {
    cx.update_window(opened.handle, |_, window, _| {
        window.find(id).disabled().unwrap_or(false)
    })
    .unwrap()
}

fn click(cx: &mut TestAppContext, opened: &Opened, id: &'static str) {
    cx.update_window(opened.handle, |_, window, cx| window.click(id, cx))
        .unwrap();
    cx.run_until_parked();
    cx.update_window(opened.handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
}

#[gpui_kit::test]
fn each_lit_layer_clears_alone_and_the_strip_waits(cx: &mut TestAppContext) {
    setup(cx);
    let backend = Fake::new(true);
    backend.edit(|s| {
        s.live.outputs = vec![screen(true)];
        s.live.program.message = Some("Welcome".into());
        s.live.program.camera = Some("FaceTime HD".into());
        s.answer = None;
    });
    let opened = open(cx, backend.clone(), Arc::new(Memory::default()));

    // Lit layers are enabled and named for what clicking does; empty ones dim.
    assert_eq!(
        label(cx, &opened, "clear-messages").as_deref(),
        Some("Clear messages")
    );
    assert!(!disabled(cx, &opened, "clear-messages"));
    assert!(disabled(cx, &opened, "clear-audio"));
    assert_eq!(label(cx, &opened, "clear-audio").as_deref(), Some("Audio"));
    assert!(!disabled(cx, &opened, "clear-all"));

    click(cx, &opened, "clear-messages");
    assert_eq!(
        backend.commands(),
        vec![OperatorCommand::Clear(Layer::Messages)]
    );
    // Busy: every button waits for the request.
    assert!(disabled(cx, &opened, "clear-camera"));
    click(cx, &opened, "clear-camera");
    assert_eq!(backend.commands().len(), 1, "a busy strip ignores clicks");

    // The program follows; the message layer goes dark on the next read.
    backend.edit(|s| s.live.program.message = None);
    backend.finish(Ok(()));
    settle(cx, &opened);
    assert!(disabled(cx, &opened, "clear-messages"));
    assert!(!disabled(cx, &opened, "clear-camera"));
}

#[gpui_kit::test]
fn clear_all_takes_text_and_background_together_then_the_rest(cx: &mut TestAppContext) {
    setup(cx);
    let backend = Fake::new(true);
    backend.edit(|s| {
        s.live.outputs = vec![screen(true)];
        s.live.track_live = true;
        s.live.program.logo = true;
        s.live.program.current = Some(
            serde_json::from_value(json!({"reference": "John 3:16", "text": "For God"})).unwrap(),
        );
        s.live.media.items = vec![video()];
        s.live.media.live_item_id = Some("loop-1".into());
    });
    let opened = open(cx, backend.clone(), Arc::new(Memory::default()));
    click(cx, &opened, "clear-all");
    assert_eq!(
        backend.commands(),
        vec![OperatorCommand::ClearAll {
            others: vec![Layer::Audio, Layer::Props]
        }]
    );

    // Everything down: Clear all has nothing to do.
    backend.edit(|s| {
        s.live = LiveSnapshot {
            outputs: vec![screen(true)],
            ..Default::default()
        }
    });
    settle(cx, &opened);
    assert!(disabled(cx, &opened, "clear-all"));
}

#[gpui_kit::test]
fn a_failed_clear_is_shown_and_the_strip_comes_back(cx: &mut TestAppContext) {
    setup(cx);
    let backend = Fake::new(true);
    backend.edit(|s| {
        s.live.outputs = vec![screen(true)];
        s.live.program.message = Some("Hi".into());
        s.answer = Some(Err("The screen engine is not running.".into()));
    });
    let opened = open(cx, backend.clone(), Arc::new(Memory::default()));
    click(cx, &opened, "clear-messages");
    assert_eq!(
        label(cx, &opened, "live-rail-error").as_deref(),
        Some("The screen engine is not running.")
    );
    assert!(
        !disabled(cx, &opened, "clear-messages"),
        "the operator can try again"
    );
}

#[gpui_kit::test]
fn the_monitor_says_why_it_is_empty(cx: &mut TestAppContext) {
    setup(cx);
    // No screens: Preview only, with the way to fix it.
    let backend = Fake::new(true);
    let opened = open(cx, backend.clone(), Arc::new(Memory::default()));
    assert_eq!(
        label(cx, &opened, "live-monitor-empty").as_deref(),
        Some("No screens on. Turn one on in Theme → Output")
    );
    click(cx, &opened, "turn-on-screen");
    assert_eq!(
        *opened.events.lock().unwrap(),
        vec![OperatorEvent::OpenScreens]
    );

    // A screen on, nothing live.
    backend.edit(|s| s.live.outputs = vec![screen(true)]);
    settle(cx, &opened);
    assert_eq!(
        label(cx, &opened, "live-monitor-empty").as_deref(),
        Some("Nothing is live. Send a verse or click a background")
    );
    assert_eq!(label(cx, &opened, "turn-on-screen"), None);

    // Something drawn: the engine's frame, not a placeholder.
    backend.edit(|s| s.live.program.clock_on_screens = true);
    settle(cx, &opened);
    assert_eq!(label(cx, &opened, "live-monitor-empty"), None);

    // The chosen screen is off: said under the monitor.
    backend.edit(|s| s.live.outputs = vec![screen(false)]);
    settle(cx, &opened);
    assert_eq!(
        label(cx, &opened, "preview-off").as_deref(),
        Some("Preview only — this screen is off.")
    );
}

#[gpui_kit::test]
fn without_an_engine_the_monitor_says_so(cx: &mut TestAppContext) {
    setup(cx);
    let backend = Fake::new(false);
    backend.edit(|s| {
        s.live.outputs = vec![screen(true)];
        s.live.program.message = Some("Hi".into());
    });
    let opened = open(cx, backend, Arc::new(Memory::default()));
    assert_eq!(
        label(cx, &opened, "live-monitor-empty").as_deref(),
        Some("Preview unavailable. The screen engine is not running")
    );
}

#[gpui_kit::test]
fn the_transport_drives_the_engine_video(cx: &mut TestAppContext) {
    setup(cx);
    let backend = Fake::new(true);
    backend.edit(|s| {
        s.live.outputs = vec![screen(true)];
        s.live.media.items = vec![video()];
        s.live.media.live_item_id = Some("loop-1".into());
        s.time = Some(VideoTime {
            current_time: 12.0,
            duration: 30.0,
            ended: false,
        });
    });
    let opened = open(cx, backend.clone(), Arc::new(Memory::default()));
    settle(cx, &opened);
    assert!(
        backend
            .state
            .lock()
            .unwrap()
            .time_asks
            .contains(&"projector".to_string())
            || !backend.state.lock().unwrap().time_asks.is_empty(),
        "the clock asks the engine for the time"
    );
    assert_eq!(
        opened
            .workspace
            .read_with(cx, |w, cx| w.rail().read(cx).time().current_time),
        12.0
    );
    assert_eq!(label(cx, &opened, "video-play").as_deref(), Some("Pause"));

    click(cx, &opened, "video-play");
    click(cx, &opened, "video-forward");
    click(cx, &opened, "video-restart");
    click(cx, &opened, "video-loop");
    assert_eq!(
        backend.commands(),
        vec![
            OperatorCommand::SetPaused(true),
            OperatorCommand::Seek(17.0),
            OperatorCommand::Seek(0.0),
            OperatorCommand::SetLoop {
                item_id: "loop-1".into(),
                looped: true
            },
        ]
    );

    // Back from the restart point stays at 0; `k` toggles, as Space does.
    cx.update_window(opened.handle, |_, window, cx| {
        window.click("video-back", cx);
        window.press("k", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let commands = backend.commands();
    assert_eq!(commands[commands.len() - 2], OperatorCommand::Seek(0.0));
    assert_eq!(
        commands[commands.len() - 1],
        OperatorCommand::SetPaused(true)
    );

    // An image background has no transport.
    backend.edit(|s| s.live.media.items[0].kind = MediaKind::Image);
    settle(cx, &opened);
    assert_eq!(label(cx, &opened, "video-play"), None);
}

#[gpui_kit::test]
fn panel_widths_persist_in_ui_state(cx: &mut TestAppContext) {
    setup(cx);
    let state = Arc::new(Memory::default());
    state.set(PREVIEW_WIDTH_KEY, json!("320"));
    let opened = open(cx, Fake::new(true), state.clone());
    let rail_width =
        |cx: &mut TestAppContext| cx.update(|cx| cx.global::<ui_operator::rail::RailWidth>().0);
    // A legacy default is treated as unset.
    assert_eq!(rail_width(cx), 460.0);

    cx.update_window(opened.handle, |_, window, cx| {
        window.click("live-rail-edge", cx);
        window.press("left", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(rail_width(cx), 476.0);
    assert_eq!(state.get(PREVIEW_WIDTH_KEY), Some(json!("476")));

    cx.update_window(opened.handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("operator-transcript-edge", cx);
        window.press("right", cx);
        window.press("right", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        opened.workspace.read_with(cx, |w, _| w.transcript_width()),
        312.0
    );
    assert_eq!(state.get(TRANSCRIPT_WIDTH_KEY), Some(json!("312")));
}

#[gpui_kit::test]
fn one_clock_runs_the_workspace(cx: &mut TestAppContext) {
    setup(cx);
    let opened = open(cx, Fake::new(true), Arc::new(Memory::default()));
    let before = opened.workspace.read_with(cx, |w, _| w.ticks());
    for _ in 0..4 {
        cx.executor().advance_clock(TICK);
        cx.run_until_parked();
    }
    let after = opened.workspace.read_with(cx, |w, _| w.ticks());
    assert_eq!(after - before, 4, "one tick per period");
}

struct Pinned {
    top: PinnedScroll,
    bottom: PinnedScroll,
    version: u64,
}

impl Render for Pinned {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.top.follow(self.version);
        self.bottom.follow(self.version);
        let tall = px(500. + self.version as f32 * 100.);
        let list = |id: &'static str, handle: &ScrollHandle| {
            div()
                .id(id)
                .h(px(80.))
                .overflow_y_scroll()
                .track_scroll(handle)
                .child(div().h(tall).child(id))
        };
        div()
            .child(list("top", self.top.handle()))
            .child(list("bottom", self.bottom.handle()))
    }
}

#[gpui_kit::test]
fn new_content_follows_its_edge_and_manual_scrolling_stays(cx: &mut TestAppContext) {
    setup(cx);
    let (handle, view) = cx.update(|cx| {
        gpui_kit::open_window(Default::default(), cx, |_, cx| {
            cx.new(|_| Pinned {
                top: PinnedScroll::new(Edge::Top),
                bottom: PinnedScroll::new(Edge::Bottom),
                version: 0,
            })
        })
        .unwrap()
    });
    let frame = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.render_frame(cx);
        })
        .unwrap()
    };
    let offsets = |cx: &mut TestAppContext| {
        view.read_with(cx, |v, _| {
            (
                f32::from(v.top.handle().offset().y),
                f32::from(v.bottom.handle().offset().y),
                f32::from(v.bottom.handle().max_offset().y),
            )
        })
    };
    frame(cx);
    let (top, bottom, max) = offsets(cx);
    assert_eq!(top, 0.0);
    assert_eq!(bottom, -max, "the transcript starts at its newest line");

    // The operator scrolls both lists; a redraw without new content keeps them.
    view.update(cx, |v, _| {
        v.top
            .handle()
            .set_offset(gpui_kit::point(px(0.), px(-100.)));
        v.bottom
            .handle()
            .set_offset(gpui_kit::point(px(0.), px(0.)));
    });
    frame(cx);
    let (top, bottom, _) = offsets(cx);
    assert_eq!((top, bottom), (-100.0, 0.0));

    // New content: each list moves to its newest edge once.
    view.update(cx, |v, cx| {
        v.version += 1;
        cx.notify();
    });
    frame(cx);
    let (top, bottom, max) = offsets(cx);
    assert_eq!(top, 0.0);
    assert_eq!(bottom, -max);

    // Scrolling again after the update is left alone.
    view.update(cx, |v, cx| {
        v.top.handle().set_offset(gpui_kit::point(px(0.), px(-80.)));
        v.bottom
            .handle()
            .set_offset(gpui_kit::point(px(0.), px(-40.)));
        cx.notify();
    });
    frame(cx);
    let (top, bottom, _) = offsets(cx);
    assert_eq!((top, bottom), (-80.0, -40.0));
}
