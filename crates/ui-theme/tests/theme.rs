//! UI tests for the Theme editor (prompt 086) against a scripted core:
//! opening, selecting, drag, resize, rotate, snapping, nudging, delete and
//! hide, undo with coalescing, saving, the library and the screens.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui_kit::AppContext;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, Bounds, Entity, TestAppContext, WindowBounds, WindowOptions, px, size,
};
use kairo_domain::overlay::{OverlayContentKind, OverlayElementKind, OverlayOutputKind};
use kairo_domain::overlay_boxes::overlay_box;
use kairo_domain::overlay_defaults::{
    DEFAULT_OVERLAY_SETTINGS, DEFAULT_OVERLAY_THEME, make_overlay_output,
};
use kairo_domain::overlay_elements::create_overlay_element;
use kairo_domain::settings::AppSettings;
use kairo_domain::theme_library::{assign_theme_to_output, create_custom_theme};
use ui_theme::backend::{FeedSink, Listening, MediaKind, Reply, ThemeBackend};
use ui_theme::ops::{Op, apply};
use ui_theme::{ThemeEditor, ids};

struct State {
    settings: AppSettings,
    ops: Vec<Op>,
    fail: bool,
    sink: Option<FeedSink>,
    picked: Option<String>,
}

struct Core(Mutex<State>);

impl ThemeBackend for Core {
    fn snapshot(&self) -> AppSettings {
        self.0.lock().unwrap().settings.clone()
    }
    fn listen(&self, sink: FeedSink) -> Listening {
        self.0.lock().unwrap().sink = Some(sink);
        Box::new(())
    }
    fn edit(&self, op: Op, reply: Reply<()>) {
        let mut s = self.0.lock().unwrap();
        s.ops.push(op.clone());
        if s.fail {
            drop(s);
            reply(Err("disk full".into()));
            return;
        }
        let result = apply(&mut s.settings, &op);
        drop(s);
        reply(result);
    }
    fn pick_media(&self, _: MediaKind, reply: Reply<Option<String>>) {
        let picked = self.0.lock().unwrap().picked.clone();
        reply(Ok(picked));
    }
    fn media_aspect(&self, _: String, _: MediaKind, reply: Reply<Option<f64>>) {
        reply(Ok(Some(2.0)));
    }
}

impl Core {
    fn new() -> Arc<Self> {
        let mut s = AppSettings::defaults(DEFAULT_OVERLAY_SETTINGS.clone());
        let mut under = create_overlay_element(OverlayElementKind::Rectangle, None, "box", None);
        under.frame = overlay_box(10.0, 10.0, 20.0, 20.0);
        under.above_text = true;
        let mut theme = DEFAULT_OVERLAY_THEME.clone();
        theme.reference.show = true;
        theme.elements = vec![under];
        let a = create_custom_theme("Sunday", &theme, OverlayContentKind::Scripture, 1.0, "a");
        let b = create_custom_theme(
            "Plain",
            &DEFAULT_OVERLAY_THEME,
            OverlayContentKind::Scripture,
            1.0,
            "b",
        );
        let l = create_custom_theme(
            "Songs",
            &DEFAULT_OVERLAY_THEME,
            OverlayContentKind::Lyrics,
            1.0,
            "l",
        );
        let mut screen = make_overlay_output("screen-1", OverlayOutputKind::Screen);
        screen.name = "Main".into();
        screen.enabled = true;
        screen = assign_theme_to_output(&screen, &a);
        s.theme_library = vec![a, b, l];
        s.overlay.outputs = vec![screen];
        Arc::new(Self(Mutex::new(State {
            settings: s,
            ops: vec![],
            fail: false,
            sink: None,
            picked: None,
        })))
    }
    fn ops(&self) -> Vec<Op> {
        self.0.lock().unwrap().ops.clone()
    }
    fn saved(&self, id: &str) -> kairo_domain::overlay::CustomOverlayTheme {
        self.0
            .lock()
            .unwrap()
            .settings
            .theme_library
            .iter()
            .find(|t| t.id == id)
            .unwrap()
            .clone()
    }
}

struct Opened {
    handle: AnyWindowHandle,
    editor: Entity<ThemeEditor>,
}

