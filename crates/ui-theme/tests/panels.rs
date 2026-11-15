//! UI tests for the Theme editor's inspector and the Screen Themes picker
//! (prompt 087) against a scripted core. Ports
//! `tests/theme-text-size.integration.cjs` and
//! `tests/theme-picker.integration.cjs`, and covers the element panel, the
//! tabs and the write-back of fields after undo.

use std::sync::{Arc, Mutex};

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
use kairo_ui_kit::components::NumberFieldChanged;
use ui_theme::backend::{FeedSink, Listening, MediaKind, Reply, ThemeBackend};
use ui_theme::inspector::{Tab, ids};
use ui_theme::ops::{Op, apply};
use ui_theme::picker::{self, ThemePicker};
use ui_theme::{ThemeEditor, ThemeInspector};

struct State {
    settings: AppSettings,
    fail: bool,
    picked: Option<String>,
}

struct Core(Mutex<State>);

impl ThemeBackend for Core {
    fn snapshot(&self) -> AppSettings {
        self.0.lock().unwrap().settings.clone()
    }
    fn listen(&self, _: FeedSink) -> Listening {
        Box::new(())
    }
    fn edit(&self, op: Op, reply: Reply<()>) {
        let mut s = self.0.lock().unwrap();
        if s.fail {
            drop(s);
            reply(Err("Simulated save failure".into()));
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
    /// Two scripture themes and one lyrics theme; a pastor screen on the
    /// first scripture theme and a lower-third NDI feed with none.
    fn new() -> Arc<Self> {
        let mut s = AppSettings::defaults(DEFAULT_OVERLAY_SETTINGS.clone());
        let mut theme = DEFAULT_OVERLAY_THEME.clone();
        theme.reference.show = true;
        theme.layout.auto_fit_text = true;
        let mut el = create_overlay_element(OverlayElementKind::Rectangle, None, "box", None);
        el.frame = overlay_box(10.0, 10.0, 20.0, 20.0);
        theme.elements = vec![el];
        let a = create_custom_theme(
            "Sunday Scripture",
            &theme,
            OverlayContentKind::Scripture,
            1.0,
            "sunday",
        );
        let l = create_custom_theme(
            "Sunday Lyrics",
            &theme,
            OverlayContentKind::Lyrics,
            1.0,
            "lyrics",
        );
        let mut pastor = make_overlay_output("pastor", OverlayOutputKind::Screen);
        pastor.name = "Pastor screen".into();
        pastor.enabled = true;
        let mut lower = make_overlay_output("lower", OverlayOutputKind::Ndi);
        lower.name = "Lower third".into();
        pastor = assign_theme_to_output(&pastor, &a);
        s.theme_library = vec![a, l];
        s.overlay.outputs = vec![pastor, lower];
        Arc::new(Self(Mutex::new(State {
            settings: s,
            fail: false,
            picked: None,
        })))
    }
    fn settings(&self) -> AppSettings {
        self.0.lock().unwrap().settings.clone()
    }
}

type Pair = (Entity<ThemeEditor>, Entity<ThemeInspector>);

struct Opened {
    handle: AnyWindowHandle,
    editor: Entity<ThemeEditor>,
    inspector: Entity<ThemeInspector>,
}

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        kairo_ui_kit::init(kairo_ui_kit::Appearance::Dark, cx);
    });
}

fn options() -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            Default::default(),
            size(px(1400.), px(1000.)),
        ))),
        ..Default::default()
    }
}

fn open(cx: &mut TestAppContext, core: Arc<Core>) -> Opened {
    init(cx);
    let slot: Arc<Mutex<Option<Pair>>> = Default::default();
    let out = slot.clone();
    let handle = cx
        .update(|cx| {
            gpui_kit::open_window(options(), cx, move |window, cx| {
                let e = cx.new(|cx| ThemeEditor::new(core, window, cx));
                let i = ThemeInspector::attach(&e, window, cx);
                *out.lock().unwrap() = Some((e.clone(), i));
                e
            })
        })
        .unwrap()
        .0;
    let (editor, inspector) = slot.lock().unwrap().take().unwrap();
    let o = Opened {
        handle,
        editor,
        inspector,
    };
    for _ in 0..4 {
        draw(cx, &o.handle);
    }
    o
}

