//! Background video through the real engine: `__videoEnded`, the transport
//! and the `mediaCue` rules end to end, as the lobby player and the media
//! dock drive them.

use kairo_core::engine::*;
use kairo_domain::overlay::OverlayBackgroundType;
use kairo_domain::overlay_defaults::DEFAULT_OVERLAY_THEME;
use kairo_engine_proto::*;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const ENGINE: &str = env!("CARGO_BIN_EXE_kairo-engine");

fn clip(dir: &Path, seconds: f64) -> PathBuf {
    let path = dir.join(format!("bg-{seconds}.mp4"));
    let status = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!("testsrc2=s=640x360:r=30:d={seconds}"))
        .args(["-pix_fmt", "yuv420p", "-g", "30"])
        .arg(&path)
        .status()
        .expect("the ffmpeg command makes the fixture");
    assert!(status.success());
    path
}

fn slide(path: &Path, cue: Option<f64>, rate: f64, text: &str) -> SlideContent {
    let mut theme = DEFAULT_OVERLAY_THEME.clone();
    theme.background.kind = OverlayBackgroundType::Video;
    theme.background.media_path = Some(path.to_string_lossy().into_owned());
    theme.background.media_loop = Some(false);
    theme.background.media_rate = Some(rate);
    theme.background.media_cue = cue;
    theme.background.opacity = 1.0;
    SlideContent {
        reference: String::new(),
        text: text.into(),
        theme: Json::encode(&theme),
        colored_lines: None,
    }
}

async fn ended(engine: &EngineHandle) -> bool {
    match engine
        .query(Query::VideoEnded {
            output: "main".into(),
        })
        .await
    {
        Some(Reply::VideoEnded(e)) => e,
        other => panic!("{other:?}"),
    }
}

async fn time(engine: &EngineHandle) -> Option<VideoTime> {
    match engine
        .query(Query::VideoTime {
            output: "main".into(),
        })
        .await
    {
        Some(Reply::VideoTime(t)) => t,
        other => panic!("{other:?}"),
    }
}