fn open(cx: &mut TestAppContext, core: Arc<Core>) -> Opened {
    cx.update(|cx| {
        gpui_kit::init(cx);
        kairo_ui_kit::init(kairo_ui_kit::Appearance::Dark, cx);
    });
    let slot: Arc<Mutex<Option<Entity<ThemeEditor>>>> = Default::default();
    let out = slot.clone();
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
                    let e = cx.new(|cx| ThemeEditor::new(core, window, cx));
                    *out.lock().unwrap() = Some(e.clone());
                    e
                },
            )
        })
        .unwrap()
        .0;
    let editor = slot.lock().unwrap().take().unwrap();
    let o = Opened { handle, editor };
    for _ in 0..4 {
        draw(cx, &o);
    }
    o
}

fn draw(cx: &mut TestAppContext, o: &Opened) {
    cx.run_until_parked();
    cx.update_window(o.handle, |_, w, cx| w.render_frame(cx))
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
    f: impl FnOnce(&mut ThemeEditor, &mut gpui_kit::Window, &mut gpui_kit::Context<ThemeEditor>) -> R,
) -> R {
    let editor = o.editor.clone();
    let r = cx
        .update_window(o.handle, move |_, w, cx| {
            editor.update(cx, |e, cx| f(e, w, cx))
        })
        .unwrap();
    draw(cx, o);
    r
}

fn read<R>(cx: &mut TestAppContext, o: &Opened, f: impl FnOnce(&ThemeEditor) -> R) -> R {
    cx.read(|cx| f(o.editor.read(cx)))
}

fn advance(cx: &mut TestAppContext, o: &Opened, ms: u64) {
    cx.executor().advance_clock(Duration::from_millis(ms));
    draw(cx, o);
}

/// A point in the frame, from its box in %: (x %, y %).
fn at(cx: &mut TestAppContext, o: &Opened, x: f64, y: f64) -> (f64, f64) {
    let b = read(cx, o, |e| e.frame_size_for_tests());
    (x / 100.0 * b.0, y / 100.0 * b.1)
}

fn element_box(cx: &mut TestAppContext, o: &Opened, id: &str) -> kairo_domain::overlay::OverlayBox {
    read(cx, o, |e| {
        e.draft()
            .elements
            .iter()
            .find(|x| x.id == id)
            .unwrap()
            .frame
            .clone()
    })
}

#[gpui_kit::test]
fn opens_the_screens_theme_with_its_tiles_layers_and_frame(cx: &mut TestAppContext) {
    let o = open(cx, Core::new());
    assert_eq!(
        read(cx, &o, |e| e.selected_theme().map(|t| t.id.clone())).as_deref(),
        Some("a")
    );
    for id in [
        ids::FRAME,
        ids::CANVAS,
        ids::STATUS,
        ids::UNDO,
        ids::TOOLBAR,
    ] {
        assert!(exists(cx, &o, id), "{id}");
    }
    assert!(exists(cx, &o, &ids::tile("a")) && exists(cx, &o, &ids::tile("b")));
    // The lyrics theme is on the other tab.
    assert!(!exists(cx, &o, &ids::tile("l")));
    for layer in ["box", "verse", "reference"] {
        assert!(exists(cx, &o, &ids::layer(layer)), "{layer}");
    }
    // The screen using the theme is lit and, being scripture, locked.
    assert!(exists(cx, &o, &ids::screen("screen-1")));
    assert!(read(cx, &o, |e| e.preview_failed()));
    assert!(exists(cx, &o, ids::NO_PREVIEW));
}

#[gpui_kit::test]
fn dragging_moves_one_undo_step_and_saves_after_a_pause(cx: &mut TestAppContext) {
    let core = Core::new();
    let o = open(cx, core.clone());
    act(cx, &o, |e, _, cx| e.select_layer(Some("box".into()), cx));
    let start = at(cx, &o, 20.0, 20.0);
    act(cx, &o, |e, _, cx| assert!(e.pointer_down(start, cx)));
    for step in 1..=5 {
        let p = (
            start.0 + f64::from(step) * 20.0,
            start.1 + f64::from(step) * 10.0,
        );
        act(cx, &o, |e, _, cx| e.pointer_move(p, false, cx));
    }
    act(cx, &o, |e, _, cx| e.pointer_up(cx));
    let b = element_box(cx, &o, "box");
    assert!(b.x_pct > 10.0 && b.y_pct > 10.0, "{b:?}");
    assert!(read(cx, &o, |e| e.can_undo()));
    // Nothing is written until the pause.
    assert!(core.ops().is_empty());
    advance(cx, &o, 450);
    assert_eq!(core.ops().len(), 1);
    assert_eq!(core.saved("a").theme.elements[0].frame.x_pct, b.x_pct);
    // The whole drag is one step.
    act(cx, &o, |e, _, cx| e.undo(cx));
    assert_eq!(element_box(cx, &o, "box").x_pct, 10.0);
    assert!(!read(cx, &o, |e| e.can_undo()));
    act(cx, &o, |e, _, cx| e.redo(cx));
    assert_eq!(element_box(cx, &o, "box").x_pct, b.x_pct);
}

