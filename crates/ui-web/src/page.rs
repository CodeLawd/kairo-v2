//! The Web workspace (prompt 089), from `WebPage.tsx` and
//! `ScaledWebFrame.tsx`: browse a page, then put it on the screens behind
//! the words.
//!
//! Bookmarks on the left; the address bar, the preview and the screen
//! controls in the middle. Screens load their own copy of the same address
//! in `kairo-engine` (prompt 059). While the preview's page is the live one
//! and "Screens follow" is on, links followed and scrolling here carry over
//! to them, and the pointer is drawn on screens. The preview is muted.
//!
//! The preview is its own `gpui-webview` page laid out 1920 wide and zoomed
//! to its box (`set_zoom`), so text wraps as the audience sees it and a
//! scroll position or pointer here is the same place on every screen.

use kairo_ui_kit::ControlSize;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Disableable, Icon, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyElement, AnyView, App, AppContext, Bounds, Context, Entity, FocusHandle, Focusable,
    FontWeight, InteractiveElement, IntoElement, ParentElement, Pixels, Render, Role, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Task, Window, canvas, div, px,
};
use kairo_domain::web::{
    WEB_FRAME_HEIGHT, WEB_FRAME_WIDTH, WebBookmark, WebSettings, display_address, web_address,
};
use kairo_ui_kit::tokens::{TextStep, radius, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};

// ─── The core's side ───────────────────────────────────────────────────────

/// A push from the core.
#[derive(Clone, Debug, PartialEq)]
pub enum WebFeed {
    /// What is on screens (`ProgramState.web.url`).
    Live(Option<String>),
    /// `AppSettings.web`, normalised.
    Settings(WebSettings),
}

pub type FeedSink = Arc<dyn Fn(WebFeed) + Send + Sync>;
pub type Listening = Box<dyn Send>;
pub type Reply = Box<dyn FnOnce(Result<(), String>) + Send>;

/// What the page reads at start.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WebSnapshot {
    pub live: Option<String>,
    pub settings: WebSettings,
}

/// `window.api.program.*Web*` and `settings.set('web')`.
pub trait WebBackend: Send + Sync + 'static {
    fn snapshot(&self) -> WebSnapshot;
    fn listen(&self, sink: FeedSink) -> Listening;
    /// `program.setWeb(url)`: `None` takes the page down. Fails on an
    /// address screens will not load.
    fn set_web(&self, url: Option<String>, reply: Reply);
    fn reload_web(&self);
    fn scroll_web(&self, x: f64, y: f64);
    /// The pointer over the live page in its 1920-wide coordinates; `None` hides it.
    fn web_pointer(&self, point: Option<(f64, f64)>);
    fn save_settings(&self, settings: WebSettings);
}

// ─── The preview's side ────────────────────────────────────────────────────

/// What the preview page reports.
#[derive(Clone, Debug, PartialEq)]
pub enum PreviewEvent {
    /// The page moved to another address (a link, a redirect, a load).
    Navigated(String),
    /// Where the page is scrolled, and whether its layout follows the
    /// window's height (`SCROLL_SCRIPT`).
    Scroll { x: f64, y: f64, adaptive: bool },
    /// The pointer in page coordinates; `None` once it leaves.
    Pointer(Option<(f64, f64)>),
}

pub type PreviewSink = Rc<dyn Fn(PreviewEvent)>;

/// One preview page on the GPUI thread.
pub trait PreviewPage {
    fn load(&mut self, url: &str, cx: &mut App);
    /// The page's zoom: its box's width over 1920.
    fn set_zoom(&mut self, zoom: f64, cx: &mut App);
    /// Hidden while the tab is not showing or a window covers it.
    fn set_visible(&mut self, visible: bool, cx: &mut App);
    fn element(&self) -> AnyElement;
}

/// Builds the preview, or says why there is none (no web engine on this
/// system).
pub type PreviewFactory =
    Rc<dyn Fn(PreviewSink, &mut Window, &mut App) -> Result<Box<dyn PreviewPage>, String>>;

// ─── Pure rules ────────────────────────────────────────────────────────────

/// The back and forward list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct History {
    pub entries: Vec<String>,
    /// -1 while empty.
    pub index: isize,
}

impl History {
    pub fn starting_at(url: Option<&str>) -> Self {
        match url {
            Some(u) => Self {
                entries: vec![u.to_owned()],
                index: 0,
            },
            None => Self {
                entries: vec![],
                index: -1,
            },
        }
    }
    pub fn current(&self) -> Option<&str> {
        (self.index >= 0)
            .then(|| self.entries.get(self.index as usize).map(String::as_str))
            .flatten()
    }
    /// A new entry after the current one; anything ahead is dropped.
    pub fn push(&mut self, url: &str) {
        let keep = (self.index + 1).max(0) as usize;
        self.entries.truncate(keep);
        self.entries.push(url.to_owned());
        self.index = self.entries.len() as isize - 1;
    }
    /// The current entry becomes `url` (a redirect of a load we started).
    pub fn replace(&mut self, url: &str) {
        match self.index {
            i if i >= 0 => self.entries[i as usize] = url.to_owned(),
            _ => self.push(url),
        }
    }
    pub fn can_back(&self) -> bool {
        self.index > 0
    }
    pub fn can_forward(&self) -> bool {
        self.index + 1 < self.entries.len() as isize
    }
    /// Moves by `by` and returns the address there.
    pub fn step(&mut self, by: isize) -> Option<String> {
        let index = self.index + by;
        let url = self.entries.get(usize::try_from(index).ok()?)?.clone();
        self.index = index;
        Some(url)
    }
}

/// The preview's zoom for a box `width` wide: the page lays out at 1920.
pub fn zoom_for(width: f32) -> f64 {
    f64::from(width.max(0.0)) / f64::from(WEB_FRAME_WIDTH)
}

