//! UI tests for the Props editor (prompt 088) against a scripted core:
//! props and timers, adding objects (a picture through the picker), drag
//! and corner resize, keys, stacking, the inspector, deleting a prop on
//! screen or the timer on screens, the timer design's readout, the object
//! cap, and saving after the pause or at once.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, Entity, TestAppContext, WindowBounds, WindowOptions, px,
    size,
};
use kairo_domain::program::{
    MAX_PROGRAM_TIMERS, MAX_PROP_OBJECTS, PresentationSettings, ProgramProp, ProgramTimerDef,
    PropObject, PropShape,
};
use kairo_domain::prop_geometry::PropCorner;
use kairo_domain::serde_util::Extra;
use kairo_ui_kit::components::NumberFieldChanged;
use ui_operator::booth::backend::{
    BoothAnswer, BoothCall, BoothSnapshot, FeedSink, Listening, Reply,
};
use ui_operator::booth::{BoothBackend, BoothViews};
use ui_operator::props::{self, PropsEditor, PropsEvent, Tool, frame_of, ids};

#[derive(Default)]
struct State {
    snapshot: BoothSnapshot,
    calls: Vec<BoothCall>,
    saved: Vec<PresentationSettings>,
    picked: Option<String>,
    sink: Option<FeedSink>,
}

struct Core(Mutex<State>);

impl BoothBackend for Core {
    fn snapshot(&self) -> BoothSnapshot {
        self.0.lock().unwrap().snapshot.clone()
    }
    fn listen(&self, sink: FeedSink) -> Listening {
        self.0.lock().unwrap().sink = Some(sink);
        Box::new(())
    }
    fn call(&self, call: BoothCall, reply: Reply) {
        let mut s = self.0.lock().unwrap();
        s.calls.push(call.clone());
        let answer = match call {
            BoothCall::PickImage => BoothAnswer::Picked(s.picked.clone()),
            _ => BoothAnswer::Done,
        };
        drop(s);
        reply(Ok(answer));
    }
    fn mark_rendered(&self, _: &str) {}
    fn save_presentation(&self, p: PresentationSettings) {
        self.0.lock().unwrap().saved.push(p);
    }
}

fn prop(id: &str, name: &str) -> ProgramProp {
    ProgramProp {
        id: id.into(),
        name: name.into(),
        background: None,
        objects: vec![],
        extra: Extra::new(),
    }
}

impl Core {
    /// Two props, the first on screen; two timers, the first on screens.
    fn new() -> Arc<Self> {
        let second = ProgramTimerDef {
            id: "timer-two".into(),
            name: "Two".into(),
            ..ProgramTimerDef::default()
        };
        let presentation = PresentationSettings {
            props: vec![prop("welcome", "Welcome"), prop("giving", "Giving")],
            timers: vec![ProgramTimerDef::default(), second],
            ..Default::default()
        };
        let mut snapshot = BoothSnapshot {
            presentation: Some(presentation),
            ..Default::default()
        };
        snapshot.program.active_prop_ids = vec!["welcome".into()];
        snapshot.program.screen_timer_id = Some("timer-main".into());
        Arc::new(Self(Mutex::new(State {
            snapshot,
            ..Default::default()
        })))
    }
    fn calls(&self) -> Vec<BoothCall> {
        self.0.lock().unwrap().calls.clone()
    }
    fn saved(&self) -> Vec<PresentationSettings> {
        self.0.lock().unwrap().saved.clone()
    }
}

struct Opened {
    handle: AnyWindowHandle,
    editor: Entity<PropsEditor>,
    closed: Arc<Mutex<usize>>,
}

#[gpui_kit::test]
fn prop_thumbnail_clock_invalidates_its_own_retained_panel(cx: &mut TestAppContext) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let core = Core::new();
    {
        let mut state = core.0.lock().unwrap();
        let timer = kairo_domain::program::default_timer_theme("#ffffff", "#ff0000")
            .objects
            .into_iter()
            .find(|o| matches!(o, PropObject::Timer { .. }))
            .unwrap();
        state.snapshot.presentation.as_mut().unwrap().props[0]
            .objects
            .push(timer);
    }
    cx.update(|cx| {
        gpui_kit::init(cx);
        kairo_ui_kit::init(kairo_ui_kit::Appearance::Dark, cx);
    });
    let (_, panel) = cx.update(|cx| {
        gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
            let booth = cx.new(|cx| ui_operator::booth::Booth::new(core, cx));
            cx.new(|cx| ui_operator::booth::toolbox::PropsPanel::new(booth, window, cx))
        })
        .unwrap()
    });
    let notifications = Arc::new(AtomicUsize::new(0));
    let seen = notifications.clone();
    let _observer = cx.update(|cx| {
        cx.observe(&panel, move |_, _| {
            seen.fetch_add(1, Ordering::SeqCst);
        })
    });
    cx.run_until_parked();
    notifications.store(0, Ordering::SeqCst);
    cx.executor().advance_clock(Duration::from_millis(300));
    cx.run_until_parked();
    assert!(
        notifications.load(Ordering::SeqCst) > 0,
        "a timer in a prop thumbnail must invalidate PropsPanel itself"
    );
}

