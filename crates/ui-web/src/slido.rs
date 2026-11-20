//! The Slido window: a separate GPUI window holding one `gpui-webview`.
//!
//! Policy lives in `kairo-audience` ([`allowed_slido_frame_navigation`] and
//! [`allowed_slido_window`]); this file applies it to a real web view.
//! What differs from Electron is listed in DECISIONS.md, 047.

use crate::{permissions, platform};
use futures_channel::{mpsc, oneshot};
use futures_util::StreamExt;
use gpui_kit::*;
use gpui_webview::WebView;
use kairo_audience::{
    AudienceError, BoxFuture, SlidoEventSink, SlidoWindow, SlidoWindowConfig, SlidoWindowEvent,
    SlidoWindowFactory, allowed_slido_frame_navigation, allowed_slido_window,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use wry::{NewWindowResponse, PageLoadEvent, WebContext, WebViewBuilder};

/// How often the live address is copied for the source thread. Wry reports
/// no in-page (history API) navigations, and the source needs the address.
const URL_POLL: Duration = Duration::from_millis(250);

type Allow = Arc<dyn Fn(&str) -> bool + Send + Sync>;

pub struct HostOptions {
    /// Folder for the persisted profile (`Partitions/audience-slido`).
    pub profile_dir: PathBuf,
    /// Decides which addresses may load. Production uses the Slido allowlist;
    /// only tests replace it, to serve fixtures from the loopback address.
    pub navigation: Option<Allow>,
    /// Run the permission-denying script. Only the native-denial test turns it off.
    pub permission_script: bool,
}

impl HostOptions {
    pub fn new(profile_dir: PathBuf) -> Self {
        Self {
            profile_dir,
            navigation: None,
            permission_script: true,
        }
    }
}

struct Shared {
    url: Mutex<String>,
    visible: AtomicBool,
}

enum Command {
    Open {
        id: u64,
        config: SlidoWindowConfig,
        events: SlidoEventSink,
        shared: Arc<Shared>,
        reply: oneshot::Sender<Result<(), String>>,
    },
    Load(u64, String),
    Show(u64),
    Hide(u64),
    Focus(u64),
    Destroy(u64),
}

/// Opens Slido windows on the GPUI thread.
#[derive(Clone)]
pub struct SlidoWebHost {
    commands: mpsc::UnboundedSender<Command>,
    next_id: Arc<AtomicU64>,
}

impl SlidoWebHost {
    /// Starts the host on the GPUI thread. Call once, from `Application::run`.
    pub fn install(cx: &mut App, options: HostOptions) -> Self {
        let (commands, mut inbox) = mpsc::unbounded();
        let options = Arc::new(options);
        cx.spawn(async move |cx| {
            while let Some(command) = inbox.next().await {
                let options = options.clone();
                cx.update(|cx| handle(command, &options, cx));
            }
        })
        .detach();
        Self {
            commands,
            next_id: Arc::new(AtomicU64::new(1)),
        }
    }
}

impl SlidoWindowFactory for SlidoWebHost {
    fn open(
        &self,
        config: SlidoWindowConfig,
        events: SlidoEventSink,
    ) -> BoxFuture<'_, kairo_audience::Result<Arc<dyn SlidoWindow>>> {
        Box::pin(async move {
            let id = self.next_id.fetch_add(1, Ordering::SeqCst);
            let shared = Arc::new(Shared {
                url: Mutex::new(config.start_url.clone()),
                visible: AtomicBool::new(false),
            });
            let (reply, done) = oneshot::channel();
            self.commands
                .unbounded_send(Command::Open {
                    id,
                    config,
                    events,
                    shared: shared.clone(),
                    reply,
                })
                .map_err(|_| AudienceError::new("The Slido window could not be opened."))?;
            match done.await {
                Ok(Ok(())) => {}
                Ok(Err(message)) => {
                    tracing::warn!(%message, "[Slido] window failed to open");
                    return Err(AudienceError::new("The Slido window could not be opened."));
                }
                Err(_) => return Err(AudienceError::new("The Slido window could not be opened.")),
            }
            let window: Arc<dyn SlidoWindow> = Arc::new(Window_ {
                id,
                commands: self.commands.clone(),
                shared,
            });
            Ok(window)
        })
    }
}

