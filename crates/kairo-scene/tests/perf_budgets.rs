//! Prompt 107: slide text raster budget (06-testing-performance-strategy.md
//! row B01, p95 under 16 ms). The budget is asserted as written in optimised
//! builds and when `KAIRO_PERF_STRICT=1`; an unoptimised build (the text
//! crates are still optimised in the dev profile) gets four times the budget,
//! which only catches gross regressions.
mod support;
use std::time::Instant;
use support::*;

const BUDGET_P95_MS: f64 = 16.0;

fn strict() -> bool {
    !cfg!(debug_assertions) || std::env::var("KAIRO_PERF_STRICT").as_deref() == Ok("1")
}

fn pct(sorted: &[f64], p: usize) -> f64 {
    sorted[(sorted.len() * p).div_ceil(100) - 1]
}

#[test]
fn slide_raster_p95_is_within_budget() {
    let cases: Vec<_> = corpus()
        .into_iter()
        .filter(|c| !c.text.trim().is_empty())
        .collect();
    assert!(!cases.is_empty(), "the raster corpus is empty");
    // Font start-up and the glyph cache are measured separately (cold start);
    // this budget is for a running app.
    for c in &cases {
        let _ = kairo_scene::rasterize(&c.scene());
    }
    let mut samples = Vec::with_capacity(cases.len());
    for c in &cases {
        let start = Instant::now();
        let raster = kairo_scene::rasterize(&c.scene());
        samples.push(start.elapsed().as_secs_f64() * 1e3);
        std::hint::black_box(raster);
    }
    samples.sort_by(f64::total_cmp);
    let (p50, p95, max) = (
        pct(&samples, 50),
        pct(&samples, 95),
        samples[samples.len() - 1],
    );
    eprintln!(
        "slide raster, layout and fit included, n={} p50 {p50:.2} ms p95 {p95:.2} ms max {max:.2} ms (strict: {})",
        samples.len(),
        strict()
    );
    let budget = if strict() {
        BUDGET_P95_MS
    } else {
        BUDGET_P95_MS * 4.0
    };
    assert!(
        p95 < budget,
        "slide raster p95 {p95:.2} ms over {budget} ms"
    );
}
