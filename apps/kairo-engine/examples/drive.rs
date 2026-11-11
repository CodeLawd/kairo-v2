//! Drives a real `kairo-engine` through `kairo-core`'s supervisor, for
//! checking output by eye.
//!
//! ```text
//! cargo run -p kairo-engine --example drive -- [--display N] [--full] [--secs S]
//!     [--kill-after S] [--fade MS] [--frame-height H]
//! ```
//!
//! Opens one output on display N (default 0): a rehearsal window, or the
//! whole display with `--full`. Shows a red slide; with `--fade` it then
//! crossfades to blue over MS milliseconds two seconds later. Logs every
//! engine event, optionally SIGKILLs the engine once to show the relaunch,
//! then shuts down after S seconds.

use kairo_core::engine::*;
use kairo_engine_proto::*;
use std::time::{Duration, Instant};

fn value<T: std::str::FromStr>(name: &str) -> Option<T> {
    let mut args = std::env::args();
    args.find(|a| a == name)?;
    args.next()?.parse().ok()
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    tracing_subscriber::fmt().with_target(true).init();
    let index: usize = value("--display").unwrap_or(0);
    let secs: u64 = value("--secs").unwrap_or(10);
    // Killing the engine is Unix-only (SIGKILL).
    #[cfg_attr(not(unix), allow(unused_variables))]
    let kill_after: Option<u64> = value("--kill-after");
    let full = std::env::args().any(|a| a == "--full");
    let fade: Option<f64> = value("--fade");
    let frame_height: u32 = value("--frame-height").unwrap_or(1080);

    let binary = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("kairo-engine");
    let engine = EngineHandle::spawn(EngineOptions::new(binary));
    let mut events = engine.subscribe();
    let start = Instant::now();
    let display = loop {
        if let Some(display) = engine.displays().get(index).cloned() {
            break display;
        }
        if let EngineStatus::Failed { reason } = engine.status() {
            panic!("engine failed: {reason}");
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "no display {index}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    println!("display {index}: {display:?}");
    engine.command(Command::OpenSurface(SurfaceSpec {
        output: "main".into(),
        label: "Kairo output".into(),
        sink: SinkSpec::Display(DisplaySink {
            binding: DisplayBinding {
                display_id: Some(display.id),
                label: display.label.clone(),
                size: Some((display.bounds.width, display.bounds.height)),
            },
            rehearsal: !full,
            frame_height,
        }),
    }));
    let slide = |color: &str| SlideContent {
        reference: "John 3:16".into(),
        text: "For God so loved the world".into(),
        theme: Json(format!(
            r#"{{"background":{{"type":"color","color":"{color}","opacity":1}}}}"#
        )),
        colored_lines: None,
    };
    if let Some(ms) = fade {
        engine.command(Command::Configure {
            output: "main".into(),
            config: SurfaceConfig {
                transition_ms: ms,
                ..SurfaceConfig::default()
            },
        });
    }
    let sent = engine.command(Command::ShowSlide {
        output: "main".into(),
        source: "main".into(),
        slide: slide("#C81E1E"),
        instant: true,
    });
    println!(
        "slide accepted={} frame within 2 s: {}",
        sent.accepted,
        engine
            .wait_frame("main", sent.seq, Duration::from_secs(2))
            .await
    );

    let mut faded = fade.is_none();
    #[cfg_attr(not(unix), allow(unused_mut, unused_variables))]
    let mut killed = false;
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        #[cfg(unix)]
        if let Some(after) = kill_after
            && !killed
            && start.elapsed() >= Duration::from_secs(after)
            && let Some(pid) = engine.pid()
        {
            killed = true;
            println!("SIGKILL engine {pid}");
            // SAFETY: the engine this example launched.
            unsafe { libc::kill(pid as i32, libc::SIGKILL) };
        }
        if !faded && start.elapsed() >= Duration::from_secs(3) {
            faded = true;
            println!("{:>8.3}s crossfade to blue", start.elapsed().as_secs_f64());
            engine.command(Command::ShowSlide {
                output: "main".into(),
                source: "main".into(),
                slide: slide("#1E3CC8"),
                instant: false,
            });
        }
        match tokio::time::timeout(Duration::from_millis(50), events.recv()).await {
            Ok(Ok(event)) => println!("{:>8.3}s {event:?}", start.elapsed().as_secs_f64()),
            Ok(Err(error)) => println!("events: {error}"),
            Err(_) => {}
        }
    }
    engine.shutdown();
    println!("status {:?}", engine.status());
}
