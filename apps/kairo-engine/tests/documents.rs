//! Document pages through the real engine (prompt 058): `showDocument`,
//! `swapDocumentPage` and `preloadDocumentPage`, answered by `PageSwapped`
//! and `FrameCommitted`, as the clicker drives them.

use kairo_core::engine::*;
use kairo_domain::overlay::{MediaFit, OverlayBackgroundType};
use kairo_domain::overlay_defaults::DEFAULT_OVERLAY_THEME;
use kairo_engine_proto::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::sync::broadcast::Receiver;

const ENGINE: &str = env!("CARGO_BIN_EXE_kairo-engine");
const DECK: &str = "deck";

fn page(dir: &Path, name: &str, color: &str) -> String {
    let path: PathBuf = dir.join(format!("{name}.png"));
    let status = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!("color=c={color}:s=1920x1080"))
        .args(["-frames:v", "1"])
        .arg(&path)
        .status()
        .expect("the ffmpeg command makes the fixture");
    assert!(status.success());
    path.to_string_lossy().into_owned()
}

/// The program service's document slide: the page as a contain-fit
/// picture, with a blur the engine must drop.
fn document(path: &str) -> SlideContent {
    let mut theme = DEFAULT_OVERLAY_THEME.clone();
    theme.background.kind = OverlayBackgroundType::Image;
    theme.background.media_path = Some(path.to_owned());
    theme.background.media_fit = Some(MediaFit::Contain);
    theme.background.blur_px = Some(24.0);
    theme.background.opacity = 1.0;
    SlideContent {
        reference: String::new(),
        text: String::new(),
        theme: Json::encode(&theme),
        colored_lines: None,
    }
}

async fn start() -> Option<EngineHandle> {
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
                return None;
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
        label: "Kairo document test".into(),
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
    Some(engine)
}

/// What the engine said about each command: `PageSwapped` and when its
/// frame was committed.
#[derive(Default)]
struct Seen {
    swapped: HashMap<u64, bool>,
    swap_order: Vec<(u64, bool)>,
    committed: HashMap<u64, Instant>,
    newest_committed: u64,
}

impl Seen {
    fn drain(&mut self, events: &mut Receiver<EngineEvent>) {
        while let Ok(event) = events.try_recv() {
            match event {
                EngineEvent::Engine(Event::PageSwapped { seq, swapped, .. }) => {
                    assert!(
                        self.swapped.insert(seq, swapped).is_none(),
                        "one answer per turn"
                    );
                    self.swap_order.push((seq, swapped));
                }
                EngineEvent::Engine(Event::FrameCommitted { seq, .. }) => {
                    self.committed.entry(seq).or_insert_with(Instant::now);
                    self.newest_committed = self.newest_committed.max(seq);
                }
                _ => {}
            }
        }
    }

    async fn wait(
        &mut self,
        events: &mut Receiver<EngineEvent>,
        what: &str,
        mut done: impl FnMut(&Self) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            self.drain(events);
            if done(self) {
                return;
            }
            assert!(Instant::now() < deadline, "{what}");
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    }
}

fn swap(engine: &EngineHandle, path: &str, source: &str) -> Sent {
    engine.command(Command::SwapPage {
        output: "main".into(),
        source: source.into(),
        media_path: path.to_owned(),
        document: None,
    })
}

fn preload(engine: &EngineHandle, path: &str) {
    engine.command(Command::PreloadPage {
        output: "main".into(),
        media_path: path.to_owned(),
    });
}