/// The page height the preview gives the page in `fill` mode: the box's
/// height in page pixels, rounded as the frame does.
pub fn fill_height(box_height: f32, zoom: f64) -> f64 {
    if zoom > 0.0 {
        (f64::from(box_height) / zoom).round()
    } else {
        f64::from(WEB_FRAME_HEIGHT)
    }
}

/// Where the screens' picture ends in the box, when the preview is taller
/// than the screens; `None` otherwise.
pub fn cut_line(box_height: f32, zoom: f64) -> Option<f32> {
    (zoom > 0.0 && fill_height(box_height, zoom) > f64::from(WEB_FRAME_HEIGHT))
        .then(|| (f64::from(WEB_FRAME_HEIGHT) * zoom).round() as f32)
}

/// A point in the preview's box (GPUI pixels from its top left) in the
/// page's own coordinates.
pub fn page_point(local: (f32, f32), zoom: f64) -> Option<(f64, f64)> {
    (zoom > 0.0).then(|| (f64::from(local.0) / zoom, f64::from(local.1) / zoom))
}

/// The exact-mode box: 16:9, as large as fits in `area`.
pub fn exact_box(area: (f32, f32)) -> (f32, f32) {
    let w = area.0.min(area.1 * 16.0 / 9.0).max(0.0);
    (w, w * 9.0 / 16.0)
}

/// Whether the pointer at `point` is drawn on screens.
pub fn pointer_shows(
    enabled: bool,
    follow: bool,
    live: Option<&str>,
    here: Option<&str>,
    point: Option<(f64, f64)>,
) -> Option<(f64, f64)> {
    let (x, y) = point?;
    let on = enabled
        && follow
        && live.is_some()
        && live == here
        && (0.0..=f64::from(WEB_FRAME_WIDTH)).contains(&x)
        && (0.0..=f64::from(WEB_FRAME_HEIGHT)).contains(&y);
    on.then_some((x, y))
}

/// The address bar's message for text that is neither an address nor a search.
pub const NOTHING_TO_OPEN: &str = "Type a web address or something to search for.";

/// `SCROLL_SCRIPT`, reporting through Wry's IPC instead of `postMessage`.
pub const SCROLL_SCRIPT: &str = r#"(() => {
  if (window.__kairoScroll) return;
  window.__kairoScroll = true;
  const post = (m) => { try { window.ipc.postMessage(JSON.stringify(m)); } catch (_) {} };
  const measureAdaptive = () => {
    const h = innerHeight;
    if (!document.body || h < 200) return false;
    const all = document.body.getElementsByTagName('*');
    const n = Math.min(all.length, 4000);
    for (let i = 0; i < n; i++) {
      const el = all[i];
      const height = el.offsetHeight;
      if (height < h * 0.5 || el.offsetWidth < innerWidth * 0.5) continue;
      if (Math.abs(height - h) <= 2) return true;
      if (getComputedStyle(el).position === 'sticky') return true;
    }
    return false;
  };
  let adaptive = false;
  let queued = false;
  const report = () => {
    if (queued) return;
    queued = true;
    requestAnimationFrame(() => {
      queued = false;
      post({ type: 'kairo:web-scroll', x: scrollX, y: scrollY, adaptive });
    });
  };
  let settle = 0;
  const start = () => {
    adaptive = measureAdaptive();
    new ResizeObserver(() => {
      clearTimeout(settle);
      settle = setTimeout(() => { adaptive = measureAdaptive(); report(); }, 250);
    }).observe(document.documentElement);
    report();
  };
  addEventListener('scroll', report, { passive: true });
  if (document.readyState === 'loading') addEventListener('DOMContentLoaded', start); else start();
  let px = 0, py = 0, pq = false;
  const sendPointer = () => {
    if (pq) return;
    pq = true;
    requestAnimationFrame(() => { pq = false; post({ type: 'kairo:web-pointer', x: px, y: py }); });
  };
  addEventListener('pointermove', (e) => { px = e.clientX; py = e.clientY; sendPointer(); }, { passive: true, capture: true });
  document.addEventListener('pointerleave', () => post({ type: 'kairo:web-pointer', x: null, y: null }));
  addEventListener('blur', () => post({ type: 'kairo:web-pointer', x: null, y: null }));
})()"#;

/// `MUTE_SCRIPT`: the preview never sounds; the screens carry the sound.
pub const MUTE_SCRIPT: &str = r#"(() => {
  if (window.__kairoMuted) return;
  window.__kairoMuted = true;
  const hush = (media) => { media.muted = true; };
  const sweep = () => document.querySelectorAll('video, audio').forEach(hush);
  const play = HTMLMediaElement.prototype.play;
  HTMLMediaElement.prototype.play = function () { hush(this); return play.apply(this, arguments); };
  document.addEventListener('volumechange', (e) => {
    if (e.target instanceof HTMLMediaElement && !e.target.muted) hush(e.target);
  }, true);
  const begin = () => { sweep(); new MutationObserver(sweep).observe(document, { childList: true, subtree: true }); };
  if (document.readyState === 'loading') addEventListener('DOMContentLoaded', begin); else begin();
})()"#;

/// Reads one IPC message from the scripts.
pub fn parse_message(body: &str) -> Option<PreviewEvent> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    match v["type"].as_str()? {
        "kairo:web-scroll" => Some(PreviewEvent::Scroll {
            x: v["x"].as_f64()?,
            y: v["y"].as_f64()?,
            adaptive: v["adaptive"].as_bool() == Some(true),
        }),
        "kairo:web-pointer" => Some(PreviewEvent::Pointer(
            match (v["x"].as_f64(), v["y"].as_f64()) {
                (Some(x), Some(y)) => Some((x, y)),
                _ => None,
            },
        )),
        _ => None,
    }
}

/// The preview's own navigation rule: web pages only. Screens load only
/// http(s) (`setWeb`), so the preview does not wander elsewhere.
pub fn preview_may_load(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://") || url == "about:blank"
}

// ─── The page ──────────────────────────────────────────────────────────────

