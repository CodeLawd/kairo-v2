//! The recorder end to end: frames go in as BGRA bytes, a real movie file is
//! written, and AVFoundation (and ffprobe, when installed) reads it back the
//! way QuickTime or an editor would. Electron's `recorder.test.mjs`, minus the
//! IOSurface path this port does not carry.
#![cfg(target_os = "macos")]

// These cases exercise real-time media on the same host. Run them
// in series so fixture encoding and playback do not contend.
static MEDIA_TEST: std::sync::Mutex<()> = std::sync::Mutex::new(());

use kairo_native_sys::recorder::{self, Open, Pushed, Recorder, probe};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const W: u32 = 320;
const H: u32 = 180;
const WAIT: Duration = Duration::from_secs(10);

fn solid(w: u32, h: u32, bgra: [u8; 4]) -> Vec<u8> {
    bgra.iter()
        .copied()
        .cycle()
        .take((w * h * 4) as usize)
        .collect()
}

fn average(bgra: &[u8]) -> [f64; 4] {
    let mut sum = [0f64; 4];
    for px in bgra.chunks(4) {
        for c in 0..4 {
            sum[c] += f64::from(px[c]);
        }
    }
    let n = (bgra.len() / 4) as f64;
    sum.map(|s| s / n)
}

fn open(dir: &Path, name: &str, codec: &str) -> (PathBuf, Recorder) {
    let path = dir.join(name);
    let rec = Recorder::open(&Open {
        path: path.to_str().unwrap(),
        width: W,
        height: H,
        fps: 30,
        codec,
        bitrate: 2_000_000.0,
        audio_device_id: "",
    })
    .unwrap();
    (path, rec)
}

/// ffprobe's view of the video stream: codec name and pixel format.
fn ffprobe(path: &Path) -> Option<(String, String)> {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=codec_name,pix_fmt",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut parts = text.trim().split(',');
    Some((
        parts.next()?.to_owned(),
        parts.next().unwrap_or_default().to_owned(),
    ))
}

#[test]
fn available_on_this_mac_with_its_audio_inputs() {
    let _media = MEDIA_TEST.lock().unwrap_or_else(|error| error.into_inner());
    assert_eq!(recorder::available(), Ok(()));
    // Listing never fails; it may be empty.
    let _ = recorder::audio_inputs();
}