async fn becomes(engine: &EngineHandle, want: bool, within: Duration, what: &str) {
    let deadline = Instant::now() + within;
    while ended(engine).await != want {
        assert!(
            Instant::now() < deadline,
            "{what}: ended never became {want}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn background_video_transport_and_ended() {
    let dir = tempfile::tempdir().unwrap();
    let short = clip(dir.path(), 1.0);
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
    let deadline = Instant::now() + Duration::from_secs(10);
    while engine.displays().is_empty() {
        assert!(Instant::now() < deadline, "no displays");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let display = engine.displays()[0].clone();
    engine.command(Command::OpenSurface(SurfaceSpec {
        output: "main".into(),
        label: "Kairo video test".into(),
        sink: SinkSpec::Display(DisplaySink {
            binding: DisplayBinding {
                display_id: Some(display.id),
                label: display.label.clone(),
                size: Some((display.bounds.width, display.bounds.height)),
            },
            rehearsal: true,
            frame_height: 360,
        }),
    }));
    // Nothing shown: no video counts as ended, and there is no clock.
    assert!(ended(&engine).await);
    assert_eq!(time(&engine).await, None);

    let show = |s: SlideContent| {
        engine.command(Command::ShowSlide {
            output: "main".into(),
            source: "main".into(),
            slide: s,
            instant: true,
        })
    };
    let transport = |t: VideoTransport| {
        engine.command(Command::VideoTransport {
            output: "main".into(),
            transport: t,
        })
    };

    // A one-second clip without loop plays, then ends (the lobby moves on).
    show(slide(&short, None, 1.0, "one"));
    becomes(&engine, false, Duration::from_secs(3), "playing").await;
    becomes(&engine, true, Duration::from_secs(4), "finished").await;
    // The operator's transport reads the same clock: the end of the range.
    let at_end = time(&engine).await.expect("a background video has a clock");
    assert!(at_end.ended);
    assert!((at_end.duration - 1.0).abs() < 0.05, "{at_end:?}");
    assert!(
        (at_end.current_time - at_end.duration).abs() < 0.05,
        "{at_end:?}"
    );

    // The lobby's repeat of the same item: seek(0) and resume.
    transport(VideoTransport::Seek(0.0));
    transport(VideoTransport::Pause(false));
    becomes(&engine, false, Duration::from_secs(2), "replaying").await;
    // Paused mid-way is not ended; resuming carries on and ends.
    transport(VideoTransport::Pause(true));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(!ended(&engine).await, "paused part-way");
    let paused = time(&engine).await.unwrap();
    assert!(
        !paused.ended && paused.current_time < paused.duration,
        "{paused:?}"
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        time(&engine).await.unwrap().current_time,
        paused.current_time,
        "a paused clock holds"
    );
    transport(VideoTransport::Pause(false));
    becomes(&engine, true, Duration::from_secs(4), "finished again").await;

    // Resuming from the end rewinds.
    transport(VideoTransport::Pause(false));
    becomes(&engine, false, Duration::from_secs(2), "rewound").await;
    becomes(&engine, true, Duration::from_secs(4), "third end").await;

    // Turning loop on for the finished video restarts it (`__patchBackground`).
    let mut playback = serde_json::json!({
        "loop": true, "hue": 0, "saturation": 1, "brightness": 1, "contrast": 1
    });
    engine.command(Command::PatchBackground {
        output: "main".into(),
        playback: Json::from_value(&playback),
        filter: String::new(),
    });
    becomes(&engine, false, Duration::from_secs(2), "loop on").await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(!ended(&engine).await, "loops past its end");
    // A slider move with loop still on: keeps playing.
    playback["hue"] = 40.into();
    engine.command(Command::PatchBackground {
        output: "main".into(),
        playback: Json::from_value(&playback),
        filter: "hue-rotate(40deg)".into(),
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!ended(&engine).await);

    // Seeking to the end while paused is ended; a seek that is not finite
    // is ignored.
    transport(VideoTransport::Pause(true));
    transport(VideoTransport::Seek(99.0));
    becomes(&engine, true, Duration::from_secs(2), "paused at the end").await;
    transport(VideoTransport::Seek(f64::NAN));
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(ended(&engine).await);

    // A new cue is a new video that plays from the start, unpaused.
    // Give the test time to observe the transition on a software renderer.
    show(slide(&short, Some(2.0), 0.25, "two"));
    becomes(&engine, false, Duration::from_secs(6), "new cue").await;
    // A text update at the same cue preserves the current transport.
    transport(VideoTransport::Pause(true));
    transport(VideoTransport::Seek(0.6));
    let deadline = Instant::now() + Duration::from_secs(4);
    while (time(&engine).await.unwrap().current_time - 0.6).abs() >= 0.05 {
        assert!(Instant::now() < deadline, "the seek never settled");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    show(slide(&short, Some(2.0), 0.25, "three"));
    tokio::time::sleep(Duration::from_millis(500)).await;
    let paused = time(&engine).await.unwrap();
    assert!(!paused.ended && (paused.current_time - 0.6).abs() < 0.05);
    transport(VideoTransport::Pause(false));
    becomes(&engine, true, Duration::from_secs(4), "the cue's end").await;

    // A file that cannot play counts as ended at once, and is reported.
    let mut events = engine.subscribe();
    let broken = dir.path().join("broken.mp4");
    std::fs::write(&broken, b"not a video").unwrap();
    show(slide(&broken, Some(3.0), 1.0, "four"));
    becomes(&engine, true, Duration::from_secs(2), "broken").await;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match tokio::time::timeout(Duration::from_millis(200), events.recv()).await {
            Ok(Ok(EngineEvent::Engine(Event::Error { message, .. })))
                if message.contains("broken.mp4") =>
            {
                break;
            }
            _ => assert!(
                Instant::now() < deadline,
                "no error event for the broken file"
            ),
        }
    }
    engine.shutdown();
}
