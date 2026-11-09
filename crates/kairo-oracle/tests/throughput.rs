//! Oracle round trip throughput, against the target of 5,000 calls per second.
//! Run with:
//! `KAIRO_ORACLE_MODE=live cargo test -p kairo-oracle --release --test throughput -- --ignored --nocapture`

use kairo_oracle::{Mode, Oracle, electron_dir};
use serde_json::json;
use std::time::Instant;

#[test]
#[ignore = "needs Node and measures time"]
fn calls_per_second() {
    let unused_cache = std::env::temp_dir().join("kairo-oracle-throughput-unused");
    let mut oracle = Oracle::new(Mode::Live, unused_cache, electron_dir());
    let inputs: Vec<_> = (0..50_000)
        .map(|i| {
            let n = f64::from(i % 300) - 100.0;
            json!([{"xPct": n, "yPct": -n, "widthPct": n / 2.0, "heightPct": 7}])
        })
        .collect();
    // The first call pays for starting Node and loading the module.
    let start = Instant::now();
    oracle
        .results("overlay-boxes.clampOverlayBox", &inputs[..1])
        .unwrap();
    let startup = start.elapsed();
    for _ in 0..3 {
        let start = Instant::now();
        let results = oracle
            .results("overlay-boxes.clampOverlayBox", &inputs)
            .unwrap();
        let seconds = start.elapsed().as_secs_f64();
        assert_eq!(results.len(), inputs.len());
        println!(
            "startup {:.0} ms, {} calls in {:.3} s = {:.0} calls/s",
            startup.as_secs_f64() * 1000.0,
            inputs.len(),
            seconds,
            inputs.len() as f64 / seconds
        );
    }
}
