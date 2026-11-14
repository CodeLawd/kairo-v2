//! Live preview soak (prompt 066): a GPUI window showing a real engine
//! output's preview while the program changes continuously, with the
//! preview taken off screen on a schedule.
//!
//! ```text
//! cargo build --release -p kairo-engine
//! cargo run --release -p kairo-core-gpui --example preview_soak -- \
//!     --engine target/release/kairo-engine [--secs 7200] [--sample 60] \
//!     [--hide-every 600] [--hide-for 60] [--fps 30]
//! ```
//!
//! Every sample prints one line: elapsed seconds, the UI's and the
//! engine's CPU over the interval (% of one core), the UI's physical
//! footprint and both resident sizes, preview frames shown and images
//! evicted, and whether the preview was on screen and subscribed.

use gpui_kit::*;
use kairo_core::engine::{EngineHandle, EngineOptions, OutputEngine};
use kairo_core::preview::PreviewHub;
use kairo_core_gpui::preview::LivePreview;
use kairo_engine_proto::*;
use std::sync::Arc;
use std::time::{Duration, Instant};

struct Args {
    engine: String,
    secs: u64,
    sample: u64,
    hide_every: u64,
    hide_for: u64,
    fps: u32,
}

fn args() -> Args {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let value = |name: &str| {
        all.iter()
            .position(|a| a == name)
            .and_then(|i| all.get(i + 1))
            .cloned()
    };
    Args {
        engine: value("--engine").unwrap_or_else(|| "target/release/kairo-engine".into()),
        secs: value("--secs").and_then(|v| v.parse().ok()).unwrap_or(120),
        sample: value("--sample").and_then(|v| v.parse().ok()).unwrap_or(10),
        hide_every: value("--hide-every")
            .and_then(|v| v.parse().ok())
            .unwrap_or(60),
        hide_for: value("--hide-for")
            .and_then(|v| v.parse().ok())
            .unwrap_or(20),
        fps: value("--fps").and_then(|v| v.parse().ok()).unwrap_or(30),
    }
}

struct Root {
    preview: Entity<LivePreview>,
    shown: bool,
}

impl Render for Root {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(rgb(0x202020))
            .p_2()
            .children(self.shown.then(|| self.preview.clone()))
    }
}