#[gpui_kit::test]
fn a_move_near_the_middle_snaps_and_shows_the_guides(cx: &mut TestAppContext) {
    let o = open(cx, Core::new());
    act(cx, &o, |e, _, cx| e.select_layer(Some("box".into()), cx));
    let start = at(cx, &o, 20.0, 20.0);
    act(cx, &o, |e, _, cx| {
        e.pointer_down(start, cx);
    });
    // Box centre is (20, 20) %; move it to (50.5, 50.5) %.
    let p = at(cx, &o, 20.0 + 30.5, 20.0 + 30.5);
    act(cx, &o, |e, _, cx| e.pointer_move(p, false, cx));
    assert!(exists(cx, &o, ids::GUIDE_V) && exists(cx, &o, ids::GUIDE_H));
    let b = element_box(cx, &o, "box");
    assert!((b.x_pct + b.width_pct / 2.0 - 50.0).abs() < 1e-6);
    act(cx, &o, |e, _, cx| e.pointer_up(cx));
    assert!(!exists(cx, &o, ids::GUIDE_V));
}

#[gpui_kit::test]
fn resizing_by_a_corner_handle_and_turning_by_the_knob(cx: &mut TestAppContext) {
    let o = open(cx, Core::new());
    act(cx, &o, |e, _, cx| e.select_layer(Some("box".into()), cx));
    // South-east corner is at (30, 30) %.
    let se = at(cx, &o, 30.0, 30.0);
    act(cx, &o, |e, _, cx| assert!(e.pointer_down(se, cx)));
    let to = at(cx, &o, 40.0, 40.0);
    act(cx, &o, |e, _, cx| e.pointer_move(to, false, cx));
    act(cx, &o, |e, _, cx| e.pointer_up(cx));
    let b = element_box(cx, &o, "box");
    assert!(
        (b.width_pct - 30.0).abs() < 1e-6 && (b.x_pct - 10.0).abs() < 1e-6,
        "{b:?}"
    );
    // The knob sits 22 px under the box; drag it to the right of the centre.
    let (cx0, cy0) = at(cx, &o, 25.0, 25.0);
    let h = read(cx, &o, |e| e.frame_size_for_tests().1);
    let knob = (cx0, (b.y_pct + b.height_pct) / 100.0 * h + 22.0);
    act(cx, &o, |e, _, cx| assert!(e.pointer_down(knob, cx)));
    act(cx, &o, |e, _, cx| {
        e.pointer_move((cx0 + 200.0, cy0), false, cx)
    });
    act(cx, &o, |e, _, cx| e.pointer_up(cx));
    let rotation = read(cx, &o, |e| e.draft().elements[0].rotation_deg);
    assert_eq!(rotation, 90.0);
}

