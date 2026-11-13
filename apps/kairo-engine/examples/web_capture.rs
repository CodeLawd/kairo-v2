//! Prompt 059's first gate: Wry/WKWebView capture, before shipping a web layer.
//! Run `cargo run -p kairo-engine --release --example web_capture` on macOS.
//! Captures 300 frames at 30 Hz, after 30 warm-up frames; prints request-to-CPU
//! latency including pixel copying, missed deadlines and errors. No IPC or network.

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("This probe uses macOS WKWebView.takeSnapshot; Windows capture is not qualified.");
    std::process::exit(2);
}

#[cfg(target_os = "macos")]
fn main() {
    mac::run();
}

#[cfg(target_os = "macos")]
mod mac {
    use block2::RcBlock;
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
    use objc2_foundation::{NSDictionary, NSError, NSNumber};
    use objc2_web_kit::WKSnapshotConfiguration;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::{Duration, Instant};
    use winit::application::ApplicationHandler;
    use winit::dpi::LogicalSize;
    use winit::event::WindowEvent;
    use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
    use winit::window::{Window, WindowId};
    use wry::{WebView, WebViewBuilder, WebViewExtMacOS};

    const PERIOD: Duration = Duration::from_nanos(1_000_000_000 / 30);
    const HTML: &str = r#"<!doctype html><meta name="viewport" content="width=device-width"><style>
body{margin:0;background:white;font:48px sans-serif}header{background:#126; color:white;padding:40px}
canvas{display:block}p{margin:40px}</style><header>Kairo 059 · live web capture · 1920 × 1080</header>
<canvas width="1920" height="720"></canvas><p>Animated canvas, text and scrolling content</p>
<script>let n=0;const c=document.querySelector('canvas'),x=c.getContext('2d');function draw(){
x.fillStyle='#e3edf1';x.fillRect(0,0,1920,720);x.fillStyle='#d64';x.fillRect((n*8)%1800,80,120,480);
x.fillStyle='#123';x.font='64px sans-serif';x.fillText('Frame '+n++,80,650);requestAnimationFrame(draw)}draw()</script>"#;

    #[derive(Default)]
    struct Results {
        busy: bool,
        count: usize,
        latencies: Vec<f64>,
        errors: Vec<String>,
        missed: usize,
        bytes: usize,
        passed: bool,
        finished: bool,
    }

    struct Probe {
        view: Option<WebView>,
        window: Option<Window>,
        next: Instant,
        started: Instant,
        results: Rc<RefCell<Results>>,
    }

    impl ApplicationHandler for Probe {
        fn resumed(&mut self, el: &ActiveEventLoop) {
            if self.window.is_some() {
                return;
            }
            let window = el
                .create_window(
                    Window::default_attributes()
                        .with_title("Kairo 059 capture qualification")
                        .with_inner_size(LogicalSize::new(1920., 1080.)),
                )
                .expect("probe window");
            let view = WebViewBuilder::new()
                .with_html(HTML)
                .with_bounds(wry::Rect {
                    position: winit::dpi::LogicalPosition::new(0., 0.).into(),
                    size: LogicalSize::new(1920., 1080.).into(),
                })
                .build_as_child(&window)
                .expect("Wry child");
            println!(
                "layout=1920x1080 capture=1920x1080 scale={} profile={}",
                window.scale_factor(),
                if cfg!(debug_assertions) {
                    "debug"
                } else {
                    "release"
                }
            );
            self.window = Some(window);
            self.view = Some(view);
            self.next = Instant::now() + Duration::from_secs(3);
            self.started = self.next;
        }

        fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
            if event == WindowEvent::CloseRequested {
                el.exit();
            }
        }

        fn about_to_wait(&mut self, el: &ActiveEventLoop) {
            if self.results.borrow().finished {
                return;
            }
            let now = Instant::now();
            if now < self.next {
                el.set_control_flow(ControlFlow::WaitUntil(self.next));
                return;
            }
            if self.results.borrow().count >= 330 {
                let mut r = self.results.borrow_mut();
                r.finished = true;
                let mut times = r.latencies.clone();
                times.sort_by(f64::total_cmp);
                if times.is_empty() {
                    eprintln!("FAILED: no captured frames: {:?}", r.errors);
                } else {
                    println!(
                        "frames={} elapsed_s={:.3} p50_ms={:.3} p99_ms={:.3} max_ms={:.3} missed_ticks={} bytes={} errors={:?}",
                        times.len(),
                        self.started.elapsed().as_secs_f64(),
                        times[times.len() / 2],
                        times[(times.len() * 99).div_ceil(100) - 1],
                        times[times.len() - 1],
                        r.missed,
                        r.bytes,
                        r.errors
                    );
                }
                r.passed = times.len() == 300
                    && times[(times.len() * 99).div_ceil(100) - 1] < 50.
                    && r.errors.is_empty()
                    && r.missed == 0;
                println!("capture_gate={}", if r.passed { "PASS" } else { "FAIL" });
                el.exit();
                return;
            }
            self.next += PERIOD;
            // Skip late ticks; never queue overlapping capture requests.
            while self.next <= now {
                self.next += PERIOD;
                self.results.borrow_mut().missed += 1;
            }
            if self.results.borrow().busy {
                self.results.borrow_mut().missed += 1;
            } else if let (Some(view), Some(window)) = (&self.view, &self.window) {
                self.results.borrow_mut().busy = true;
                // SAFETY: winit callbacks run on the application main thread.
                let config =
                    unsafe { WKSnapshotConfiguration::new(MainThreadMarker::new().unwrap()) };
                // WK's width is in points; request exactly 1920 physical pixels.
                unsafe {
                    config
                        .setSnapshotWidth(Some(&NSNumber::new_f64(1920. / window.scale_factor())));
                }
                let results = self.results.clone();
                let request = Instant::now();
                let callback = RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
                    let mut r = results.borrow_mut();
                    r.busy = false;
                    r.count += 1;
                    if !error.is_null() || image.is_null() {
                        r.errors.push(format!(
                            "snapshot failed (image null: {}, error: {error:p})",
                            image.is_null()
                        ));
                        return;
                    }
                    // SAFETY: WebKit owns the image throughout this callback. The
                    // bitmap owns its storage; copy it before returning to WebKit.
                    unsafe {
                        let image = &*image;
                        let Some(cg) = image.CGImageForProposedRect_context_hints(
                            std::ptr::null_mut(),
                            None,
                            None,
                        ) else {
                            r.errors.push("snapshot has no CGImage".into());
                            return;
                        };
                        let bitmap =
                            NSBitmapImageRep::initWithCGImage(NSBitmapImageRep::alloc(), &cg);
                        if bitmap.pixelsWide() != 1920 || bitmap.pixelsHigh() != 1080 {
                            r.errors.push(format!(
                                "unexpected pixels {}x{}",
                                bitmap.pixelsWide(),
                                bitmap.pixelsHigh()
                            ));
                            return;
                        }
                        let len = (bitmap.bytesPerRow() * bitmap.pixelsHigh()) as usize;
                        if bitmap.bitmapData().is_null() || len > 1920 * 1080 * 8 {
                            r.errors.push("invalid bitmap storage".into());
                            return;
                        }
                        let pixels = std::slice::from_raw_parts(bitmap.bitmapData(), len).to_vec();
                        r.bytes += pixels.len();
                        std::hint::black_box(&pixels);
                    }
                    if r.count > 30 {
                        r.latencies.push(request.elapsed().as_secs_f64() * 1000.);
                    }
                    // Save one frame after timing; encoding is not capture work.
                    if r.count == 330
                        && let Ok(path) = std::env::var("KAIRO_WEB_CAPTURE_PNG")
                    {
                        unsafe {
                            if let Some(cg) = (&*image).CGImageForProposedRect_context_hints(
                                std::ptr::null_mut(),
                                None,
                                None,
                            ) {
                                let bitmap = NSBitmapImageRep::initWithCGImage(
                                    NSBitmapImageRep::alloc(),
                                    &cg,
                                );
                                if let Some(data) = bitmap.representationUsingType_properties(
                                    NSBitmapImageFileType::PNG,
                                    &NSDictionary::new(),
                                ) {
                                    std::fs::write(path, data.to_vec()).expect("write capture PNG");
                                }
                            }
                        }
                    }
                });
                // SAFETY: main-thread WKWebView and a copied completion block.
                unsafe {
                    view.webview()
                        .takeSnapshotWithConfiguration_completionHandler(Some(&config), &callback);
                }
            }
            if self.started.elapsed() > Duration::from_secs(60) {
                eprintln!("FAILED: capture timed out after 60 s");
                el.exit();
                return;
            }
            el.set_control_flow(ControlFlow::WaitUntil(self.next));
        }
    }

    pub fn run() {
        let el = EventLoop::new().expect("event loop");
        let mut probe = Probe {
            view: None,
            window: None,
            next: Instant::now(),
            started: Instant::now(),
            results: Rc::default(),
        };
        el.run_app(&mut probe).expect("run probe");
        if !probe.results.borrow().passed {
            std::process::exit(1);
        }
    }
}
