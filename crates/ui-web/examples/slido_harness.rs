//! The Slido window against a real web view. Run from the repository root:
//!
//! - `cargo run -p ui-web --example slido_harness -- fixture` loads local
//!   fixtures (the Electron harness' selector fixture, permission probes,
//!   navigation blocking, hide-on-close and profile persistence).
//! - `... -- persist-set <dir>` then `... -- persist-get <dir>` check that a
//!   cookie survives a new process.
//! - `... -- live <code> [--moderator]` reads a real Slido event. It only
//!   reads: it never submits questions, votes or approvals.
#![allow(clippy::print_stdout)]

use gpui_kit::*;
use kairo_audience::{
    AudienceService, AudienceSource, SlidoAudienceSource, SlidoEventSink, SlidoWindow,
    SlidoWindowConfig, SlidoWindowEvent, SlidoWindowFactory, allowed_slido_frame_navigation,
    allowed_slido_navigation, allowed_slido_window,
};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;
use ui_web::{HostOptions, SlidoWebHost};

const SELECTORS: &str = include_str!("../../kairo-audience/src/slido/selectors.js");

const FIXTURE: &str = r#"<!doctype html><meta charset=utf-8><body>
<div class="question-item" data-qid="7"><div data-testid="question-author">Anna</div><div data-testid="question-text">A visible question?</div><button class="score__btn--plus"><span>12</span></button></div>
<div class="question-item" data-qid="8" hidden><div data-testid="question-text">Hidden must stay hidden</div><button class="score__btn--plus"><span>99</span></button></div>
<div data-item-question-card aria-label="Question number 1 from Anonymous: A pending question? 3 votes"><div data-question-id="9" data-testid="question"><span data-testid="user-name">Anonymous</span><span class="questionText-1-2-123">A pending question?</span><span class="srOnly-1-2-123">3 votes</span></div></div>
</body>"#;

/// Posted by the page itself once loaded.
const PROBE: &str = r#"<!doctype html><meta charset=utf-8><body><script>
const out = {};
const done = () => window.ipc.postMessage(JSON.stringify(out));
window.ipc.postMessage(JSON.stringify({ stage: 'loaded', secure: window.isSecureContext, md: !!(navigator.mediaDevices) }));
(async () => {
  try { await navigator.mediaDevices.getUserMedia({ audio: true }); out.media = 'granted'; } catch (e) { out.media = e.name; }
  await new Promise(r => navigator.geolocation.getCurrentPosition(() => { out.geo = 'granted'; r(); }, e => { out.geo = 'denied:' + e.code; r(); }, { timeout: 4000 }));
  out.open = String(window.open('https://app.sli.do/', '_blank'));
  out.cookieBefore = document.cookie;
  document.cookie = 'kairo=persisted; max-age=3600; path=/';
  setTimeout(() => { location.href = 'https://evil.example/'; }, 50);
  setTimeout(() => { out.host = location.hostname; done(); }, 2500);
})();
</script></body>"#;

fn serve() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let mut buf = [0u8; 2048];
            let n = stream.read(&mut buf).unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]);
            let path = request.split_whitespace().nth(1).unwrap_or("/");
            let body = if path.starts_with("/probe") {
                PROBE
            } else {
                FIXTURE
            };
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    port
}

struct Opened {
    window: Arc<dyn SlidoWindow>,
    events: mpsc::Receiver<SlidoWindowEvent>,
}

async fn open(host: &SlidoWebHost, script: String, url: String, moderator: bool) -> Opened {
    let (tx, events) = mpsc::channel();
    let tx = Mutex::new(tx);
    let sink: SlidoEventSink = Arc::new(move |event| {
        let _ = tx.lock().unwrap().send(event);
    });
    let window = host
        .open(
            SlidoWindowConfig {
                title: "Sign in to Slido".into(),
                width: 1100,
                height: 800,
                profile: "audience-slido",
                moderator,
                script,
                start_url: url,
            },
            sink,
        )
        .await
        .expect("window opens");
    Opened { window, events }
}

fn message(opened: &Opened, seconds: u64) -> Value {
    let deadline = std::time::Instant::now() + Duration::from_secs(seconds);
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        match opened.events.recv_timeout(left) {
            Ok(SlidoWindowEvent::Message { body, .. }) => {
                let value: Value = serde_json::from_str(&body).expect("json");
                if value.get("stage").is_some() {
                    println!("  page: {value}");
                    continue;
                }
                return value;
            }
            Ok(_) => {}
            Err(_) => panic!("no message from the page in {seconds} s"),
        }
    }
}

