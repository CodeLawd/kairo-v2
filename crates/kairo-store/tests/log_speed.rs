//! How long a log call takes the caller with the non-blocking file writer.
//! Run: `cargo test --release -p kairo-store --test log_speed -- --ignored --nocapture`
//! (a release build needs `KAIRO_API_URL` and `KAIRO_WEB_URL` set).

use kairo_store::logging::{ElectronLogFormat, file_writer};
use std::time::Instant;
use tracing_subscriber::Layer;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;

#[test]
#[ignore = "measurement"]
fn log_call_cost() {
    let dir = tempfile::tempdir().unwrap();
    let (writer, guard) = file_writer(&dir.path().join("kairo.log")).unwrap();
    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .event_format(ElectronLogFormat)
            .with_writer(writer)
            .with_filter(LevelFilter::INFO),
    );
    const N: usize = 100_000;
    let mut samples = Vec::with_capacity(N);
    tracing::subscriber::with_default(subscriber, || {
        for i in 0..N {
            let start = Instant::now();
            tracing::info!(slide = i, output = "projector", "[Program] Pushed slide");
            samples.push(start.elapsed().as_nanos() as u64);
        }
    });
    drop(guard);
    samples.sort_unstable();
    let pct = |p: f64| samples[((N as f64) * p) as usize - 1] as f64 / 1000.0;
    println!(
        "log call over {N} events: p50 {:.2} µs, p99 {:.2} µs, max {:.2} µs",
        pct(0.5),
        pct(0.99),
        pct(1.0)
    );
}