fn draw(cx: &mut TestAppContext, handle: &AnyWindowHandle) {
    cx.run_until_parked();
    cx.update_window(*handle, |_, w, cx| w.render_frame(cx))
        .unwrap();
    cx.run_until_parked();
}

fn exists(cx: &mut TestAppContext, h: &AnyWindowHandle, id: &str) -> bool {
    let id: &'static str = Box::leak(id.to_owned().into_boxed_str());
    cx.update_window(*h, move |_, w, _| w.try_find(id).is_some())
        .unwrap()
}

fn click(cx: &mut TestAppContext, h: &AnyWindowHandle, id: &str) {
    let id: &'static str = Box::leak(id.to_owned().into_boxed_str());
    cx.update_window(*h, move |_, w, cx| w.click(id, cx))
        .unwrap();
    draw(cx, h);
}

fn editor<R>(
    cx: &mut TestAppContext,
    o: &Opened,
    f: impl FnOnce(&mut ThemeEditor, &mut gpui_kit::Window, &mut gpui_kit::Context<ThemeEditor>) -> R,
) -> R {
    let e = o.editor.clone();
    let r = cx
        .update_window(o.handle, move |_, w, cx| e.update(cx, |e, cx| f(e, w, cx)))
        .unwrap();
    draw(cx, &o.handle);
    r
}

fn inspector<R>(
    cx: &mut TestAppContext,
    o: &Opened,
    f: impl FnOnce(&mut ThemeInspector, &mut gpui_kit::Context<ThemeInspector>) -> R,
) -> R {
    let i = o.inspector.clone();
    let r = cx.update(|cx| i.update(cx, f));
    draw(cx, &o.handle);
    r
}

fn draft(cx: &mut TestAppContext, o: &Opened) -> kairo_domain::overlay::OverlayTheme {
    cx.read(|cx| o.editor.read(cx).draft().clone())
}

/// Types `text` in the size box and leaves it with Enter.
fn type_size(cx: &mut TestAppContext, o: &Opened, text: &str) {
    let input = cx
        .read(|cx| o.inspector.read(cx).size_input())
        .expect("size box");
    let text = text.to_owned();
    cx.update_window(o.handle, move |_, w, cx| {
        input.update(cx, |i, cx| {
            i.set_value(text, w, cx);
            i.focus(w, cx);
        });
    })
    .unwrap();
    draw(cx, &o.handle);
    cx.update_window(o.handle, |_, w, cx| w.press("enter", cx))
        .unwrap();
    draw(cx, &o.handle);
}

fn size_text(cx: &mut TestAppContext, o: &Opened) -> String {
    cx.read(|cx| {
        o.inspector
            .read(cx)
            .size_input()
            .unwrap()
            .read(cx)
            .value()
            .to_string()
    })
}

#[gpui_kit::test]
fn a_typed_size_switches_fit_to_box_to_manual_on_lyrics_and_scripture(cx: &mut TestAppContext) {
    let o = open(cx, Core::new());
    // Lyrics: fit to box is on, and the size box still takes a size.
    editor(cx, &o, |e, w, cx| {
        e.switch_kind(OverlayContentKind::Lyrics, w, cx)
    });
    editor(cx, &o, |e, _, cx| e.select_layer(Some("verse".into()), cx));
    inspector(cx, &o, |i, cx| i.set_tab(Tab::Text, cx));
    assert!(draft(cx, &o).layout.auto_fit_text);
    assert!(exists(cx, &o.handle, ids::SIZE));
    type_size(cx, &o, "73");
    assert_eq!(size_text(cx, &o), "73", "custom size is retained");
    let d = draft(cx, &o);
    assert_eq!(d.verse.font_size_px, 73.0);
    assert!(!d.layout.auto_fit_text, "editing size turns off fitting");

    // Scripture: turn fitting on, then a size turns it off again.
    editor(cx, &o, |e, w, cx| {
        e.switch_kind(OverlayContentKind::Scripture, w, cx)
    });
    editor(cx, &o, |e, _, cx| e.select_layer(Some("verse".into()), cx));
    inspector(cx, &o, |i, cx| {
        i.set_tab(Tab::Text, cx);
        i.edit(
            "test.fit",
            |t| {
                let mut n = t.clone();
                n.layout.auto_fit_text = true;
                n
            },
            cx,
        )
    });
    assert!(draft(cx, &o).layout.auto_fit_text);
    type_size(cx, &o, "91");
    assert_eq!(size_text(cx, &o), "91");
    let d = draft(cx, &o);
    assert_eq!(d.verse.font_size_px, 91.0);
    assert!(!d.layout.auto_fit_text);

    // Out of range and unreadable sizes.
    type_size(cx, &o, "500");
    assert_eq!(draft(cx, &o).verse.font_size_px, 200.0);
    assert_eq!(size_text(cx, &o), "200");
    type_size(cx, &o, "abc");
    assert_eq!(draft(cx, &o).verse.font_size_px, 200.0);
    assert_eq!(
        size_text(cx, &o),
        "200",
        "unreadable text puts the size back"
    );
}