macro_rules! check {
    ($cond:expr, $($msg:tt)*) => {
        if $cond { println!("PASS: {}", format!($($msg)*)); } else { println!("FAIL: {}", format!($($msg)*)); std::process::exit(1); }
    };
}

#[cfg(target_os = "macos")]
fn native_windows() -> (usize, usize) {
    use objc2::runtime::{AnyClass, AnyObject};
    // SAFETY: NSApplication.sharedApplication and NSWindow queries on the main thread's app.
    unsafe {
        let class = AnyClass::get(c"NSApplication").expect("NSApplication");
        let app: *mut AnyObject = objc2::msg_send![class, sharedApplication];
        let windows: *mut AnyObject = objc2::msg_send![app, windows];
        let count: usize = objc2::msg_send![windows, count];
        let mut visible = 0;
        for index in 0..count {
            let window: *mut AnyObject = objc2::msg_send![windows, objectAtIndex: index];
            let on: bool = objc2::msg_send![window, isVisible];
            visible += usize::from(on);
        }
        (count, visible)
    }
}

#[cfg(target_os = "macos")]
fn perform_close_on_slido_window() {
    use objc2::runtime::{AnyClass, AnyObject};
    // SAFETY: as above; performClose: is what the title-bar close button sends.
    unsafe {
        let class = AnyClass::get(c"NSApplication").expect("NSApplication");
        let app: *mut AnyObject = objc2::msg_send![class, sharedApplication];
        let windows: *mut AnyObject = objc2::msg_send![app, windows];
        let window: *mut AnyObject = objc2::msg_send![windows, objectAtIndex: 0usize];
        // On the main thread, where the title-bar button runs.
        let _: () = objc2::msg_send![window, performSelectorOnMainThread: objc2::sel!(performClose:), withObject: std::ptr::null::<AnyObject>(), waitUntilDone: true];
    }
}

fn fixture_options(dir: PathBuf, port: u16) -> HostOptions {
    let mut options = HostOptions::new(dir);
    let origin = format!("http://127.0.0.1:{port}/");
    options.navigation = Some(Arc::new(move |url| {
        url.starts_with(&origin) || allowed_slido_frame_navigation(url)
    }));
    options
}

async fn fixture(host: SlidoWebHost, port: u16) {
    // Policy functions, as the Electron harness checks them.
    check!(
        allowed_slido_navigation("https://app.sli.do/event/test"),
        "allowlist accepts a Slido event"
    );
    for bad in [
        "file:///tmp/x",
        "javascript:alert(1)",
        "https://sli.do.evil.example",
        "http://app.sli.do",
    ] {
        check!(!allowed_slido_navigation(bad), "allowlist refuses {bad}");
    }
    check!(
        !allowed_slido_window("https://app.sli.do/", false),
        "participant source cannot open windows"
    );

    // 1. Selectors, injected unchanged, run in a real web view.
    let script = format!(
        "{SELECTORS}\ndocument.addEventListener('DOMContentLoaded', () => window.ipc.postMessage(JSON.stringify({{ pub: qaSelectors.extractSlidoQuestions(document, 'public'), mod: qaSelectors.extractSlidoQuestions(document, 'moderator') }})));"
    );
    let selectors = open(
        &host,
        script,
        format!("http://127.0.0.1:{port}/public"),
        false,
    )
    .await;
    let read = message(&selectors, 20);
    check!(
        read["pub"]
            == json!([{ "externalId": "7", "text": "A visible question?", "author": "Anna", "votes": 12 }]),
        "public selectors, author, votes, hidden-card exclusion: {}",
        read["pub"]
    );
    check!(
        read["mod"]
            == json!([{ "externalId": "9", "text": "A pending question?", "author": null, "votes": 3 }]),
        "moderator selectors, Anonymous to null, votes from aria-label: {}",
        read["mod"]
    );
    check!(
        !selectors.window.is_visible(),
        "a participant window stays hidden"
    );
    #[cfg(target_os = "macos")]
    {
        let (total, visible) = native_windows();
        check!(
            total >= 1 && visible == 0,
            "native: {total} window(s), {visible} visible"
        );
        // 2. Hide on close.
        selectors.window.show();
        std::thread::sleep(Duration::from_millis(500));
        let (_, visible) = native_windows();
        check!(
            visible == 1,
            "show() puts the window on screen ({visible} visible)"
        );
        perform_close_on_slido_window();
        std::thread::sleep(Duration::from_millis(500));
        let (total, visible) = native_windows();
        check!(
            total >= 1 && visible == 0,
            "the close button hides it and keeps it ({total} window(s), {visible} visible)"
        );
        check!(
            !selectors.window.is_visible(),
            "the source sees it as hidden"
        );
        selectors.window.show();
        std::thread::sleep(Duration::from_millis(300));
        selectors.window.hide();
        std::thread::sleep(Duration::from_millis(300));
        check!(native_windows().1 == 0, "hide() takes it off screen");
    }
    selectors.window.destroy();

    // 3. Permissions (script layer), navigation, window.open and cookies.
    let probe = open(
        &host,
        String::new(),
        format!("http://127.0.0.1:{port}/probe"),
        false,
    )
    .await;
    let result = message(&probe, 30);
    check!(
        result["media"] == "NotAllowedError",
        "script layer: getUserMedia refused ({})",
        result["media"]
    );
    check!(
        result["geo"] == "denied:1",
        "script layer: geolocation refused ({})",
        result["geo"]
    );
    check!(
        result["open"] == "null",
        "participant window.open returns null ({})",
        result["open"]
    );
    check!(
        result["host"] == "127.0.0.1",
        "navigation to evil.example was blocked ({})",
        result["host"]
    );
    probe.window.destroy();

    println!("fixture: all checks passed");
}

