//! Plays looping videos through the real compositor on offscreen outputs
//! and samples this process's CPU and resident memory, for the prompt 055
//! performance figures.
//!
//! ```text
//! cargo run --release -p kairo-video --example soak -- FILE [--streams N]
//!     [--secs S] [--warmup S] [--every S] [--software] [--decode-only]
//! ```
//!
//! Each stream is its own source (a different cue) on its own 1080p
//! offscreen output. After `--warmup` seconds (default 15) it prints one
//! line every `--every` seconds (default 1): elapsed time, CPU percent of
//! one core over the interval, RSS, and frames shown and dropped so far.

use kairo_compositor::thread::{Compositor, CompositorEvent, Op, Sink};
use kairo_compositor::{Content, Fill, MediaParams, MediaUse, Slot};
use kairo_video::{Feed, Videos};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn value<T: std::str::FromStr>(name: &str) -> Option<T> {
    let mut args = std::env::args();
    args.find(|a| a == name)?;
    args.next()?.parse().ok()
}

struct CompositorFeed(kairo_compositor::thread::SourceFeed);

/// `--decode-only`: frames are decoded and timed but not drawn.
struct Discard;

impl Feed for Discard {
    fn frame(&self, _: u64, _: Arc<kairo_compositor::YuvFrame>) {}
    fn remove(&self, _: u64) {}
}

impl Feed for CompositorFeed {
    fn frame(&self, id: u64, frame: Arc<kairo_compositor::YuvFrame>) {
        self.0.video_frame(id, frame);
    }
    fn remove(&self, id: u64) {
        self.0.remove(id);
    }
}

fn cpu_seconds() -> f64 {
    // SAFETY: getrusage fills the struct it is given.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    let t = |v: libc::timeval| v.tv_sec as f64 + v.tv_usec as f64 / 1e6;
    t(usage.ru_utime) + t(usage.ru_stime)
}

fn rss_kib() -> u64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .expect("ps");
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .unwrap_or(0)
}

fn main() {
    let file = std::env::args().nth(1).expect("a video file");
    let streams: u64 = value("--streams").unwrap_or(1);
    let secs: u64 = value("--secs").unwrap_or(75);
    let warmup: u64 = value("--warmup").unwrap_or(15);
    let every: u64 = value("--every").unwrap_or(1);
    let compositor = Compositor::start(|event| {
        if let CompositorEvent::Error { message, .. } | CompositorEvent::DeviceLost(message) = event
        {
            eprintln!("compositor: {message}");
        }
    })
    .expect("a GPU");
    let feed: Arc<dyn Feed> = if std::env::args().any(|a| a == "--decode-only") {
        Arc::new(Discard)
    } else {
        Arc::new(CompositorFeed(compositor.feed()))
    };
    let mut videos = Videos::new(feed, |_, path, message| {
        eprintln!("video {path}: {message}");
    });
    if std::env::args().any(|a| a == "--software") {
        videos.set_hardware(false);
    }
    for i in 0..streams {
        let output = format!("out{i}");
        let id = 1000 + i;
        let params = MediaParams {
            path: file.as_str().into(),
            looped: true,
            start_seconds: None,
            end_seconds: None,
            rate: 1.0,
            element: false,
        };
        compositor.open(&output, 1080, Sink::Offscreen);
        compositor.apply(
            &output,
            1,
            vec![Op::Keyed {
                slot: Slot::Background,
                content: Some((
                    Content {
                        media: Some(params.clone()),
                        ..Content::new(Fill::Source {
                            id,
                            backdrop: [0.0; 4],
                        })
                    },
                    format!("VIDEO:{file}:{i}"),
                )),
                ms: 0,
            }],
        );
        videos.sync(
            &output,
            vec![MediaUse {
                node: i,
                id,
                params,
                transport: true,
            }],
        );
    }
    std::thread::sleep(Duration::from_secs(1));
    for i in 0..streams {
        println!(
            "stream {i}: {:?}",
            videos.player(1000 + i).and_then(|p| p.backend())
        );
    }
    std::thread::sleep(Duration::from_secs(warmup.saturating_sub(1)));
    let start = Instant::now();
    let mut last = (Instant::now(), cpu_seconds());
    println!("t_s cpu_pct rss_kib shown dropped");
    while start.elapsed() < Duration::from_secs(secs) {
        std::thread::sleep(Duration::from_secs(every));
        let (now, cpu) = (Instant::now(), cpu_seconds());
        let pct = (cpu - last.1) / now.duration_since(last.0).as_secs_f64() * 100.0;
        last = (now, cpu);
        let (shown, dropped) = (0..streams)
            .filter_map(|i| videos.player(1000 + i).map(|p| p.counts()))
            .fold((0, 0), |a, c| (a.0 + c.0, a.1 + c.1));
        println!(
            "{:.0} {pct:.2} {} {shown} {dropped}",
            start.elapsed().as_secs_f64(),
            rss_kib()
        );
    }
    for i in 0..streams {
        if let Some(s) = videos.status(1000 + i) {
            println!("stream {i}: {s:?}");
        }
    }
    videos.shutdown();
}