pub mod ids {
    pub const ROOT: &str = "web-page";
    pub const BOOKMARK_THIS: &str = "web-bookmark-this";
    pub const BACK: &str = "web-back";
    pub const FORWARD: &str = "web-forward";
    pub const RELOAD: &str = "web-reload";
    pub const ADDRESS: &str = "web-address";
    pub const ERROR: &str = "web-error";
    pub const PREVIEW: &str = "web-preview";
    pub const EMPTY: &str = "web-empty";
    pub const UNAVAILABLE: &str = "web-unavailable";
    pub const CUT: &str = "web-cut-line";
    pub const EXACT_NOTE: &str = "web-exact-note";
    pub const FOLLOW: &str = "web-follow";
    pub const POINTER: &str = "web-pointer";
    pub const RELOAD_SCREENS: &str = "web-reload-screens";
    pub const TAKE_DOWN: &str = "web-take-down";
    pub const SHOW: &str = "web-show";
    pub const STATUS: &str = "web-status";
    pub const NO_BOOKMARKS: &str = "web-no-bookmarks";
    pub fn bookmark(id: &str) -> String {
        format!("web-bookmark-{id}")
    }
    pub fn remove(id: &str) -> String {
        format!("web-bookmark-remove-{id}")
    }
}

enum Preview {
    /// Not built yet: nothing has been opened.
    None,
    Ready(Box<dyn PreviewPage>),
    /// The system has no web engine the preview can use.
    Unavailable(String),
}

pub struct WebPage {
    backend: Arc<dyn WebBackend>,
    factory: PreviewFactory,
    preview: Preview,
    live: Option<String>,
    settings: WebSettings,
    history: History,
    address: Entity<InputState>,
    focus: FocusHandle,
    follow: bool,
    error: Option<String>,
    /// The first navigation after a load we started is that load.
    loading: bool,
    adaptive: Option<bool>,
    /// The tab is showing and nothing covers it.
    active: bool,
    pointer_on: bool,
    area: Rc<Cell<Bounds<Pixels>>>,
    zoom: f64,
    rail: Option<AnyView>,
    /// The media dock under the page (as Bible and Lyrics draw it).
    media_dock: Option<AnyView>,
    _feed: Task<()>,
    _preview_feed: Option<Task<()>>,
    _listening: Listening,
    _subscriptions: Vec<Subscription>,
}

impl Focusable for WebPage {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl WebPage {
    pub fn new(
        backend: Arc<dyn WebBackend>,
        factory: PreviewFactory,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let snapshot = backend.snapshot();
        let (tx, mut rx) = futures_channel::mpsc::unbounded::<WebFeed>();
        let sink: FeedSink = Arc::new(move |f| {
            let _ = tx.unbounded_send(f);
        });
        let listening = backend.listen(sink);
        let feed = cx.spawn(async move |this, cx| {
            use futures_util::StreamExt;
            while let Some(f) = rx.next().await {
                if this.update(cx, |t, cx| t.on_feed(f, cx)).is_err() {
                    break;
                }
            }
        });
        let address = cx.new(|cx| InputState::new(window, cx).placeholder("Web address or search"));
        let subscriptions = vec![cx.subscribe_in(
            &address,
            window,
            |this, input, e: &InputEvent, window, cx| match e {
                InputEvent::PressEnter { .. } => this.go(window, cx),
                InputEvent::Focus => {
                    // Editing shows the whole address.
                    if let Some(current) = this.history.current().map(str::to_owned) {
                        input.update(cx, |i, cx| {
                            if i.value() != current.as_str() {
                                i.set_value(current, window, cx)
                            }
                        });
                    }
                }
                InputEvent::Blur => this.show_address(window, cx),
                InputEvent::Change => {}
            },
        )];
        let live = snapshot.live.clone();
        let mut page = Self {
            backend,
            factory,
            preview: Preview::None,
            live: live.clone(),
            settings: snapshot.settings,
            history: History::starting_at(live.as_deref()),
            address,
            focus: cx.focus_handle(),
            follow: true,
            error: None,
            loading: false,
            adaptive: None,
            active: false,
            pointer_on: false,
            area: Rc::default(),
            zoom: 0.0,
            rail: None,
            media_dock: None,
            _feed: feed,
            _preview_feed: None,
            _listening: listening,
            _subscriptions: subscriptions,
        };
        // A page already on screens opens in the preview.
        if let Some(url) = live {
            page.load_preview(&url, window, cx);
            page.show_address(window, cx);
        }
        page
    }

    /// The media dock, drawn under the page and beside its right panel.
    pub fn set_dock(&mut self, dock: Option<AnyView>, cx: &mut Context<Self>) {
        self.media_dock = dock;
        cx.notify();
    }

    pub fn set_rail(&mut self, rail: Option<AnyView>, cx: &mut Context<Self>) {
        self.rail = rail;
        cx.notify();
    }

    // ─── Reading ───────────────────────────────────────────────────────────

