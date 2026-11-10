//! Live microphone check: capture keeps running while the calling thread is
//! blocked, and reports chunk cadence, level and drain latency.
//!
//! `cargo run -p kairo-stt --example live_capture -- [seconds] [device-id]`
//! The calling thread sleeps for the whole run to stand in for a frozen UI.

use kairo_stt::capture::{CaptureEvent, start_capture};
use kairo_stt::relay::AudioRelay;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn main() {
    let mut args = std::env::args().skip(1);
    let seconds: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(10);
    let device = args.next().unwrap_or_default();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let granted = runtime.block_on(kairo_stt::permission::ensure_microphone_access());
    println!("microphone permission: {granted}");
    if !granted {
        std::process::exit(2);
    }

    println!("inputs:");
    for d in kairo_stt::devices::list_audio_input_devices() {
        println!("  {} | {}", d.id, d.label);
    }

    let relay = AudioRelay::default();
    let stream = relay.create_stream();
    let stats = Arc::new(Mutex::new((0u64, 0u64, 0.0f64, Vec::<Duration>::new())));
    let started = Instant::now();
    let capture = {
        let stats = stats.clone();
        let relay = relay.clone();
        let mut last_chunk: Option<Instant> = None;
        start_capture(&device, move |event| {
            let mut s = stats.lock().unwrap();
            match event {
                CaptureEvent::Pcm(pcm) => {
                    s.0 += 1;
                    let now = Instant::now();
                    if let Some(prev) = last_chunk {
                        s.3.push(now - prev);
                    }
                    last_chunk = Some(now);
                    relay.feed(pcm);
                }
                CaptureEvent::Level(level) => {
                    s.1 += 1;
                    s.2 = s.2.max(level.peak);
                }
                CaptureEvent::FellBackToDefault { requested } => {
                    println!("fell back to default from {requested}")
                }
                CaptureEvent::Error(e) => println!("capture error: {e}"),
            }
        })
        .expect("capture")
    };

    // Stand-in for a frozen UI thread.
    std::thread::sleep(Duration::from_secs(seconds));
    let elapsed = started.elapsed();
    let overruns = capture.overruns();
    capture.stop();

    let s = stats.lock().unwrap();
    let mut gaps = s.3.clone();
    gaps.sort();
    let p = |q: f64| {
        gaps.get(((gaps.len() as f64 - 1.0) * q) as usize)
            .copied()
            .unwrap_or_default()
    };
    let mut queued = 0;
    while stream.try_recv().is_some() {
        queued += 1;
    }
    println!(
        "{:.1} s: {} chunks (expected {}), {} meters, peak {:.3}, overruns {}, relay held {} chunks",
        elapsed.as_secs_f64(),
        s.0,
        (elapsed.as_millis() / 100),
        s.1,
        s.2,
        overruns,
        queued
    );
    println!(
        "chunk spacing p50 {:?} p95 {:?} max {:?}",
        p(0.5),
        p(0.95),
        gaps.last().copied().unwrap_or_default()
    );
}