#[tokio::test(flavor = "multi_thread")]
async fn page_turns_answer_and_never_end_on_a_stale_page() {
    let dir = tempfile::tempdir().unwrap();
    let colors = ["red", "green", "blue", "yellow", "white", "purple"];
    let pages: Vec<String> = colors
        .iter()
        .enumerate()
        .map(|(i, c)| page(dir.path(), &format!("{i}"), c))
        .collect();
    let Some(engine) = start().await else {
        return;
    };
    let mut events = engine.subscribe();
    let mut seen = Seen::default();

    // A turn before any document: the caller does the full push.
    assert!(!swap(&engine, &pages[1], DECK).accepted);

    let shown = engine.command(Command::ShowDocument {
        output: "main".into(),
        source: DECK.into(),
        slide: document(&pages[0]),
        document: None,
    });
    assert!(shown.accepted);
    seen.wait(&mut events, "document shown", |s| {
        s.newest_committed >= shown.seq
    })
    .await;

    // Another output's deck: refused before it reaches the engine.
    assert!(!swap(&engine, &pages[1], "other").accepted);

    // A page that cannot be decoded: answered false, the page stays.
    let broken = dir.path().join("broken.png");
    std::fs::write(&broken, b"not a picture").unwrap();
    let sent = swap(&engine, &broken.to_string_lossy(), DECK);
    assert!(sent.accepted);
    seen.wait(&mut events, "broken page answered", |s| {
        s.swapped.contains_key(&sent.seq)
    })
    .await;
    assert!(!seen.swapped[&sent.seq], "an unreadable page is not shown");

    // A missing file: the same.
    let sent = swap(
        &engine,
        &dir.path().join("gone.png").to_string_lossy(),
        DECK,
    );
    seen.wait(&mut events, "missing page answered", |s| {
        s.swapped.contains_key(&sent.seq)
    })
    .await;
    assert!(!seen.swapped[&sent.seq]);

    // A preloaded page: press to committed frame.
    let mut latencies = Vec::new();
    for (i, path) in pages.iter().enumerate().skip(1) {
        preload(&engine, path);
        tokio::time::sleep(Duration::from_millis(150)).await;
        seen.drain(&mut events);
        let pressed = Instant::now();
        let sent = swap(&engine, path, DECK);
        assert!(sent.accepted);
        seen.wait(&mut events, "preloaded turn committed", |s| {
            s.committed.contains_key(&sent.seq)
        })
        .await;
        assert_eq!(seen.swapped.get(&sent.seq), Some(&true), "page {i}");
        latencies.push(seen.committed[&sent.seq] - pressed);
    }
    latencies.sort();
    eprintln!(
        "preloaded press to FrameCommitted: min {:?}, median {:?}, max {:?}",
        latencies[0],
        latencies[latencies.len() / 2],
        latencies[latencies.len() - 1]
    );

    // A clicker at 20 turns a second, nothing preloaded and every page
    // new to the cache at first: the last page asked for is the one shown.
    let mut last = 0;
    for turn in 0..60 {
        let path = &pages[(turn * 7 + 3) % pages.len()];
        let sent = swap(&engine, path, DECK);
        assert!(sent.accepted);
        last = sent.seq;
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    seen.wait(&mut events, "rapid turns answered", |s| {
        s.swapped.contains_key(&last) && s.newest_committed >= last
    })
    .await;
    assert!(seen.swapped[&last], "the newest turn is shown");
    let final_shown = seen
        .swap_order
        .iter()
        .rev()
        .find(|(_, swapped)| *swapped)
        .map(|(seq, _)| *seq);
    assert_eq!(final_shown, Some(last), "no older page is shown after it");
    // Shown turns are applied in the order they were pressed.
    let shown: Vec<u64> = seen
        .swap_order
        .iter()
        .filter(|(_, s)| *s)
        .map(|(q, _)| *q)
        .collect();
    assert!(shown.windows(2).all(|w| w[0] < w[1]), "{shown:?}");

    // Leaving document mode: a turn is refused again.
    engine.command(Command::ShowSlide {
        output: "main".into(),
        source: DECK.into(),
        slide: document(&pages[0]),
        instant: true,
    });
    assert!(!swap(&engine, &pages[1], DECK).accepted);
}

fn clip(dir: &Path, seconds: f64) -> String {
    let path = dir.join(format!("clip-{seconds}.mp4"));
    let status = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!("testsrc2=s=640x360:r=30:d={seconds}"))
        .args(["-pix_fmt", "yuv420p", "-g", "30"])
        .arg(&path)
        .status()
        .expect("the ffmpeg command makes the fixture");
    assert!(status.success());
    path.to_string_lossy().into_owned()
}

async fn playback(
    engine: &EngineHandle,
) -> Option<kairo_domain::documents::DocumentPlaybackStatus> {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        match engine
            .query(Query::DocumentPlayback {
                output: "main".into(),
            })
            .await
        {
            Some(Reply::DocumentPlayback(status)) => return status.map(|s| s.decode().unwrap()),
            None if Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            other => panic!("document playback query: {other:?}"),
        }
    }
}