    pub fn current(&self) -> Option<&str> {
        self.history.current()
    }
    pub fn history(&self) -> &History {
        &self.history
    }
    pub fn live(&self) -> Option<&str> {
        self.live.as_deref()
    }
    pub fn is_live(&self) -> bool {
        self.live.is_some() && self.live.as_deref() == self.current()
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    pub fn settings(&self) -> &WebSettings {
        &self.settings
    }
    pub fn follow(&self) -> bool {
        self.follow
    }
    pub fn adaptive(&self) -> Option<bool> {
        self.adaptive
    }
    pub fn active(&self) -> bool {
        self.active
    }
    pub fn unavailable(&self) -> Option<&str> {
        match &self.preview {
            Preview::Unavailable(why) => Some(why),
            _ => None,
        }
    }
    pub fn address_input(&self) -> Entity<InputState> {
        self.address.clone()
    }
    pub fn bookmarked(&self) -> bool {
        self.current()
            .is_some_and(|c| self.settings.bookmarks.iter().any(|b| b.url == c))
    }

    // ─── The core ──────────────────────────────────────────────────────────

    fn on_feed(&mut self, feed: WebFeed, cx: &mut Context<Self>) {
        match feed {
            WebFeed::Live(url) => self.live = url,
            WebFeed::Settings(s) => self.settings = s,
        }
        cx.notify();
    }

    /// Takes the pointer off screens.
    fn hide_pointer(&mut self) {
        if self.pointer_on {
            self.pointer_on = false;
        }
        self.backend.web_pointer(None);
    }

    /// `put`: shows `url` on screens, or takes the page down.
    pub fn put(&mut self, url: Option<String>, cx: &mut Context<Self>) {
        self.error = None;
        self.hide_pointer();
        let (tx, rx) = futures_channel::oneshot::channel();
        self.backend.set_web(
            url,
            Box::new(move |r| {
                let _ = tx.send(r);
            }),
        );
        cx.spawn(async move |this, cx| {
            let result = rx
                .await
                .unwrap_or_else(|_| Err("The request was dropped.".into()));
            if let Err(error) = result {
                let _ = this.update(cx, |t, cx| {
                    t.error = Some(error);
                    cx.notify();
                });
            }
        })
        .detach();
        cx.notify();
    }

    pub fn show_on_screens(&mut self, cx: &mut Context<Self>) {
        if let Some(url) = self.current().map(str::to_owned) {
            self.put(Some(url), cx);
        }
    }

    pub fn take_down(&mut self, cx: &mut Context<Self>) {
        self.put(None, cx);
    }

    pub fn reload_screens(&mut self) {
        self.backend.reload_web();
    }

    pub fn set_follow(&mut self, follow: bool, cx: &mut Context<Self>) {
        self.follow = follow;
        // Without follow the preview and screens can differ, so the pointer
        // would point at the wrong thing.
        if !follow {
            self.hide_pointer();
        }
        cx.notify();
    }

    fn save_settings(&mut self, next: WebSettings, cx: &mut Context<Self>) {
        let value = serde_json::to_value(&next).unwrap_or_default();
        let normalized = kairo_domain::web::normalize_web_settings(&value);
        self.settings = normalized.clone();
        self.backend.save_settings(normalized);
        cx.notify();
    }

    pub fn set_show_pointer(&mut self, on: bool, cx: &mut Context<Self>) {
        let mut next = self.settings.clone();
        next.show_pointer = on;
        self.save_settings(next, cx);
        if !on {
            self.hide_pointer();
        }
    }

    pub fn toggle_bookmark(&mut self, cx: &mut Context<Self>) {
        let Some(current) = self.current().map(str::to_owned) else {
            return;
        };
        let mut next = self.settings.clone();
        if self.bookmarked() {
            next.bookmarks.retain(|b| b.url != current);
        } else {
            next.bookmarks.push(WebBookmark {
                id: uuid::Uuid::new_v4().to_string(),
                name: display_address(&current),
                url: current,
                extra: Default::default(),
            });
        }
        self.save_settings(next, cx);
    }

    pub fn remove_bookmark(&mut self, id: &str, cx: &mut Context<Self>) {
        let mut next = self.settings.clone();
        next.bookmarks.retain(|b| b.id != id);
        self.save_settings(next, cx);
    }

    // ─── Browsing ──────────────────────────────────────────────────────────

    fn show_address(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shown = self.current().map(display_address).unwrap_or_default();
        self.address.update(cx, |i, cx| {
            if i.value() != shown.as_str() {
                i.set_value(shown, window, cx)
            }
        });
    }

    fn load_preview(&mut self, url: &str, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.preview, Preview::None) {
            let (tx, mut rx) = futures_channel::mpsc::unbounded::<PreviewEvent>();
            let sink: PreviewSink = Rc::new(move |e| {
                let _ = tx.unbounded_send(e);
            });
            match (self.factory)(sink, window, cx) {
                Ok(mut page) => {
                    // Laid out before the page existed: give it the zoom now.
                    if self.zoom > 0.0 {
                        page.set_zoom(self.zoom, cx);
                    }
                    self.preview = Preview::Ready(page);
                    self._preview_feed = Some(cx.spawn_in(window, async move |this, cx| {
                        use futures_util::StreamExt;
                        while let Some(e) = rx.next().await {
                            if this
                                .update_in(cx, |t, w, cx| t.on_preview(e, w, cx))
                                .is_err()
                            {
                                break;
                            }
                        }
                    }));
                }
                Err(why) => {
                    tracing::error!(error = %why, "[Web] preview unavailable");
                    self.preview = Preview::Unavailable(why);
                }
            }
        }
        if let Preview::Ready(page) = &mut self.preview {
            page.load(url, cx);
            page.set_visible(self.active, cx);
        }
    }

    /// `open`: loads `url` in the preview, as a new entry unless `push` is off.
    pub fn open(&mut self, url: &str, push: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.hide_pointer();
        self.loading = true;
        self.adaptive = None;
        self.error = None;
        if push {
            self.history.push(url);
        }
        self.load_preview(url, window, cx);
        self.show_address(window, cx);
        cx.notify();
    }

    /// The address bar's Enter: an address, else a search.
    pub fn go(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self.address.read(cx).value().to_string();
        match web_address(&typed, true) {
            Some(url) => self.open(&url, true, window, cx),
            None => {
                self.error = Some(NOTHING_TO_OPEN.into());
                cx.notify();
            }
        }
    }

    pub fn step(&mut self, by: isize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(url) = self.history.step(by) {
            self.open(&url, false, window, cx);
        }
    }

    pub fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(url) = self.current().map(str::to_owned) {
            self.open(&url, false, window, cx);
        }
    }

