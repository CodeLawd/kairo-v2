//! Real Wry + real GPU integration test, with a localhost adversarial page.
//! `cargo run -p kairo-engine --example verify_web`
//! Requires a macOS desktop (Windows runtime qualification is deferred).

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn main() {
    eprintln!("Run this native verification on macOS or Windows");
    std::process::exit(2);
}
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn main() {
    native::run();
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod native {
    use kairo_compositor::{
        Content, Fill, Slot,
        thread::{Compositor, Op, Sink},
    };
    use kairo_engine::web::{self, Webs};
    use kairo_engine_proto::{WebInput, WebLayer};
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{Arc, Mutex},
        time::{Duration, Instant},
    };
    use winit::{
        application::ApplicationHandler,
        event::WindowEvent,
        event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
        window::WindowId,
    };

    #[derive(Default, Debug)]
    struct Observed {
        loads: usize,
        click: bool,
        text: bool,
        scroll: bool,
        denied: bool,
        attempt: bool,
        attack: bool,
    }
    fn site() -> (String, Arc<Mutex<Observed>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Observed::default()));
        let state = seen.clone();
        std::thread::spawn(move || {
            for connection in listener.incoming() {
                let Ok(mut connection) = connection else {
                    break;
                };
                let _ = connection.set_read_timeout(Some(Duration::from_secs(2)));
                let mut data = [0u8; 8192];
                let Ok(n) = connection.read(&mut data) else {
                    continue;
                };
                let request = String::from_utf8_lossy(&data[..n]);
                let path = request.split_whitespace().nth(1).unwrap_or("");
                let mut s = state.lock().unwrap();
                let body = match path {
                    "/" | "/next" | "/replacement" => {
                        s.loads += 1;
                        r#"<!doctype html><style>body{margin:0;height:4000px;background:#126548}button{position:fixed;top:20px;left:20px;width:200px;height:60px}input{position:fixed;top:90px;left:20px;width:250px;height:50px}#deny{top:160px}</style><button id='click'>click</button><input id='text'><button id='deny'>capture</button><a id="next" href="/next" style="position:fixed;left:20px;top:240px">Next</a><iframe src='/attack' style='position:fixed;left:400px;top:20px;width:300px;height:100px;border:0'></iframe><script>
const send=(s)=>fetch('/'+s).catch(()=>{});
document.querySelector('#click').onclick=()=>send('click');
document.querySelector('#text').oninput=e=>{if(e.target.value==='verified')send('text')};
document.querySelector('#deny').onclick=()=>navigator.mediaDevices.getUserMedia({audio:true,video:true}).then(()=>send('granted'),()=>send('denied'));
addEventListener('scroll',()=>{if(scrollY===700)send('scroll')});
window.ipc.postMessage(JSON.stringify({type:'Shutdown'}));
window.ipc.postMessage('x'.repeat(300));
</script>"#.to_string()
                    }
                    "/click" => {
                        s.click = true;
                        "ok".into()
                    }
                    "/text" => {
                        s.text = true;
                        "ok".into()
                    }
                    "/scroll" => {
                        s.scroll = true;
                        "ok".into()
                    }
                    "/denied" => {
                        s.denied = true;
                        "ok".into()
                    }
                    "/favicon.ico" => String::new(),
                    "/attempt" => { s.attempt = true; "ok".into() },
                    "/attack" => r#"<style>body{margin:0}button{width:100%;height:100px}</style><button onclick="fetch('/attempt');top.location='/hijack'">Try top navigation</button>"#.into(),
                    _ => {
                        s.attack = true;
                        "unexpected".into()
                    }
                };
                drop(s);
                let body = body.replace(
                    "src='/attack'",
                    &format!("src='http://localhost:{}/attack'", address.port()),
                );
                let framing = if path == "/attack" {
                    ""
                } else {
                    "X-Frame-Options: DENY\r\nContent-Security-Policy: frame-ancestors 'none'\r\n"
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n{framing}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = connection.write_all(response.as_bytes());
            }
        });
        (format!("http://{address}/"), seen)
    }
    struct Rig {
        browser: Webs,
        gpu: Option<Compositor>,
        url: String,
        seen: Arc<Mutex<Observed>>,
        started: Instant,
        next: Instant,
        stage: u8,
        stage_at: Instant,
        passed: bool,
    }
    impl Rig {
        fn wanted(&mut self, el: &ActiveEventLoop, n: usize, reload: f64) {
            let wants = ["a", "b"]
                .into_iter()
                .take(n)
                .map(|o| {
                    (
                        o.into(),
                        WebLayer {
                            url: self.url.clone(),
                            reload,
                        },
                    )
                })
                .collect();
            let messages = self
                .browser
                .sync(el, wants, &self.gpu.as_ref().unwrap().feed());
            for m in messages {
                if let web::Notice::Error(e) = m {
                    panic!("{e}")
                }
            }
        }
    }
    impl ApplicationHandler for Rig {
        fn resumed(&mut self, el: &ActiveEventLoop) {
            if self.gpu.is_some() {
                return;
            }
            let gpu = Compositor::start(|_| {}).expect("GPU");
            for o in ["a", "b"] {
                gpu.open(o, 1080, Sink::Offscreen);
                gpu.apply(
                    o,
                    1,
                    vec![Op::Swap {
                        slot: Slot::Web,
                        content: Some(Content::new(Fill::Source {
                            id: web::source(&self.url),
                            backdrop: [1.; 4],
                        })),
                        ms: 0,
                    }],
                );
            }
            self.gpu = Some(gpu);
            self.wanted(el, 2, 0.);
            self.started = Instant::now();
            self.stage_at = self.started;
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
        fn about_to_wait(&mut self, el: &ActiveEventLoop) {
            let now = Instant::now();
            assert!(
                self.started.elapsed() < Duration::from_secs(45),
                "native verification timed out: {:?}",
                self.seen.lock().unwrap()
            );
            if now < self.next {
                el.set_control_flow(ControlFlow::WaitUntil(self.next));
                return;
            }
            self.next = now + web::PERIOD;
            if let Some(gpu) = self.gpu.as_ref() {
                for m in self.browser.tick(now, &gpu.feed()) {
                    if let web::Notice::Error(e) = m {
                        panic!("{e}")
                    }
                }
            }
            let elapsed = self.stage_at.elapsed();
            let next = match self.stage {
                0 if elapsed > Duration::from_secs(3) => {
                    assert_eq!(
                        self.browser.live_pages(),
                        1,
                        "two consumers must use one browser"
                    );
                    assert_eq!(self.seen.lock().unwrap().loads, 1);
                    let gpu = self.gpu.as_ref().unwrap();
                    let a = gpu.read("a").unwrap();
                    let b = gpu.read("b").unwrap();
                    assert_eq!((a.0, a.1), (1920, 1080));
                    assert_eq!(a.2, b.2, "both outputs draw the same live texture");
                    let p = (900 * 1920 + 1500) * 4;
                    assert_eq!(
                        &a.2[p..p + 4],
                        &[0x12, 0x65, 0x48, 255],
                        "page pixels reached the GPU"
                    );
                    self.browser
                        .input("a", &WebInput::Click { x: 100., y: 40. });
                    true
                }
                1 if self.seen.lock().unwrap().click => {
                    self.browser.scroll("b", 0., 700.);
                    true
                }
                2 if self.seen.lock().unwrap().scroll => {
                    self.browser
                        .input("b", &WebInput::Click { x: 100., y: 110. });
                    self.browser.input(
                        "b",
                        &WebInput::Text {
                            text: "verified".into(),
                        },
                    );
                    true
                }
                3 if self.seen.lock().unwrap().text => {
                    self.browser
                        .input("a", &WebInput::Click { x: 100., y: 180. });
                    true
                }
                4 if self.seen.lock().unwrap().denied => {
                    self.browser
                        .input("a", &WebInput::Click { x: 500., y: 50. });
                    true
                }
                5 if self.seen.lock().unwrap().attempt && elapsed > Duration::from_millis(300) => {
                    assert!(!self.seen.lock().unwrap().attack);
                    self.wanted(el, 1, 0.);
                    assert_eq!(self.browser.live_pages(), 1);
                    assert_eq!(self.seen.lock().unwrap().loads, 1);
                    true
                }
                6 if elapsed > Duration::from_millis(200) => {
                    self.wanted(el, 0, 0.);
                    assert_eq!(
                        self.browser.live_pages(),
                        0,
                        "last consumer tears down the browser immediately"
                    );
                    self.wanted(el, 2, 0.);
                    true
                }
                7 if self.seen.lock().unwrap().loads == 2 && elapsed > Duration::from_secs(2) => {
                    self.wanted(el, 2, 1.);
                    true
                }
                8 if self.seen.lock().unwrap().loads == 3 && elapsed > Duration::from_secs(2) => {
                    self.browser
                        .input("a", &WebInput::Click { x: 40., y: 250. });
                    true
                }
                9 if self.seen.lock().unwrap().loads == 4 && elapsed > Duration::from_secs(2) => {
                    let base = self.url.trim_end_matches('/').to_string();
                    self.url = format!("{base}/next");
                    self.wanted(el, 2, 1.);
                    assert_eq!(
                        self.browser.live_pages(),
                        1,
                        "adopt navigation without another browser"
                    );
                    true
                }
                10 if elapsed > Duration::from_millis(300) => {
                    assert_eq!(
                        self.seen.lock().unwrap().loads,
                        4,
                        "URL adoption does not reload"
                    );
                    let root = self.url.strip_suffix("next").unwrap().to_string();
                    let wants = vec![
                        (
                            "a".into(),
                            WebLayer {
                                url: root,
                                reload: 1.,
                            },
                        ),
                        (
                            "b".into(),
                            WebLayer {
                                url: self.url.clone(),
                                reload: 1.,
                            },
                        ),
                    ];
                    self.browser
                        .sync(el, wants, &self.gpu.as_ref().unwrap().feed());
                    assert_eq!(self.browser.live_pages(), 2);
                    true
                }
                11 if self.seen.lock().unwrap().loads == 5 && elapsed > Duration::from_secs(2) => {
                    let root = self.url.strip_suffix("next").unwrap();
                    assert_eq!(self.browser.location("a").as_deref(), Some(root));
                    assert_eq!(self.browser.location("b"), Some(self.url.clone()));
                    let wants = vec![
                        (
                            "a".into(),
                            WebLayer {
                                url: format!("{root}replacement"),
                                reload: 1.,
                            },
                        ),
                        (
                            "b".into(),
                            WebLayer {
                                url: self.url.clone(),
                                reload: 1.,
                            },
                        ),
                    ];
                    let notices = self
                        .browser
                        .sync(el, wants, &self.gpu.as_ref().unwrap().feed());
                    assert!(!notices.iter().any(|n| matches!(n, web::Notice::Error(_))));
                    assert_eq!(
                        self.browser.live_pages(),
                        2,
                        "replacement at capacity releases its old owner first"
                    );
                    true
                }
                12 if self.seen.lock().unwrap().loads == 6 && elapsed > Duration::from_secs(2) => {
                    self.wanted(el, 0, 0.);
                    self.gpu.as_ref().unwrap().begin_shutdown();
                    true
                }
                13 if self.gpu.as_ref().unwrap().finished() => {
                    self.gpu = None;
                    self.passed = true;
                    println!(
                        "PASS: one browser/two GPU outputs; framing bypass; direct click/text/scroll; camera+mic denial; blocked subframe top navigation; teardown/reopen/reload; URL adoption/return-to-original; replacement at capacity; malformed IPC ignored"
                    );
                    el.exit();
                    false
                }
                _ => false,
            };
            if next {
                self.stage += 1;
                self.stage_at = now;
            }
            el.set_control_flow(ControlFlow::WaitUntil(self.next));
        }
    }
    pub fn run() {
        let (url, seen) = site();
        let el = EventLoop::new().unwrap();
        let now = Instant::now();
        let mut rig = Rig {
            browser: Webs::with_page_limit(2),
            gpu: None,
            url,
            seen,
            started: now,
            next: now,
            stage: 0,
            stage_at: now,
            passed: false,
        };
        el.run_app(&mut rig).unwrap();
        assert!(rig.passed);
    }
}