fn open(cx: &mut TestAppContext, core: Arc<Core>, select: Option<&str>) -> Opened {
    cx.update(|cx| {
        gpui_kit::init(cx);
        kairo_ui_kit::init(kairo_ui_kit::Appearance::Dark, cx);
    });
    let slot: Arc<Mutex<Option<Entity<PropsEditor>>>> = Default::default();
    let out = slot.clone();
    let select = select.map(str::to_owned);
    let handle = cx
        .update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        Default::default(),
                        size(px(1400.), px(900.)),
                    ))),
                    ..Default::default()
                },
                cx,
                move |window, cx| {
                    let views = BoothViews::new(core, window, cx);
                    let e = cx.new(|cx| PropsEditor::new(views.booth.clone(), select, window, cx));
                    *out.lock().unwrap() = Some(e.clone());
                    e
                },
            )
        })
        .unwrap()
        .0;
    let editor = slot.lock().unwrap().take().unwrap();
    let closed: Arc<Mutex<usize>> = Default::default();
    let seen = closed.clone();
    cx.update(|cx| {
        cx.subscribe(&editor, move |_, _: &PropsEvent, _| {
            *seen.lock().unwrap() += 1
        })
        .detach()
    });
    let o = Opened {
        handle,
        editor,
        closed,
    };
    for _ in 0..3 {
        draw(cx, &o);
    }
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

fn act<R>(
    cx: &mut TestAppContext,
    o: &Opened,
    f: impl FnOnce(&mut PropsEditor, &mut gpui_kit::Context<PropsEditor>) -> R,
) -> R {
    let e = o.editor.clone();
    let r = cx.update(|cx| e.update(cx, f));
    draw(cx, o);
    r
}

fn design(cx: &mut TestAppContext, o: &Opened) -> Option<ProgramProp> {
    cx.read(|cx| o.editor.read(cx).current(cx))
}

fn presentation(cx: &mut TestAppContext, o: &Opened) -> PresentationSettings {
    cx.read(|cx| o.editor.read(cx).presentation(cx).clone())
}

fn advance(cx: &mut TestAppContext, o: &Opened, ms: u64) {
    cx.executor().advance_clock(Duration::from_millis(ms));
    draw(cx, o);
}

/// Scrolls the inspector to its end, where Place and Order are.
fn scroll_inspector(cx: &mut TestAppContext, o: &Opened) {
    cx.update_window(o.handle, |_, w, cx| {
        w.scroll(
            "props-inspector",
            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-2000.))),
            cx,
        )
    })
    .unwrap();
    draw(cx, o);
}

fn selected(cx: &mut TestAppContext, o: &Opened) -> Option<String> {
    cx.read(|cx| o.editor.read(cx).selected_object().map(str::to_owned))
}

#[gpui_kit::test]
fn opens_on_the_first_prop_and_adds_objects_from_the_bar(cx: &mut TestAppContext) {
    let o = open(cx, Core::new(), None);
    assert_eq!(design(cx, &o).unwrap().id, "welcome");
    assert!(
        exists(cx, &o, ids::STAGE_HINT),
        "an empty prop says what to do"
    );
    for tool in [Tool::Text, Tool::Rectangle, Tool::Ellipse] {
        click(cx, &o, &ids::tool(tool.key()));
    }
    let d = design(cx, &o).unwrap();
    assert_eq!(d.objects.len(), 3);
    assert!(matches!(d.objects[0], PropObject::Text { .. }));
    assert!(matches!(
        d.objects[2],
        PropObject::Shape {
            shape: PropShape::Ellipse,
            ..
        }
    ));
    assert_eq!(
        selected(cx, &o).as_deref(),
        Some(frame_of(&d.objects[2]).id.as_str())
    );
    assert!(!exists(cx, &o, ids::STAGE_HINT));
    assert!(exists(cx, &o, &ids::object(&frame_of(&d.objects[0]).id)));
}

