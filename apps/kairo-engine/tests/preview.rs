//! The operator preview on the real engine (prompt 066): frames reach the
//! map only while a view holds it, show what the output drew, stop when it
//! lets go, and resume after the engine is killed and relaunched.
//!
//! Opens one rehearsal window on the first display. Skips where no window
//! system is available unless `KAIRO_REQUIRE_ENGINE` is set.
#![cfg(unix)]

use kairo_core::engine::*;
use kairo_core::preview::PreviewHub;
use kairo_engine_proto::*;
use std::sync::Arc;
use std::time::{Duration, Instant};

const ENGINE: &str = env!("CARGO_BIN_EXE_kairo-engine");

async fn until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn slide(color: &str) -> Command {
    Command::ShowSlide {
        output: "screen".into(),
        source: "screen".into(),
        slide: SlideContent {
            reference: String::new(),
            text: String::new(),
            theme: Json(format!(
                r#"{{"background":{{"type":"color","color":"{color}"}}}}"#
            )),
            colored_lines: None,
        },
        instant: true,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_preview_follows_the_view_and_survives_an_engine_crash() {
    let mut options = EngineOptions::new(ENGINE);
    options.grace = Duration::from_secs(2);
    let engine = EngineHandle::spawn(options);
    let mut status = engine.watch_status();
    loop {
        match status.borrow_and_update().clone() {
            EngineStatus::Running { .. } => break,
            EngineStatus::Restarting { reason } | EngineStatus::Failed { reason }
                if reason.contains("exit status: 70")
                    && std::env::var_os("KAIRO_REQUIRE_ENGINE").is_none() =>
            {
                eprintln!("skipped: no window system ({reason})");
                return;
            }
            EngineStatus::Failed { reason } => panic!("{reason}"),
            _ => {}
        }
        tokio::time::timeout(Duration::from_secs(10), status.changed())
            .await
            .expect("engine start")
            .unwrap();
    }
    until("displays", || !engine.displays().is_empty()).await;
    let display = engine.displays()[0].clone();
    engine.report_control_display(Some(display.native_id));
    engine.command(Command::OpenSurface(SurfaceSpec {
        output: "screen".into(),
        label: "Preview test".into(),
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
    until("screen ready", || {
        engine.surface_status("screen") == Some(SurfaceStatus::Ready)
    })
    .await;
    engine.command(slide("#C81E1E"));

    let hub = PreviewHub::new(Arc::new(engine.clone()) as Arc<dyn OutputEngine>).unwrap();
    let mut sub = hub.subscribe("screen", (640, 360), 30).unwrap();
    let mut buffer = Vec::new();
    let mut read = |sub: &kairo_core::preview::PreviewSubscription, seen: u64| {
        sub.read_into(seen, &mut buffer)
            .map(|(n, w, h)| (n, w, h, buffer[..4].to_vec()))
    };
    // The current picture arrives at once, at the view's size, as BGRA.
    until("first preview frame", || sub.published() > 0).await;
    let (n, w, h, px) = read(&sub, 0).unwrap();
    assert_eq!((w, h), (640, 360));
    assert_eq!(px, [30, 30, 200, 255], "#C81E1E as straight BGRA");

    // A new slide reaches it; a still program sends nothing more.
    engine.command(slide("#1E3CC8"));
    until("blue", || {
        read(&sub, n).is_some_and(|(_, _, _, px)| px == [200, 60, 30, 255])
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let still = sub.published();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(sub.published(), still, "a still program publishes nothing");

    // Rate: a stream of slide changes at about 120 a second gives at most
    // 30 frames a second, plus the trailing one.
    let before = sub.published();
    let started = Instant::now();
    for i in 0..120 {
        engine.command(slide(if i % 2 == 0 { "#C81E1E" } else { "#1E3CC8" }));
        tokio::time::sleep(Duration::from_millis(8)).await;
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    let secs = started.elapsed().as_secs_f64();
    let frames = sub.published() - before;
    eprintln!("{frames} preview frames in {secs:.2} s of changes");
    assert!(
        frames as f64 <= 30.0 * secs + 3.0,
        "{frames} in {secs:.2} s"
    );
    assert!(frames >= 5);

    // The engine is killed: the supervisor replays the surface with its
    // preview, and frames resume into the same map.
    let pid = engine.pid().expect("running");
    assert_eq!(unsafe { libc::kill(pid as i32, libc::SIGKILL) }, 0);
    until("relaunch", || engine.pid().is_some_and(|p| p != pid)).await;
    let before = sub.published();
    engine.command(slide("#C81E1E"));
    until("frames after relaunch", || sub.published() > before).await;
    sub.ensure();

    // The view lets go: the engine stops publishing.
    drop(sub);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(engine.surface_state("screen").unwrap().preview.is_none());
    assert!(hub.active().is_empty());
    engine.command(Command::CloseSurface {
        output: "screen".into(),
        destroy: true,
    });
    engine.shutdown();
}
