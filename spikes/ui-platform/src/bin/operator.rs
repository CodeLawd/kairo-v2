//! Runs the shell, the grid and the spike 002 preview.

use engine_window_spike::{
    Command, Event, PREVIEW_HEIGHT, PREVIEW_WIDTH, PreviewShm, read_message, write_message,
};
use gpui_kit::*;
use image::{Frame, RgbaImage};
use interprocess::local_socket::{GenericNamespaced, Stream, ToNsName, prelude::*};
use std::cell::RefCell;
use std::path::PathBuf;
use std::process::{Child, Command as Process};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use ui_platform_spike::model;
use ui_platform_spike::view::{Shell, SlideGrid, Tokens, bind_keys};

fn value<T: std::str::FromStr>(name: &str) -> Option<T> {
    let mut arguments = std::env::args();
    arguments.find(|a| a == name)?;
    arguments.next()?.parse().ok()
}

fn flag(name: &str) -> bool {
    std::env::args().any(|a| a == name)
}

/// Starts the engine with no outputs, for its preview only. `None` is a usable state.
fn start_engine(engine: &PathBuf, shm: &std::path::Path) -> Option<(Child, Arc<Stream>)> {
    let socket = format!("kairo-ui-{}.sock", std::process::id());
    let child = Process::new(engine)
        .args(["--socket", &socket, "--shm"])
        .arg(shm)
        .spawn()
        .ok()?;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if let Some(stream) = socket
            .as_str()
            .to_ns_name::<GenericNamespaced>()
            .ok()
            .and_then(|name| Stream::connect(name).ok())
        {
            let stream = Arc::new(stream);
            let reader = stream.clone();
            std::thread::spawn(move || while read_message::<Event>(&mut &*reader).is_ok() {});
            write_message(
                &mut &*stream,
                &Command::Configure {
                    outputs: Vec::new(),
                    preview_fps: 15,
                },
            )
            .ok()?;
            return Some((child, stream));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    None
}

struct Preview {
    shm: Option<PreviewShm>,
    last: u64,
    current: Option<Arc<RenderImage>>,
    retired: Option<Arc<RenderImage>>,
    tokens: Tokens,
}

impl Preview {
    fn poll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((frame, pixels)) = self.shm.as_ref().and_then(|shm| shm.read_new(self.last))
        else {
            return;
        };
        self.last = frame;
        if let Some(old) = self.retired.take() {
            let _ = window.drop_image(old);
        }
        if let Some(buffer) = RgbaImage::from_raw(PREVIEW_WIDTH, PREVIEW_HEIGHT, pixels) {
            self.retired = self.current.take();
            self.current = Some(Arc::new(RenderImage::new([Frame::new(buffer)])));
            cx.notify();
        }
    }
}

impl Render for Preview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let tokens = self.tokens;
        let body: AnyElement = match self.current.clone() {
            Some(image) => img(image).w_full().h(px(180.)).into_any_element(),
            None => div()
                .text_sm()
                .text_color(tokens.muted)
                .child("Preview unavailable")
                .into_any_element(),
        };
        div()
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .text_color(tokens.text)
            .child(div().text_sm().child("Live"))
            .child(body)
    }
}

#[derive(Default)]
struct Measure {
    last: Option<Instant>,
    intervals: Vec<f64>,
    direction: f32,
    begun: Option<Instant>,
}

fn report(name: &str, mut samples: Vec<f64>) {
    samples.sort_by(f64::total_cmp);
    if samples.is_empty() {
        println!("{name}: no samples");
        return;
    }
    let at = |q: f64| samples[((samples.len() as f64 * q) as usize).min(samples.len() - 1)];
    println!(
        "{name}: n={} p50={:.2} p95={:.2} p99={:.2} max={:.2} over8.4={} over16={}",
        samples.len(),
        at(0.5),
        at(0.95),
        at(0.99),
        samples[samples.len() - 1],
        samples.iter().filter(|v| **v > 8.4).count(),
        samples.iter().filter(|v| **v > 16.0).count()
    );
}

