//! Video sources: decoded YUV planes become premultiplied RGBA on the GPU,
//! and the thread reports video nodes and routes the transport in order.

use kairo_compositor::thread::{Compositor, CompositorEvent, MediaControl, Op, Sink};
use kairo_compositor::*;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

fn renderer() -> Option<Renderer> {
    match Renderer::headless(true) {
        Ok(r) => Some(r),
        Err(error) => {
            assert!(
                !std::env::var("KAIRO_REQUIRE_GPU").is_ok_and(|v| !v.is_empty()),
                "no GPU: {error}"
            );
            eprintln!("skipped: {error}");
            None
        }
    }
}

fn media(path: &str, element: bool) -> MediaParams {
    MediaParams {
        path: path.into(),
        looped: true,
        start_seconds: None,
        end_seconds: None,
        rate: 1.0,
        element,
    }
}

fn video_node(id: u64) -> Content {
    Content {
        media: Some(media("/v.mp4", false)),
        ..Content::new(Fill::Source {
            id,
            backdrop: [0.0; 4],
        })
    }
}

fn draw(r: &mut Renderer, scene: &Scene) -> Vec<u8> {
    let target = r.target(1080);
    let mut encoder = r.device().create_command_encoder(&Default::default());
    r.draw_frame(&mut encoder, scene, &target, Instant::now());
    r.queue().submit([encoder.finish()]);
    r.read_frame(&target)
}

fn centre(frame: &[u8]) -> [u8; 4] {
    let at = ((540 * 1920 + 960) * 4) as usize;
    frame[at..at + 4].try_into().unwrap()
}

fn near(a: [u8; 4], b: [u8; 4], what: &str) {
    for i in 0..4 {
        assert!(
            (i32::from(a[i]) - i32::from(b[i])).abs() <= 2,
            "{what}: {a:?} vs {b:?}"
        );
    }
}

#[test]
fn yuv_planes_convert_with_their_matrix_range_depth_and_alpha() {
    let Some(mut r) = renderer() else {
        return;
    };
    let now = Instant::now();
    let mut scene = Scene::default();
    scene.stack.set_keyed(
        Slot::Background,
        Some((video_node(7), "VIDEO".into())),
        0,
        now,
    );
    // No frame yet: the backdrop (transparent).
    near(
        centre(&draw(&mut r, &scene)),
        [0, 0, 0, 0],
        "before a frame",
    );
    // Limited-range white and black.
    r.update_source_yuv(7, &YuvFrame::solid(64, 36, 235, 128, 128))
        .unwrap();
    near(centre(&draw(&mut r, &scene)), [255, 255, 255, 255], "white");
    r.update_source_yuv(7, &YuvFrame::solid(64, 36, 16, 128, 128))
        .unwrap();
    near(centre(&draw(&mut r, &scene)), [0, 0, 0, 255], "black");
    // BT.709 limited red is Y 63, Cb 102, Cr 240.
    r.update_source_yuv(7, &YuvFrame::solid(64, 36, 63, 102, 240))
        .unwrap();
    near(centre(&draw(&mut r, &scene)), [255, 0, 0, 255], "709 red");
    // BT.601 limited red is Y 81, Cb 90, Cr 240.
    let mut red601 = YuvFrame::solid(64, 36, 81, 90, 240);
    red601.color = YuvColor::BT601;
    r.update_source_yuv(7, &red601).unwrap();
    near(centre(&draw(&mut r, &scene)), [255, 0, 0, 255], "601 red");
    // Full range: 0 is black, 255 white.
    let mut full = YuvFrame::solid(64, 36, 255, 128, 128);
    full.color.full_range = true;
    r.update_source_yuv(7, &full).unwrap();
    near(
        centre(&draw(&mut r, &scene)),
        [255, 255, 255, 255],
        "full white",
    );
    // P010: ten bits at the top of 16. Limited white is 940 << 6.
    let mut p010 = YuvFrame::solid(64, 36, 0, 0, 0);
    p010.bits = 10;
    let y = (940u16 << 6).to_le_bytes();
    let c = (512u16 << 6).to_le_bytes();
    p010.luma = y.repeat(64 * 36);
    p010.luma_stride = 128;
    p010.chroma = c.repeat(32 * 18 * 2);
    p010.chroma_stride = 128;
    r.update_source_yuv(7, &p010).unwrap();
    near(
        centre(&draw(&mut r, &scene)),
        [255, 255, 255, 255],
        "P010 white",
    );
    // Alpha: premultiplied.
    let mut alpha = YuvFrame::solid(64, 36, 235, 128, 128);
    alpha.alpha = Some(vec![128; 64 * 36]);
    r.update_source_yuv(7, &alpha).unwrap();
    near(
        centre(&draw(&mut r, &scene)),
        [128, 128, 128, 128],
        "half alpha",
    );
    // A broken frame is refused and the last one stays.
    let mut short = YuvFrame::solid(64, 36, 16, 128, 128);
    short.luma.truncate(10);
    assert!(r.update_source_yuv(7, &short).is_err());
    near(centre(&draw(&mut r, &scene)), [128, 128, 128, 128], "kept");
    // Padded strides read the right bytes.
    let mut padded = YuvFrame::solid(64, 36, 16, 128, 128);
    padded.luma = (0..36)
        .flat_map(|_| [vec![235u8; 64], vec![16u8; 16]].concat())
        .collect();
    padded.luma_stride = 80;
    r.update_source_yuv(7, &padded).unwrap();
    near(
        centre(&draw(&mut r, &scene)),
        [255, 255, 255, 255],
        "padded",
    );
    // A removed source draws the backdrop again.
    r.remove_source(7);
    near(centre(&draw(&mut r, &scene)), [0, 0, 0, 0], "removed");
}