#[gpui_kit::test]
fn the_reference_size_leaves_fitting_on(cx: &mut TestAppContext) {
    let o = open(cx, Core::new());
    editor(cx, &o, |e, _, cx| {
        e.select_layer(Some("reference".into()), cx)
    });
    inspector(cx, &o, |i, cx| i.set_tab(Tab::Text, cx));
    assert!(exists(cx, &o.handle, ids::SHOW_REFERENCE));
    type_size(cx, &o, "40");
    let d = draft(cx, &o);
    assert_eq!(d.reference.text.font_size_px, 40.0);
    assert!(d.layout.auto_fit_text);
}

#[gpui_kit::test]
fn selecting_a_text_layer_leaves_the_background_tab(cx: &mut TestAppContext) {
    let o = open(cx, Core::new());
    editor(cx, &o, |e, _, cx| e.select_layer(None, cx));
    inspector(cx, &o, |i, cx| i.set_tab(Tab::Background, cx));
    assert!(exists(cx, &o.handle, ids::BG_TYPE));
    editor(cx, &o, |e, _, cx| {
        e.select_layer(Some("reference".into()), cx)
    });
    assert_eq!(cx.read(|cx| o.inspector.read(cx).tab()), Tab::Text);
    assert!(exists(cx, &o.handle, ids::FONT));
    inspector(cx, &o, |i, cx| i.set_tab(Tab::Layout, cx));
    assert!(exists(cx, &o.handle, &ids::preset("lower-third")));
}

#[gpui_kit::test]
fn stroke_shadow_and_hiding_the_reference(cx: &mut TestAppContext) {
    let o = open(cx, Core::new());
    editor(cx, &o, |e, _, cx| e.select_layer(Some("verse".into()), cx));
    inspector(cx, &o, |i, cx| i.set_tab(Tab::Text, cx));
    let before = draft(cx, &o).verse.outline.clone();
    click(cx, &o.handle, ids::STROKE);
    let after = draft(cx, &o).verse.outline;
    assert_eq!(after.enabled, !before.enabled);
    if after.enabled && before.width_px == 0.0 {
        assert_eq!(after.width_px, 2.0);
    }
    let shadow = draft(cx, &o).verse.shadow.enabled;
    click(cx, &o.handle, ids::SHADOW);
    assert_eq!(draft(cx, &o).verse.shadow.enabled, !shadow);
    if !shadow {
        assert!(exists(cx, &o.handle, &ids::field("shadow.x")));
    }
    // Underline, then pressing it again turns it off.
    click(cx, &o.handle, &ids::decoration("underline"));
    assert_eq!(
        draft(cx, &o).verse.text_decoration,
        kairo_domain::overlay::OverlayTextDecoration::Underline
    );
    click(cx, &o.handle, &ids::decoration("underline"));
    assert_eq!(
        draft(cx, &o).verse.text_decoration,
        kairo_domain::overlay::OverlayTextDecoration::None
    );
    click(cx, &o.handle, &ids::align("h-left"));
    assert_eq!(
        draft(cx, &o).verse.align,
        kairo_domain::overlay::OverlayTextAlign::Left
    );

    editor(cx, &o, |e, _, cx| {
        e.select_layer(Some("reference".into()), cx)
    });
    click(cx, &o.handle, ids::SHOW_REFERENCE);
    assert!(!draft(cx, &o).reference.show);
    // Every inspector edit is one undo step.
    editor(cx, &o, |e, _, cx| e.undo(cx));
    assert!(draft(cx, &o).reference.show);
}