    /// What the preview page reported.
    pub fn on_preview(&mut self, event: PreviewEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            PreviewEvent::Navigated(url) => {
                let before = self.current().map(str::to_owned);
                if Some(url.as_str()) == before.as_deref() {
                    // Our own load arriving: the next navigation is a link.
                    self.loading = false;
                    return;
                }
                if std::mem::take(&mut self.loading) && self.history.index >= 0 {
                    self.history.replace(&url);
                } else {
                    self.history.push(&url);
                }
                self.adaptive = None;
                self.hide_pointer();
                self.show_address(window, cx);
                // The page on screens moved on in the preview: take screens along.
                if self.follow && self.live.is_some() && self.live == before {
                    self.put(Some(url), cx);
                }
            }
            PreviewEvent::Scroll { x, y, adaptive } => {
                self.adaptive = Some(adaptive);
                if self.follow && self.live.is_some() && self.live.as_deref() == self.current() {
                    self.backend.scroll_web(x, y);
                }
            }
            PreviewEvent::Pointer(point) => {
                match pointer_shows(
                    self.settings.show_pointer,
                    self.follow,
                    self.live.as_deref(),
                    self.current(),
                    point,
                ) {
                    Some(p) => {
                        self.pointer_on = true;
                        self.backend.web_pointer(Some(p));
                    }
                    None => {
                        if self.pointer_on {
                            self.hide_pointer();
                        }
                    }
                }
                return;
            }
        }
        cx.notify();
    }

    /// The tab showed or hid (or a window now covers it). Hidden, the
    /// preview stops drawing and the pointer comes off screens.
    pub fn set_active(&mut self, active: bool, cx: &mut Context<Self>) {
        if self.active == active {
            return;
        }
        self.active = active;
        if !active {
            self.hide_pointer();
        }
        if let Preview::Ready(page) = &mut self.preview {
            page.set_visible(active, cx);
        }
        cx.notify();
    }

    fn fit_zoom(&mut self, width: f32, cx: &mut App) {
        let zoom = zoom_for(width);
        if zoom > 0.0 && (zoom - self.zoom).abs() > 1e-6 {
            self.zoom = zoom;
            if let Preview::Ready(page) = &mut self.preview {
                page.set_zoom(zoom, cx);
            }
        }
    }

    pub fn zoom(&self) -> f64 {
        self.zoom
    }

    /// Sets the preview box width as layout would, for tests.
    pub fn layout_for_tests(&mut self, width: f32, cx: &mut Context<Self>) {
        self.fit_zoom(width, cx);
    }
}

// ─── Drawing ───────────────────────────────────────────────────────────────