async fn until_status(
    engine: &EngineHandle,
    what: &str,
    mut done: impl FnMut(&kairo_domain::documents::DocumentVideoStatus) -> bool,
) -> kairo_domain::documents::DocumentVideoStatus {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let video = playback(engine).await.expect("document mode").videos[0].clone();
        if done(&video) {
            return video;
        }
        assert!(Instant::now() < deadline, "{what}: {video:?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn embedded_videos_wait_hidden_and_follow_their_commands() {
    use kairo_domain::documents::DocumentVideoCommand as C;
    let dir = tempfile::tempdir().unwrap();
    let page0 = page(dir.path(), "p0", "navy");
    let video = clip(dir.path(), 3.0);
    let Some(engine) = start().await else {
        return;
    };
    assert!(playback(&engine).await.is_none(), "no document yet");
    let slide = serde_json::json!({
        "id": "deck-1", "page": 0, "path": page0, "width": 1440, "height": 1080,
        "videos": [
            { "file": video, "box": { "x": 0.1, "y": 0.1, "width": 0.5, "height": 0.5 }, "start": 0.5, "end": 1.5 },
            { "file": dir.path().join("missing.mp4").to_string_lossy(), "box": { "x": 0.6, "y": 0.6, "width": 0.3, "height": 0.3 }, "start": 0 }
        ]
    });
    engine.command(Command::ShowDocument {
        output: "main".into(),
        source: DECK.into(),
        slide: document(&page0),
        document: Some(Json::from_value(&slide)),
    });
    let control = |index: u32, command: C| {
        engine.command(Command::ControlDocumentVideo {
            output: "main".into(),
            id: "deck-1".into(),
            page: 0.0,
            video: index,
            command: Json::encode(&command),
        })
    };

    // Hidden and paused at the clip start, the duration being the clip's.
    let first = until_status(&engine, "known duration and paused", |v| {
        v.duration > 0.0 && v.paused
    })
    .await;
    assert!(first.paused && !first.visible && !first.ended, "{first:?}");
    assert!(first.current_time < 0.1, "{first:?}");
    assert!((first.duration - 1.0).abs() < 0.05, "{first:?}");
    tokio::time::sleep(Duration::from_millis(400)).await;
    let still = until_status(&engine, "waiting", |v| v.paused && !v.visible).await;
    assert!(still.current_time < 0.1 && !still.ended, "waits: {still:?}");

    // A command for another page or a video the slide lacks is refused.
    let wrong = engine.command(Command::ControlDocumentVideo {
        output: "main".into(),
        id: "deck-1".into(),
        page: 3.0,
        video: 0,
        command: Json::encode(&C::Play),
    });
    assert!(!wrong.accepted);
    assert!(!control(5, C::Play).accepted);

    // Play: shown, moving, then stopped at the clip end.
    // The status reports the clip's duration from the slide before the
    // player exists, and a play with no player yet is refused. On a slow
    // machine the player can come after the waits above, so play again
    // until one takes.
    control(0, C::Play);
    let moving = until_status(&engine, "playing", |v| {
        if v.error.as_deref()
            == Some("Video playback could not start. Check the video format and try again.")
        {
            control(0, C::Play);
        }
        v.current_time > 0.2
    })
    .await;
    assert!(moving.visible && !moving.paused, "{moving:?}");
    let done = until_status(&engine, "ended at the clip end", |v| v.ended).await;
    assert!(
        done.current_time <= 1.0 + 0.05,
        "stops at the clip end: {done:?}"
    );

    // Play again after the end starts from the clip start.
    control(0, C::Play);
    let again = until_status(&engine, "replaying", |v| !v.ended && !v.paused).await;
    assert!(again.current_time < 0.5, "{again:?}");
    control(0, C::Pause);
    let paused = until_status(&engine, "paused", |v| v.paused).await;
    assert!(paused.visible, "pause keeps it shown");

    // Seek is relative to the clip start and clamped to the clip.
    control(0, C::Seek { seconds: 0.6 });
    until_status(&engine, "seeked", |v| (v.current_time - 0.6).abs() < 0.05).await;
    control(0, C::Seek { seconds: 99.0 });
    until_status(&engine, "clamped", |v| v.current_time >= 0.95).await;
    control(0, C::Restart);
    until_status(&engine, "restarted", |v| !v.paused && v.current_time < 0.5).await;

    // The missing file: the decode error, and play refused.
    control(1, C::Play);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let broken = playback(&engine).await.unwrap().videos[1].clone();
        if broken.error.is_some() {
            assert_eq!(
                broken.error.as_deref(),
                Some(
                    "This embedded video cannot be played. Convert it to MP4 (H.264/AAC) or WebM and import again."
                )
            );
            assert!(!broken.visible && broken.paused, "{broken:?}");
            break;
        }
        assert!(Instant::now() < deadline, "no error reported: {broken:?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    // A page turn without videos clears them; leaving the document ends it.
    let page1 = page(dir.path(), "p1", "maroon");
    let empty = serde_json::json!({ "id": "deck-1", "page": 1, "path": page1, "width": 1440, "height": 1080, "videos": [] });
    let sent = engine.command(Command::SwapPage {
        output: "main".into(),
        source: DECK.into(),
        media_path: page1.clone(),
        document: Some(Json::from_value(&empty)),
    });
    assert!(sent.accepted);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = playback(&engine).await
            && status.page == 1.0
        {
            assert!(status.videos.is_empty());
            break;
        }
        assert!(Instant::now() < deadline, "videos never followed the turn");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    engine.command(Command::ShowSlide {
        output: "main".into(),
        source: DECK.into(),
        slide: document(&page0),
        instant: true,
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(playback(&engine).await.is_none(), "not a document any more");
}