#[gpui_kit::test]
fn keys_nudge_delete_hide_and_deselect(cx: &mut TestAppContext) {
    let o = open(cx, Core::new());
    act(cx, &o, |e, _, cx| e.select_layer(Some("box".into()), cx));
    act(cx, &o, |e, _, cx| assert!(e.canvas_key("right", false, cx)));
    assert_eq!(element_box(cx, &o, "box").x_pct, 10.5);
    act(cx, &o, |e, _, cx| assert!(e.canvas_key("down", true, cx)));
    assert_eq!(element_box(cx, &o, "box").y_pct, 15.0);
    // A key that is not the canvas's is left alone.
    act(cx, &o, |e, _, cx| assert!(!e.canvas_key("a", false, cx)));
    // The verse cannot be deleted; the reference hides.
    act(cx, &o, |e, _, cx| e.select_layer(Some("verse".into()), cx));
    act(cx, &o, |e, _, cx| {
        assert!(!e.canvas_key("delete", false, cx))
    });
    assert!(read(cx, &o, |e| e.draft().verse.frame.width_pct > 0.0));
    act(cx, &o, |e, _, cx| {
        e.select_layer(Some("reference".into()), cx)
    });
    act(cx, &o, |e, _, cx| {
        assert!(e.canvas_key("backspace", false, cx))
    });
    assert!(!read(cx, &o, |e| e.draft().reference.show));
    assert_eq!(
        read(cx, &o, |e| e.selected_layer().map(str::to_owned)).as_deref(),
        Some("verse")
    );
    assert!(exists(cx, &o, &ids::layer("reference")));
    // An element deletes and nothing stays selected.
    act(cx, &o, |e, _, cx| e.select_layer(Some("box".into()), cx));
    act(cx, &o, |e, _, cx| {
        assert!(e.canvas_key("delete", false, cx))
    });
    assert!(read(cx, &o, |e| e.draft().elements.is_empty()
        && e.selected_layer().is_none()));
    // Esc deselects, and with nothing selected keys do nothing.
    act(cx, &o, |e, _, cx| e.select_layer(Some("verse".into()), cx));
    act(cx, &o, |e, _, cx| {
        assert!(e.canvas_key("escape", false, cx))
    });
    act(cx, &o, |e, _, cx| assert!(!e.canvas_key("left", false, cx)));
}

#[gpui_kit::test]
fn a_click_on_empty_canvas_deselects_and_a_click_on_a_layer_selects(cx: &mut TestAppContext) {
    let o = open(cx, Core::new());
    let on_box = at(cx, &o, 20.0, 20.0);
    act(cx, &o, |e, _, cx| {
        e.pointer_down(on_box, cx);
        e.pointer_up(cx);
    });
    assert_eq!(
        read(cx, &o, |e| e.selected_layer().map(str::to_owned)).as_deref(),
        Some("box")
    );
    // A press with no move edits nothing.
    assert!(!read(cx, &o, |e| e.can_undo()));
    let empty = at(cx, &o, 99.0, 1.0);
    act(cx, &o, |e, _, cx| {
        e.pointer_down(empty, cx);
    });
    assert_eq!(
        read(cx, &o, |e| e.selected_layer().map(str::to_owned)),
        None
    );
}

#[gpui_kit::test]
fn adding_a_shape_selects_it_and_the_limit_is_forty(cx: &mut TestAppContext) {
    let core = Core::new();
    let o = open(cx, core.clone());
    let pick = ui_theme::gallery::groups(true)[0].1[0].pick.clone();
    act(cx, &o, |e, _, cx| e.add_element(pick.clone(), cx));
    assert_eq!(read(cx, &o, |e| e.draft().elements.len()), 2);
    let selected = read(cx, &o, |e| e.selected_layer().map(str::to_owned)).unwrap();
    assert_eq!(read(cx, &o, |e| e.draft().elements[1].id.clone()), selected);
    for _ in 0..60 {
        act(cx, &o, |e, _, cx| e.add_element(pick.clone(), cx));
    }
    assert_eq!(read(cx, &o, |e| e.draft().elements.len()), 40);
    act(cx, &o, |e, _, cx| e.duplicate_element("box", cx));
    assert_eq!(read(cx, &o, |e| e.draft().elements.len()), 40);
}

#[gpui_kit::test]
fn adding_a_picture_takes_its_shape_and_a_closed_picker_adds_nothing(cx: &mut TestAppContext) {
    let core = Core::new();
    let o = open(cx, core.clone());
    act(cx, &o, |e, _, cx| e.add_media(MediaKind::Image, cx));
    assert_eq!(read(cx, &o, |e| e.draft().elements.len()), 1);
    core.0.lock().unwrap().picked = Some("/pics/logo.png".into());
    act(cx, &o, |e, _, cx| e.add_media(MediaKind::Image, cx));
    let added = read(cx, &o, |e| e.draft().elements[1].clone());
    assert_eq!(added.media_path.as_deref(), Some("/pics/logo.png"));
    // Aspect 2:1 at 30 % of 1920 is 288 px tall: 26.67 % of 1080.
    assert!(
        (added.frame.height_pct - 26.666_666).abs() < 1e-3,
        "{:?}",
        added.frame
    );
    assert!(exists(cx, &o, &ids::layer(&added.id)));
}