#[gpui_kit::test]
fn layout_presets_boxes_and_backdrop(cx: &mut TestAppContext) {
    let o = open(cx, Core::new());
    editor(cx, &o, |e, _, cx| {
        e.select_layer(Some("reference".into()), cx)
    });
    inspector(cx, &o, |i, cx| i.set_tab(Tab::Layout, cx));
    click(cx, &o.handle, &ids::preset("top"));
    let d = draft(cx, &o);
    assert_eq!(d.layout.position.as_str(), "top");
    assert_eq!(
        cx.read(|cx| o.editor.read(cx).selected_layer().map(str::to_owned)),
        Some("verse".into()),
        "a preset selects the verse"
    );

    // A number box edits the selected box, clamped, and undo writes back.
    let field = cx.read(|cx| o.inspector.read(cx).number_field("box.w"));
    let field = field.expect("width box");
    cx.update(|cx| field.update(cx, |_, cx| cx.emit(NumberFieldChanged(3.0))));
    draw(cx, &o.handle);
    assert_eq!(
        draft(cx, &o).verse.frame.width_pct,
        8.0,
        "text boxes keep their minimum"
    );
    cx.update(|cx| field.update(cx, |_, cx| cx.emit(NumberFieldChanged(50.0))));
    draw(cx, &o.handle);
    assert_eq!(draft(cx, &o).verse.frame.width_pct, 50.0);
    editor(cx, &o, |e, _, cx| e.undo(cx));
    let undone = draft(cx, &o).verse.frame.width_pct;
    assert_ne!(undone, 50.0);
    let shown = cx.read(|cx| field.read(cx).value());
    assert_eq!(
        shown,
        (undone * 10.0).round() / 10.0,
        "undo writes the box back"
    );

    let on = draft(cx, &o).layout.backdrop_box;
    click(cx, &o.handle, ids::BACKDROP_EYE);
    assert_eq!(draft(cx, &o).layout.backdrop_box, !on);

    // The reference switch is off while the reference is hidden.
    editor(cx, &o, |e, _, cx| {
        e.edit_theme(
            |t| {
                let mut n = t.clone();
                n.reference.show = false;
                n
            },
            cx,
        )
    });
    click(cx, &o.handle, &ids::layer_switch("reference"));
    assert_eq!(
        cx.read(|cx| o.editor.read(cx).selected_layer().map(str::to_owned)),
        Some("verse".into())
    );
}

#[gpui_kit::test]
fn the_element_panel_flips_aligns_arranges_and_returns(cx: &mut TestAppContext) {
    let core = Core::new();
    core.0.lock().unwrap().picked = None;
    let o = open(cx, core);
    editor(cx, &o, |e, _, cx| e.select_layer(Some("box".into()), cx));
    assert!(exists(cx, &o.handle, ids::DUPLICATE));
    assert!(!exists(cx, &o.handle, ids::TABS), "no tabs for an element");
    click(cx, &o.handle, &ids::flip("x"));
    assert!(draft(cx, &o).elements[0].flip_x);
    click(cx, &o.handle, &ids::element_align("left"));
    assert_eq!(draft(cx, &o).elements[0].frame.x_pct, 0.0);
    click(cx, &o.handle, &ids::element_align("bottom"));
    let f = draft(cx, &o).elements[0].frame.clone();
    assert_eq!(f.y_pct, 100.0 - f.height_pct);

    click(cx, &o.handle, ids::DUPLICATE);
    let d = draft(cx, &o);
    assert_eq!(d.elements.len(), 2);
    let copy = cx.read(|cx| o.editor.read(cx).selected_layer().map(str::to_owned));
    assert_eq!(copy.as_deref(), Some(d.elements[1].id.as_str()));
    click(cx, &o.handle, &ids::reorder("back"));
    assert_eq!(draft(cx, &o).elements[0].id, copy.clone().unwrap());

    // The opacity box stores a fraction.
    let field = cx
        .read(|cx| o.inspector.read(cx).number_field("element.opacity"))
        .expect("opacity box");
    cx.update(|cx| field.update(cx, |_, cx| cx.emit(NumberFieldChanged(40.0))));
    draw(cx, &o.handle);
    let d = draft(cx, &o);
    let el = d
        .elements
        .iter()
        .find(|e| Some(&e.id) == copy.as_ref())
        .unwrap();
    assert!((el.opacity - 0.4).abs() < 1e-9);

    click(cx, &o.handle, ids::DELETE);
    assert_eq!(draft(cx, &o).elements.len(), 1);
    editor(cx, &o, |e, _, cx| e.select_layer(Some("box".into()), cx));
    click(cx, &o.handle, ids::DONE);
    assert_eq!(
        cx.read(|cx| o.editor.read(cx).selected_layer().map(str::to_owned)),
        None
    );
    assert!(exists(cx, &o.handle, ids::TABS));
}