#[test]
fn a_frame_is_recorded_to_h264_at_its_colour_and_a_still_slide_keeps_the_frame_rate() {
    let _media = MEDIA_TEST.lock().unwrap_or_else(|error| error.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let (path, rec) = open(dir.path(), "h264.mov", "h264");
    assert_eq!(
        rec.push(&solid(W, H, [0, 0, 255, 255]), W, H, (W * 4) as usize),
        Pushed::Taken
    );
    std::thread::sleep(Duration::from_millis(1500));
    let finished = rec.stop(WAIT).unwrap();
    assert!(
        (1.4..2.0).contains(&finished.seconds),
        "ran {}",
        finished.seconds
    );
    let seen = probe(path.to_str().unwrap(), W, H).unwrap();
    assert_eq!(seen.codec, "avc1");
    assert_eq!((seen.width, seen.height), (W, H));
    assert!(
        (seen.seconds - finished.seconds).abs() < 0.15,
        "{} vs {}",
        seen.seconds,
        finished.seconds
    );
    // Shared macOS VMs can miss timer ticks. Still verify that a single
    // submitted image is repeated and that encoding never exceeds its rate.
    let min_frames = if std::env::var("KAIRO_PERF_HOSTED").as_deref() == Ok("1") {
        2
    } else {
        40
    };
    assert!(
        (min_frames..=55).contains(&seen.frames),
        "{} frames for 1.5 s at 30 fps",
        seen.frames
    );
    assert!(!seen.audio);
    let [b, g, r, _] = average(&seen.last);
    assert!(
        r > 200.0 && g < 40.0 && b < 40.0,
        "red holds to the end ({r}, {g}, {b})"
    );
    if let Some((codec, _)) = ffprobe(&path) {
        assert_eq!(codec, "h264");
    }
}

#[test]
fn frames_of_another_size_are_scaled_with_padded_rows() {
    let _media = MEDIA_TEST.lock().unwrap_or_else(|error| error.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let (path, rec) = open(dir.path(), "scaled.mov", "h264");
    // 640 × 360 with 64 bytes of padding per row, as a GPU read-back has.
    let stride = 640 * 4 + 64;
    let mut bytes = vec![0u8; stride * 360];
    for row in bytes.chunks_mut(stride) {
        row[..640 * 4].copy_from_slice(&solid(640, 1, [0, 255, 0, 255]));
    }
    assert_eq!(rec.push(&bytes, 640, 360, stride), Pushed::Taken);
    std::thread::sleep(Duration::from_millis(300));
    rec.stop(WAIT).unwrap();
    let seen = probe(path.to_str().unwrap(), W, H).unwrap();
    assert_eq!((seen.width, seen.height), (W, H));
    let [b, g, r, _] = average(&seen.last);
    assert!(
        g > 200.0 && r < 40.0 && b < 40.0,
        "scaled frame keeps its colour ({r}, {g}, {b})"
    );
}

#[test]
fn prores_4444_keeps_transparency_for_keying() {
    let _media = MEDIA_TEST.lock().unwrap_or_else(|error| error.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let (path, rec) = open(dir.path(), "alpha.mov", "prores4444");
    // Half-transparent red, premultiplied.
    rec.push(&solid(W, H, [0, 0, 128, 128]), W, H, (W * 4) as usize);
    std::thread::sleep(Duration::from_millis(300));
    rec.stop(WAIT).unwrap();
    let seen = probe(path.to_str().unwrap(), W, H).unwrap();
    assert_eq!(seen.codec, "ap4h");
    let [b, g, r, a] = average(&seen.last);
    assert!((a - 128.0).abs() < 6.0, "alpha survives ({a})");
    assert!(
        r > 100.0 && g < 20.0 && b < 20.0,
        "colour survives ({r}, {g}, {b})"
    );
    if let Some((codec, pix)) = ffprobe(&path) {
        assert_eq!(codec, "prores");
        assert!(
            pix.starts_with("yuva444"),
            "ffprobe pixel format {pix} has alpha"
        );
    }
}

#[test]
fn hevc_and_prores_422_encode() {
    let _media = MEDIA_TEST.lock().unwrap_or_else(|error| error.into_inner());
    let dir = tempfile::tempdir().unwrap();
    for (codec, fourcc, ff) in [("hevc", "hvc1", "hevc"), ("prores422", "apcn", "prores")] {
        let (path, rec) = open(dir.path(), &format!("{codec}.mov"), codec);
        rec.push(&solid(W, H, [40, 80, 120, 255]), W, H, (W * 4) as usize);
        std::thread::sleep(Duration::from_millis(200));
        rec.stop(WAIT).unwrap();
        assert_eq!(probe(path.to_str().unwrap(), W, H).unwrap().codec, fourcc);
        assert!(std::fs::metadata(&path).unwrap().len() > 0);
        if let Some((name, _)) = ffprobe(&path) {
            assert_eq!(name, ff);
        }
    }
}

#[test]
fn an_instant_stop_still_writes_the_first_frame_and_stats_count() {
    let _media = MEDIA_TEST.lock().unwrap_or_else(|error| error.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let (_, rec) = open(dir.path(), "instant.mov", "h264");
    let (stats, error) = rec.stats();
    assert_eq!(error, None);
    assert_eq!(stats.failed, 0);
    let finished = rec.stop(WAIT).unwrap();
    assert!(
        finished.frames >= 1,
        "even an instant stop writes the first frame"
    );
}

#[test]
fn bad_input_is_refused_before_a_file_is_made() {
    let _media = MEDIA_TEST.lock().unwrap_or_else(|error| error.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.mov");
    let p = path.to_str().unwrap();
    let base = Open {
        path: p,
        width: W,
        height: H,
        fps: 30,
        codec: "h264",
        bitrate: 0.0,
        audio_device_id: "",
    };
    let err = |o: Open| Recorder::open(&o).err().unwrap();
    assert!(
        err(Open {
            codec: "vp9",
            ..base.clone()
        })
        .contains("unknown codec")
    );
    assert!(
        err(Open {
            width: 321,
            ..base.clone()
        })
        .contains("unsupported size")
    );
    assert!(
        err(Open {
            fps: 0,
            ..base.clone()
        })
        .contains("unsupported frame rate")
    );
    assert!(
        err(Open {
            audio_device_id: "no-such-device",
            ..base.clone()
        })
        .contains("not connected")
    );
    assert!(
        err(Open {
            path: "",
            ..base.clone()
        })
        .contains("path is required")
    );
    assert!(!path.exists());
    // Pixels that do not match their size are refused.
    let (_, rec) = open(dir.path(), "refuse.mov", "h264");
    assert_eq!(rec.push(&[0; 16], W, H, (W * 4) as usize), Pushed::Refused);
    assert_eq!(rec.push(&solid(W, H, [0; 4]), W, H, 8), Pushed::Refused);
    rec.stop(WAIT).unwrap();
}

#[test]
fn a_recorder_dropped_without_stop_abandons_the_file() {
    let _media = MEDIA_TEST.lock().unwrap_or_else(|error| error.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let (path, rec) = open(dir.path(), "dropped.mov", "h264");
    rec.push(&solid(W, H, [0, 0, 255, 255]), W, H, (W * 4) as usize);
    drop(rec);
    // Cancelled writing leaves no movie behind.
    assert!(!path.exists() || probe(path.to_str().unwrap(), W, H).is_none());
}