#[gpui_kit::test]
fn undo_redo_buttons_and_keys(cx: &mut TestAppContext) {
    let o = open(cx, Core::new());
    act(cx, &o, |e, _, cx| e.select_layer(Some("box".into()), cx));
    act(cx, &o, |e, _, cx| {
        e.canvas_key("right", false, cx);
    });
    click(cx, &o, ids::UNDO);
    assert_eq!(element_box(cx, &o, "box").x_pct, 10.0);
    click(cx, &o, ids::REDO);
    assert_eq!(element_box(cx, &o, "box").x_pct, 10.5);
    act(cx, &o, |e, _, cx| assert!(e.history_key("z", false, cx)));
    assert_eq!(element_box(cx, &o, "box").x_pct, 10.0);
    act(cx, &o, |e, _, cx| assert!(e.history_key("z", true, cx)));
    act(cx, &o, |e, _, cx| assert!(e.history_key("y", false, cx)));
    assert_eq!(element_box(cx, &o, "box").x_pct, 10.5);
    act(cx, &o, |e, _, cx| assert!(!e.history_key("q", false, cx)));
}

#[gpui_kit::test]
fn opening_another_theme_flushes_the_edit_and_clears_history(cx: &mut TestAppContext) {
    let core = Core::new();
    let o = open(cx, core.clone());
    act(cx, &o, |e, _, cx| e.select_layer(Some("box".into()), cx));
    act(cx, &o, |e, _, cx| {
        e.canvas_key("right", false, cx);
    });
    click(cx, &o, &ids::tile("b"));
    assert_eq!(core.ops().len(), 1);
    assert_eq!(core.saved("a").theme.elements[0].frame.x_pct, 10.5);
    assert!(!read(cx, &o, |e| e.can_undo()));
    assert_eq!(
        read(cx, &o, |e| e.selected_theme().map(|t| t.id.clone())).as_deref(),
        Some("b")
    );
}

#[gpui_kit::test]
fn reset_returns_to_the_saved_default_and_undo_brings_edits_back(cx: &mut TestAppContext) {
    let o = open(cx, Core::new());
    assert!(read(cx, &o, |e| e.is_default()));
    act(cx, &o, |e, _, cx| e.select_layer(Some("box".into()), cx));
    act(cx, &o, |e, _, cx| {
        e.canvas_key("right", false, cx);
    });
    assert!(!read(cx, &o, |e| e.is_default()));
    // Edits closer than 600 ms are one undo step, so let the clock pass.
    std::thread::sleep(Duration::from_millis(700));
    click(cx, &o, ids::RESET);
    assert_eq!(element_box(cx, &o, "box").x_pct, 10.0);
    act(cx, &o, |e, _, cx| e.undo(cx));
    assert_eq!(element_box(cx, &o, "box").x_pct, 10.5);
    click(cx, &o, ids::SAVE);
    assert!(read(cx, &o, |e| e.is_default()));
}

#[gpui_kit::test]
fn new_duplicate_and_delete(cx: &mut TestAppContext) {
    let core = Core::new();
    let o = open(cx, core.clone());
    act(cx, &o, |e, w, cx| e.create_blank(w, cx));
    assert_eq!(read(cx, &o, |e| e.library().len()), 4);
    assert_eq!(
        read(cx, &o, |e| e.selected_theme().unwrap().name.clone()),
        "Untitled theme"
    );
    act(cx, &o, |e, w, cx| e.duplicate("a", w, cx));
    assert_eq!(
        read(cx, &o, |e| e.selected_theme().unwrap().name.clone()),
        "Sunday copy"
    );
    assert!(read(cx, &o, |e| e.draft().elements.len() == 1));
    let copy = read(cx, &o, |e| e.selected_theme().unwrap().id.clone());
    act(cx, &o, |e, w, cx| e.delete_confirmed(&copy, w, cx));
    assert_eq!(read(cx, &o, |e| e.library().len()), 4);
    // The first scripture theme opens in its place.
    assert_eq!(
        read(cx, &o, |e| e.selected_theme().unwrap().id.clone()),
        "a"
    );
    // Deleting the theme a screen uses leaves the screen on its own copy.
    act(cx, &o, |e, w, cx| e.delete_confirmed("a", w, cx));
    assert_eq!(
        core.0.lock().unwrap().settings.overlay.outputs[0].theme_id,
        None
    );
}