/// The source-side handle to one window.
struct Window_ {
    id: u64,
    commands: mpsc::UnboundedSender<Command>,
    shared: Arc<Shared>,
}

impl Window_ {
    fn send(&self, command: Command) {
        let _ = self.commands.unbounded_send(command);
    }
}

impl SlidoWindow for Window_ {
    fn load_url(&self, url: &str) {
        self.send(Command::Load(self.id, url.to_owned()));
    }
    fn current_url(&self) -> String {
        self.shared
            .url
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    fn show(&self) {
        // Visible at once, so a read in the same moment sees it.
        self.shared.visible.store(true, Ordering::SeqCst);
        self.send(Command::Show(self.id));
    }
    fn hide(&self) {
        self.shared.visible.store(false, Ordering::SeqCst);
        self.send(Command::Hide(self.id));
    }
    fn focus(&self) {
        self.send(Command::Focus(self.id));
    }
    fn is_visible(&self) -> bool {
        self.shared.visible.load(Ordering::SeqCst)
    }
    fn destroy(&self) {
        self.shared.visible.store(false, Ordering::SeqCst);
        self.send(Command::Destroy(self.id));
    }
}

/// The window's only content.
struct WebPage {
    view: Option<Entity<WebView>>,
}

impl Render for WebPage {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().children(self.view.clone())
    }
}

/// What the window's build closure hands back to `open`.
type Built = std::rc::Rc<std::cell::RefCell<Option<Result<(Entity<WebView>, WebContext), String>>>>;

struct Entry {
    handle: WindowHandle<WebPage>,
    view: Entity<WebView>,
    /// Must outlive the web view (Wry).
    _context: WebContext,
}

#[derive(Default)]
struct Windows(HashMap<u64, Entry>);

impl Global for Windows {}

fn handle(command: Command, options: &Arc<HostOptions>, cx: &mut App) {
    match command {
        Command::Open {
            id,
            config,
            events,
            shared,
            reply,
        } => {
            let result = open(id, &config, events, shared, options, cx);
            let _ = reply.send(result);
        }
        Command::Load(id, url) => {
            if let Some(entry) = cx.default_global::<Windows>().0.get(&id) {
                let view = entry.view.clone();
                view.update(cx, |view, _| view.load_url(&url));
            }
        }
        Command::Show(id) => with_window(id, cx, platform::show),
        Command::Hide(id) => with_window(id, cx, platform::hide),
        Command::Focus(id) => with_window(id, cx, |window| window.activate_window()),
        Command::Destroy(id) => {
            if let Some(entry) = cx.default_global::<Windows>().0.remove(&id) {
                let _ = entry
                    .handle
                    .update(cx, |_, window, _| window.remove_window());
            }
        }
    }
}

fn with_window(id: u64, cx: &mut App, f: impl FnOnce(&mut Window)) {
    if let Some(entry) = cx.default_global::<Windows>().0.get(&id) {
        let handle = entry.handle;
        let _ = handle.update(cx, |_, window, _| f(window));
    }
}

