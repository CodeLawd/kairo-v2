//! Real files through real players. Each clip is made with the `ffmpeg`
//! command and carries its frame number burned into the picture as ten
//! bars, one bit each (`geq` with `N`), so a test reads back exactly which
//! frame a player showed and when.

use kairo_compositor::{MediaParams, MediaUse, YuvFrame};
use kairo_video::{Backend, Control, Feed, Videos};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const BITS: u32 = 10;

// These cases exercise real-time media on the same host. Run them
// in series so fixture encoding and playback do not contend.
static MEDIA_TEST: Mutex<()> = Mutex::new(());

/// Fixtures are made once per test run.
fn dir() -> &'static Path {
    static DIR: OnceLock<tempfile::TempDir> = OnceLock::new();
    DIR.get_or_init(|| tempfile::tempdir().expect("temp dir"))
        .path()
}

fn has_encoder(name: &str) -> bool {
    Command::new("ffmpeg")
        .args(["-hide_banner", "-encoders"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(name))
        .unwrap_or(false)
}

/// A `seconds` clip at `fps`, 640 × 360, keyframe every second. H.264 when
/// this FFmpeg has libx264, else MPEG-4 part 2.
fn clip(name: &str, fps: u32, seconds: f64, extra: &[&str]) -> PathBuf {
    let path = dir().join(name);
    if path.exists() {
        return path;
    }
    let bars = format!(
        "color=c=black:s=640x360:r={fps}:d={seconds},format=gray,\
         geq=lum='if(mod(floor(N/pow(2,floor(X/64))),2),235,16)',format=yuv420p"
    );
    let codec: &[&str] = if has_encoder("libx264") {
        &["-c:v", "libx264", "-preset", "ultrafast", "-qp", "10"]
    } else {
        &["-c:v", "mpeg4", "-q:v", "2"]
    };
    let gop = fps.to_string();
    let status = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", &bars])
        .args(codec)
        .args(["-g", &gop])
        .args(extra)
        .arg(&path)
        .status()
        .expect("the ffmpeg command makes the fixtures");
    assert!(status.success(), "ffmpeg failed for {name}");
    path
}

/// The frame number burned into `frame`.
fn number(frame: &YuvFrame) -> u32 {
    let y = frame.height as usize / 2;
    (0..BITS)
        .map(|bit| {
            let x = (bit as usize * 64 + 32) * frame.width as usize / 640;
            let row = y * frame.luma_stride as usize;
            let high = if frame.bits > 8 {
                // P010: little-endian, ten bits at the top.
                u16::from_le_bytes([frame.luma[row + x * 2], frame.luma[row + x * 2 + 1]]) >> 8
            } else {
                u16::from(frame.luma[row + x])
            };
            u32::from(high > 128) << bit
        })
        .sum()
}

#[derive(Default)]
struct Capture {
    frames: Mutex<Vec<(u64, Instant, u32)>>,
    removed: Mutex<Vec<u64>>,
}

impl Feed for Capture {
    fn frame(&self, id: u64, frame: Arc<YuvFrame>) {
        self.frames
            .lock()
            .unwrap()
            .push((id, Instant::now(), number(&frame)));
    }
    fn remove(&self, id: u64) {
        self.removed.lock().unwrap().push(id);
    }
}

impl Capture {
    fn of(&self, id: u64) -> Vec<(Instant, u32)> {
        self.frames
            .lock()
            .unwrap()
            .iter()
            .filter(|f| f.0 == id)
            .map(|f| (f.1, f.2))
            .collect()
    }
    fn numbers(&self, id: u64) -> Vec<u32> {
        self.of(id).into_iter().map(|f| f.1).collect()
    }
    fn wait_for(&self, id: u64, n: usize, within: Duration) {
        let deadline = Instant::now() + within;
        while self.of(id).len() < n {
            assert!(
                Instant::now() < deadline,
                "only {} frames of {n}",
                self.of(id).len()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

struct Rig {
    capture: Arc<Capture>,
    videos: Videos,
    errors: Arc<Mutex<Vec<String>>>,
}

fn rig(hardware: bool) -> Rig {
    let capture = Arc::new(Capture::default());
    let errors = Arc::new(Mutex::new(Vec::new()));
    let sink = errors.clone();
    let mut videos = Videos::new(capture.clone(), move |_, path, message| {
        sink.lock().unwrap().push(format!("{path}: {message}"));
    });
    videos.set_hardware(hardware);
    Rig {
        capture,
        videos,
        errors,
    }
}

fn params(path: &Path) -> MediaParams {
    MediaParams {
        path: path.to_string_lossy().as_ref().into(),
        looped: false,
        start_seconds: None,
        end_seconds: None,
        rate: 1.0,
        element: false,
    }
}

fn background(node: u64, id: u64, params: MediaParams) -> MediaUse {
    MediaUse {
        node,
        id,
        params,
        transport: true,
    }
}

fn wait_until(what: &str, within: Duration, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + within;
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn frames_arrive_in_order_on_the_clock() {
    let _media = MEDIA_TEST.lock().unwrap_or_else(|error| error.into_inner());
    let path = clip("clock.mp4", 30, 3.0, &[]);
    let mut r = rig(false);
    r.videos.sync("a", vec![background(1, 10, params(&path))]);
    r.capture.wait_for(10, 60, Duration::from_secs(5));
    let frames = r.capture.of(10);
    let t0 = frames[0].0;
    let mut total = 0.0;
    for (i, &(at, n)) in frames.iter().enumerate().take(60) {
        assert_eq!(n, i as u32, "frames in order with none skipped");
        // Each frame is shown at n / 30 s after the first: within 25 ms
        // each (a busy CI runner can delay a thread's wake-up), and 10 ms on
        // average.
        let late = at.duration_since(t0).as_secs_f64() - f64::from(n) / 30.0;
        assert!(late.abs() < 0.025, "frame {n} {late:+.4} s off the clock");
        total += late.abs();
    }
    let mean = total / 60.0;
    eprintln!("mean distance from the clock {:.2} ms", mean * 1000.0);
    assert!(mean < 0.010, "mean {mean:.4} s off the clock");
    let (_, dropped) = r.videos.player(10).unwrap().counts();
    assert_eq!(dropped, 0);
    assert!(!r.videos.ended("a"));
    r.videos.shutdown();
}

#[test]
fn a_trimmed_loop_wraps_at_its_bounds_without_a_gap() {
    let _media = MEDIA_TEST.lock().unwrap_or_else(|error| error.into_inner());
    let path = clip("trim.mp4", 30, 3.0, &[]);
    let mut r = rig(false);
    let mut p = params(&path);
    p.start_seconds = Some(0.5);
    p.end_seconds = Some(1.5);
    p.looped = true;
    r.videos.sync("a", vec![background(1, 11, p)]);
    r.capture.wait_for(11, 75, Duration::from_secs(6));
    let frames = r.capture.of(11);
    let numbers: Vec<u32> = frames.iter().map(|f| f.1).collect();
    let lap: Vec<u32> = (15..45).collect();
    assert_eq!(
        &numbers[..30],
        &lap[..],
        "trim start 0.5 s is frame 15, end 1.5 s is before 45"
    );
    assert_eq!(&numbers[30..60], &lap[..], "the second lap");
    // The wrap from 44 to 15 is one frame period, like any other.
    let gap = frames[30].0.duration_since(frames[29].0).as_secs_f64();
    eprintln!("wrap gap {:.2} ms", gap * 1000.0);
    assert!((gap - 1.0 / 30.0).abs() < 0.025, "wrap gap {gap:.4} s");
    let worst = frames
        .windows(2)
        .map(|w| w[1].0.duration_since(w[0].0).as_secs_f64())
        .fold(0.0, f64::max);
    eprintln!("worst interval {:.2} ms", worst * 1000.0);
    assert!(worst < 2.0 / 30.0, "worst interval {worst:.4} s");
    r.videos.shutdown();
}

#[test]
fn a_trimmed_range_without_loop_ends_paused_at_its_end_and_resume_rewinds() {
    let _media = MEDIA_TEST.lock().unwrap_or_else(|error| error.into_inner());
    let path = clip("trim.mp4", 30, 3.0, &[]);
    let mut r = rig(false);
    let mut p = params(&path);
    p.start_seconds = Some(1.0);
    p.end_seconds = Some(1.5);
    r.videos.sync("a", vec![background(1, 12, p)]);
    wait_until("the end", Duration::from_secs(4), || r.videos.ended("a"));
    let numbers = r.capture.numbers(12);
    assert_eq!(numbers, (30..45).collect::<Vec<_>>());
    let s = r.videos.status(12).unwrap();
    assert!(s.paused && s.ended && (s.position - 1.5).abs() < 1e-6);
    // Resuming from the end plays the range again from its start.
    r.videos.control(12, Control::Pause(false));
    wait_until("the replay", Duration::from_secs(2), || {
        r.capture.numbers(12).len() > numbers.len()
    });
    assert_eq!(r.capture.numbers(12)[numbers.len()], 30);
    wait_until("the second end", Duration::from_secs(4), || {
        r.videos.ended("a")
    });
    r.videos.shutdown();
}

#[test]
fn seek_lands_on_the_target_frame_and_pause_holds_it() {
    let _media = MEDIA_TEST.lock().unwrap_or_else(|error| error.into_inner());
    let path = clip("clock.mp4", 30, 3.0, &[]);
    let mut r = rig(false);
    r.videos.sync("a", vec![background(1, 13, params(&path))]);
    r.capture.wait_for(13, 3, Duration::from_secs(4));
    r.videos.control(13, Control::Pause(true));
    for (target, frame) in [(2.0, 60), (1.51, 45), (0.0, 0), (2.999, 89)] {
        let before = r.capture.of(13).len();
        r.videos.control(13, Control::Seek(target));
        r.capture.wait_for(13, before + 1, Duration::from_secs(3));
        std::thread::sleep(Duration::from_millis(150));
        let numbers = r.capture.numbers(13);
        assert_eq!(numbers.len(), before + 1, "paused: one frame per seek");
        assert_eq!(*numbers.last().unwrap(), frame, "seek to {target}");
        let s = r.videos.status(13).unwrap();
        assert!((s.position - target).abs() < 1e-6 && s.paused);
    }
    // Paused at the last frame of a clip is not the end (3 s is).
    assert!(!r.videos.ended("a"));
    r.videos.control(13, Control::Seek(99.0));
    wait_until("paused at the end", Duration::from_secs(2), || {
        r.videos.ended("a")
    });
    // Not finite: ignored.
    r.videos.control(13, Control::Seek(f64::NAN));
    std::thread::sleep(Duration::from_millis(50));
    assert!(r.videos.ended("a"));
    r.videos.shutdown();
}

#[test]
fn rate_changes_the_clock() {
    let _media = MEDIA_TEST.lock().unwrap_or_else(|error| error.into_inner());
    let path = clip("clock.mp4", 30, 3.0, &[]);
    let mut r = rig(false);
    let mut p = params(&path);
    p.rate = 2.0;
    r.videos.sync("a", vec![background(1, 14, p)]);
    r.capture.wait_for(14, 40, Duration::from_secs(4));
    let frames = r.capture.of(14);
    let span = frames[39].0.duration_since(frames[0].0).as_secs_f64();
    let shown = f64::from(frames[39].1 - frames[0].1);
    // 2× of 30 fps: 60 source frames a second.
    assert!(
        (shown / span - 60.0).abs() < 4.0,
        "{shown} frames in {span:.3} s"
    );
    r.videos.shutdown();
}

#[test]
fn outputs_at_the_same_cue_share_one_decoder_and_a_new_cue_restarts() {
    let _media = MEDIA_TEST.lock().unwrap_or_else(|error| error.into_inner());
    let path = clip("clock.mp4", 30, 3.0, &[]);
    let mut r = rig(false);
    r.videos.sync("a", vec![background(1, 20, params(&path))]);
    r.capture.wait_for(20, 15, Duration::from_secs(4));
    // Output b shows the same file at the same cue: it joins, nothing restarts.
    r.videos.sync("b", vec![background(7, 20, params(&path))]);
    assert_eq!(r.videos.len(), 1);
    let before = r.capture.numbers(20).len();
    r.capture.wait_for(20, before + 10, Duration::from_secs(2));
    let numbers = r.capture.numbers(20);
    for w in numbers.windows(2) {
        assert_eq!(w[1], w[0] + 1, "one stream, no restart: {numbers:?}");
    }
    // Both report the same transport state.
    assert_eq!(r.videos.background("a"), r.videos.background("b"));
    // The text changed (same node, new settings): still no restart.
    r.videos.sync("a", vec![background(1, 20, params(&path))]);
    // A new cue is a new source on both: it starts from frame 0 while the
    // old one keeps playing as it fades.
    r.videos.sync(
        "a",
        vec![
            background(1, 20, params(&path)),
            background(2, 21, params(&path)),
        ],
    );
    r.videos.sync(
        "b",
        vec![
            background(7, 20, params(&path)),
            background(8, 21, params(&path)),
        ],
    );
    assert_eq!(r.videos.len(), 2);
    r.capture.wait_for(21, 1, Duration::from_secs(2));
    assert_eq!(r.capture.numbers(21)[0], 0);
    // The old one is removed when no output shows it any more.
    r.videos.sync("a", vec![background(2, 21, params(&path))]);
    assert_eq!(r.videos.len(), 2, "b still shows it");
    r.videos.sync("b", vec![background(8, 21, params(&path))]);
    assert_eq!(r.videos.len(), 1);
    wait_until("the texture removed", Duration::from_secs(2), || {
        r.capture.removed.lock().unwrap().contains(&20)
    });
    // A new element for the same id on the same output (cleared and shown
    // again before the old one was gone) restarts it.
    let shown = r.capture.numbers(21).len();
    r.capture.wait_for(21, shown + 20, Duration::from_secs(2));
    r.videos.sync(
        "a",
        vec![
            background(2, 21, params(&path)),
            background(3, 21, params(&path)),
        ],
    );
    let restart = r.capture.numbers(21).len();
    r.capture.wait_for(21, restart + 1, Duration::from_secs(2));
    assert_eq!(r.capture.numbers(21)[restart], 0, "restarted");
    // The replaced player never removes its successor's texture.
    std::thread::sleep(Duration::from_millis(100));
    assert!(!r.capture.removed.lock().unwrap().contains(&21));
    r.videos.shutdown();
}

#[test]
fn loop_turned_on_after_the_end_restarts_and_a_patch_does_not() {
    let _media = MEDIA_TEST.lock().unwrap_or_else(|error| error.into_inner());
    let path = clip("short.mp4", 30, 0.5, &[]);
    let mut r = rig(false);
    r.videos.sync("a", vec![background(1, 30, params(&path))]);
    wait_until("the end", Duration::from_secs(3), || r.videos.ended("a"));
    let n = r.capture.numbers(30).len();
    assert_eq!(n, 15);
    let mut looped = params(&path);
    looped.looped = true;
    r.videos.sync("a", vec![background(1, 30, looped.clone())]);
    r.capture.wait_for(30, n + 20, Duration::from_secs(3));
    assert_eq!(r.capture.numbers(30)[n], 0);
    assert!(!r.videos.ended("a"));
    // The same settings again (a slider moved): nothing restarts.
    let before = r.capture.numbers(30).len();
    r.videos.sync("a", vec![background(1, 30, looped)]);
    r.capture.wait_for(30, before + 3, Duration::from_secs(2));
    let numbers = r.capture.numbers(30);
    assert_eq!(numbers[before], (numbers[before - 1] + 1) % 15);
    r.videos.shutdown();
}

#[test]
fn a_corrupt_or_missing_file_shows_nothing_and_counts_as_ended() {
    let _media = MEDIA_TEST.lock().unwrap_or_else(|error| error.into_inner());
    let corrupt = dir().join("corrupt.mp4");
    std::fs::write(&corrupt, b"this is not a video file at all, just text").unwrap();
    let truncated = dir().join("truncated.mp4");
    let good = std::fs::read(clip("clock.mp4", 30, 3.0, &[])).unwrap();
    std::fs::write(&truncated, &good[..good.len() / 20]).unwrap();
    let audio = dir().join("audio.m4a");
    let status = Command::new("ffmpeg")
        .args([
            "-v", "error", "-y", "-f", "lavfi", "-i", "sine=d=1", "-c:a", "aac",
        ])
        .arg(&audio)
        .status()
        .unwrap();
    assert!(status.success());
    let mut r = rig(true);
    for (i, path) in [
        corrupt.clone(),
        dir().join("missing.mp4"),
        audio,
        dir().to_path_buf(),
    ]
    .iter()
    .enumerate()
    {
        let output = format!("o{i}");
        r.videos.sync(
            &output,
            vec![background(i as u64, 40 + i as u64, params(path))],
        );
        wait_until("an error", Duration::from_secs(3), || {
            r.videos
                .status(40 + i as u64)
                .is_some_and(|s| s.error.is_some())
        });
        assert!(r.videos.ended(&output));
        assert!(r.capture.numbers(40 + i as u64).is_empty(), "{path:?} drew");
    }
    assert_eq!(r.errors.lock().unwrap().len(), 4);
    // A truncated file plays what it has, then ends.
    r.videos
        .sync("t", vec![background(9, 49, params(&truncated))]);
    wait_until("the truncated end", Duration::from_secs(4), || {
        r.videos.ended("t")
    });
    // No video at all is ended too.
    assert!(r.videos.ended("nothing"));
    r.videos.shutdown();
}

#[test]
fn hardware_decode_is_used_when_it_can_and_matches_software() {
    let _media = MEDIA_TEST.lock().unwrap_or_else(|error| error.into_inner());
    let path = clip("clock.mp4", 30, 3.0, &[]);
    let mut r = rig(true);
    r.videos.sync("a", vec![background(1, 50, params(&path))]);
    r.capture.wait_for(50, 10, Duration::from_secs(4));
    let backend = r.videos.player(50).unwrap().backend();
    eprintln!("backend: {backend:?}");
    if cfg!(target_os = "macos") && has_encoder("libx264") {
        assert_eq!(backend, Some(Backend::Hardware("videotoolbox")));
    }
    assert_eq!(r.capture.numbers(50)[..10], (0..10).collect::<Vec<_>>()[..]);
    r.videos.shutdown();
}

#[test]
fn ten_bit_and_alpha_streams_decode() {
    let _media = MEDIA_TEST.lock().unwrap_or_else(|error| error.into_inner());
    let mut r = rig(true);
    if has_encoder("libx265") {
        let path = clip(
            "ten.mov",
            30,
            1.0,
            &[
                "-c:v",
                "libx265",
                "-pix_fmt",
                "yuv420p10le",
                "-tag:v",
                "hvc1",
                "-x265-params",
                "log-level=error",
            ],
        );
        r.videos.sync("a", vec![background(1, 60, params(&path))]);
        r.capture.wait_for(60, 5, Duration::from_secs(4));
        assert_eq!(r.capture.numbers(60)[..5], [0, 1, 2, 3, 4]);
    } else {
        eprintln!("no libx265: 10-bit not tested");
    }
    if has_encoder("libvpx-vp9") {
        let path = dir().join("alpha.webm");
        let status = Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "color=c=red@0.5:s=320x180:r=30:d=0.5,format=yuva420p",
                "-c:v",
                "libvpx-vp9",
                "-pix_fmt",
                "yuva420p",
                "-auto-alt-ref",
                "0",
            ])
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success());
        let alpha = Arc::new(Mutex::new(None));
        struct AlphaFeed(Arc<Mutex<Option<u8>>>);
        impl Feed for AlphaFeed {
            fn frame(&self, _: u64, frame: Arc<YuvFrame>) {
                if let Some(a) = &frame.alpha {
                    *self.0.lock().unwrap() = Some(a[a.len() / 2]);
                }
            }
            fn remove(&self, _: u64) {}
        }
        let mut v = Videos::new(Arc::new(AlphaFeed(alpha.clone())), |_, _, _| {});
        v.sync("a", vec![background(1, 61, params(&path))]);
        wait_until("an alpha frame", Duration::from_secs(4), || {
            alpha.lock().unwrap().is_some()
        });
        let a = alpha.lock().unwrap().unwrap();
        assert!((i32::from(a) - 128).abs() <= 3, "alpha {a}");
        v.shutdown();
    } else {
        eprintln!("no libvpx-vp9: alpha not tested");
    }
    r.videos.shutdown();
}
