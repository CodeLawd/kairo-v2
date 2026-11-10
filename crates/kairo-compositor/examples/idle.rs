//! Process CPU while four outputs hold a static slide.
//!
//! ```text
//! cargo run --release -p kairo-compositor --example idle -- [--outputs N] [--secs S]
//! ```

use kairo_compositor::thread::{Compositor, CompositorEvent, Op, Sink};
use kairo_compositor::*;
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[cfg(unix)]
fn cpu_seconds() -> f64 {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: getrusage fills the struct for this process.
    unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    // SAFETY: filled above.
    let usage = unsafe { usage.assume_init() };
    let t = |tv: libc::timeval| tv.tv_sec as f64 + tv.tv_usec as f64 / 1e6;
    t(usage.ru_utime) + t(usage.ru_stime)
}

/// Process CPU time is read with `getrusage`; elsewhere it is not measured.
#[cfg(not(unix))]
fn cpu_seconds() -> f64 {
    f64::NAN
}

fn value(name: &str) -> Option<u64> {
    let mut args = std::env::args();
    args.find(|a| a == name)?;
    args.next()?.parse().ok()
}

fn main() {
    let outputs = value("--outputs").unwrap_or(4);
    let secs = value("--secs").unwrap_or(30);
    let (tx, rx) = mpsc::channel();
    let compositor = Compositor::start(move |event| {
        let _ = tx.send(event);
    })
    .expect("GPU");
    for index in 0..outputs {
        let name = format!("out-{index}");
        compositor.open(&name, 1080, Sink::Offscreen);
        compositor.apply(
            &name,
            1,
            vec![
                Op::Keyed {
                    slot: Slot::Background,
                    content: Some((Content::new(Fill::Solid([0.1, 0.2, 0.5, 1.0])), "bg".into())),
                    ms: 0,
                },
                Op::Swap {
                    slot: Slot::Text,
                    content: Some(Content {
                        rect: Some(Rect::new(200.0, 700.0, 1520.0, 300.0)),
                        ..Content::new(Fill::Solid([1.0; 4]))
                    }),
                    ms: 0,
                },
            ],
        );
    }
    let mut committed = 0;
    while committed < outputs {
        if let Ok(CompositorEvent::Committed { .. }) = rx.recv_timeout(Duration::from_secs(5)) {
            committed += 1;
        }
    }
    std::thread::sleep(Duration::from_secs(2));
    let (cpu0, t0) = (cpu_seconds(), Instant::now());
    std::thread::sleep(Duration::from_secs(secs));
    let cpu = cpu_seconds() - cpu0;
    let wall = t0.elapsed().as_secs_f64();
    let frames: u64 = (0..outputs)
        .filter_map(|i| compositor.frames(&format!("out-{i}")))
        .sum();
    println!(
        "{outputs} static outputs, {wall:.1} s: CPU {:.3}% of one core, {frames} frames drawn in total",
        cpu / wall * 100.0
    );
}
