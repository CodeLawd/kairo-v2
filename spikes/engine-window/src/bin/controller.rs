//! Parent process: GPUI controls that supervise the engine and show its preview.

use engine_window_spike::*;
use gpui_kit::*;
use image::{Frame, RgbaImage};
use interprocess::local_socket::{GenericNamespaced, Stream, ToNsName, prelude::*};
use std::io::BufReader;
use std::path::PathBuf;
use std::process::{Child, Command as Process};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone)]
struct Options {
    preview_fps: u32,
    surface: bool,
    no_drop: bool,
    duration: u64,
    latency: usize,
    kill_after: Option<u64>,
    force_borderless: bool,
    leave_engine: bool,
    grace_ms: u64,
}

fn flag(name: &str) -> bool {
    std::env::args().any(|a| a == name)
}

fn value<T: std::str::FromStr>(name: &str) -> Option<T> {
    let mut arguments = std::env::args();
    arguments.find(|a| a == name)?;
    arguments.next()?.parse().ok()
}

fn options() -> Options {
    Options {
        preview_fps: value("--preview-fps").unwrap_or(15),
        surface: flag("--surface"),
        no_drop: flag("--no-drop"),
        duration: value("--duration-secs").unwrap_or(60),
        latency: value("--latency").unwrap_or(0),
        kill_after: value("--kill-after-secs"),
        force_borderless: flag("--force-borderless"),
        leave_engine: flag("--leave-engine"),
        grace_ms: value("--grace-ms").unwrap_or(10_000),
    }
}

fn say(line: &str) {
    println!("[controller] {line}");
}

type Writer = Arc<Mutex<Option<Arc<Stream>>>>;

struct Shared {
    child: Mutex<Option<Child>>,
    writer: Writer,
    engine_pid: AtomicU64,
    stopping: AtomicBool,
}