#[gpui_kit::test]
fn a_picture_element_takes_a_new_file_and_a_closed_picker_keeps_it(cx: &mut TestAppContext) {
    let core = Core::new();
    let o = open(cx, core.clone());
    let mut pic = create_overlay_element(OverlayElementKind::Image, Some("/old.png"), "pic", None);
    pic.frame = overlay_box(5.0, 5.0, 10.0, 10.0);
    editor(cx, &o, move |e, _, cx| {
        e.edit_theme(
            move |t| {
                let mut n = t.clone();
                n.elements.push(pic.clone());
                n
            },
            cx,
        );
        e.select_layer(Some("pic".into()), cx)
    });
    assert!(exists(cx, &o.handle, ids::MEDIA));
    click(cx, &o.handle, ids::MEDIA);
    let path = |cx: &mut TestAppContext| {
        draft(cx, &o)
            .elements
            .iter()
            .find(|e| e.id == "pic")
            .unwrap()
            .media_path
            .clone()
    };
    assert_eq!(
        path(cx).as_deref(),
        Some("/old.png"),
        "a closed picker changes nothing"
    );
    core.0.lock().unwrap().picked = Some("/new.png".into());
    click(cx, &o.handle, ids::MEDIA);
    assert_eq!(path(cx).as_deref(), Some("/new.png"));
}

// ─── The Screen Themes picker ──────────────────────────────────────────────

struct Picker {
    handle: AnyWindowHandle,
    picker: Entity<ThemePicker>,
}

fn open_picker(cx: &mut TestAppContext, core: Arc<Core>) -> Picker {
    init(cx);
    let slot: Arc<Mutex<Option<Entity<ThemePicker>>>> = Default::default();
    let out = slot.clone();
    let handle = cx
        .update(|cx| {
            gpui_kit::open_window(options(), cx, move |_, cx| {
                let p = cx.new(|cx| ThemePicker::new(core, cx));
                *out.lock().unwrap() = Some(p.clone());
                p
            })
        })
        .unwrap()
        .0;
    let picker = slot.lock().unwrap().take().unwrap();
    draw(cx, &handle);
    Picker { handle, picker }
}

fn picked<R>(
    cx: &mut TestAppContext,
    p: &Picker,
    f: impl FnOnce(&mut ThemePicker, &mut gpui_kit::Context<ThemePicker>) -> R,
) -> R {
    let e = p.picker.clone();
    let r = cx.update(|cx| e.update(cx, f));
    draw(cx, &p.handle);
    r
}