fn drive(
    grid: Entity<SlideGrid>,
    state: Rc<RefCell<Measure>>,
    seconds: f32,
    window: &mut Window,
    cx: &mut App,
) {
    let now = Instant::now();
    let done = {
        let mut measure = state.borrow_mut();
        let begun = *measure.begun.get_or_insert(now);
        if let Some(last) = measure.last {
            measure
                .intervals
                .push(now.duration_since(last).as_secs_f64() * 1000.0);
        }
        measure.last = Some(now);
        begun.elapsed().as_secs_f32() >= seconds
    };
    if done {
        println!("window active at end: {}", window.is_window_active());
        let snapshot = window.frame_duration_snapshot();
        let ms = |v: u64| v as f64 / 1e6;
        let draw = &snapshot.draw_duration_histogram;
        println!(
            "gpui draw: n={} p50={:.2} p99={:.2} max={:.2}",
            draw.len(),
            ms(draw.value_at_quantile(0.5)),
            ms(draw.value_at_quantile(0.99)),
            ms(draw.max())
        );
        let present = &snapshot.present_interval_histogram;
        println!(
            "gpui present interval: n={} p50={:.2} p99={:.2} max={:.2} over16={}",
            present.len(),
            ms(present.value_at_quantile(0.5)),
            ms(present.value_at_quantile(0.99)),
            ms(present.max()),
            present.count_between(16_000_000, present.max().max(16_000_000))
        );
        report("on_next_frame interval", state.borrow().intervals.clone());
        println!("thumbnail cache: {}", grid.read(cx).thumbnail_count());
        cx.quit();
        return;
    }
    // Moves at a wall-clock rate, about 2,400 px/s, reversing at the ends.
    grid.update(cx, |grid, cx| {
        let handle = grid.scroll_handle().0.borrow();
        let base = &handle.base_handle;
        let mut measure = state.borrow_mut();
        if measure.direction == 0.0 {
            measure.direction = -1.0;
        }
        let dt = measure.intervals.last().copied().unwrap_or(8.3) as f32 / 1000.0;
        let max = f32::from(base.max_offset().y);
        let mut y = f32::from(base.offset().y) + measure.direction * 2400.0 * dt;
        if y < -max || y > 0.0 {
            measure.direction = -measure.direction;
            y = y.clamp(-max, 0.0);
        }
        let mut offset = base.offset();
        offset.y = px(y);
        base.set_offset(offset);
        drop(handle);
        cx.notify();
    });
    window.on_next_frame(move |window, cx| drive(grid, state, seconds, window, cx));
}

fn main() {
    let dark = !flag("--light");
    let cards: u32 = value("--cards").unwrap_or(5000);
    let measure: Option<f32> = value("--measure-secs");
    let shm_path = std::env::temp_dir().join(format!("kairo-ui-{}.shm", std::process::id()));
    let engine = value::<PathBuf>("--engine");
    let engine_state = Arc::new(Mutex::new(None::<(Child, Arc<Stream>)>));
    let application = application().with_assets(assets::Assets);
    let incoming = Arc::new(Mutex::new(Vec::<String>::new()));
    {
        // macOS can deliver the file that launched the app before the window exists.
        let incoming = incoming.clone();
        application.on_open_urls(move |urls| {
            println!("open_urls {urls:?}");
            let paths = model::kairo_paths(&urls, |p| std::path::Path::new(p).exists());
            println!("kairo files {paths:?}");
            incoming.lock().unwrap().extend(paths);
        });
    }
    let held = engine_state.clone();
    application.run(move |cx| {
        init(cx);
        bind_keys(cx);
        cx.set_window_appearance(Some(if dark {
            WindowAppearance::Dark
        } else {
            WindowAppearance::Light
        }));
        component::Theme::change(
            if dark {
                component::ThemeMode::Dark
            } else {
                component::ThemeMode::Light
            },
            None,
            cx,
        );
        let shm = PreviewShm::create(&shm_path, PREVIEW_WIDTH, PREVIEW_HEIGHT).ok();
        if let Some(engine) = engine.as_ref()
            && shm.is_some()
        {
            *held.lock().unwrap() = start_engine(engine, &shm_path);
        }
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1280.), px(800.)),
                cx,
            ))),
            window_min_size: Some(size(px(960.), px(600.))),
            titlebar: Some(TitlebarOptions {
                title: Some("Kairo".into()),
                appears_transparent: true,
                traffic_light_position: Some(point(px(18.), px(24.))),
            }),
            window_background: WindowBackgroundAppearance::Blurred,
            low_power_frame_interval: if flag("--uncapped") {
                None
            } else {
                WindowOptions::default().low_power_frame_interval
            },
            ..Default::default()
        };
        let began = Instant::now();
        open_window(options, cx, move |window, cx| {
            let grid = cx.new(|cx| SlideGrid::new(cards, dark, cx));
            let preview = cx.new(|_| Preview {
                shm,
                last: 0,
                current: None,
                retired: None,
                tokens: Tokens::new(dark),
            });
            let handle = preview.downgrade();
            window
                .spawn(cx, async move |cx| {
                    while handle
                        .update_in(cx, |view, window, cx| view.poll(window, cx))
                        .is_ok()
                    {
                        cx.background_executor()
                            .timer(Duration::from_millis(33))
                            .await;
                    }
                })
                .detach();
            let focus = grid.read(cx).focus_handle().clone();
            focus.focus(window, cx);
            let driver = grid.clone();
            window.on_next_frame(move |window, cx| {
                println!(
                    "open to first frame: {:.2} ms",
                    began.elapsed().as_secs_f64() * 1000.0
                );
                if let Some(seconds) = measure {
                    // Starts the HUD's tracing via the frame snapshot after a settle.
                    let state = Rc::new(RefCell::new(Measure::default()));
                    window.reset_debug_frame_overlay_stats();
                    drive(driver, state, seconds, window, cx);
                }
            });
            cx.new(|cx| Shell::new(grid, Some(preview.into()), dark, window, cx))
        })
        .expect("window");
        cx.activate(true);
        let held = held.clone();
        let path = shm_path.clone();
        cx.on_app_quit(move |_| {
            if let Some((mut child, stream)) = held.lock().unwrap().take() {
                let _ = write_message(&mut &*stream, &Command::Shutdown);
                let _ = child.wait();
            }
            let _ = std::fs::remove_file(&path);
            async {}
        })
        .detach();
    });
}