#[gpui_kit::test]
fn a_picture_needs_a_file_and_a_closed_picker_adds_nothing(cx: &mut TestAppContext) {
    let core = Core::new();
    let o = open(cx, core.clone(), None);
    click(cx, &o, &ids::tool("image"));
    assert!(design(cx, &o).unwrap().objects.is_empty());
    core.0.lock().unwrap().picked = Some("/pics/logo.png".into());
    click(cx, &o, &ids::tool("image"));
    let d = design(cx, &o).unwrap();
    assert!(
        matches!(&d.objects[0], PropObject::Image { media_path, .. } if media_path == "/pics/logo.png")
    );
    assert!(exists(cx, &o, ids::CHANGE_IMAGE));
    core.0.lock().unwrap().picked = Some("/pics/new.png".into());
    click(cx, &o, ids::CHANGE_IMAGE);
    let d = design(cx, &o).unwrap();
    assert!(
        matches!(&d.objects[0], PropObject::Image { media_path, .. } if media_path == "/pics/new.png")
    );
}

#[gpui_kit::test]
fn dragging_moves_and_a_corner_resizes_on_the_frame(cx: &mut TestAppContext) {
    let o = open(cx, Core::new(), None);
    click(cx, &o, &ids::tool("rectangle"));
    let id = frame_of(&design(cx, &o).unwrap().objects[0]).id.clone();
    // Rectangle at 0.3, 0.3, 0.4 × 0.4.
    act(cx, &o, |e, cx| e.pointer_down(&id, None, (0.5, 0.5), cx));
    act(cx, &o, |e, cx| e.pointer_move((0.6, 0.45), cx));
    act(cx, &o, |e, cx| e.pointer_up(cx));
    let f = frame_of(&design(cx, &o).unwrap().objects[0]).clone();
    assert!((f.x - 0.4).abs() < 1e-9 && (f.y - 0.25).abs() < 1e-9);
    // Off the frame is held at the edge.
    act(cx, &o, |e, cx| e.pointer_down(&id, None, (0.5, 0.5), cx));
    act(cx, &o, |e, cx| e.pointer_move((2.0, 0.5), cx));
    act(cx, &o, |e, cx| e.pointer_up(cx));
    let f = frame_of(&design(cx, &o).unwrap().objects[0]).clone();
    assert!((f.x - 0.6).abs() < 1e-9);
    // The south-east corner: the north-west one stays put.
    act(cx, &o, |e, cx| {
        e.pointer_down(&id, Some(PropCorner::Se), (1.0, 0.65), cx)
    });
    act(cx, &o, |e, cx| e.pointer_move((0.0, 0.0), cx));
    act(cx, &o, |e, cx| e.pointer_up(cx));
    let g = frame_of(&design(cx, &o).unwrap().objects[0]).clone();
    assert_eq!((g.x, g.y), (f.x, f.y));
    assert_eq!(g.w, kairo_domain::prop_geometry::MIN_PROP_OBJECT_SIZE);
}

#[gpui_kit::test]
fn keys_nudge_delete_and_let_go(cx: &mut TestAppContext) {
    let o = open(cx, Core::new(), None);
    click(cx, &o, &ids::tool("text"));
    let id = selected(cx, &o).unwrap();
    assert!(act(cx, &o, |e, cx| e.key("right", false, cx)));
    let f = frame_of(&design(cx, &o).unwrap().objects[0]).clone();
    assert!((f.x - 0.105).abs() < 1e-9);
    act(cx, &o, |e, cx| e.key("down", true, cx));
    let f = frame_of(&design(cx, &o).unwrap().objects[0]).clone();
    assert!((f.y - 0.45).abs() < 1e-9);
    assert!(act(cx, &o, |e, cx| e.key("escape", false, cx)));
    assert_eq!(selected(cx, &o), None);
    assert!(
        !act(cx, &o, |e, cx| e.key("escape", false, cx)),
        "with nothing selected Escape closes"
    );
    act(cx, &o, |e, cx| e.select_object(Some(id), cx));
    assert!(act(cx, &o, |e, cx| e.key("backspace", false, cx)));
    assert!(design(cx, &o).unwrap().objects.is_empty());
}