fn connect(socket: &str, deadline: Duration) -> Option<Stream> {
    let start = Instant::now();
    while start.elapsed() < deadline {
        if let Ok(name) = socket.to_ns_name::<GenericNamespaced>()
            && let Ok(stream) = Stream::connect(name)
        {
            return Some(stream);
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    None
}

fn measure_latency(
    count: usize,
    reader: &mut impl std::io::Read,
    writer: &mut impl std::io::Write,
) -> std::io::Result<()> {
    let mut samples = Vec::with_capacity(count);
    let mut sequence = 0;
    while samples.len() < count {
        sequence += 1;
        let start = Instant::now();
        write_message(writer, &Command::Ping(sequence))?;
        loop {
            // Events unrelated to the probe, such as the display list, are skipped.
            if read_message::<Event>(reader)? == Event::Pong(sequence) {
                break;
            }
        }
        samples.push(start.elapsed().as_secs_f64() * 1_000.0);
    }
    samples.sort_by(f64::total_cmp);
    let at = |p| percentile(&samples, p).unwrap_or(f64::NAN);
    say(&format!(
        "latency n={count} p50={:.4}ms p95={:.4}ms p99={:.4}ms max={:.4}ms",
        at(0.50),
        at(0.95),
        at(0.99),
        samples[count - 1]
    ));
    Ok(())
}

/// Binds each display. On a single display the output must rehearse, as in
/// Electron, unless the run explicitly asks to test the borderless path.
fn outputs(displays: &[DisplayInfo], options: &Options) -> Vec<OutputSpec> {
    displays
        .iter()
        .enumerate()
        .map(|(index, display)| OutputSpec {
            name: format!("output-{index}"),
            binding: Binding {
                display_id: Some(display.id),
                label: display.label.clone(),
                size: Some((display.bounds.width, display.bounds.height)),
            },
            rehearsal: index == 0 && !options.force_borderless,
            frame_height: 1080,
        })
        .collect()
}

fn supervise(socket: String, shm: PathBuf, options: Options, shared: Arc<Shared>) {
    let engine = std::env::current_exe().unwrap().with_file_name("engine");
    let start = Instant::now();
    let mut budget = FailureBudget::default();
    let mut killed_at: Option<Instant> = None;
    let mut kill_planned = options.kill_after;
    let mut latency = options.latency;
    loop {
        let child = Process::new(&engine)
            .args(["--socket", &socket, "--shm"])
            .arg(&shm)
            .args(["--grace-ms", &options.grace_ms.to_string()])
            .spawn();
        let child = match child {
            Ok(child) => child,
            Err(error) => {
                say(&format!("spawn failed: {error}"));
                return;
            }
        };
        let pid = child.id();
        shared.engine_pid.store(u64::from(pid), Ordering::SeqCst);
        *shared.child.lock().unwrap() = Some(child);
        say(&format!("engine pid {pid}"));

        if let Some(stream) = connect(&socket, Duration::from_secs(5)) {
            let stream = Arc::new(stream);
            let mut reader = BufReader::new(&*stream);
            if latency > 0 {
                if let Err(error) = measure_latency(latency, &mut reader, &mut &*stream) {
                    say(&format!("latency probe failed: {error}"));
                }
                latency = 0;
            }
            *shared.writer.lock().unwrap() = Some(stream.clone());
            let mut configured = false;
            while let Ok(event) = read_message::<Event>(&mut reader) {
                match event {
                    Event::Displays(displays) => {
                        say(&format!("displays: {displays:?}"));
                        if !configured {
                            configured = true;
                            // Replays the program state after a relaunch too.
                            let command = Command::Configure {
                                outputs: outputs(&displays, &options),
                                preview_fps: options.preview_fps,
                            };
                            if let Some(writer) = shared.writer.lock().unwrap().as_ref() {
                                let _ = write_message(&mut &**writer, &command);
                            }
                        }
                    }
                    Event::Ready => {
                        say(&format!(
                            "engine ready at {:.3}s",
                            start.elapsed().as_secs_f64()
                        ));
                        if let Some(at) = killed_at.take() {
                            say(&format!(
                                "relaunch kill-to-first-frame {:.1}ms",
                                at.elapsed().as_secs_f64() * 1_000.0
                            ));
                        }
                        if let Some(after) = kill_planned.take() {
                            let shared = shared.clone();
                            std::thread::spawn(move || {
                                std::thread::sleep(Duration::from_secs(after));
                                let pid = shared.engine_pid.load(Ordering::SeqCst) as i32;
                                say("sending SIGKILL to engine");
                                // SAFETY: plain syscall on the child we launched.
                                unsafe { libc::kill(pid, libc::SIGKILL) };
                            });
                            killed_at = Some(Instant::now() + Duration::from_secs(after));
                        }
                    }
                    other => say(&format!("{other:?}")),
                }
            }
        } else {
            say("could not connect to engine");
        }
        *shared.writer.lock().unwrap() = None;
        if let Some(mut child) = shared.child.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if shared.stopping.load(Ordering::SeqCst) {
            return;
        }
        let now = start.elapsed().as_millis() as u64;
        if !budget.allow(now) {
            say("engine keeps failing; not relaunching");
            return;
        }
        say("engine died; relaunching");
    }
}

fn sample(pid: u32) -> String {
    let output = Process::new("ps")
        .args(["-o", "rss=,time=", "-p", &pid.to_string()])
        .output();
    output
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default()
}

#[cfg(target_os = "macos")]
fn footprint_mb() -> f64 {
    let mut info: libc::rusage_info_v4 = unsafe { std::mem::zeroed() };
    // SAFETY: `info` is the buffer the V4 flavour writes.
    let status = unsafe {
        libc::proc_pid_rusage(
            std::process::id() as i32,
            libc::RUSAGE_INFO_V4,
            (&raw mut info).cast(),
        )
    };
    if status == 0 {
        info.ri_phys_footprint as f64 / 1_048_576.0
    } else {
        f64::NAN
    }
}

#[cfg(not(target_os = "macos"))]
fn footprint_mb() -> f64 {
    f64::NAN
}

#[cfg(target_os = "macos")]
mod nv12 {
    use objc2_core_foundation::{CFDictionary, CFRetained, CFString, CFType};
    use objc2_core_video::*;
    use std::ptr::NonNull;

    /// Converts BGRA to a full-range BT.709 NV12 IOSurface-backed buffer.
    pub fn convert(bgra: &[u8], width: usize, height: usize) -> Option<CFRetained<CVPixelBuffer>> {
        // SAFETY: CoreVideo calls follow the documented create and lock
        // protocol; plane writes stay within the reported rows and heights.
        unsafe {
            let key: &CFString = kCVPixelBufferIOSurfacePropertiesKey;
            let empty = CFDictionary::<CFString, CFType>::empty();
            let value = &*(CFRetained::as_ptr(&empty).as_ptr() as *const CFType);
            let attributes = CFDictionary::<CFString, CFType>::from_slices(&[key], &[value]);
            let mut out = std::ptr::null_mut();
            let status = CVPixelBufferCreate(
                None,
                width,
                height,
                kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
                Some(attributes.as_opaque()),
                NonNull::from(&mut out),
            );
            if status != 0 {
                return None;
            }
            let buffer = CFRetained::from_raw(NonNull::new(out)?);
            let flags = CVPixelBufferLockFlags(0);
            CVPixelBufferLockBaseAddress(&buffer, flags);
            let y = CVPixelBufferGetBaseAddressOfPlane(&buffer, 0).cast::<u8>();
            let y_row = CVPixelBufferGetBytesPerRowOfPlane(&buffer, 0);
            let uv = CVPixelBufferGetBaseAddressOfPlane(&buffer, 1).cast::<u8>();
            let uv_row = CVPixelBufferGetBytesPerRowOfPlane(&buffer, 1);
            for row in 0..height {
                for column in 0..width {
                    let p = &bgra[(row * width + column) * 4..];
                    let (b, g, r) = (f32::from(p[0]), f32::from(p[1]), f32::from(p[2]));
                    let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                    *y.add(row * y_row + column) = luma as u8;
                    if row % 2 == 0 && column % 2 == 0 {
                        let target = uv.add((row / 2) * uv_row + column);
                        *target = ((b - luma) / 1.8556 + 128.0) as u8;
                        *target.add(1) = ((r - luma) / 1.5748 + 128.0) as u8;
                    }
                }
            }
            CVPixelBufferUnlockBaseAddress(&buffer, flags);
            Some(buffer)
        }
    }
}

struct Preview {
    shm: PreviewShm,
    last_frame: u64,
    shown: u64,
    current: Option<Arc<RenderImage>>,
    retired: Option<Arc<RenderImage>>,
    #[cfg(target_os = "macos")]
    surface_frame: Option<objc2_core_foundation::CFRetained<objc2_core_video::CVPixelBuffer>>,
    options: Options,
}

impl Preview {
    fn poll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((frame, mut pixels)) = self.shm.read_new(self.last_frame) else {
            return;
        };
        self.last_frame = frame;
        self.shown += 1;
        #[cfg(target_os = "macos")]
        if self.options.surface {
            self.surface_frame =
                nv12::convert(&pixels, PREVIEW_WIDTH as usize, PREVIEW_HEIGHT as usize);
            cx.notify();
            return;
        }
        // The previous frame may still be on screen until the next paint, so
        // the frame replaced last time is the one that is safe to evict.
        if let Some(old) = self.retired.take()
            && !self.options.no_drop
            && let Err(error) = window.drop_image(old)
        {
            say(&format!("drop_image failed: {error}"));
        }
        pixels.truncate((PREVIEW_WIDTH * PREVIEW_HEIGHT * 4) as usize);
        let Some(buffer) = RgbaImage::from_raw(PREVIEW_WIDTH, PREVIEW_HEIGHT, pixels) else {
            return;
        };
        self.retired = self.current.take();
        self.current = Some(Arc::new(RenderImage::new([Frame::new(buffer)])));
        cx.notify();
    }
}

impl Render for Preview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let mut root = div()
            .size_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .bg(gpui_kit::black())
            .text_color(gpui_kit::white())
            .child(format!("Frames shown: {}", self.shown));
        #[cfg(target_os = "macos")]
        if let Some(frame) = self.surface_frame.clone() {
            root = root.child(surface(frame).w(px(640.)).h(px(360.)));
        }
        if let Some(image) = self.current.clone() {
            root = root.child(img(image).w(px(640.)).h(px(360.)));
        }
        root
    }
}