fn open(
    id: u64,
    config: &SlidoWindowConfig,
    events: SlidoEventSink,
    shared: Arc<Shared>,
    options: &Arc<HostOptions>,
    cx: &mut App,
) -> Result<(), String> {
    let (width, height) = (config.width as f32, config.height as f32);
    let window_options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(width), px(height)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some(config.title.clone().into()),
            ..Default::default()
        }),
        // Created hidden: a participant source never shows it.
        show: false,
        focus: false,
        ..Default::default()
    };
    let built: Built = Default::default();
    let slot = built.clone();
    let config_copy = config.clone();
    let options_copy = options.clone();
    let handle = cx
        .open_window(window_options, move |window, cx| {
            let result = build(&config_copy, events, &options_copy, window, cx);
            let view = result.as_ref().ok().map(|(view, _)| view.clone());
            *slot.borrow_mut() = Some(result);
            cx.new(|_| WebPage { view })
        })
        .map_err(|error| error.to_string())?;
    let (view, context) = match built.borrow_mut().take() {
        Some(Ok(built)) => built,
        Some(Err(error)) => {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
            return Err(error);
        }
        None => return Err("The window was not built.".into()),
    };
    // A window that is never shown is never painted, so GPUI would never size
    // its page. Give it the window's size once, for the page's layout.
    view.update(cx, |view, _| {
        let _ = view.set_bounds(wry::Rect {
            position: wry::dpi::LogicalPosition::new(0.0, 0.0).into(),
            size: wry::dpi::LogicalSize::new(width as f64, height as f64).into(),
        });
    });
    // Closing the window hides it; the source keeps reading.
    let closing = shared.clone();
    let _ = handle.update(cx, |_, window, cx| {
        window.on_window_should_close(cx, move |window, _| {
            closing.visible.store(false, Ordering::SeqCst);
            platform::hide(window);
            false
        });
    });
    // Keep the live address (history API changes included) for the source.
    let poll = shared.clone();
    let watched = view.clone();
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(URL_POLL).await;
            let alive = cx.update(|cx| {
                if !cx.default_global::<Windows>().0.contains_key(&id) {
                    return false;
                }
                if let Ok(url) = watched.read(cx).url() {
                    *poll.url.lock().unwrap_or_else(|e| e.into_inner()) = url;
                }
                true
            });
            if !alive {
                break;
            }
        }
    })
    .detach();
    cx.default_global::<Windows>().0.insert(
        id,
        Entry {
            handle,
            view,
            _context: context,
        },
    );
    Ok(())
}

fn build(
    config: &SlidoWindowConfig,
    events: SlidoEventSink,
    options: &Arc<HostOptions>,
    window: &mut Window,
    cx: &mut App,
) -> Result<(Entity<WebView>, WebContext), String> {
    std::fs::create_dir_all(&options.profile_dir).map_err(|error| error.to_string())?;
    let mut context = WebContext::new(Some(options.profile_dir.clone()));
    let allow: Allow = options
        .navigation
        .clone()
        .unwrap_or_else(|| Arc::new(allowed_slido_frame_navigation));
    let moderator = config.moderator;
    let page_events = events.clone();
    let load_events = events;
    let mut builder = WebViewBuilder::new_with_web_context(&mut context)
        .with_initialization_script(config.script.clone())
        .with_url(config.start_url.clone())
        .with_devtools(false)
        .with_navigation_handler(move |url| {
            let allowed = allow(&url);
            if !allowed {
                tracing::warn!(%url, "Slido window refused a navigation");
            }
            allowed
        })
        .with_new_window_req_handler(move |url, _| {
            if allowed_slido_window(&url, moderator) {
                NewWindowResponse::Allow
            } else {
                NewWindowResponse::Deny
            }
        })
        .with_download_started_handler(|_, _| false)
        .with_ipc_handler(move |request| {
            page_events(SlidoWindowEvent::Message {
                frame_url: request.uri().to_string(),
                body: request.into_body(),
            });
        })
        .with_on_page_load_handler(move |event, _| {
            if matches!(event, PageLoadEvent::Finished) {
                load_events(SlidoWindowEvent::Loaded);
            }
        })
        .with_background_throttling(wry::BackgroundThrottlingPolicy::Disabled);
    if options.permission_script {
        builder = builder.with_initialization_script_for_main_only(permissions::GUARD_JS, false);
    }
    #[cfg(target_os = "macos")]
    {
        use wry::WebViewBuilderExtDarwin;
        // macOS 14 and later: a store of its own. Earlier versions use the shared default.
        let id = uuid::Uuid::new_v5(
            &uuid::Uuid::NAMESPACE_URL,
            options.profile_dir.to_string_lossy().as_bytes(),
        );
        builder = builder.with_data_store_identifier(*id.as_bytes());
    }
    let webview = WebView::build(builder, window, cx).map_err(|error| error.to_string())?;
    if !permissions::deny_media_capture(webview.raw()) {
        tracing::warn!("[Slido] native media permission denial is not available on this platform");
    }
    Ok((cx.new(|_| webview), context))
}
