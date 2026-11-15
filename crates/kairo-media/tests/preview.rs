//! The dock's in-process preview decoder over a real clip (made with the
//! `ffmpeg` command, as the poster tests do).

use kairo_media::preview::PreviewStream;
use std::process::Command;

#[test]
fn frames_arrive_in_order_scaled_and_seek_moves_the_playhead() {
    let dir = tempfile::tempdir().unwrap();
    let clip = dir.path().join("clip.mp4");
    let status = Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=s=1280x720:r=30:d=2",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&clip)
        .status()
        .expect("ffmpeg on PATH");
    assert!(status.success());
    let mut stream = PreviewStream::open(&clip, 320).unwrap();
    assert!((stream.interval() - 1.0 / 30.0).abs() < 1e-6);
    assert!(stream.duration().is_some_and(|d| (d - 2.0).abs() < 0.1));
    let first = stream.next_frame().unwrap().unwrap();
    assert_eq!((first.frame.width, first.frame.height), (320, 180));
    assert_eq!(first.frame.pixels.len(), 320 * 180 * 4);
    let second = stream.next_frame().unwrap().unwrap();
    assert!(second.seconds > first.seconds);
    stream.seek(1.5).unwrap();
    let mut after = stream.next_frame().unwrap().unwrap();
    while after.seconds + 1e-6 < 1.5 {
        after = stream.next_frame().unwrap().unwrap();
    }
    assert!(after.seconds < 1.6, "{}", after.seconds);
    let mut n = 0;
    while stream.next_frame().unwrap().is_some() {
        n += 1;
    }
    assert!(n < 20, "ends after the last frame");
    assert!(PreviewStream::open(&dir.path().join("missing.mp4"), 320).is_err());
}
