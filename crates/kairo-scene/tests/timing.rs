mod support;
use std::time::Instant;
use support::*;

fn pct(v: &mut [f64], p: usize) -> f64 {
    v.sort_by(f64::total_cmp);
    v[(v.len() * p).div_ceil(100) - 1]
}

#[test]
#[ignore = "prints timings"]
fn phases() {
    let cases: Vec<_> = corpus()
        .into_iter()
        .filter(|c| !c.text.trim().is_empty())
        .collect();
    // Warm up fonts.
    let _ = kairo_scene::rasterize(&cases[0].scene());
    let (mut build, mut fit, mut total) = (vec![], vec![], vec![]);
    let mut worst = vec![];
    for c in &cases {
        let t = Instant::now();
        let scene = c.scene();
        build.push(t.elapsed().as_secs_f64() * 1e3);
        let t = Instant::now();
        let _ = kairo_scene::analyse(&scene);
        fit.push(t.elapsed().as_secs_f64() * 1e3);
        let t = Instant::now();
        let _ = kairo_scene::rasterize(&scene);
        let ms = t.elapsed().as_secs_f64() * 1e3;
        total.push(ms);
        worst.push((ms, c.id.clone()));
    }
    let mut sums = kairo_scene::Timings::default();
    let mut n = 0.0;
    for c in &cases {
        let (_, t) = kairo_scene::rasterize_timed(&c.scene());
        sums.layout_ms += t.layout_ms;
        sums.shadow_ms += t.shadow_ms;
        sums.stroke_ms += t.stroke_ms;
        sums.fill_ms += t.fill_ms;
        sums.composite_ms += t.composite_ms;
        n += 1.0;
    }
    eprintln!(
        "mean ms: layout {:.2} shadow {:.2} stroke {:.2} fill {:.2} composite {:.2}",
        sums.layout_ms / n,
        sums.shadow_ms / n,
        sums.stroke_ms / n,
        sums.fill_ms / n,
        sums.composite_ms / n
    );
    worst.sort_by(|a, b| b.0.total_cmp(&a.0));
    eprintln!(
        "cases {}: build p95 {:.3}; layout+fit p50 {:.2} p95 {:.2}; rasterize(all) p50 {:.2} p95 {:.2} max {:.2}",
        cases.len(),
        pct(&mut build, 95),
        pct(&mut fit.clone(), 50),
        pct(&mut fit, 95),
        pct(&mut total.clone(), 50),
        pct(&mut total, 95),
        worst[0].0
    );
    for w in worst.iter().take(4) {
        let c = cases.iter().find(|c| c.id == w.1).unwrap();
        let (_, t) = kairo_scene::rasterize_timed(&c.scene());
        eprintln!("    {} {:?}", c.id, t);
    }
    for w in worst.iter().take(12) {
        eprintln!("  {:.1} ms {}", w.0, w.1);
    }
}
