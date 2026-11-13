//! The Syphon sink against the real compositor, read back by a Syphon client
//! the way OBS would; and the channel's start, stop, clear and naming rules
//! from Electron's `tests/syphon-channel.test.ts`.
#![cfg(target_os = "macos")]

use kairo_compositor::thread::{Compositor, CompositorEvent, Op, Sink};
use kairo_compositor::{Content, Fill, Slot};
use kairo_sinks::syphon::SyphonChannel;
use std::sync::{Arc, Mutex, mpsc};
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

fn committed(rx: &mpsc::Receiver<CompositorEvent>, seq: u64) {
    loop {
        match rx.recv_timeout(Duration::from_secs(5)).expect("committed") {
            CompositorEvent::Committed { seq: s, .. } if s >= seq => return,
            CompositorEvent::Error { message, .. } => panic!("{message}"),
            _ => {}
        }
    }
}

fn swap(color: [f32; 4]) -> Vec<Op> {
    vec![Op::Swap {
        slot: Slot::Background,
        content: Some(Content::new(Fill::Solid(color))),
        ms: 0,
    }]
}

fn errors() -> (kairo_sinks::syphon::Report, Arc<Mutex<Vec<String>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    (Arc::new(move |e| sink.lock().unwrap().push(e)), seen)
}

/// The compositor's own frame, premultiplied RGBA, as BGRA.
fn bgra(compositor: &Compositor, output: &str) -> (u32, u32, Vec<u8>) {
    let (w, h, mut px) = compositor.read(output).expect("frame");
    for p in px.as_chunks_mut::<4>().0 {
        p.swap(0, 2);
    }
    (w, h, px)
}

