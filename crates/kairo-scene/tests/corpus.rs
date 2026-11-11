//! The prompt 003 corpus against Chromium: line breaks, fitted size, SSIM.

mod support;

use kairo_scene::{BoxKind, analyse};
use serde_json::Value;
use std::time::Instant;
use support::*;

struct Outcome {
    id: String,
    kind: String,
    exact: bool,
    size: f32,
    reference: f64,
    expected: Vec<String>,
    actual: Vec<String>,
}

fn compare() -> (Vec<Outcome>, usize) {
    let reference = reference_layout();
    let cases = corpus();
    assert_eq!(cases.len(), reference.len(), "corpus size");
    let mut out = Vec::new();
    let mut with_text = 0;
    for (case, chromium) in cases.iter().zip(&reference) {
        assert_eq!(chromium["id"].as_str(), Some(case.id.as_str()));
        let layers = chromium["layers"].as_array().expect("layers");
        let boxes = analyse(&case.scene());
        assert_eq!(boxes.len(), layers.len(), "{}: layer count", case.id);
        with_text += usize::from(!layers.is_empty());
        for (b, layer) in boxes.iter().zip(layers) {
            let kind = layer["kind"].as_str().unwrap();
            assert_eq!(
                kind,
                match b.kind {
                    BoxKind::Verse => "verse",
                    BoxKind::Reference => "reference",
                },
                "{}",
                case.id
            );
            let expected: Vec<String> = layer["lines"]
                .as_array()
                .unwrap()
                .iter()
                .map(|l| l["text"].as_str().unwrap().to_owned())
                .collect();
            let exact = expected
                .iter()
                .map(|l| canonical(l))
                .eq(b.lines.iter().map(|l| canonical(l)));
            let chromium_size = layer["fontSize"].as_f64().unwrap();
            out.push(Outcome {
                id: case.id.clone(),
                kind: kind.to_owned(),
                exact,
                size: b.size,
                reference: chromium_size,
                expected,
                actual: b.lines.clone(),
            });
        }
    }
    (out, with_text)
}

#[test]
fn line_breaks_and_fitted_sizes_match_chromium() {
    let (outcomes, _) = compare();
    let fitted: Vec<&Outcome> = outcomes
        .iter()
        .filter(|o| {
            reference_layout()
                .iter()
                .any(|c| c["id"] == o.id.as_str() && layer_fit(c, &o.kind))
        })
        .collect();
    let breaks = outcomes.iter().filter(|o| o.exact).count();
    let exact_sizes = fitted
        .iter()
        .filter(|o| (f64::from(o.size) - o.reference).abs() < 0.5)
        .count();
    let near_sizes = fitted
        .iter()
        .filter(|o| (f64::from(o.size) - o.reference).abs() <= 1.0)
        .count();
    for o in outcomes.iter().filter(|o| !o.exact) {
        eprintln!(
            "break mismatch {} {}\n  chromium {:?}\n  ours     {:?}",
            o.id, o.kind, o.expected, o.actual
        );
    }
    for o in fitted
        .iter()
        .filter(|o| (f64::from(o.size) - o.reference).abs() > 1.0)
    {
        eprintln!(
            "size mismatch {} {}: ours {} chromium {}",
            o.id, o.kind, o.size, o.reference
        );
    }
    eprintln!(
        "layers {}: exact breaks {breaks}; fitted layers {}: exact size {exact_sizes}, within 1 px {near_sizes}",
        outcomes.len(),
        fitted.len()
    );
    // DECISIONS.md 052: at least 99% of layers break exactly and every fitted
    // size is within one pixel.
    assert!(
        breaks * 100 >= outcomes.len() * 99,
        "{breaks}/{}",
        outcomes.len()
    );
    assert_eq!(near_sizes, fitted.len());
}

fn layer_fit(case: &Value, kind: &str) -> bool {
    case["layers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|l| l["kind"] == kind && l["autoFit"] == true)
}

#[test]
fn text_blocks_match_chromium_pixels() {
    let dir = reference_pngs();
    if !dir.is_dir() {
        assert!(
            std::env::var_os("KAIRO_REQUIRE_ORACLE").is_none(),
            "no Chromium renders in {dir:?}; run tools/golden-capture"
        );
        eprintln!("skipped: no Chromium renders in {dir:?}; run tools/golden-capture");
        return;
    }
    let out_dir = std::env::var_os("KAIRO_TEXT_OUT").map(std::path::PathBuf::from);
    if let Some(d) = &out_dir {
        std::fs::create_dir_all(d).unwrap();
    }
    let mut rows = Vec::new();
    let mut raster_ms = Vec::new();
    for case in corpus() {
        let chromium = load_png(&dir.join(format!("{}.png", case.id))).expect("reference png");
        let scene = case.scene();
        let started = Instant::now();
        let raster = kairo_scene::rasterize(&scene);
        raster_ms.push(started.elapsed().as_secs_f64() * 1000.0);
        let ours = over_gray(raster.as_ref());
        if let Some(d) = &out_dir {
            save_png(&d.join(format!("{}.png", case.id)), &ours);
        }
        let (all, text) = ssim(&chromium, &ours);
        rows.push((case.id, all, text));
    }
    let n = rows.len() as f64;
    let mean_all = rows.iter().map(|r| r.1).sum::<f64>() / n;
    let mean_text = rows.iter().map(|r| r.2).sum::<f64>() / n;
    let worst_all = rows.iter().map(|r| r.1).fold(f64::INFINITY, f64::min);
    let worst_text = rows.iter().map(|r| r.2).fold(f64::INFINITY, f64::min);
    let mut sorted = raster_ms.clone();
    sorted.sort_by(f64::total_cmp);
    eprintln!(
        "ssim whole mean {mean_all:.5} worst {worst_all:.5}; text blocks mean {mean_text:.5} worst {worst_text:.5}; raster median {:.2} ms p95 {:.2} ms max {:.2} ms",
        sorted[sorted.len() / 2],
        sorted[(sorted.len() * 95).div_ceil(100) - 1],
        sorted[sorted.len() - 1]
    );
    for key in [
        "original", "left", "right", "outline", "rtl", "yoruba", "long", "short",
    ] {
        let group: Vec<_> = rows
            .iter()
            .filter(|r| r.0.contains(&format!("-{key}-")))
            .collect();
        if !group.is_empty() {
            eprintln!(
                "  group {key}: {} cases, text blocks mean {:.4}",
                group.len(),
                group.iter().map(|r| r.2).sum::<f64>() / group.len() as f64
            );
        }
    }
    rows.sort_by(|a, b| a.2.total_cmp(&b.2));
    for r in rows.iter().take(8) {
        eprintln!("  low {} whole {:.4} text {:.4}", r.0, r.1, r.2);
    }
    // DECISIONS.md 052: whole-frame mean >= 0.99 and worst >= 0.90, text
    // blocks mean >= 0.92 and worst >= 0.55 (spike 003 reached 0.986 / 0.816 /
    // 0.878 / 0.480 with decorations drawn at guessed offsets).
    assert!(mean_all >= 0.99, "whole-frame mean {mean_all}");
    assert!(worst_all >= 0.90, "whole-frame worst {worst_all}");
    assert!(mean_text >= 0.92, "text-block mean {mean_text}");
    assert!(worst_text >= 0.55, "text-block worst {worst_text}");
}
