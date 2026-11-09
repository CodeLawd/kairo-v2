//! FFmpeg poster frames. Fixtures are made with the `ffmpeg` command, which
//! these tests need on PATH alongside the FFmpeg libraries the crate links.

use kairo_media::poster::{
    POSTER_MAX_WIDTH, PosterError, cache_key, decode_poster, poster_for, prune_cache,
};
use std::path::Path;
use std::process::Command;

/// A clip whose first 0.1 s is red and the rest blue.
fn make_clip(path: &Path, width: u32, seconds: f64) {
    let status = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!("color=c=red:s={width}x180:r=30:d=0.1"))
        .args(["-f", "lavfi", "-i"])
        .arg(format!("color=c=blue:s={width}x180:r=30:d={seconds}"))
        .args([
            "-filter_complex",
            "[0][1]concat=n=2:v=1",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(path)
        .status()
        .expect("ffmpeg on PATH");
    assert!(status.success());
}

fn pixel(frame: &kairo_media::poster::RgbaFrame, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * frame.width + x) * 4) as usize;
    frame.pixels[i..i + 4].try_into().unwrap()
}

#[test]
fn the_poster_is_the_frame_at_a_tenth_of_a_second() {
    let dir = tempfile::tempdir().unwrap();
    let clip = dir.path().join("clip.mp4");
    make_clip(&clip, 320, 1.0);
    let (frame, duration) = decode_poster(&clip).unwrap();
    assert_eq!((frame.width, frame.height), (320, 180));
    let [r, _, b, a] = pixel(&frame, 160, 90);
    assert!(b > 200 && r < 60 && a == 255, "expected blue, got {r} {b}");
    let duration = duration.unwrap();
    assert!((duration - 1.1).abs() < 0.05, "{duration}");
}

#[test]
fn wide_frames_scale_down_and_short_clips_use_their_last_frame() {
    let dir = tempfile::tempdir().unwrap();
    let clip = dir.path().join("wide.webm");
    let status = Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=c=green:s=1920x1080:r=30:d=0.05",
        ])
        .arg(&clip)
        .status()
        .unwrap();
    assert!(status.success());
    let (frame, _) = decode_poster(&clip).unwrap();
    assert_eq!((frame.width, frame.height), (POSTER_MAX_WIDTH, 360));
}

#[test]
fn posters_are_cached_by_path_mtime_and_size() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("cache");
    let clip = dir.path().join("clip.mov");
    make_clip(&clip, 320, 0.5);
    let first = poster_for(&cache, &clip).unwrap();
    assert!(first.image.is_file());
    let png = std::fs::read(&first.image).unwrap();
    assert_eq!(&png[1..4], b"PNG");

    let modified = std::fs::metadata(&first.image).unwrap().modified().unwrap();
    assert_eq!(poster_for(&cache, &clip).unwrap(), first);
    assert_eq!(
        std::fs::metadata(&first.image).unwrap().modified().unwrap(),
        modified
    );

    // A changed file gets a new key and a new poster.
    let old_key = cache_key(&clip).unwrap();
    make_clip(&clip, 160, 0.5);
    let second = poster_for(&cache, &clip).unwrap();
    assert_ne!(cache_key(&clip).unwrap(), old_key);
    assert_eq!(second.width, 160);

    let keep = std::collections::HashSet::from([cache_key(&clip).unwrap()]);
    assert_eq!(prune_cache(&cache, &keep), 2);
    assert!(second.image.is_file());
}

#[test]
fn unreadable_and_broken_files_fail_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("cache");
    assert!(matches!(
        poster_for(&cache, &dir.path().join("missing.mp4")),
        Err(PosterError::Io(_))
    ));
    let broken = dir.path().join("broken.mp4");
    std::fs::write(&broken, b"not a video").unwrap();
    let error = poster_for(&cache, &broken).unwrap_err();
    assert!(matches!(error, PosterError::Decode(_)));
    assert!(
        error
            .to_string()
            .starts_with("Could not decode the video — ")
    );
}