/// CPU seconds and resident KiB of a process, from `ps`.
fn ps(pid: u32) -> Option<(f64, u64)> {
    let out = std::process::Command::new("ps")
        .args(["-o", "time=,rss=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut parts = text.split_whitespace();
    let time = parts.next()?;
    let rss = parts.next()?.parse().ok()?;
    // [[dd-]hh:]mm:ss.cc
    let mut secs = 0.0;
    for field in time.replace('-', ":").split(':') {
        secs = secs * 60.0 + field.parse::<f64>().ok()?;
    }
    Some((secs, rss))
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

fn slide(i: u64) -> Command {
    let colors = ["#C81E1E", "#1E3CC8", "#1E8C3C", "#8C1E8C"];
    Command::ShowSlide {
        output: "screen".into(),
        source: "screen".into(),
        slide: SlideContent {
            reference: "Preview soak".into(),
            text: format!("Slide {i}"),
            theme: Json(format!(
                r#"{{"background":{{"type":"color","color":"{}"}}}}"#,
                colors[(i % 4) as usize]
            )),
            colored_lines: None,
        },
        instant: false,
    }
}

fn main() {
    let args = args();
    let runtime = tokio::runtime::Runtime::new().expect("tokio");
    let _enter = runtime.enter();
    let mut options = EngineOptions::new(&args.engine);
    options.grace = Duration::from_secs(3);
    let engine = EngineHandle::spawn(options);
    let started = Instant::now();
    while engine.displays().is_empty() {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "engine did not start"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let display = engine.displays()[0].clone();
    engine.report_control_display(Some(display.native_id));
    engine.command(Command::OpenSurface(SurfaceSpec {
        output: "screen".into(),
        label: "Preview soak".into(),
        sink: SinkSpec::Display(DisplaySink {
            binding: DisplayBinding {
                display_id: Some(display.id),
                label: display.label.clone(),
                size: Some((display.bounds.width, display.bounds.height)),
            },
            rehearsal: true,
            frame_height: 1080,
        }),
    }));
    engine.command(Command::Configure {
        output: "screen".into(),
        config: SurfaceConfig {
            transition_ms: 400.0,
            ..SurfaceConfig::default()
        },
    });
    // A new slide every 300 ms with a 400 ms fade: the program never rests,
    // so the preview runs at its full rate.
    {
        let engine = engine.clone();
        std::thread::spawn(move || {
            let mut i = 0;
            loop {
                engine.command(slide(i));
                i += 1;
                std::thread::sleep(Duration::from_millis(300));
            }
        });
    }
    let hub = PreviewHub::new(Arc::new(engine.clone()) as Arc<dyn OutputEngine>).expect("hub");
    let engine_for_quit = engine.clone();
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(560.), px(330.)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some("Kairo preview soak".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let fps = args.fps;
        let handle = {
            let hub = hub.clone();
            cx
                .open_window(options, |window, cx| {
                    let preview = cx.new(|cx| {
                        let mut view = LivePreview::new(hub, cx);
                        view.set_fps(fps);
                        view
                    });
                    preview.update(cx, |v, cx| v.set_output(Some("screen".into()), window, cx));
                    cx.new(|_| Root {
                        preview,
                        shown: true,
                    })
                })
                .expect("window")
        };
        cx.activate(true);
        let hub = hub.clone();
        let engine = engine_for_quit.clone();
        let (secs, sample, hide_every, hide_for) =
            (args.secs, args.sample, args.hide_every, args.hide_for);
        cx.spawn(async move |cx| {
            let me = std::process::id();
            let begin = Instant::now();
            let mut last = (Instant::now(), ps(me).map_or(0.0, |p| p.0), 0.0f64, 0u32);
            let mut next_sample = sample;
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let t = begin.elapsed().as_secs();
                let hidden = hide_every > 0 && t % hide_every >= hide_every - hide_for;
                let _ = handle.update(cx, |root, _, cx| {
                    if root.shown == hidden {
                        root.shown = !hidden;
                        cx.notify();
                    }
                });
                if t >= next_sample {
                    next_sample += sample;
                    let engine_pid = engine.pid().unwrap_or(0);
                    let ui = ps(me).unwrap_or((0.0, 0));
                    let eng = ps(engine_pid).unwrap_or((0.0, 0));
                    let dt = last.0.elapsed().as_secs_f64();
                    let engine_cpu = if engine_pid == last.3 && dt > 0.0 {
                        (eng.0 - last.2) / dt * 100.0
                    } else {
                        f64::NAN
                    };
                    let stats = handle
                        .update(cx, |root, _, cx| root.preview.read(cx).stats())
                        .unwrap_or_default();
                    println!(
                        "sample t={t}s ui_cpu={:.2}% engine_cpu={:.2}% ui_footprint={:.1}MB ui_rss={}KiB engine_rss={}KiB shown={} dropped={} on_screen={} subscribed={} load={}",
                        (ui.0 - last.1) / dt * 100.0,
                        engine_cpu,
                        footprint_mb(),
                        ui.1,
                        eng.1,
                        stats.frames_shown,
                        stats.images_dropped,
                        !hidden,
                        !hub.active().is_empty(),
                        std::process::Command::new("sysctl")
                            .args(["-n", "vm.loadavg"])
                            .output()
                            .map(|o| String::from_utf8_lossy(&o.stdout).trim().replace(' ', ""))
                            .unwrap_or_default(),
                    );
                    last = (Instant::now(), ui.0, eng.0, engine_pid);
                }
                if t >= secs {
                    engine.shutdown();
                    cx.update(|cx| cx.quit());
                    break;
                }
            }
        })
        .detach();
    });
}
