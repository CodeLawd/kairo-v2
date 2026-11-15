//! UI tests for the Web workspace (prompt 089) against a scripted core and a
//! scripted preview page: the address bar and its search fallback, history,
//! bookmarks, showing and taking down, links and scrolling followed by
//! screens, the pointer, a hidden tab, a refused address, no web engine,
//! and the preview's zoom under scaling.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyElement, AnyWindowHandle, App, AppContext, Bounds, Entity, IntoElement, Styled,
    TestAppContext, WindowBounds, WindowOptions, div, px, size,
};
use gpui_kit::{InteractiveElement, base::TestSupportExt};
use kairo_domain::web::{WebBookmark, WebSettings};
use ui_web::page::{
    FeedSink, Listening, NOTHING_TO_OPEN, PreviewEvent, PreviewFactory, PreviewPage, PreviewSink,
    Reply, WebBackend, WebFeed, WebPage, WebSnapshot, ids,
};

#[derive(Default)]
struct State {
    snapshot: WebSnapshot,
    sink: Option<FeedSink>,
    set: Vec<Option<String>>,
    reloads: usize,
    scrolls: Vec<(f64, f64)>,
    pointers: Vec<Option<(f64, f64)>>,
    saved: Vec<WebSettings>,
    refuse: bool,
}

struct Core(Mutex<State>);

impl WebBackend for Core {
    fn snapshot(&self) -> WebSnapshot {
        self.0.lock().unwrap().snapshot.clone()
    }
    fn listen(&self, sink: FeedSink) -> Listening {
        self.0.lock().unwrap().sink = Some(sink);
        Box::new(())
    }
    fn set_web(&self, url: Option<String>, reply: Reply) {
        let mut s = self.0.lock().unwrap();
        if s.refuse && url.is_some() {
            drop(s);
            reply(Err("That isn\u{2019}t a web address.".into()));
            return;
        }
        s.set.push(url.clone());
        let sink = s.sink.clone();
        drop(s);
        if let Some(sink) = sink {
            sink(WebFeed::Live(url));
        }
        reply(Ok(()));
    }
    fn reload_web(&self) {
        self.0.lock().unwrap().reloads += 1;
    }
    fn scroll_web(&self, x: f64, y: f64) {
        self.0.lock().unwrap().scrolls.push((x, y));
    }
    fn web_pointer(&self, p: Option<(f64, f64)>) {
        self.0.lock().unwrap().pointers.push(p);
    }
    fn save_settings(&self, s: WebSettings) {
        self.0.lock().unwrap().saved.push(s);
    }
}

impl Core {
    fn new(live: Option<&str>) -> Arc<Self> {
        Arc::new(Self(Mutex::new(State {
            snapshot: WebSnapshot {
                live: live.map(str::to_owned),
                settings: WebSettings {
                    show_pointer: true,
                    ..Default::default()
                },
            },
            ..Default::default()
        })))
    }
    fn last_pointer(&self) -> Option<Option<(f64, f64)>> {
        self.0.lock().unwrap().pointers.last().copied()
    }
}

/// What the scripted preview was told.
#[derive(Default)]
struct Seen {
    loads: Vec<String>,
    zoom: Vec<f64>,
    visible: Vec<bool>,
    sink: Option<PreviewSink>,
}

struct FakePreview(Rc<RefCell<Seen>>);

impl PreviewPage for FakePreview {
    fn load(&mut self, url: &str, _: &mut App) {
        self.0.borrow_mut().loads.push(url.into());
    }
    fn set_zoom(&mut self, zoom: f64, _: &mut App) {
        self.0.borrow_mut().zoom.push(zoom);
    }
    fn set_visible(&mut self, v: bool, _: &mut App) {
        self.0.borrow_mut().visible.push(v);
    }
    fn element(&self) -> AnyElement {
        div()
            .id("fake-preview")
            .test_support()
            .size_full()
            .into_any_element()
    }
}

struct Opened {
    handle: AnyWindowHandle,
    page: Entity<WebPage>,
    seen: Rc<RefCell<Seen>>,
}

fn open_with(cx: &mut TestAppContext, core: Arc<Core>, engine: bool) -> Opened {
    cx.update(|cx| {
        gpui_kit::init(cx);
        kairo_ui_kit::init(kairo_ui_kit::Appearance::Dark, cx);
    });
    let seen: Rc<RefCell<Seen>> = Default::default();
    let s = seen.clone();
    let factory: PreviewFactory = Rc::new(move |sink, _, _| {
        if !engine {
            return Err("no web engine".into());
        }
        s.borrow_mut().sink = Some(sink);
        Ok(Box::new(FakePreview(s.clone())) as Box<dyn PreviewPage>)
    });
    let slot: Rc<RefCell<Option<Entity<WebPage>>>> = Default::default();
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
                    let p = cx.new(|cx| WebPage::new(core, factory, window, cx));
                    *out.borrow_mut() = Some(p.clone());
                    p
                },
            )
        })
        .unwrap()
        .0;
    let page = slot.borrow_mut().take().unwrap();
    let o = Opened { handle, page, seen };
    cx.update(|cx| o.page.update(cx, |p, cx| p.set_active(true, cx)));
    for _ in 0..3 {
        draw(cx, &o);
    }
    o
}

