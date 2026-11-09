//! Splitting a 200-line song into slides, against the 1 ms target (prompt
//! 014). Run with:
//! `cargo test -p kairo-domain --release --test lyrics_speed -- --ignored --nocapture`

use kairo_domain::lyrics::slides::build_slides;
use kairo_domain::lyrics::{LyricsSong, SongPresentOptions};
use serde_json::json;
use std::time::Instant;

/// 20 sections of 10 lines: couplets separated by blank lines, a gloss line
/// in each, some lines long enough to soft-wrap, Yoruba marks and emoji.
fn two_hundred_line_song() -> LyricsSong {
    let sections: Vec<_> = (0..20)
        .map(|s| {
            let lines: Vec<String> = (0..10)
                .map(|l| match l % 5 {
                    2 => String::new(),
                    3 => "(Glory to the Lord who reigns forever and ever more)".to_string(),
                    4 => format!(
                        "Ọlọ́run mi, Ẹ ṣé o 🙏🏾 line {s}-{l} that is long enough to wrap twice over"
                    ),
                    _ => format!("Way maker, miracle worker {s}-{l}"),
                })
                .collect();
            json!({"type": "verse", "label": format!("Verse {}", s + 1), "lines": lines})
        })
        .collect();
    serde_json::from_value(json!({
        "id": "song", "title": "Speed", "artist": "", "sections": sections,
        "createdAt": 0, "updatedAt": 0,
    }))
    .unwrap()
}

#[test]
#[ignore = "timing; run in release with --ignored"]
fn splitting_a_200_line_song_takes_under_a_millisecond() {
    let song = two_hundred_line_song();
    assert_eq!(
        song.sections.iter().map(|s| s.lines.len()).sum::<usize>(),
        200
    );
    let opts = SongPresentOptions::default();
    let mut runs: Vec<f64> = (0..201)
        .map(|_| {
            let start = Instant::now();
            let slides = build_slides(&song, &opts, Some("#AABED7"));
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            assert!(!slides.is_empty());
            elapsed
        })
        .collect();
    runs.sort_by(f64::total_cmp);
    let (median, p99, max) = (runs[100], runs[198], runs[200]);
    println!(
        "build_slides, 200 lines, 201 runs: median {median:.4} ms, p99 {p99:.4} ms, max {max:.4} ms"
    );
    assert!(p99 < 1.0, "p99 {p99} ms is over the 1 ms target");
}
