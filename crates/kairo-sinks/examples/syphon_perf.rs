//! Apply-to-committed time for one 1920 × 1080 offscreen output with no
//! sink, with the Syphon sink on its IOSurface path, and with the BGRA
//! read-back path: what Syphon adds to each frame.
//!
//! cargo run --release -p kairo-sinks --example syphon_perf -- [frames]

#[cfg(target_os = "macos")]
fn main() {
    use kairo_compositor::thread::{Compositor, CompositorEvent, Op, Sink};
    use kairo_compositor::{Content, Fill, Slot};
    use kairo_sinks::syphon::SyphonChannel;
    use std::sync::{Arc, mpsc};
    use std::time::{Duration, Instant};

    let n: u64 = std::env::args()
        .nth(1)
        .map_or(300, |s| s.parse().expect("frames"));
    let (tx, rx) = mpsc::channel();
    let compositor = Compositor::start(move |e| {
        let _ = tx.send(e);
    })
    .expect("GPU");
    for mode in ["none", "iosurface", "readback"] {
        let output = format!("perf-{mode}");
        let mut channel = SyphonChannel::new(format!("Kairo perf {mode}"));
        if mode == "readback" {
            channel = channel.force_readback();
        }
        compositor.open(&output, 1080, Sink::Offscreen);
        if mode != "none" {
            compositor.attach(&output, channel.sink(Arc::new(|e| eprintln!("{e}"))));
        }
        let mut times = Vec::new();
        for seq in 1..=n {
            let shade = (seq % 255) as f32 / 255.0;
            let started = Instant::now();
            compositor.apply(
                &output,
                seq,
                vec![Op::Swap {
                    slot: Slot::Background,
                    content: Some(Content::new(Fill::Solid([shade, 0.2, 1.0 - shade, 1.0]))),
                    ms: 0,
                }],
            );
            loop {
                match rx.recv_timeout(Duration::from_secs(5)).expect("event") {
                    CompositorEvent::Committed { output: o, seq: s } if o == output && s >= seq => {
                        break;
                    }
                    CompositorEvent::Error { message, .. } => panic!("{message}"),
                    _ => {}
                }
            }
            times.push(started.elapsed());
        }
        times.sort();
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        let stats = channel.stats();
        println!(
            "{mode:>9}: n={n} p50 {:.2} ms, p99 {:.2} ms, max {:.2} ms; {:?}",
            ms(times[times.len() / 2]),
            ms(times[times.len() * 99 / 100]),
            ms(*times.last().unwrap()),
            stats
        );
        compositor.close(&output);
        channel.stop();
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("Syphon is only available on a Mac");
}
