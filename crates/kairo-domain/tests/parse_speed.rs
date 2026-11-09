//! Parsing a 5 MB settings file, against the 20 ms target. Run with:
//! `cargo test -p kairo-domain --release --test parse_speed -- --ignored --nocapture`

mod common;

use common::{fixture, read};
use kairo_domain::{AppSettings, StoredSettings};
use std::time::Instant;

/// The owner's settings, with the theme library repeated until the file is
/// at least 5 MB. Themes are most of a real file's size.
fn five_megabytes() -> String {
    let mut settings = read(&fixture("owner/current-kairo-settings.json"));
    let themes = settings["themeLibrary"].as_array().unwrap().clone();
    assert!(!themes.is_empty());
    let mut text = serde_json::to_string(&settings).unwrap();
    while text.len() < 5 * 1024 * 1024 {
        settings["themeLibrary"]
            .as_array_mut()
            .unwrap()
            .extend(themes.iter().cloned());
        text = serde_json::to_string(&settings).unwrap();
    }
    text
}

fn median_ms(text: &str, parse: impl Fn(&str)) -> f64 {
    let mut runs: Vec<f64> = (0..21)
        .map(|_| {
            let start = Instant::now();
            parse(text);
            start.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    runs.sort_by(f64::total_cmp);
    runs[runs.len() / 2]
}

#[test]
#[ignore = "measures time; run in release"]
fn five_megabyte_settings_parse_under_20_ms() {
    let text = five_megabytes();
    let stored = median_ms(&text, |t| {
        drop(serde_json::from_str::<StoredSettings>(t).unwrap())
    });
    let canonical = median_ms(&text, |t| {
        drop(serde_json::from_str::<AppSettings>(t).unwrap())
    });
    println!(
        "{:.2} MB: StoredSettings median {stored:.2} ms, AppSettings median {canonical:.2} ms (21 runs each)",
        text.len() as f64 / 1_048_576.0
    );
    assert!(stored < 20.0 && canonical < 20.0);
}