/// Polls the client until it sees `want` (frames are published async).
fn wait_for(channel: &SyphonChannel, want: &(u32, u32, Vec<u8>)) -> (u32, u32, Vec<u8>) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let seen = channel.read_latest(3000.0).expect("a Syphon frame");
        if &seen == want || Instant::now() > deadline {
            return seen;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn frames_reach_a_syphon_client_exactly_with_no_readback() {
    let Some((compositor, rx)) = start() else {
        return;
    };
    let mut channel = SyphonChannel::new("Kairo sink test");
    let (report, seen) = errors();
    compositor.open("out", 1080, Sink::Offscreen);
    compositor.attach("out", channel.sink(report));
    // Half-transparent red: alpha must survive.
    compositor.apply("out", 1, swap([1.0, 0.0, 0.0, 0.5]));
    committed(&rx, 1);
    let want = bgra(&compositor, "out");
    assert_eq!((want.0, want.1), (1920, 1080));
    let got = wait_for(&channel, &want);
    assert_eq!((got.0, got.1), (1920, 1080));
    assert!(got.2 == want.2, "Syphon frame equals the compositor frame");
    assert!(
        got.2[3] > 0 && got.2[3] < 255,
        "alpha kept: {:?}",
        &got.2[..4]
    );

    // The next slide follows.
    compositor.apply("out", 2, swap([0.0, 0.0, 1.0, 1.0]));
    committed(&rx, 2);
    let want = bgra(&compositor, "out");
    assert!(wait_for(&channel, &want).2 == want.2);

    let stats = channel.stats().unwrap();
    assert!(stats.published >= 2, "{stats:?}");
    assert_eq!(stats.readbacks, 0, "the IOSurface path reads nothing back");
    assert_eq!(stats.failed, 0);
    assert!(
        seen.lock().unwrap().is_empty(),
        "{:?}",
        seen.lock().unwrap()
    );
}

#[test]
fn a_frame_height_change_is_followed() {
    let Some((compositor, rx)) = start() else {
        return;
    };
    let mut channel = SyphonChannel::new("Kairo size test");
    let (report, _) = errors();
    compositor.open("out", 720, Sink::Offscreen);
    compositor.attach("out", channel.sink(report.clone()));
    compositor.apply("out", 1, swap([0.0, 1.0, 0.0, 1.0]));
    committed(&rx, 1);
    assert_eq!(wait_for(&channel, &bgra(&compositor, "out")).1, 720);
    compositor.open("out", 1080, Sink::Offscreen);
    compositor.attach("out", channel.sink(report));
    compositor.apply("out", 2, swap([0.0, 1.0, 0.0, 1.0]));
    committed(&rx, 2);
    assert_eq!(wait_for(&channel, &bgra(&compositor, "out")).1, 1080);
}

#[test]
fn a_stopped_server_takes_no_frames_and_the_sink_goes_quiet() {
    let Some((compositor, rx)) = start() else {
        return;
    };
    let mut channel = SyphonChannel::new("Kairo stop test");
    let (report, seen) = errors();
    compositor.open("out", 360, Sink::Offscreen);
    compositor.attach("out", channel.sink(report));
    compositor.apply("out", 1, swap([1.0, 1.0, 1.0, 1.0]));
    committed(&rx, 1);
    channel.stop();
    channel.stop();
    assert!(!channel.running());
    assert!(channel.stats().is_none());
    // The sink still attached finds the server closed: nothing panics.
    compositor.apply("out", 2, swap([0.0, 0.0, 0.0, 1.0]));
    committed(&rx, 2);
    assert!(
        seen.lock().unwrap().is_empty(),
        "{:?}",
        seen.lock().unwrap()
    );
    // A later start makes a fresh server.
    assert!(channel.start());
    assert_eq!(channel.stats().unwrap().published, 0);
}

#[test]
fn clearing_publishes_a_transparent_frame_but_never_starts_a_server() {
    let mut channel = SyphonChannel::new("Kairo clear test");
    channel.clear(4, 2);
    assert!(
        !channel.running(),
        "nothing to clear before the first frame"
    );
    assert!(channel.start());
    assert!(channel.start(), "starting twice keeps one server");
    channel.clear(4, 2);
    let (w, h, px) = channel.read_latest(3000.0).expect("cleared frame");
    assert_eq!((w, h), (4, 2));
    assert!(px.iter().all(|b| *b == 0));
    assert_eq!(channel.status(), Ok(()));
}

#[test]
fn a_server_name_with_a_nul_fails_and_says_why() {
    let mut channel = SyphonChannel::new("Kairo\0bad");
    let (report, _) = errors();
    assert!(channel.sink(report).is_none());
    let reason = channel.status().unwrap_err();
    assert!(reason.contains("NUL"), "{reason}");
}

#[test]
fn the_readback_path_publishes_the_same_pixels() {
    let Some((compositor, rx)) = start() else {
        return;
    };
    let mut channel = SyphonChannel::new("Kairo readback test").force_readback();
    let (report, seen) = errors();
    compositor.open("out", 1080, Sink::Offscreen);
    compositor.attach("out", channel.sink(report));
    compositor.apply("out", 1, swap([0.0, 0.5, 1.0, 0.75]));
    committed(&rx, 1);
    let want = bgra(&compositor, "out");
    assert!(wait_for(&channel, &want).2 == want.2);
    assert!(channel.stats().unwrap().readbacks >= 1);
    assert!(seen.lock().unwrap().is_empty());
}

#[test]
fn detaching_the_sink_stops_frames_reaching_the_server() {
    let Some((compositor, rx)) = start() else {
        return;
    };
    let mut channel = SyphonChannel::new("Kairo detach test");
    let (report, _) = errors();
    compositor.open("out", 360, Sink::Offscreen);
    compositor.attach("out", channel.sink(report));
    compositor.apply("out", 1, swap([1.0, 0.0, 0.0, 1.0]));
    committed(&rx, 1);
    let before = channel.stats().unwrap().published;
    compositor.attach("out", None);
    compositor.apply("out", 2, swap([0.0, 1.0, 0.0, 1.0]));
    committed(&rx, 2);
    assert_eq!(channel.stats().unwrap().published, before);
    // Attaching to an output the compositor lacks is reported, not a panic.
    compositor.attach("missing", channel.sink(Arc::new(|_| {})));
    loop {
        if let CompositorEvent::Error { output, .. } = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("error event")
        {
            assert_eq!(output.as_deref(), Some("missing"));
            break;
        }
    }
}