impl WebPage {
    fn render_bookmarks(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let this = cx.entity();
        let current = self.current().map(str::to_owned);
        let bookmarked = self.bookmarked();
        let top = {
            let this = this.clone();
            h_flex()
                .id(ids::BOOKMARK_THIS)
                .test_support()
                .role(Role::Button)
                .aria_label(if bookmarked {
                    "Remove bookmark"
                } else {
                    "Bookmark this page"
                })
                .gap_3()
                .px_3()
                .py_2p5()
                .rounded(radius::lg())
                .bg(k.surface_tertiary)
                // No page: a press from assistive technology does nothing, as the pointer does.
                .when(current.is_none(), |d| d.opacity(0.5).on_click(|_, _, _| {}))
                .when(current.is_some(), |d| {
                    d.cursor_pointer()
                        .hover(|s| s.bg(k.surface_elevated))
                        .on_click(move |_, _, cx| this.update(cx, |t, cx| t.toggle_bookmark(cx)))
                })
                .child(
                    div()
                        .size_8()
                        .flex_none()
                        .rounded(radius::md())
                        .bg(k.accent[5])
                        .text_color(k.on_accent)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(Icon::new(KairoIcon::BookmarkSimple.weight(bookmarked)).size_4()),
                )
                .child(
                    v_flex()
                        .min_w_0()
                        .child(
                            div()
                                .text_size(text::SM.size)
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(if bookmarked {
                                    "Remove bookmark"
                                } else {
                                    "Bookmark this page"
                                }),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(text::LABEL.size)
                                .text_color(k.text(TextStep::T400))
                                .child(
                                    current
                                        .as_deref()
                                        .map(display_address)
                                        .unwrap_or_else(|| "Open a page to save it".into()),
                                ),
                        ),
                )
        };
        let rows: Vec<AnyElement> = self
            .settings
            .bookmarks
            .iter()
            .map(|b| {
                let on_screens = self.live.as_deref() == Some(b.url.as_str());
                let selected = current.as_deref() == Some(b.url.as_str());
                let (open, remove, url, id) =
                    (this.clone(), this.clone(), b.url.clone(), b.id.clone());
                h_flex()
                    .group("web-bookmark")
                    .rounded(radius::md())
                    .when(selected, |d| d.bg(k.surface_tertiary))
                    .when(!selected, |d| d.hover(|s| s.bg(k.surface_menu_hover)))
                    .child(
                        h_flex()
                            .id(SharedString::from(ids::bookmark(&b.id)))
                            .test_support()
                            .role(Role::Button)
                            .aria_label(b.name.clone())
                            .flex_1()
                            .min_w_0()
                            .gap_2()
                            .px_2()
                            .py_1p5()
                            .cursor_pointer()
                            .on_click(move |_, window, cx| {
                                let url = url.clone();
                                open.update(cx, |t, cx| t.open(&url, true, window, cx))
                            })
                            .child(
                                Icon::new(KairoIcon::Globe)
                                    .size_3p5()
                                    .text_color(k.text(TextStep::T500)),
                            )
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(text::SM.size)
                                            .child(b.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(text::LABEL.size)
                                            .text_color(k.text(TextStep::T500))
                                            .child(display_address(&b.url)),
                                    ),
                            )
                            .when(on_screens, |d| {
                                d.child(div().size_1p5().flex_none().rounded_full().bg(k.live))
                            }),
                    )
                    .child(
                        div()
                            .mr_1()
                            .invisible()
                            .group_hover("web-bookmark", |s| s.visible())
                            .child(
                                Button::new(SharedString::from(ids::remove(&b.id)))
                                    .cursor_pointer()
                                    .ghost()
                                    .xsmall()
                                    .icon(Icon::new(KairoIcon::X))
                                    .accessibility_label(format!("Remove {}", b.name))
                                    .on_click(move |_, _, cx| {
                                        let id = id.clone();
                                        remove.update(cx, |t, cx| t.remove_bookmark(&id, cx))
                                    }),
                            ),
                    )
                    .into_any_element()
            })
            .collect();
        v_flex()
            .flex_none()
            .w(px(
                kairo_ui_kit::components::page_sidebar::PageSidebar::width(None, cx),
            ))
            .h_full()
            .min_h_0()
            .bg(k.surface_secondary)
            .border_r_1()
            .border_color(k.surface_line)
            .child(div().flex_none().px_3().pt_3().child(top))
            .child(
                div()
                    .flex_none()
                    .px_4()
                    .pt_4()
                    .pb_1p5()
                    .text_size(text::LABEL.size)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(k.text(TextStep::T500))
                    .child("BOOKMARKS"),
            )
            .child(
                v_flex()
                    .id("web-bookmarks")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .gap_0p5()
                    .px_2()
                    .pb_2()
                    .when(rows.is_empty(), |d| {
                        d.child(
                            div()
                                .id(ids::NO_BOOKMARKS)
                                .test_support()
                                .px_2()
                                .py_1p5()
                                .text_size(text::XS.size)
                                .text_color(k.text(TextStep::T600))
                                .child("Pages you save show here"),
                        )
                    })
                    .children(rows),
            )
            .into_any_element()
    }

    fn render_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.entity();
        let nav = |id: &'static str,
                   icon: KairoIcon,
                   label: &'static str,
                   disabled: bool,
                   f: fn(&mut WebPage, &mut Window, &mut Context<WebPage>)| {
            let this = this.clone();
            Button::new(id)
                .cursor_pointer()
                .ghost()
                .small()
                .icon(Icon::new(icon))
                .accessibility_label(label)
                .tooltip(label)
                .disabled(disabled)
                .on_click(move |_, window, cx| this.update(cx, |t, cx| f(t, window, cx)))
        };
        h_flex()
            .flex_none()
            .gap_1()
            .px_4()
            .pt_3()
            .child(nav(
                ids::BACK,
                KairoIcon::ChevronLeft,
                "Back",
                !self.history.can_back(),
                |t, w, cx| t.step(-1, w, cx),
            ))
            .child(nav(
                ids::FORWARD,
                KairoIcon::ChevronRight,
                "Forward",
                !self.history.can_forward(),
                |t, w, cx| t.step(1, w, cx),
            ))
            .child(nav(
                ids::RELOAD,
                KairoIcon::RefreshCw,
                "Reload",
                self.current().is_none(),
                |t, w, cx| t.reload(w, cx),
            ))
            .child(
                div()
                    .id(ids::ADDRESS)
                    .test_support()
                    .ml_1()
                    .flex_1()
                    .min_w_0()
                    .child(
                        Input::new(&self.address)
                            .kairo_size()
                            .small()
                            .aria_label("Web address"),
                    ),
            )
            .into_any_element()
    }

    fn render_preview(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let area = self.area.get();
        let (aw, ah) = (f32::from(area.size.width), f32::from(area.size.height));
        let exact = self.adaptive == Some(true);
        let (bw, bh) = if exact { exact_box((aw, ah)) } else { (aw, ah) };
        if bw > 0.0 {
            self.fit_zoom(bw, cx);
        }
        let zoom = self.zoom;
        let cell = self.area.clone();
        let body: AnyElement = match &self.preview {
            Preview::Unavailable(_) => v_flex()
                .id(ids::UNAVAILABLE)
                .test_support()
                .size_full()
                .items_center()
                .justify_center()
                .gap_2()
                .px_6()
                .text_center()
                .text_color(k.text(TextStep::T500))
                .child(Icon::new(KairoIcon::Globe).size_8())
                .child(
                    div()
                        .text_size(text::SM.size)
                        .child("Web pages can't be shown here on this computer."),
                )
                .child(
                    div()
                        .text_size(text::XS.size)
                        .child("The system web view did not start. Pages can still go on screens."),
                )
                .into_any_element(),
            Preview::None => v_flex()
                .id(ids::EMPTY)
                .test_support()
                .size_full()
                .items_center()
                .justify_center()
                .gap_2()
                .text_color(k.text(TextStep::T600))
                .child(Icon::new(KairoIcon::Globe).size_8())
                .child(
                    div()
                        .text_size(text::SM.size)
                        .child("Type a web address or search above"),
                )
                .into_any_element(),
            Preview::Ready(page) => {
                let cut = if exact { None } else { cut_line(bh, zoom) };
                div()
                    .id(ids::PREVIEW)
                    .test_support()
                    .relative()
                    .size_full()
                    .overflow_hidden()
                    .bg(k.white)
                    .when(self.active, |d| d.child(page.element()))
                    .children(cut.map(|top| {
                        div()
                            .id(ids::CUT)
                            .test_support()
                            .absolute()
                            .left_0()
                            .right_0()
                            .top(px(top))
                            .border_t_2()
                            .border_color(k.live)
                            .child(
                                div()
                                    .absolute()
                                    .right_2()
                                    .top_1p5()
                                    .rounded(radius::sm())
                                    .bg(k.live)
                                    .px_1p5()
                                    .py_0p5()
                                    .text_size(text::MICRO.size)
                                    .text_color(k.white)
                                    .child("Screens show down to here"),
                            )
                    }))
                    .into_any_element()
            }
        };
        let live = self.is_live();
        v_flex()
            .flex_1()
            .min_h_0()
            .gap_2()
            .p_4()
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .flex()
                    .justify_center()
                    .child(
                        canvas(move |b, _, _| cell.set(b), |_, _, _, _| {})
                            .absolute()
                            .inset_0(),
                    )
                    .child(
                        div()
                            .relative()
                            .flex_none()
                            .w(px(bw))
                            .h(px(bh))
                            .overflow_hidden()
                            .rounded(radius::lg())
                            .bg(k.surface_secondary)
                            .when(live, |d| d.border_2().border_color(k.live))
                            .child(body),
                    ),
            )
            .when(exact, |d| {
                d.child(
                    div()
                        .id(ids::EXACT_NOTE)
                        .test_support()
                        .flex_none()
                        .text_size(text::LABEL.size)
                        .text_color(k.text(TextStep::T500))
                        .child("This page arranges itself to fit the window, so the preview shows exactly the screens."),
                )
            })
            .into_any_element()
    }

    fn render_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let this = cx.entity();
        let status: AnyElement = match &self.live {
            Some(url) => h_flex()
                .gap_1p5()
                .child(div().size_1p5().rounded_full().bg(k.live))
                .child(
                    div()
                        .text_color(k.text(TextStep::T200))
                        .child("On screens:"),
                )
                .child(div().truncate().child(display_address(url)))
                .into_any_element(),
            None => div()
                .child("Nothing from the web on screens")
                .into_any_element(),
        };
        let live = self.live.is_some();
        let is_live = self.is_live();
        let (f, p, r, d, s) = (
            this.clone(),
            this.clone(),
            this.clone(),
            this.clone(),
            this.clone(),
        );
        h_flex()
            .flex_none()
            .justify_between()
            .gap_3()
            .px_4()
            .pb_3()
            .child(
                div()
                    .id(ids::STATUS)
                    .test_support()
                    .min_w_0()
                    .truncate()
                    .text_size(text::XS.size)
                    .text_color(k.text(TextStep::T400))
                    .child(status),
            )
            .child(
                h_flex()
                    .flex_none()
                    .gap_2()
                    .text_size(text::XS.size)
                    .text_color(k.text(TextStep::T400))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Switch::new(ids::FOLLOW)
                                    .small()
                                    .checked(self.follow)
                                    .accessibility_label("Screens follow links and scrolling")
                                    .on_change(move |on, _, cx| {
                                        let on = *on;
                                        f.update(cx, |t, cx| t.set_follow(on, cx))
                                    }),
                            )
                            .child("Screens follow"),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Switch::new(ids::POINTER)
                                    .small()
                                    .checked(self.settings.show_pointer)
                                    .accessibility_label("Show pointer on screens")
                                    .on_change(move |on, _, cx| {
                                        let on = *on;
                                        p.update(cx, |t, cx| t.set_show_pointer(on, cx))
                                    }),
                            )
                            .child(Icon::new(KairoIcon::MousePointer).size_3p5())
                            .child("Pointer"),
                    )
                    .when(live, |row| {
                        row.child(
                            Button::new(ids::RELOAD_SCREENS)
                                .cursor_pointer()
                                .small()
                                .label("Reload screens")
                                .on_click(move |_, _, cx| r.update(cx, |t, _| t.reload_screens())),
                        )
                        .child(
                            Button::new(ids::TAKE_DOWN)
                                .cursor_pointer()
                                .small()
                                .when(is_live, |b| b.danger())
                                .label("Take down")
                                .on_click(move |_, _, cx| d.update(cx, |t, cx| t.take_down(cx))),
                        )
                    })
                    .when(!is_live, |row| {
                        row.child(
                            Button::new(ids::SHOW)
                                .cursor_pointer()
                                .small()
                                .primary()
                                .label("Show on screens")
                                .disabled(self.current().is_none())
                                .on_click(move |_, _, cx| {
                                    s.update(cx, |t, cx| t.show_on_screens(cx))
                                }),
                        )
                    }),
            )
            .into_any_element()
    }
}

