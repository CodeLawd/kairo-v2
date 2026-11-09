//! Spike 006: one CEF host for the program page, a sign-in window and PDF output.
mod policy;

use cef::{
    application_mac::{CefAppProtocol, CrAppControlProtocol, CrAppProtocol},
    args::Args,
    library_loader::LibraryLoader,
    *,
};
use objc2::{
    ClassType, DefinedClass, MainThreadMarker, define_class, msg_send, rc::Retained, runtime::*,
};
use objc2_app_kit::{NSApp, NSApplication};
use objc2_foundation::NSObject;
use std::{
    cell::Cell,
    collections::HashMap,
    fs::OpenOptions,
    os::{fd::AsRawFd, raw::c_int},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

const WIDTH: i32 = 1920;
const HEIGHT: i32 = 1080;
const HEADER: usize = 32;

define_class!(
    #[unsafe(super(NSApplication))]
    #[name = "KairoSpikeApplication"]
    #[ivars = Cell<bool>]
    struct SpikeApplication;

    unsafe impl CrAppControlProtocol for SpikeApplication {
        #[unsafe(method(setHandlingSendEvent:))]
        unsafe fn set_handling_send_event(&self, value: Bool) {
            self.ivars().set(value.as_bool());
        }
    }
    unsafe impl CrAppProtocol for SpikeApplication {
        #[unsafe(method(isHandlingSendEvent))]
        unsafe fn is_handling_send_event(&self) -> Bool {
            Bool::new(self.ivars().get())
        }
    }
    unsafe impl CefAppProtocol for SpikeApplication {}

    impl SpikeApplication {
        // CEF ends its message loop itself. AppKit's terminate would exit the
        // process before CefShutdown, so it must not run.
        #[unsafe(method(terminate:))]
        unsafe fn terminate(&self, _sender: Option<&AnyObject>) {}
    }
);

#[derive(Clone, PartialEq)]
enum Mode {
    Offscreen,
    Window,
    Pdf,
}

struct Options {
    mode: Mode,
    url: String,
    shm: Option<String>,
    seconds: f64,
    script: bool,
    accelerated: bool,
    sandbox: bool,
    profile: Option<String>,
    init_script: Option<String>,
    extra_host: Option<String>,
    out: Option<String>,
    page_height: f64,
}

struct Shm(*mut u8);
struct State {
    options: Options,
    started: Instant,
    browser: Mutex<Option<Browser>>,
    shm: Option<Shm>,
    paints: AtomicU64,
    accelerated: AtomicU64,
    audio_packets: AtomicU64,
    audio_frames: AtomicU64,
    crashed: AtomicU64,
    step: AtomicU64,
}
// CEF calls these handlers on its own threads; browser access is UI-thread only.
unsafe impl Send for State {}
unsafe impl Sync for State {}

fn say(state: &State, text: impl AsRef<str>) {
    println!(
        "[{:9.3}] {}",
        state.started.elapsed().as_secs_f64(),
        text.as_ref()
    );
}

fn text(value: Option<&CefString>) -> String {
    value.map(|v| v.to_string()).unwrap_or_default()
}

unsafe extern "C" {
    fn clock_gettime_nsec_np(clock: u32) -> u64;
}

/// CLOCK_UPTIME_RAW, comparable across processes.
fn now_ns() -> u64 {
    unsafe { clock_gettime_nsec_np(8) }
}

fn rss_kib() -> u64 {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    usage.ru_maxrss as u64 / 1024
}

fn publish(state: &State, buffer: *const u8) {
    let Some(Shm(map)) = state.shm else { return };
    let seq = unsafe { &*(map as *const AtomicU64) };
    let stamp = unsafe { &*(map.add(16) as *const AtomicU64) };
    let next = seq.load(Ordering::Relaxed) + 1;
    seq.store(next, Ordering::Release);
    stamp.store(now_ns(), Ordering::Relaxed);
    unsafe {
        std::ptr::copy_nonoverlapping(buffer, map.add(HEADER), (WIDTH * HEIGHT * 4) as usize)
    };
    seq.store(next + 1, Ordering::Release);
}

wrap_render_handler! {
    struct Render { state: Arc<State> }
    impl RenderHandler {
        fn view_rect(&self, _browser: Option<&mut Browser>, rect: Option<&mut Rect>) {
            if let Some(rect) = rect {
                *rect = Rect { x: 0, y: 0, width: WIDTH, height: HEIGHT };
            }
        }
        fn on_paint(&self, _browser: Option<&mut Browser>, kind: PaintElementType,
            _dirty: Option<&[Rect]>, buffer: *const u8, width: c_int, height: c_int) {
            if kind == PaintElementType::default() && width == WIDTH && height == HEIGHT {
                self.state.paints.fetch_add(1, Ordering::Relaxed);
                publish(&self.state, buffer);
            }
        }
        fn on_accelerated_paint(&self, _browser: Option<&mut Browser>, kind: PaintElementType,
            _dirty: Option<&[Rect]>, info: Option<&AcceleratedPaintInfo>) {
            if kind == PaintElementType::default() && info.is_some_and(|i| !i.shared_texture_io_surface.is_null()) {
                self.state.accelerated.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

wrap_audio_handler! {
    struct Audio { state: Arc<State> }
    impl AudioHandler {
        fn audio_parameters(&self, _browser: Option<&mut Browser>, params: Option<&mut AudioParameters>) -> c_int {
            if let Some(params) = params {
                params.channel_layout = ChannelLayout::from(sys::cef_channel_layout_t::CEF_CHANNEL_LAYOUT_STEREO);
                params.sample_rate = 48_000;
                params.frames_per_buffer = 1024;
            }
            1
        }
        fn on_audio_stream_started(&self, _browser: Option<&mut Browser>, params: Option<&AudioParameters>, channels: c_int) {
            say(&self.state, format!("AUDIO started rate={} channels={channels}", params.map_or(0, |p| p.sample_rate)));
        }
        fn on_audio_stream_packet(&self, _browser: Option<&mut Browser>, _data: *mut *const f32, frames: c_int, _pts: i64) {
            self.state.audio_packets.fetch_add(1, Ordering::Relaxed);
            self.state.audio_frames.fetch_add(frames as u64, Ordering::Relaxed);
        }
        fn on_audio_stream_stopped(&self, _browser: Option<&mut Browser>) {
            say(&self.state, "AUDIO stopped");
        }
        fn on_audio_stream_error(&self, _browser: Option<&mut Browser>, message: Option<&CefString>) {
            say(&self.state, format!("AUDIO error {}", text(message)));
        }
    }
}

wrap_load_handler! {
    struct Load { state: Arc<State> }
    impl LoadHandler {
        fn on_load_end(&self, browser: Option<&mut Browser>, frame: Option<&mut Frame>, status: c_int) {
            let Some(frame) = frame else { return };
            let main = frame.is_main() == 1;
            say(&self.state, format!("LOAD end main={main} status={status} url={}", CefString::from(&frame.url())));
            if main && self.state.options.mode == Mode::Pdf && let Some(browser) = browser {
                print_pdf(self.state.clone(), browser);
            }
        }
        fn on_load_error(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, code: Errorcode,
            error: Option<&CefString>, url: Option<&CefString>) {
            let main = frame.is_some_and(|f| f.is_main() == 1);
            say(&self.state, format!("LOAD error main={main} code={} text={} url={}", sys::cef_errorcode_t::from(code) as i32, text(error), text(url)));
        }
    }
}

wrap_pdf_print_callback! {
    struct PdfDone { state: Arc<State> }
    impl PdfPrintCallback {
        fn on_pdf_print_finished(&self, path: Option<&CefString>, ok: c_int) {
            let path = text(path);
            let head = std::fs::read(&path).ok().map(|d| (d.len(), d.starts_with(b"%PDF-")));
            say(&self.state, format!("PDF finished ok={ok} path={path} bytes_and_magic={head:?}"));
            close(&self.state);
        }
    }
}

fn print_pdf(state: Arc<State>, browser: &mut Browser) {
    let height = state.options.page_height;
    let settings = PdfPrintSettings {
        print_background: 1,
        prefer_css_page_size: 1,
        landscape: (height <= 1920.0) as c_int,
        // Electron's 508000 micron page width is 20 inches. CEF takes inches.
        paper_width: 20.0,
        paper_height: 20.0 * height / 1920.0,
        margin_type: PdfPrintMarginType::from(
            sys::cef_pdf_print_margin_type_t::PDF_PRINT_MARGIN_NONE,
        ),
        ..Default::default()
    };
    let out = CefString::from(state.options.out.as_deref().unwrap_or("out.pdf"));
    let mut callback = PdfDone::new(state);
    browser
        .host()
        .expect("host")
        .print_to_pdf(Some(&out), Some(&settings), Some(&mut callback));
}

fn close(state: &State) {
    // Clone first: close_browser can run on_before_close synchronously, and that
    // takes this same lock.
    let browser = state.browser.lock().unwrap().clone();
    if let Some(browser) = browser {
        browser.host().expect("host").close_browser(1);
    }
}

wrap_display_handler! {
    struct Display { state: Arc<State> }
    impl DisplayHandler {
        fn on_address_change(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, url: Option<&CefString>) {
            if frame.is_some_and(|f| f.is_main() == 1) {
                say(&self.state, format!("NAV address={}", text(url)));
            }
        }
        fn on_console_message(&self, _browser: Option<&mut Browser>, _level: LogSeverity,
            message: Option<&CefString>, _source: Option<&CefString>, _line: c_int) -> c_int {
            say(&self.state, format!("PAGE {}", text(message)));
            1
        }
    }
}

wrap_life_span_handler! {
    struct Life { state: Arc<State> }
    impl LifeSpanHandler {
        fn on_before_popup(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, _id: c_int,
            url: Option<&CefString>, _name: Option<&CefString>, _disposition: WindowOpenDisposition, _gesture: c_int,
            _features: Option<&PopupFeatures>, _info: Option<&mut WindowInfo>, _client: Option<&mut Option<Client>>,
            _settings: Option<&mut BrowserSettings>, _extra: Option<&mut Option<DictionaryValue>>,
            _no_js: Option<&mut c_int>) -> c_int {
            let url = text(url);
            let allow = self.state.options.mode == Mode::Window
                && policy::allowed(&url, self.state.options.extra_host.as_deref());
            say(&self.state, format!("POPUP allow={allow} url={url}"));
            (!allow) as c_int
        }
        fn on_after_created(&self, browser: Option<&mut Browser>) {
            if let Some(browser) = browser {
                let mut slot = self.state.browser.lock().unwrap();
                if slot.is_none() { *slot = Some(browser.clone()); }
            }
        }
        fn on_before_close(&self, _browser: Option<&mut Browser>) {
            say(&self.state, format!(
                "CLOSE paints={} accelerated={} audio_packets={} audio_frames={} own_peak_rss_kib={}",
                self.state.paints.load(Ordering::Relaxed), self.state.accelerated.load(Ordering::Relaxed),
                self.state.audio_packets.load(Ordering::Relaxed), self.state.audio_frames.load(Ordering::Relaxed), rss_kib()));
            *self.state.browser.lock().unwrap() = None;
            let mut task = Quit::new();
            post_delayed_task(ThreadId::from(sys::cef_thread_id_t::TID_UI), Some(&mut task), 50);
        }
    }
}

wrap_request_handler! {
    struct Requests { state: Arc<State> }
    impl RequestHandler {
        fn on_before_browse(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, request: Option<&mut Request>,
            _gesture: c_int, redirect: c_int) -> c_int {
            if self.state.options.mode != Mode::Window { return 0; }
            let url = request.map(|r| CefString::from(&r.url()).to_string()).unwrap_or_default();
            if !frame.is_some_and(|f| f.is_main() == 1) { return 0; }
            let allow = policy::allowed(&url, self.state.options.extra_host.as_deref());
            say(&self.state, format!("BROWSE allow={allow} redirect={redirect} url={url}"));
            (!allow) as c_int
        }
        fn on_render_process_terminated(&self, _browser: Option<&mut Browser>, status: TerminationStatus, code: c_int, error: Option<&CefString>) {
            self.state.crashed.fetch_add(1, Ordering::Relaxed);
            say(&self.state, format!("CRASH status={} code={code} text={}", sys::cef_termination_status_t::from(status) as i32, text(error)));
        }
    }
}

wrap_permission_handler! {
    struct Permissions { state: Arc<State> }
    impl PermissionHandler {
        fn on_request_media_access_permission(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>,
            origin: Option<&CefString>, requested: u32, callback: Option<&mut MediaAccessCallback>) -> c_int {
            say(&self.state, format!("PERMISSION media denied origin={} mask={requested}", text(origin)));
            if let Some(callback) = callback { callback.cancel(); }
            1
        }
        fn on_show_permission_prompt(&self, _browser: Option<&mut Browser>, _id: u64, origin: Option<&CefString>,
            requested: u32, callback: Option<&mut PermissionPromptCallback>) -> c_int {
            say(&self.state, format!("PERMISSION prompt denied origin={} mask={requested}", text(origin)));
            if let Some(callback) = callback {
                callback.cont(PermissionRequestResult::from(sys::cef_permission_request_result_t::CEF_PERMISSION_RESULT_DENY));
            }
            1
        }
    }
}

wrap_client! {
    struct Handlers { state: Arc<State> }
    impl Client {
        fn render_handler(&self) -> Option<RenderHandler> {
            (self.state.options.mode != Mode::Window).then(|| Render::new(self.state.clone()))
        }
        fn audio_handler(&self) -> Option<AudioHandler> { Some(Audio::new(self.state.clone())) }
        fn load_handler(&self) -> Option<LoadHandler> { Some(Load::new(self.state.clone())) }
        fn display_handler(&self) -> Option<DisplayHandler> { Some(Display::new(self.state.clone())) }
        fn life_span_handler(&self) -> Option<LifeSpanHandler> { Some(Life::new(self.state.clone())) }
        fn request_handler(&self) -> Option<RequestHandler> { Some(Requests::new(self.state.clone())) }
        fn permission_handler(&self) -> Option<PermissionHandler> { Some(Permissions::new(self.state.clone())) }
    }
}

wrap_task! {
    struct Quit;
    impl Task {
        fn execute(&self) {
            quit_message_loop();
        }
    }
}

wrap_task! {
    struct Tick { state: Arc<State> }
    impl Task {
        fn execute(&self) {
            let state = &self.state;
            let elapsed = state.started.elapsed().as_secs_f64();
            if elapsed >= state.options.seconds && state.options.mode != Mode::Pdf {
                close(state);
                return;
            }
            let browser = state.browser.lock().unwrap().clone();
            if state.options.script && let Some(browser) = browser {
                let step = state.step.load(Ordering::Relaxed);
                let due = [4.0, 5.0, 6.0, 9.0, 12.0, 15.0];
                if due.get(step as usize).is_some_and(|d| elapsed >= *d) {
                    state.step.fetch_add(1, Ordering::Relaxed);
                    run_step(state, &browser, step);
                }
            }
            let mut task = Tick::new(state.clone());
            post_delayed_task(ThreadId::from(sys::cef_thread_id_t::TID_UI), Some(&mut task), 100);
        }
    }
}

fn run_step(state: &State, browser: &Browser, step: u64) {
    let host = browser.host().expect("host");
    let event = MouseEvent {
        x: 300,
        y: 300,
        modifiers: 0,
    };
    say(state, format!("STEP {step}"));
    match step {
        0 => {
            host.set_focus(1);
            host.send_mouse_move_event(Some(&event), 0);
        }
        1 => {
            let button = MouseButtonType::from(sys::cef_mouse_button_type_t::MBT_LEFT);
            host.send_mouse_click_event(Some(&event), button, 0, 1);
            host.send_mouse_click_event(Some(&event), button, 1, 1);
        }
        2 => host.send_mouse_wheel_event(Some(&event), 0, -400),
        3 => browser.reload(),
        4 => {
            let url = CefString::from("chrome://crash");
            browser.main_frame().expect("frame").load_url(Some(&url));
        }
        _ => {
            let url = CefString::from(state.options.url.as_str());
            browser.main_frame().expect("frame").load_url(Some(&url));
        }
    }
}

wrap_browser_process_handler! {
    struct Process { state: Arc<State> }
    impl BrowserProcessHandler {
        fn on_before_child_process_launch(&self, line: Option<&mut CommandLine>) {
            // The sandboxed renderer cannot read the script from disk.
            if let (Some(line), Some(script)) = (line, self.state.options.init_script.as_ref()) {
                line.append_switch_with_value(Some(&CefString::from("kairo-init")), Some(&CefString::from(script.as_str())));
            }
        }
        fn on_context_initialized(&self) {
            let state = self.state.clone();
            let offscreen = state.options.mode != Mode::Window;
            let info = if offscreen {
                WindowInfo {
                    windowless_rendering_enabled: 1,
                    shared_texture_enabled: state.options.accelerated as c_int,
                    runtime_style: RuntimeStyle::from(sys::cef_runtime_style_t::CEF_RUNTIME_STYLE_ALLOY),
                    ..Default::default()
                }
            } else {
                WindowInfo {
                    window_name: CefString::from("Sign in to Slido"),
                    bounds: Rect { x: 100, y: 100, width: 1100, height: 800 },
                    ..Default::default()
                }
            };
            let settings = BrowserSettings { windowless_frame_rate: 30, ..Default::default() };
            let mut client = Handlers::new(state.clone());
            let url = CefString::from(state.options.url.as_str());
            let created = browser_host_create_browser(Some(&info), Some(&mut client), Some(&url), Some(&settings), None, None);
            say(&state, format!("CREATE ok={created}"));
            let mut task = Tick::new(state);
            post_delayed_task(ThreadId::from(sys::cef_thread_id_t::TID_UI), Some(&mut task), 100);
        }
    }
}

wrap_app! {
    struct Application { state: Arc<State> }
    impl App {
        fn on_before_command_line_processing(&self, _kind: Option<&CefString>, line: Option<&mut CommandLine>) {
            if let Some(line) = line {
                line.append_switch_with_value(Some(&CefString::from("autoplay-policy")), Some(&CefString::from("no-user-gesture-required")));
                line.append_switch(Some(&CefString::from("use-mock-keychain")));
            }
        }
        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> { Some(Process::new(self.state.clone())) }
    }
}

fn parse() -> Options {
    let mut flags: HashMap<String, String> = HashMap::new();
    let mut args = std::env::args().skip(1).peekable();
    while let Some(arg) = args.next() {
        if let Some(name) = arg.strip_prefix("--k-") {
            let value = if args.peek().is_some_and(|v| !v.starts_with("--")) {
                args.next().unwrap()
            } else {
                String::new()
            };
            flags.insert(name.to_owned(), value);
        }
    }
    let mode = match flags.get("mode").map(String::as_str) {
        Some("window") => Mode::Window,
        Some("pdf") => Mode::Pdf,
        _ => Mode::Offscreen,
    };
    Options {
        mode,
        url: flags
            .get("url")
            .cloned()
            .unwrap_or_else(|| "about:blank".into()),
        shm: flags.get("shm").cloned(),
        seconds: flags
            .get("seconds")
            .and_then(|v| v.parse().ok())
            .unwrap_or(20.0),
        script: flags.contains_key("script"),
        accelerated: flags.contains_key("accelerated"),
        sandbox: flags.contains_key("sandbox"),
        profile: flags.get("profile").cloned(),
        init_script: flags
            .get("init")
            .map(|p| std::fs::read_to_string(p).expect("init script")),
        extra_host: flags.get("allow-host").cloned(),
        out: flags.get("out").cloned(),
        page_height: flags
            .get("page-height")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1080.0),
    }
}

fn map_shm(path: &str) -> Shm {
    let size = HEADER + (WIDTH * HEIGHT * 4) as usize;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .expect("shared memory file");
    file.set_len(size as u64).expect("size shared memory");
    let map = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            size,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            file.as_raw_fd(),
            0,
        )
    };
    assert_ne!(map, libc::MAP_FAILED);
    Shm(map.cast())
}

fn main() {
    let args = Args::new();
    let exe = std::env::current_exe().expect("current executable");
    let loader = LibraryLoader::new(&exe, false);
    assert!(loader.load(), "could not load the CEF framework");
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);

    let mtm = MainThreadMarker::new().expect("main thread");
    let _application: Retained<NSObject> = unsafe {
        let this: Retained<SpikeApplication> =
            msg_send![SpikeApplication::class(), sharedApplication];
        Retained::cast_unchecked(this)
    };
    let _ = NSApp(mtm);

    let options = parse();
    let shm = options.shm.as_deref().map(map_shm);
    let profile = options.profile.clone().unwrap_or_else(|| {
        std::env::temp_dir()
            .join(format!("kairo-cef-{}", std::process::id()))
            .display()
            .to_string()
    });
    let sandbox = options.sandbox;
    let windowless = options.mode != Mode::Window;
    let state = Arc::new(State {
        options,
        started: Instant::now(),
        browser: Mutex::new(None),
        shm,
        paints: AtomicU64::new(0),
        accelerated: AtomicU64::new(0),
        audio_packets: AtomicU64::new(0),
        audio_frames: AtomicU64::new(0),
        crashed: AtomicU64::new(0),
        step: AtomicU64::new(0),
    });
    let mut app = Application::new(state.clone());
    let settings = Settings {
        no_sandbox: (!sandbox) as c_int,
        windowless_rendering_enabled: windowless as c_int,
        root_cache_path: CefString::from(profile.as_str()),
        cache_path: CefString::from(profile.as_str()),
        persist_session_cookies: 1,
        log_severity: LogSeverity::from(sys::cef_log_severity_t::LOGSEVERITY_WARNING),
        ..Default::default()
    };
    let ok = initialize(
        Some(args.as_main_args()),
        Some(&settings),
        Some(&mut app),
        std::ptr::null_mut(),
    );
    assert_eq!(ok, 1, "CEF failed to initialize");
    run_message_loop();
    say(&state, "LOOP returned");
    drop(app);
    shutdown();
    say(&state, "SHUTDOWN clean");
}