#[gpui_kit::test]
fn the_inspector_edits_stacks_and_aligns(cx: &mut TestAppContext) {
    let o = open(cx, Core::new(), None);
    click(cx, &o, &ids::tool("rectangle"));
    click(cx, &o, &ids::tool("text"));
    let text_id = selected(cx, &o).unwrap();
    scroll_inspector(cx, &o);
    // Back moves the text below the rectangle; Back again cannot.
    click(cx, &o, ids::BACK);
    assert_eq!(frame_of(&design(cx, &o).unwrap().objects[0]).id, text_id);
    click(cx, &o, ids::FORWARD);
    assert_eq!(frame_of(&design(cx, &o).unwrap().objects[1]).id, text_id);
    // Align right on the slide.
    click(cx, &o, &ids::slide("right"));
    let f = frame_of(&design(cx, &o).unwrap().objects[1]).clone();
    assert!((f.x - (1.0 - f.w)).abs() < 1e-9);
    // Size is kept in 8–400.
    let size = cx
        .read(|cx| o.editor.read(cx).number_field("size"))
        .expect("size box");
    cx.update(|cx| size.update(cx, |_, cx| cx.emit(NumberFieldChanged(500.0))));
    draw(cx, &o);
    assert!(
        matches!(design(cx, &o).unwrap().objects[1], PropObject::Text { font_size_px, .. } if font_size_px == 400.0)
    );
    // X is kept so the object stays on the frame.
    let x = cx.read(|cx| o.editor.read(cx).number_field("x")).unwrap();
    cx.update(|cx| x.update(cx, |_, cx| cx.emit(NumberFieldChanged(99.0))));
    draw(cx, &o);
    let f = frame_of(&design(cx, &o).unwrap().objects[1]).clone();
    assert!((f.x - (1.0 - f.w)).abs() < 1e-9);
    // Bold and alignment.
    click(cx, &o, ids::BOLD);
    assert!(matches!(
        design(cx, &o).unwrap().objects[1],
        PropObject::Text { bold: false, .. }
    ));
    click(cx, &o, &ids::align("text-left"));
    assert!(matches!(
        design(cx, &o).unwrap().objects[1],
        PropObject::Text {
            align: kairo_domain::program::PropAlign::Left,
            ..
        }
    ));
    // The list selects; Delete from the inspector.
    act(cx, &o, |e, cx| e.select_object(None, cx));
    assert!(exists(cx, &o, &ids::list_object(&text_id)));
    click(cx, &o, &ids::list_object(&text_id));
    click(cx, &o, ids::DELETE_OBJECT);
    assert_eq!(design(cx, &o).unwrap().objects.len(), 1);
}

#[gpui_kit::test]
fn props_add_rename_background_and_delete_one_on_screen(cx: &mut TestAppContext) {
    let core = Core::new();
    let o = open(cx, core.clone(), None);
    click(cx, &o, ids::NEW_PROP);
    let d = design(cx, &o).unwrap();
    assert_eq!(d.name, "Prop 3");
    act(cx, &o, |e, cx| e.rename("Banner".into(), cx));
    assert_eq!(design(cx, &o).unwrap().name, "Banner");
    click(cx, &o, ids::BACKGROUND);
    assert_eq!(
        design(cx, &o).unwrap().background.as_deref(),
        Some("#000000")
    );
    click(cx, &o, ids::BACKGROUND);
    assert_eq!(design(cx, &o).unwrap().background, None);

    // Deleting the prop on screen takes it down first.
    act(cx, &o, |e, cx| e.select_prop(Some("welcome".into()), cx));
    click(cx, &o, ids::REMOVE);
    assert!(
        core.calls()
            .iter()
            .any(|c| matches!(c, BoothCall::SetProp { id, on: false } if id == "welcome"))
    );
    let p = presentation(cx, &o);
    assert!(p.props.iter().all(|x| x.id != "welcome"));
    assert_eq!(
        design(cx, &o).unwrap().id,
        "giving",
        "the next prop is selected"
    );
    // A prop that is not on screen is not taken down.
    let before = core.calls().len();
    act(cx, &o, |e, cx| e.remove_prop("giving", cx));
    assert_eq!(core.calls().len(), before);
}

#[gpui_kit::test]
fn with_no_props_the_stage_offers_one(cx: &mut TestAppContext) {
    let core = Core::new();
    core.0
        .lock()
        .unwrap()
        .snapshot
        .presentation
        .as_mut()
        .unwrap()
        .props
        .clear();
    let o = open(cx, core, None);
    assert!(design(cx, &o).is_none());
    assert!(exists(cx, &o, ids::EMPTY));
    assert!(!exists(cx, &o, ids::SAVE));
    click(cx, &o, ids::EMPTY_NEW);
    assert_eq!(design(cx, &o).unwrap().name, "Prop 1");
}