impl Render for WebPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = *cx.kairo();
        let bookmarks = self.render_bookmarks(cx);
        let bar = self.render_bar(cx);
        let preview = self.render_preview(cx);
        let controls = self.render_controls(cx);
        let page = h_flex()
            .flex_1()
            .w_full()
            .min_h_0()
            .child(bookmarks)
            .child(kairo_ui_kit::components::page_sidebar::handle(
                "web-sidebar-handle",
                None,
                cx,
            ))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(bar)
                    .children(self.error.clone().map(|e| {
                        div()
                            .id(ids::ERROR)
                            .test_support()
                            .role(Role::Alert)
                            .flex_none()
                            .px_5()
                            .pt_2()
                            .text_size(text::XS.size)
                            .text_color(k.danger_text)
                            .child(e)
                    }))
                    .child(preview)
                    .child(controls),
            );
        h_flex()
            .id(ids::ROOT)
            .test_support()
            .track_focus(&self.focus)
            .size_full()
            .min_h_0()
            .bg(k.surface)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(page)
                    .children(self.media_dock.clone()),
            )
            .children(self.rail.clone())
    }
}

// ─── The real preview ──────────────────────────────────────────────────────

/// A `gpui-webview` preview with the scroll, pointer and mute scripts.
pub struct NativePreview {
    view: Entity<gpui_webview::WebView>,
}

impl NativePreview {
    /// Builds the preview, muted, refusing new windows and non-web addresses.
    pub fn factory() -> PreviewFactory {
        Rc::new(|sink: PreviewSink, window: &mut Window, cx: &mut App| {
            let (tx, mut rx) = futures_channel::mpsc::unbounded::<PreviewEvent>();
            let (ipc, load) = (tx.clone(), tx);
            let builder = wry::WebViewBuilder::new()
                .with_initialization_script(MUTE_SCRIPT)
                .with_initialization_script(SCROLL_SCRIPT)
                .with_url("about:blank")
                .with_devtools(false)
                .with_navigation_handler(|url| preview_may_load(&url))
                .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
                .with_download_started_handler(|_, _| false)
                .with_ipc_handler(move |request| {
                    if let Some(e) = parse_message(request.body()) {
                        let _ = ipc.unbounded_send(e);
                    }
                })
                .with_on_page_load_handler(move |event, url| {
                    if matches!(event, wry::PageLoadEvent::Finished) && url != "about:blank" {
                        let _ = load.unbounded_send(PreviewEvent::Navigated(url));
                    }
                });
            let webview =
                gpui_webview::WebView::build(builder, window, cx).map_err(|e| e.to_string())?;
            let view = cx.new(|_| webview);
            // Wry's handlers run on the platform thread; hand events to the page.
            cx.spawn(async move |_| {
                use futures_util::StreamExt;
                while let Some(e) = rx.next().await {
                    sink(e);
                }
            })
            .detach();
            Ok(Box::new(NativePreview { view }) as Box<dyn PreviewPage>)
        })
    }
}