#[gpui_kit::test]
fn the_picker_sets_each_kind_per_screen_and_reports_a_failure(cx: &mut TestAppContext) {
    let core = Core::new();
    let p = open_picker(cx, core.clone());
    picked(cx, &p, |t, cx| t.opened(false, cx));
    assert!(exists(cx, &p.handle, picker::ids::ROOT));
    assert!(exists(cx, &p.handle, &picker::ids::apply("sunday")));
    let screen = cx.read(|cx| p.picker.read(cx).screen().map(|s| s.id.clone()));
    assert_eq!(
        screen.as_deref(),
        Some("pastor"),
        "the first screen is chosen"
    );

    click(cx, &p.handle, &picker::ids::apply("sunday"));
    let s = core.settings();
    assert_eq!(s.overlay.outputs[0].theme_id.as_deref(), Some("sunday"));
    assert_eq!(
        s.overlay.outputs[1].theme_id, None,
        "the lower third is unchanged"
    );
    assert_eq!(
        s.theme_defaults,
        AppSettings::defaults(DEFAULT_OVERLAY_SETTINGS.clone()).theme_defaults,
        "per-screen picks do not change defaults"
    );
    assert_eq!(
        cx.read(|cx| p.picker.read(cx).applied().map(str::to_owned))
            .as_deref(),
        Some("Sunday Scripture applied to Pastor screen")
    );

    // Lyrics keeps the scripture theme.
    picked(cx, &p, |t, cx| t.set_kind(OverlayContentKind::Lyrics, cx));
    assert!(!exists(cx, &p.handle, &picker::ids::apply("sunday")));
    click(cx, &p.handle, &picker::ids::apply("lyrics"));
    let s = core.settings();
    assert_eq!(
        s.overlay.outputs[0]
            .lyrics
            .as_ref()
            .and_then(|l| l.theme_id.as_deref()),
        Some("lyrics")
    );
    assert_eq!(s.overlay.outputs[0].theme_id.as_deref(), Some("sunday"));
    assert!(s.overlay.outputs[1].lyrics.is_none());

    // Another screen: the check follows it.
    picked(cx, &p, |t, cx| t.set_screen("lower", cx));
    let uses = cx.read(|cx| {
        let t = p.picker.read(cx);
        let s = t.screen().unwrap();
        kairo_domain::theme_library::output_uses_theme(s, "lyrics", OverlayContentKind::Lyrics)
    });
    assert!(!uses, "checkmark follows the selected screen");
    click(cx, &p.handle, &picker::ids::apply("lyrics"));
    let s = core.settings();
    assert_eq!(
        s.overlay.outputs[1]
            .lyrics
            .as_ref()
            .and_then(|l| l.theme_id.as_deref()),
        Some("lyrics")
    );
    assert_eq!(s.overlay.outputs[1].theme_id, None);
    assert_eq!(s.overlay.outputs[0].theme_id.as_deref(), Some("sunday"));

    // A failed write says so.
    core.0.lock().unwrap().fail = true;
    click(cx, &p.handle, &picker::ids::apply("lyrics"));
    assert_eq!(
        cx.read(|cx| p.picker.read(cx).error().map(str::to_owned))
            .as_deref(),
        Some(picker::APPLY_FAILED)
    );
    assert!(exists(cx, &p.handle, picker::ids::ALERT));
    // Opening again clears it and follows the page.
    picked(cx, &p, |t, cx| t.opened(false, cx));
    assert!(!exists(cx, &p.handle, picker::ids::ALERT));
    assert_eq!(
        cx.read(|cx| p.picker.read(cx).kind()),
        OverlayContentKind::Scripture
    );
}

#[gpui_kit::test]
fn the_picker_with_no_screens_or_themes_and_a_screen_that_went(cx: &mut TestAppContext) {
    let core = Core::new();
    {
        let mut s = core.0.lock().unwrap();
        s.settings
            .theme_library
            .retain(|t| t.kind == OverlayContentKind::Scripture);
    }
    let p = open_picker(cx, core.clone());
    picked(cx, &p, |t, cx| t.opened(true, cx));
    assert!(
        exists(cx, &p.handle, picker::ids::EMPTY),
        "no lyrics themes yet"
    );

    // The screen is removed after the picker read it.
    picked(cx, &p, |t, cx| {
        t.set_kind(OverlayContentKind::Scripture, cx)
    });
    core.0.lock().unwrap().settings.overlay.outputs.clear();
    click(cx, &p.handle, &picker::ids::apply("sunday"));
    assert_eq!(
        cx.read(|cx| p.picker.read(cx).error().map(str::to_owned))
            .as_deref(),
        Some(picker::APPLY_FAILED)
    );
    // Read back: nothing to choose now, and the tiles are off.
    assert!(cx.read(|cx| p.picker.read(cx).screen().is_none()));
    click(cx, &p.handle, &picker::ids::apply("sunday"));
    assert!(core.settings().overlay.outputs.is_empty());
}