#[gpui_kit::test]
fn timers_are_designed_like_props_and_keep_their_readout(cx: &mut TestAppContext) {
    let core = Core::new();
    let o = open(cx, core.clone(), Some("timer:timer-two"));
    assert_eq!(
        cx.read(|cx| o.editor.read(cx).selected_timer().map(str::to_owned))
            .as_deref(),
        Some("timer-two")
    );
    assert!(exists(cx, &o, ids::NOTE));
    let d = design(cx, &o).unwrap();
    assert_eq!(d.id, "timer:timer-two");
    let readout = frame_of(&d.objects[0]).id.clone();
    // The readout cannot be deleted, by key or button.
    act(cx, &o, |e, cx| e.select_object(Some(readout.clone()), cx));
    act(cx, &o, |e, cx| e.key("delete", false, cx));
    assert_eq!(design(cx, &o).unwrap().objects.len(), 1);
    // Text added to the design is written to the timer's theme.
    click(cx, &o, &ids::tool("text"));
    let p = presentation(cx, &o);
    let t = p.timers.iter().find(|t| t.id == "timer-two").unwrap();
    assert_eq!(t.theme.objects.len(), 2);
    assert!(
        p.props.iter().all(|x| x.objects.is_empty()),
        "props are untouched"
    );

    // Deleting the timer on screens moves screens to the next one.
    act(cx, &o, |e, cx| e.select_timer("timer-main", cx));
    click(cx, &o, ids::REMOVE);
    assert!(
        core.calls()
            .iter()
            .any(|c| matches!(c, BoothCall::TimerShow(id) if id == "timer-two"))
    );
    assert_eq!(presentation(cx, &o).timers.len(), 1);
    // The last timer stays.
    act(cx, &o, |e, cx| e.remove_timer("timer-two", cx));
    assert_eq!(presentation(cx, &o).timers.len(), 1);
}

#[gpui_kit::test]
fn new_timers_stop_at_the_limit(cx: &mut TestAppContext) {
    let o = open(cx, Core::new(), None);
    for _ in 0..(MAX_PROGRAM_TIMERS + 2) {
        act(cx, &o, |e, cx| e.add_timer(cx));
    }
    assert_eq!(presentation(cx, &o).timers.len(), MAX_PROGRAM_TIMERS);
    assert_eq!(
        presentation(cx, &o).timers.last().unwrap().name,
        format!("Timer {MAX_PROGRAM_TIMERS}")
    );
}

#[gpui_kit::test]
fn objects_stop_at_forty(cx: &mut TestAppContext) {
    let o = open(cx, Core::new(), None);
    for _ in 0..(MAX_PROP_OBJECTS + 3) {
        act(cx, &o, |e, cx| e.add_object(Tool::Rectangle, cx));
    }
    assert_eq!(design(cx, &o).unwrap().objects.len(), MAX_PROP_OBJECTS);
}

#[gpui_kit::test]
fn edits_save_after_a_pause_and_save_writes_now_and_closes(cx: &mut TestAppContext) {
    let core = Core::new();
    let o = open(cx, core.clone(), None);
    click(cx, &o, &ids::tool("text"));
    act(cx, &o, |e, cx| e.key("right", false, cx));
    assert!(core.saved().is_empty(), "nothing is written while editing");
    advance(cx, &o, props::SAVE_DELAY_MS + 10);
    assert_eq!(core.saved().len(), 1, "one write once edits settle");
    assert_eq!(core.saved()[0].props[0].objects.len(), 1);

    act(cx, &o, |e, cx| e.key("right", false, cx));
    click(cx, &o, ids::SAVE);
    assert_eq!(core.saved().len(), 2);
    assert_eq!(*o.closed.lock().unwrap(), 1);
    advance(cx, &o, props::SAVE_DELAY_MS + 10);
    assert_eq!(core.saved().len(), 2, "nothing waits after Save");
}

#[gpui_kit::test]
fn the_feed_and_reopening_at_a_prop(cx: &mut TestAppContext) {
    let core = Core::new();
    let o = open(cx, core.clone(), Some("giving"));
    assert_eq!(design(cx, &o).unwrap().id, "giving");
    act(cx, &o, |e, cx| e.select(Some("timer:missing".into()), cx));
    assert_eq!(
        design(cx, &o).unwrap().id,
        "giving",
        "an unknown timer is ignored"
    );
    act(cx, &o, |e, cx| e.select(Some("welcome".into()), cx));
    assert_eq!(design(cx, &o).unwrap().id, "welcome");
}