impl PreviewPage for NativePreview {
    fn load(&mut self, url: &str, cx: &mut App) {
        self.view.update(cx, |v, _| v.load_url(url));
    }
    fn set_zoom(&mut self, zoom: f64, cx: &mut App) {
        if let Err(error) = self.view.read(cx).set_zoom(zoom) {
            tracing::warn!(%error, "[Web] preview zoom failed");
        }
    }
    fn set_visible(&mut self, visible: bool, cx: &mut App) {
        if let Err(error) = self.view.read(cx).set_visible(visible) {
            tracing::warn!(%error, "[Web] preview visibility failed");
        }
    }
    fn element(&self) -> AnyElement {
        self.view.clone().into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_pushes_replaces_and_steps() {
        let mut h = History::starting_at(None);
        assert_eq!(h.current(), None);
        assert!(!h.can_back());
        h.push("a");
        h.push("b");
        h.push("c");
        assert_eq!(h.step(-1).as_deref(), Some("b"));
        assert!(h.can_forward());
        h.push("d");
        assert_eq!(
            h.entries,
            ["a", "b", "d"],
            "going somewhere new drops what was ahead"
        );
        h.replace("e");
        assert_eq!(h.current(), Some("e"));
        assert_eq!(h.step(5), None);
        assert_eq!(h.current(), Some("e"));
        let mut empty = History::starting_at(None);
        empty.replace("x");
        assert_eq!(empty.current(), Some("x"));
    }

    #[test]
    fn the_preview_lays_out_at_1920_under_any_scale() {
        for width in [480.0_f32, 960.0, 1234.5, 1920.0, 2400.0] {
            let zoom = zoom_for(width);
            // The box's right edge is the page's 1920th pixel.
            let (x, _) = page_point((width, 0.0), zoom).unwrap();
            assert!((x - 1920.0).abs() < 1e-6, "width {width}");
            // A point in the box maps to the same share of the page.
            let (x, y) = page_point((width / 4.0, 90.0), zoom).unwrap();
            assert!((x - 480.0).abs() < 1e-6);
            assert!((y - 90.0 / zoom).abs() < 1e-9);
        }
        assert_eq!(zoom_for(0.0), 0.0);
        assert_eq!(page_point((10.0, 10.0), 0.0), None);
    }

    #[test]
    fn the_cut_line_marks_where_screens_end() {
        let zoom = zoom_for(960.0); // 0.5
        assert_eq!(fill_height(700.0, zoom), 1400.0);
        assert_eq!(cut_line(700.0, zoom), Some(540.0));
        assert_eq!(
            cut_line(500.0, zoom),
            None,
            "no line while the box is shorter than the screens"
        );
        assert_eq!(fill_height(700.0, 0.0), 1080.0);
        assert_eq!(exact_box((1600.0, 450.0)), (800.0, 450.0));
        assert_eq!(exact_box((800.0, 900.0)), (800.0, 450.0));
    }

    #[test]
    fn the_pointer_shows_only_on_the_live_page_inside_the_frame() {
        let on = |p| pointer_shows(true, true, Some("u"), Some("u"), p);
        assert_eq!(on(Some((10.0, 20.0))), Some((10.0, 20.0)));
        assert_eq!(on(Some((1920.0, 1080.0))), Some((1920.0, 1080.0)));
        assert_eq!(on(Some((1921.0, 10.0))), None);
        assert_eq!(on(Some((10.0, -1.0))), None);
        assert_eq!(on(None), None);
        assert_eq!(
            pointer_shows(false, true, Some("u"), Some("u"), Some((1.0, 1.0))),
            None
        );
        assert_eq!(
            pointer_shows(true, false, Some("u"), Some("u"), Some((1.0, 1.0))),
            None
        );
        assert_eq!(
            pointer_shows(true, true, Some("u"), Some("v"), Some((1.0, 1.0))),
            None
        );
        assert_eq!(
            pointer_shows(true, true, None, Some("u"), Some((1.0, 1.0))),
            None
        );
    }

    #[test]
    fn script_messages_and_the_navigation_rule() {
        assert_eq!(
            parse_message(r#"{"type":"kairo:web-scroll","x":0,"y":420.5,"adaptive":true}"#),
            Some(PreviewEvent::Scroll {
                x: 0.0,
                y: 420.5,
                adaptive: true
            })
        );
        assert_eq!(
            parse_message(r#"{"type":"kairo:web-pointer","x":null,"y":null}"#),
            Some(PreviewEvent::Pointer(None))
        );
        assert_eq!(
            parse_message(r#"{"type":"kairo:web-pointer","x":5,"y":6}"#),
            Some(PreviewEvent::Pointer(Some((5.0, 6.0))))
        );
        assert_eq!(
            parse_message(r#"{"type":"kairo:web-scroll","x":"a"}"#),
            None
        );
        assert_eq!(parse_message("not json"), None);
        assert_eq!(parse_message(r#"{"type":"other"}"#), None);
        assert!(preview_may_load("https://example.com/"));
        assert!(!preview_may_load("file:///etc/passwd"));
        assert!(!preview_may_load("javascript:alert(1)"));
    }
}
