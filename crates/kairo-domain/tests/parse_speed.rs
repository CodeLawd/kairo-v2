//! Parsing a 5 MB settings file, against the 20 ms target. Run with:
//! `cargo test -p kairo-domain --release --test parse_speed -- --ignored --nocapture`

use kairo_domain::{AppSettings, StoredSettings};
use serde_json::Value;
use std::time::Instant;

/// The owner's settings, with the theme library repeated until the file is
/// at least 5 MB. Themes are most of a real file's size.
fn five_megabytes() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/owner/current-kairo-settings.json"
    );
    let mut settings: Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
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

#[test]
#[ignore = "measures time; run in release"]
fn settings_normalization_under_5_ms() {
    use kairo_domain::overlay_defaults::normalize_overlay_settings;
    use kairo_domain::pp_retirement::retire_pro_presenter_settings;
    use kairo_domain::theme_library::normalize_theme_library;
    use std::hint::black_box;

    let text = include_str!("fixtures/owner/current-kairo-settings.json");
    let settings: Value = serde_json::from_str(text).unwrap();
    let elapsed = median_ms(text, |_| {
        let retired = retire_pro_presenter_settings(Some(black_box(&settings)));
        let overlay = normalize_overlay_settings(retired.get("overlay"));
        let themes = normalize_theme_library(
            retired.get("themeLibrary"),
            &overlay.theme,
            0.0,
            "benchmark-legacy",
        );
        black_box((retired, overlay, themes));
    });
    println!(
        "Settings normalization median {elapsed:.3} ms ({} bytes, 21 runs, JSON already parsed)",
        text.len()
    );
    assert!(elapsed < 5.0);
}