async fn persist(host: SlidoWebHost, port: u16, phase: &str) {
    let probe = open(
        &host,
        String::new(),
        format!("http://127.0.0.1:{port}/probe"),
        false,
    )
    .await;
    let result = message(&probe, 30);
    let cookie = result["cookieBefore"].as_str().unwrap_or("").to_owned();
    if phase == "set" {
        check!(
            !cookie.contains("kairo=persisted"),
            "first run starts without the cookie"
        );
        // Give WebKit time to write the store.
        tokio::time::sleep(Duration::from_secs(3)).await;
    } else {
        check!(
            cookie.contains("kairo=persisted"),
            "a new process sees the cookie from the last one ({cookie:?})"
        );
    }
}

async fn live(host: SlidoWebHost, code: String, moderator: bool) {
    let event = kairo_domain::audience::parse_slido_event(&code).expect("a Slido code or link");
    let service = AudienceService::new(Arc::new(NoScreen), false);
    let source = Arc::new(
        SlidoAudienceSource::new(event, moderator, false, Arc::new(host)).expect("source"),
    );
    let id = source.id().to_owned();
    service.start(source).await.expect("start");
    if moderator {
        println!("Sign in in the Slido window and open the event. This only reads.");
    }
    let limit = if moderator { 300 } else { 60 };
    for second in 0..limit {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let state = service.state();
        let status = state.sources.iter().find(|s| s.id == id);
        if second % 5 == 0 {
            println!(
                "{second:>3} s: {:?} {:?} questions={}",
                status.map(|s| s.state),
                status.and_then(|s| s.error.clone()),
                state.questions.len()
            );
        }
        if !state.questions.is_empty() {
            println!(
                "PASS: live {} event {code}; {} question(s) read into Review",
                if moderator { "moderator" } else { "public" },
                state.questions.len()
            );
            service.stop(&id).await.ok();
            return;
        }
    }
    println!("FAIL: no questions read in {limit} s");
    std::process::exit(1);
}

struct NoScreen;
impl kairo_audience::QuestionOutput for NoScreen {
    fn show_question(&self, _: kairo_domain::program::ProgramQuestion) {}
    fn clear_question(&self) {}
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.first().cloned().unwrap_or_else(|| "fixture".into());
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let handle = runtime.handle().clone();
    application().run(move |cx| {
        let port = serve();
        let dir = std::env::temp_dir().join(format!("kairo-slido-harness-{}", std::process::id()));
        let explicit = args
            .get(1)
            .filter(|_| mode.starts_with("persist"))
            .map(PathBuf::from);
        let profile = explicit.unwrap_or_else(|| dir.join("profile"));
        let host = SlidoWebHost::install(cx, fixture_options(profile.clone(), port));
        let live_host = SlidoWebHost::install(cx, HostOptions::new(profile));
        let args = args.clone();
        let mode = mode.clone();
        handle.spawn(async move {
            match mode.as_str() {
                "fixture" => fixture(host, port).await,
                "persist-set" => persist(host, port, "set").await,
                "persist-get" => persist(host, port, "get").await,
                "live" => {
                    live(
                        live_host,
                        args.get(1).cloned().unwrap_or_default(),
                        args.iter().any(|a| a == "--moderator"),
                    )
                    .await
                }
                other => {
                    println!("unknown mode {other}");
                    std::process::exit(2);
                }
            }
            std::process::exit(0);
        });
    });
}