fn start() -> Option<(Compositor, mpsc::Receiver<CompositorEvent>)> {
    let (tx, rx) = mpsc::channel();
    match Compositor::start(move |e| {
        let _ = tx.send(e);
    }) {
        Ok(c) => Some((c, rx)),
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

/// Media and transport events, in order, until `seq` commits.
fn events_until(rx: &mpsc::Receiver<CompositorEvent>, seq: u64) -> Vec<CompositorEvent> {
    let mut out = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(5)).expect("event") {
            CompositorEvent::Committed { seq: s, .. } if s >= seq => return out,
            e @ (CompositorEvent::Media { .. } | CompositorEvent::Transport { .. }) => out.push(e),
            CompositorEvent::Error { message, .. } => panic!("{message}"),
            _ => {}
        }
    }
}

#[test]
fn video_nodes_are_reported_and_the_transport_follows_them_in_order() {
    let Some((c, rx)) = start() else {
        return;
    };
    c.open("out", 1080, Sink::Offscreen);
    // Transport with no video: nothing.
    c.apply("out", 1, vec![Op::Media(MediaControl::Pause(true))]);
    assert!(events_until(&rx, 1).is_empty());
    // A new background video and a pause in the same batch: the pause
    // reaches it, after its report.
    c.apply(
        "out",
        2,
        vec![
            Op::Keyed {
                slot: Slot::Background,
                content: Some((video_node(1), "VIDEO:/v.mp4:1".into())),
                ms: 0,
            },
            Op::Media(MediaControl::Seek(2.5)),
        ],
    );
    let events = events_until(&rx, 2);
    assert_eq!(events.len(), 2, "{events:?}");
    let CompositorEvent::Media { uses, .. } = &events[0] else {
        panic!("{events:?}")
    };
    assert_eq!(uses.len(), 1);
    assert!(uses[0].transport && uses[0].id == 1);
    let first = uses[0].node;
    assert_eq!(
        events[1],
        CompositorEvent::Transport {
            output: "out".into(),
            id: 1,
            control: MediaControl::Seek(2.5)
        }
    );
    // The same key kept with a new loop: same node, new params.
    let mut patched = video_node(1);
    patched.media.as_mut().unwrap().looped = false;
    c.apply(
        "out",
        3,
        vec![Op::Keyed {
            slot: Slot::Background,
            content: Some((patched, "VIDEO:/v.mp4:1".into())),
            ms: 0,
        }],
    );
    let events = events_until(&rx, 3);
    let CompositorEvent::Media { uses, .. } = &events[0] else {
        panic!("{events:?}")
    };
    assert_eq!(uses[0].node, first);
    assert!(!uses[0].params.looped);
    // A new cue fades in over 200 ms: both report, only the new one takes
    // the transport; the old one goes once removed.
    c.apply(
        "out",
        4,
        vec![
            Op::Keyed {
                slot: Slot::Background,
                content: Some((video_node(2), "VIDEO:/v.mp4:2".into())),
                ms: 200,
            },
            Op::Media(MediaControl::Pause(false)),
        ],
    );
    let events = events_until(&rx, 4);
    let CompositorEvent::Media { uses, .. } = &events[0] else {
        panic!("{events:?}")
    };
    assert_eq!(uses.len(), 2);
    assert_eq!(
        uses.iter()
            .filter(|u| u.transport)
            .map(|u| u.id)
            .collect::<Vec<_>>(),
        [2]
    );
    assert!(matches!(
        events[1],
        CompositorEvent::Transport { id: 2, .. }
    ));
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(CompositorEvent::Media { uses, .. }) => {
                assert_eq!(uses.iter().map(|u| u.id).collect::<Vec<_>>(), [2]);
                break;
            }
            Ok(_) => {}
            Err(_) => panic!("the retired video was not reported gone"),
        }
    }
    // Element videos are reported but never take the transport.
    let el = Content {
        media: Some(media("/e.mp4", true)),
        rect: Some(Rect::new(0.0, 0.0, 100.0, 100.0)),
        ..Content::new(Fill::Source {
            id: 9,
            backdrop: [0.0; 4],
        })
    };
    c.apply(
        "out",
        5,
        vec![Op::Keyed {
            slot: Slot::ElementsBelow,
            content: Some((Content::new(Fill::Group(Arc::new(vec![el]))), "EL".into())),
            ms: 0,
        }],
    );
    let events = events_until(&rx, 5);
    let CompositorEvent::Media { uses, .. } = &events[0] else {
        panic!("{events:?}")
    };
    let element = uses.iter().find(|u| u.id == 9).expect("element video");
    assert!(!element.transport);
    // Closing reports nothing left.
    c.close("out");
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(CompositorEvent::Media { uses, .. }) if uses.is_empty() => break,
            Ok(_) => {}
            Err(_) => panic!("closing did not report the videos gone"),
        }
    }
    // Frames from the feed reach the GPU and mark the output dirty.
    c.open("b", 1080, Sink::Offscreen);
    c.apply(
        "b",
        6,
        vec![Op::Keyed {
            slot: Slot::Background,
            content: Some((video_node(3), "VIDEO:/v.mp4:3".into())),
            ms: 0,
        }],
    );
    events_until(&rx, 6);
    let frames = c.frames("b").unwrap();
    c.feed()
        .video_frame(3, Arc::new(YuvFrame::solid(32, 18, 235, 128, 128)));
    let deadline = Instant::now() + Duration::from_secs(2);
    while c.frames("b").unwrap() == frames {
        assert!(Instant::now() < deadline, "a video frame drew nothing");
        std::thread::sleep(Duration::from_millis(5));
    }
    let (w, _, px) = c.read("b").unwrap();
    let at = ((540 * w + 960) * 4) as usize;
    near(px[at..at + 4].try_into().unwrap(), [255; 4], "fed frame");
}