#[gpui_kit::test]
fn deleting_the_last_theme_of_a_kind_shows_the_empty_state(cx: &mut TestAppContext) {
    let o = open(cx, Core::new());
    act(cx, &o, |e, w, cx| {
        e.switch_kind(OverlayContentKind::Lyrics, w, cx)
    });
    assert!(exists(cx, &o, &ids::tile("l")));
    act(cx, &o, |e, w, cx| e.delete_confirmed("l", w, cx));
    assert!(exists(cx, &o, ids::EMPTY));
    click(cx, &o, ids::EMPTY_BLANK);
    assert!(!exists(cx, &o, ids::EMPTY));
    assert_eq!(read(cx, &o, |e| e.kind()), OverlayContentKind::Lyrics);
}

#[gpui_kit::test]
fn screens_chips_toggle_and_scripture_stays_locked(cx: &mut TestAppContext) {
    let core = Core::new();
    let o = open(cx, core.clone());
    // On and scripture: locked.
    click(cx, &o, &ids::screen("screen-1"));
    assert!(core.ops().is_empty());
    act(cx, &o, |e, w, cx| e.select_theme("b", w, cx));
    click(cx, &o, &ids::screen("screen-1"));
    assert_eq!(
        core.0.lock().unwrap().settings.overlay.outputs[0]
            .theme_id
            .as_deref(),
        Some("b")
    );
}

#[gpui_kit::test]
fn make_default_moves_every_screen_and_sets_the_kind(cx: &mut TestAppContext) {
    let core = Core::new();
    let o = open(cx, core.clone());
    act(cx, &o, |e, _, cx| e.make_default("b", cx));
    let s = core.0.lock().unwrap().settings.clone();
    assert_eq!(s.theme_defaults.scripture.as_deref(), Some("b"));
    assert_eq!(s.overlay.outputs[0].theme_id.as_deref(), Some("b"));
}

#[gpui_kit::test]
fn a_failed_write_shows_a_notice_and_a_good_one_clears_it(cx: &mut TestAppContext) {
    let core = Core::new();
    let o = open(cx, core.clone());
    core.0.lock().unwrap().fail = true;
    act(cx, &o, |e, _, cx| e.make_default("b", cx));
    assert!(exists(cx, &o, ids::NOTICE));
    // The screen shows the settled state, not the optimistic one.
    assert_eq!(read(cx, &o, |e| e.library().len()), 3);
    core.0.lock().unwrap().fail = false;
    act(cx, &o, |e, _, cx| e.make_default("b", cx));
    assert!(!exists(cx, &o, ids::NOTICE));
}

#[gpui_kit::test]
fn renaming_saves_a_trimmed_name_and_an_empty_one_becomes_untitled(cx: &mut TestAppContext) {
    let core = Core::new();
    let o = open(cx, core.clone());
    act(cx, &o, |e, _, cx| e.rename("  Easter ".into(), cx));
    advance(cx, &o, 450);
    assert_eq!(core.saved("a").name, "Easter");
    act(cx, &o, |e, _, cx| e.rename("   ".into(), cx));
    advance(cx, &o, 450);
    assert_eq!(core.saved("a").name, "Untitled theme");
}

#[gpui_kit::test]
fn settings_changed_elsewhere_reach_the_screen_chips(cx: &mut TestAppContext) {
    let core = Core::new();
    let o = open(cx, core.clone());
    let mut changed = core.snapshot();
    let mut extra = make_overlay_output("screen-2", OverlayOutputKind::Ndi);
    extra.name = "Stream".into();
    extra.enabled = true;
    changed.overlay.outputs.push(extra);
    let sink = core.0.lock().unwrap().sink.clone().unwrap();
    sink(ui_theme::ThemeFeed::Settings(Box::new(changed)));
    draw(cx, &o);
    assert!(exists(cx, &o, &ids::screen("screen-2")));
}

#[gpui_kit::test]
fn the_sample_toggle_and_hidden_reference_return(cx: &mut TestAppContext) {
    let o = open(cx, Core::new());
    act(cx, &o, |e, _, cx| e.set_long_sample(true, cx));
    act(cx, &o, |e, _, cx| {
        e.edit_theme(
            |t| {
                let mut n = t.clone();
                n.reference.show = false;
                n
            },
            cx,
        )
    });
    click(cx, &o, ids::ADD_REFERENCE);
    assert!(read(cx, &o, |e| e.draft().reference.show));
    assert_eq!(
        read(cx, &o, |e| e.selected_layer().map(str::to_owned)).as_deref(),
        Some("reference")
    );
}
