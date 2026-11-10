//! The compositor thread: render on change, fades paced, idle draws
//! nothing, and `Committed` follows the GPU work.

use kairo_compositor::thread::{Compositor, CompositorEvent, Op, Sink};
use kairo_compositor::*;
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn start() -> Option<(Compositor, mpsc::Receiver<CompositorEvent>)> {
    let (tx, rx) = mpsc::channel();
    match Compositor::start(move |event| {
        let _ = tx.send(event);
    }) {
        Ok(compositor) => Some((compositor, rx)),
        Err(error) => {
            assert!(
                !std::env::var("KAIRO_REQUIRE_GPU").is_ok_and(|v| !v.is_empty()),
                "{error}"
            );
            eprintln!("skipped: {error}");
            None
        }
    }
}

fn committed(rx: &mpsc::Receiver<CompositorEvent>, seq: u64) -> Duration {
    let start = Instant::now();
    loop {
        match rx.recv_timeout(Duration::from_secs(5)).expect("committed") {
            CompositorEvent::Committed { seq: s, .. } if s >= seq => return start.elapsed(),
            CompositorEvent::Error { message, .. } => panic!("{message}"),
            _ => {}
        }
    }
}

fn solid(color: [f32; 4]) -> Option<Content> {
    Some(Content::new(Fill::Solid(color)))
}

#[test]
fn idle_draws_nothing_and_a_fade_draws_until_it_ends() {
    let Some((compositor, rx)) = start() else {
        return;
    };
    compositor.open("out", 1080, Sink::Offscreen);
    compositor.apply(
        "out",
        1,
        vec![Op::Swap {
            slot: Slot::Background,
            content: solid([1.0, 0.0, 0.0, 1.0]),
            ms: 0,
        }],
    );
    committed(&rx, 1);
    let settled = compositor.frames("out").unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        compositor.frames("out").unwrap(),
        settled,
        "a still program draws nothing"
    );

    compositor.apply(
        "out",
        2,
        vec![Op::Swap {
            slot: Slot::Background,
            content: solid([0.0, 0.0, 1.0, 1.0]),
            ms: 300,
        }],
    );
    committed(&rx, 2);
    std::thread::sleep(Duration::from_millis(500));
    let after_fade = compositor.frames("out").unwrap();
    let drawn = after_fade - settled;
    // About 60 frames a second for 300 ms; at least a dozen.
    assert!(
        (12..=40).contains(&drawn),
        "{drawn} frames for a 300 ms fade"
    );
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        compositor.frames("out").unwrap(),
        after_fade,
        "still again after the fade"
    );

    let (w, h, pixels) = compositor.read("out").unwrap();
    assert_eq!((w, h), (1920, 1080));
    assert_eq!(&pixels[..4], &[0, 0, 255, 255]);
}

#[test]
fn ops_for_an_unknown_output_are_reported() {
    let Some((compositor, rx)) = start() else {
        return;
    };
    compositor.apply("missing", 1, vec![]);
    match rx.recv_timeout(Duration::from_secs(5)).unwrap() {
        CompositorEvent::Error { output, .. } => assert_eq!(output.as_deref(), Some("missing")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_reopened_output_keeps_its_scene_and_a_closed_one_is_gone() {
    let Some((compositor, rx)) = start() else {
        return;
    };
    compositor.open("out", 720, Sink::Offscreen);
    compositor.apply(
        "out",
        1,
        vec![Op::ProgramBackground(Some([0.0, 0.0, 0.0, 1.0]))],
    );
    committed(&rx, 1);
    compositor.open("out", 1440, Sink::Offscreen);
    let (_, h, pixels) = compositor.read("out").unwrap();
    assert_eq!(h, 1440);
    assert_eq!(&pixels[..4], &[0, 0, 0, 255]);
    compositor.close("out");
    assert!(compositor.read("out").is_none());
}

#[test]
fn commands_reach_the_gpu_quickly() {
    let Some((compositor, rx)) = start() else {
        return;
    };
    compositor.open("out", 1080, Sink::Offscreen);
    let mut samples = Vec::new();
    for seq in 1..=200u64 {
        compositor.apply(
            "out",
            seq,
            vec![Op::Swap {
                slot: Slot::Text,
                content: solid([seq as f32 / 200.0, 0.0, 0.0, 1.0]),
                ms: 0,
            }],
        );
        samples.push(committed(&rx, seq).as_secs_f64() * 1000.0);
    }
    samples.sort_by(f64::total_cmp);
    eprintln!(
        "apply to committed, offscreen 1080p, n=200: p50 {:.3} ms, p99 {:.3} ms",
        samples[99], samples[197]
    );
}