fn open(cx: &mut TestAppContext, core: Arc<Core>) -> Opened {
    open_with(cx, core, true)
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

fn type_address(cx: &mut TestAppContext, o: &Opened, text: &str) {
    let input = cx.read(|cx| o.page.read(cx).address_input());
    let text = text.to_owned();
    let page = o.page.clone();
    cx.update_window(o.handle, move |_, w, cx| {
        input.update(cx, |i, cx| i.set_value(text, w, cx));
        page.update(cx, |p, cx| p.go(w, cx));
    })
    .unwrap();
    draw(cx, o);
}

/// The preview page reports `e`.
fn report(cx: &mut TestAppContext, o: &Opened, e: PreviewEvent) {
    let sink = o.seen.borrow().sink.clone().expect("preview built");
    sink(e);
    draw(cx, o);
}

fn current(cx: &mut TestAppContext, o: &Opened) -> Option<String> {
    cx.read(|cx| o.page.read(cx).current().map(str::to_owned))
}

#[gpui_kit::test]
fn the_address_bar_opens_addresses_and_searches_and_refuses_nothing(cx: &mut TestAppContext) {
    let o = open(cx, Core::new(None));
    assert!(exists(cx, &o, ids::EMPTY));
    assert!(!exists(cx, &o, ids::PREVIEW));
    type_address(cx, &o, "example.com");
    assert_eq!(current(cx, &o).as_deref(), Some("https://example.com/"));
    assert_eq!(o.seen.borrow().loads, ["https://example.com/"]);
    assert!(exists(cx, &o, "fake-preview"));
    // The address shows without its scheme.
    let shown = cx.read(|cx| o.page.read(cx).address_input().read(cx).value().to_string());
    assert_eq!(shown, "example.com");

    type_address(cx, &o, "amazing grace lyrics");
    assert_eq!(
        current(cx, &o).as_deref(),
        Some("https://www.google.com/search?q=amazing%20grace%20lyrics")
    );
    type_address(cx, &o, "   ");
    assert_eq!(
        cx.read(|cx| o.page.read(cx).error().map(str::to_owned))
            .as_deref(),
        Some(NOTHING_TO_OPEN)
    );
    assert!(exists(cx, &o, ids::ERROR));
    type_address(cx, &o, "ftp://files.example.com");
    assert!(exists(cx, &o, ids::ERROR), "a non-web address is refused");
}

#[gpui_kit::test]
fn back_forward_and_reload(cx: &mut TestAppContext) {
    let o = open(cx, Core::new(None));
    type_address(cx, &o, "a.example");
    type_address(cx, &o, "b.example");
    click(cx, &o, ids::BACK);
    assert_eq!(current(cx, &o).as_deref(), Some("https://a.example/"));
    click(cx, &o, ids::FORWARD);
    assert_eq!(current(cx, &o).as_deref(), Some("https://b.example/"));
    click(cx, &o, ids::RELOAD);
    let loads = o.seen.borrow().loads.clone();
    assert_eq!(loads.last().map(String::as_str), Some("https://b.example/"));
    assert_eq!(loads.len(), 5, "a, b, back, forward, reload");
    assert_eq!(
        cx.read(|cx| o.page.read(cx).history().entries.len()),
        2,
        "reload adds no entry"
    );
}

#[gpui_kit::test]
fn links_add_history_and_a_redirect_replaces_the_load(cx: &mut TestAppContext) {
    let o = open(cx, Core::new(None));
    type_address(cx, &o, "example.com");
    // The load redirects: it replaces the entry.
    report(
        cx,
        &o,
        PreviewEvent::Navigated("https://www.example.com/".into()),
    );
    let h = cx.read(|cx| o.page.read(cx).history().clone());
    assert_eq!(h.entries, ["https://www.example.com/"]);
    // A link followed after that is a new entry.
    report(
        cx,
        &o,
        PreviewEvent::Navigated("https://www.example.com/about".into()),
    );
    let h = cx.read(|cx| o.page.read(cx).history().clone());
    assert_eq!(h.entries.len(), 2);
    assert_eq!(h.index, 1);
    // A load that arrives as itself also counts as the load.
    type_address(cx, &o, "other.example");
    report(
        cx,
        &o,
        PreviewEvent::Navigated("https://other.example/".into()),
    );
    report(
        cx,
        &o,
        PreviewEvent::Navigated("https://other.example/next".into()),
    );
    let h = cx.read(|cx| o.page.read(cx).history().clone());
    assert_eq!(
        h.entries.len(),
        4,
        "the link after our own load is a new entry"
    );
}

#[gpui_kit::test]
fn show_follow_links_and_scrolling_then_take_down(cx: &mut TestAppContext) {
    let core = Core::new(None);
    let o = open(cx, core.clone());
    assert!(exists(cx, &o, ids::SHOW));
    type_address(cx, &o, "example.com");
    // Not live yet: scrolling stays here.
    report(
        cx,
        &o,
        PreviewEvent::Scroll {
            x: 0.0,
            y: 100.0,
            adaptive: false,
        },
    );
    assert!(core.0.lock().unwrap().scrolls.is_empty());
    click(cx, &o, ids::SHOW);
    assert_eq!(
        core.0.lock().unwrap().set,
        [Some("https://example.com/".into())]
    );
    assert!(cx.read(|cx| o.page.read(cx).is_live()));
    assert!(!exists(cx, &o, ids::SHOW));
    assert!(exists(cx, &o, ids::TAKE_DOWN));
    report(
        cx,
        &o,
        PreviewEvent::Scroll {
            x: 0.0,
            y: 420.0,
            adaptive: false,
        },
    );
    assert_eq!(core.0.lock().unwrap().scrolls, [(0.0, 420.0)]);
    // A link followed takes screens along.
    report(
        cx,
        &o,
        PreviewEvent::Navigated("https://example.com/".into()),
    );
    report(
        cx,
        &o,
        PreviewEvent::Navigated("https://example.com/news".into()),
    );
    assert_eq!(
        core.0
            .lock()
            .unwrap()
            .set
            .last()
            .cloned()
            .flatten()
            .as_deref(),
        Some("https://example.com/news")
    );
    // With follow off, neither links nor scrolling reach screens.
    cx.update(|cx| o.page.update(cx, |p, cx| p.set_follow(false, cx)));
    report(
        cx,
        &o,
        PreviewEvent::Scroll {
            x: 0.0,
            y: 9.0,
            adaptive: false,
        },
    );
    report(
        cx,
        &o,
        PreviewEvent::Navigated("https://example.com/other".into()),
    );
    assert_eq!(core.0.lock().unwrap().scrolls.len(), 1);
    assert_eq!(core.0.lock().unwrap().set.len(), 2);
    click(cx, &o, ids::RELOAD_SCREENS);
    assert_eq!(core.0.lock().unwrap().reloads, 1);
    click(cx, &o, ids::TAKE_DOWN);
    assert_eq!(core.0.lock().unwrap().set.last().cloned(), Some(None));
    assert!(!exists(cx, &o, ids::TAKE_DOWN));
}

#[gpui_kit::test]
fn the_pointer_reaches_screens_only_over_the_live_page(cx: &mut TestAppContext) {
    let core = Core::new(Some("https://example.com/"));
    let o = open(cx, core.clone());
    assert_eq!(
        current(cx, &o).as_deref(),
        Some("https://example.com/"),
        "the live page opens"
    );
    report(cx, &o, PreviewEvent::Pointer(Some((960.0, 540.0))));
    assert_eq!(core.last_pointer(), Some(Some((960.0, 540.0))));
    report(cx, &o, PreviewEvent::Pointer(Some((2000.0, 540.0))));
    assert_eq!(
        core.last_pointer(),
        Some(None),
        "outside the screens' frame it comes off"
    );
    report(cx, &o, PreviewEvent::Pointer(Some((10.0, 10.0))));
    report(cx, &o, PreviewEvent::Pointer(None));
    assert_eq!(core.last_pointer(), Some(None));
    // Off in settings: saved, and the pointer comes off.
    report(cx, &o, PreviewEvent::Pointer(Some((10.0, 10.0))));
    click(cx, &o, ids::POINTER);
    assert_eq!(core.last_pointer(), Some(None));
    assert!(!core.0.lock().unwrap().saved.last().unwrap().show_pointer);
    let before = core.0.lock().unwrap().pointers.len();
    report(cx, &o, PreviewEvent::Pointer(Some((10.0, 10.0))));
    assert_eq!(
        core.0.lock().unwrap().pointers.len(),
        before,
        "nothing is sent while off"
    );
}

#[gpui_kit::test]
fn a_hidden_tab_stops_the_preview_and_the_pointer(cx: &mut TestAppContext) {
    let core = Core::new(Some("https://example.com/"));
    let o = open(cx, core.clone());
    assert!(exists(cx, &o, "fake-preview"));
    report(cx, &o, PreviewEvent::Pointer(Some((10.0, 10.0))));
    cx.update(|cx| o.page.update(cx, |p, cx| p.set_active(false, cx)));
    draw(cx, &o);
    assert_eq!(o.seen.borrow().visible.last(), Some(&false));
    assert!(
        !exists(cx, &o, "fake-preview"),
        "the page is not drawn while hidden"
    );
    assert_eq!(core.last_pointer(), Some(None));
    cx.update(|cx| o.page.update(cx, |p, cx| p.set_active(true, cx)));
    draw(cx, &o);
    assert_eq!(o.seen.borrow().visible.last(), Some(&true));
}

#[gpui_kit::test]
fn bookmarks_save_open_and_remove(cx: &mut TestAppContext) {
    let core = Core::new(None);
    let o = open(cx, core.clone());
    assert!(exists(cx, &o, ids::NO_BOOKMARKS));
    click(cx, &o, ids::BOOKMARK_THIS);
    assert!(
        core.0.lock().unwrap().saved.is_empty(),
        "nothing to save without a page"
    );
    type_address(cx, &o, "example.com/watch");
    click(cx, &o, ids::BOOKMARK_THIS);
    let saved = core.0.lock().unwrap().saved.last().cloned().unwrap();
    assert_eq!(saved.bookmarks.len(), 1);
    assert_eq!(saved.bookmarks[0].name, "example.com/watch");
    assert!(cx.read(|cx| o.page.read(cx).bookmarked()));
    let id = saved.bookmarks[0].id.clone();
    type_address(cx, &o, "other.example");
    click(cx, &o, &ids::bookmark(&id));
    assert_eq!(
        current(cx, &o).as_deref(),
        Some("https://example.com/watch")
    );
    // Pressing it again removes it.
    click(cx, &o, ids::BOOKMARK_THIS);
    assert!(
        core.0
            .lock()
            .unwrap()
            .saved
            .last()
            .unwrap()
            .bookmarks
            .is_empty()
    );
    // A bookmark from settings, removed with its own button.
    let sink = core.0.lock().unwrap().sink.clone().unwrap();
    sink(WebFeed::Settings(WebSettings {
        bookmarks: vec![WebBookmark {
            id: "b1".into(),
            name: "Giving".into(),
            url: "https://give.example/".into(),
            extra: Default::default(),
        }],
        show_pointer: true,
        ..Default::default()
    }));
    draw(cx, &o);
    assert!(exists(cx, &o, &ids::bookmark("b1")));
    cx.update(|cx| o.page.update(cx, |p, cx| p.remove_bookmark("b1", cx)));
    draw(cx, &o);
    assert!(!exists(cx, &o, &ids::bookmark("b1")));
}

#[gpui_kit::test]
fn a_refused_address_is_reported(cx: &mut TestAppContext) {
    let core = Core::new(None);
    core.0.lock().unwrap().refuse = true;
    let o = open(cx, core);
    type_address(cx, &o, "example.com");
    click(cx, &o, ids::SHOW);
    assert!(exists(cx, &o, ids::ERROR));
    assert!(!cx.read(|cx| o.page.read(cx).is_live()));
}

#[gpui_kit::test]
fn without_a_web_engine_the_page_still_shows_on_screens(cx: &mut TestAppContext) {
    let core = Core::new(None);
    let o = open_with(cx, core.clone(), false);
    type_address(cx, &o, "example.com");
    assert!(exists(cx, &o, ids::UNAVAILABLE));
    assert!(cx.read(|cx| o.page.read(cx).unavailable().is_some()));
    click(cx, &o, ids::SHOW);
    assert_eq!(
        core.0.lock().unwrap().set,
        [Some("https://example.com/".into())]
    );
}

#[gpui_kit::test]
fn the_preview_zoom_follows_its_box_and_adaptive_pages_get_the_screens_shape(
    cx: &mut TestAppContext,
) {
    let o = open(cx, Core::new(None));
    type_address(cx, &o, "example.com");
    draw(cx, &o);
    let zoom = cx.read(|cx| o.page.read(cx).zoom());
    assert!(zoom > 0.0, "laid out");
    assert_eq!(o.seen.borrow().zoom.last().copied(), Some(zoom));
    cx.update(|cx| o.page.update(cx, |p, cx| p.layout_for_tests(960.0, cx)));
    assert_eq!(o.seen.borrow().zoom.last().copied(), Some(0.5));
    assert!(!exists(cx, &o, ids::EXACT_NOTE));
    report(
        cx,
        &o,
        PreviewEvent::Scroll {
            x: 0.0,
            y: 0.0,
            adaptive: true,
        },
    );
    draw(cx, &o);
    assert!(exists(cx, &o, ids::EXACT_NOTE));
    assert!(!exists(cx, &o, ids::CUT));
    // A new load forgets the page's shape.
    type_address(cx, &o, "other.example");
    assert_eq!(cx.read(|cx| o.page.read(cx).adaptive()), None);
}