#[cfg(target_os = "macos")]
fn frontmost_is_self() -> bool {
    use objc2_app_kit::NSWorkspace;
    NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .is_some_and(|app| app.processIdentifier() == std::process::id() as i32)
}

fn main() {
    let options = options();
    let pid = std::process::id();
    let socket = format!("kairo-spike-{pid}.sock");
    let shm_path = std::env::temp_dir().join(format!("kairo-spike-{pid}.shm"));
    let shm = PreviewShm::create(&shm_path, PREVIEW_WIDTH, PREVIEW_HEIGHT).expect("preview shm");
    let shared = Arc::new(Shared {
        child: Mutex::new(None),
        writer: Arc::new(Mutex::new(None)),
        engine_pid: AtomicU64::new(0),
        stopping: AtomicBool::new(false),
    });
    {
        let (socket, path, options, shared) = (
            socket.clone(),
            shm_path.clone(),
            options.clone(),
            shared.clone(),
        );
        std::thread::spawn(move || supervise(socket, path, options, shared));
    }
    {
        let shared = shared.clone();
        let duration = options.duration;
        std::thread::spawn(move || {
            let start = Instant::now();
            while start.elapsed().as_secs() < duration {
                let engine = shared.engine_pid.load(Ordering::SeqCst) as u32;
                say(&format!(
                    "sample t={}s controller[{}] footprint={:.1}MB engine[{}] load={}",
                    start.elapsed().as_secs(),
                    sample(pid),
                    footprint_mb(),
                    sample(engine),
                    Process::new("sysctl")
                        .args(["-n", "vm.loadavg"])
                        .output()
                        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                        .unwrap_or_default(),
                ));
                std::thread::sleep(Duration::from_secs(10).min(Duration::from_secs(
                    duration.saturating_sub(start.elapsed().as_secs()).max(1),
                )));
            }
        });
    }
    let poll = Duration::from_secs_f64(1.0 / f64::from(options.preview_fps.max(1) * 2));
    let run_options = options.clone();
    application().with_assets(assets::Assets).run(move |cx| {
        init(cx);
        let options = run_options.clone();
        let bounds = Bounds::centered(None, size(px(680.), px(440.)), cx);
        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        };
        open_window(window_options, cx, move |window, cx| {
            let view = cx.new(|_| Preview {
                shm,
                last_frame: 0,
                shown: 0,
                current: None,
                retired: None,
                #[cfg(target_os = "macos")]
                surface_frame: None,
                options: options.clone(),
            });
            let handle = view.downgrade();
            window
                .spawn(cx, async move |cx| {
                    while handle
                        .update_in(cx, |view, window, cx| view.poll(window, cx))
                        .is_ok()
                    {
                        cx.background_executor().timer(poll).await;
                    }
                })
                .detach();
            view
        })
        .expect("controller window");
        cx.activate(true);
        let shared = shared.clone();
        let path = shm_path.clone();
        let options = run_options.clone();
        cx.spawn(async move |cx| {
            cx.background_executor().timer(Duration::from_secs(4)).await;
            #[cfg(target_os = "macos")]
            say(&format!("frontmost_is_controller={}", frontmost_is_self()));
            cx.background_executor()
                .timer(Duration::from_secs(options.duration))
                .await;
            #[cfg(target_os = "macos")]
            say(&format!("frontmost_is_controller={}", frontmost_is_self()));
            shared.stopping.store(true, Ordering::SeqCst);
            if !options.leave_engine
                && let Some(writer) = shared.writer.lock().unwrap().as_ref()
            {
                let _ = write_message(&mut &**writer, &Command::Shutdown);
            }
            std::thread::sleep(Duration::from_millis(200));
            let _ = std::fs::remove_file(&path);
            say("done");
            std::process::exit(0);
        })
        .detach();
    });
}
