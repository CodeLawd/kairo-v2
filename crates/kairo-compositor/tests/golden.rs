//! Headless frames: transitions at known progress, camera layouts, the
//! display letterbox and layer independence.
//!
//! Runs on the software adapter (WARP, lavapipe) where one exists, else on
//! whatever adapter the machine has. With no adapter at all each test says
//! so and returns, unless `KAIRO_REQUIRE_GPU` is set. Golden PNGs live in
//! `tests/golden/`; `KAIRO_UPDATE_GOLDENS=1` rewrites them.

use kairo_compositor::*;
use serde_json::json;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

fn renderer() -> Option<std::sync::MutexGuard<'static, Renderer>> {
    static RENDERER: OnceLock<Option<std::sync::Mutex<Renderer>>> = OnceLock::new();
    let shared = RENDERER.get_or_init(|| match Renderer::headless(true) {
        Ok(renderer) => {
            eprintln!("adapter: {:?}", renderer.adapter_info());
            Some(std::sync::Mutex::new(renderer))
        }
        Err(error) => {
            assert!(
                !std::env::var("KAIRO_REQUIRE_GPU").is_ok_and(|v| !v.is_empty()),
                "no GPU: {error}"
            );
            eprintln!("skipped: {error}");
            None
        }
    });
    shared
        .as_ref()
        .map(|m| m.lock().unwrap_or_else(|e| e.into_inner()))
}

fn render(renderer: &mut Renderer, scene: &Scene, height: u32, now: Instant) -> Vec<u8> {
    let target = renderer.target(height);
    let mut encoder = renderer
        .device()
        .create_command_encoder(&Default::default());
    renderer.draw_frame(&mut encoder, scene, &target, now);
    renderer.queue().submit([encoder.finish()]);
    renderer.read_frame(&target)
}

fn pixel(frame: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let at = ((y * width + x) * 4) as usize;
    frame[at..at + 4].try_into().unwrap()
}

/// Premultiplied 0..1 to bytes, as the GPU stores them.
fn bytes(c: [f32; 4]) -> [u8; 4] {
    c.map(|v| (v * 255.0).round() as u8)
}

fn close(actual: [u8; 4], expected: [u8; 4], what: &str) {
    for i in 0..4 {
        assert!(
            (i32::from(actual[i]) - i32::from(expected[i])).abs() <= 2,
            "{what}: {actual:?} vs {expected:?}"
        );
    }
}

fn solid(color: [f32; 4]) -> Content {
    Content::new(Fill::Solid(color))
}

fn boxed(color: [f32; 4], rect: Rect) -> Content {
    Content {
        rect: Some(rect),
        ..solid(color)
    }
}

const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const BLUE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
const GREEN: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
const WHITE: [f32; 4] = [1.0; 4];

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// Compares with a stored PNG: every channel within 3 levels on all but
/// 0.5% of pixels (anti-aliased edges and the shadow differ by adapter).
fn golden(name: &str, width: u32, height: u32, rgba: &[u8]) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.png"));
    if std::env::var_os("KAIRO_UPDATE_GOLDENS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = std::fs::File::create(&path).unwrap();
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(rgba)
            .unwrap();
        return;
    }
    let file = std::fs::File::open(&path)
        .unwrap_or_else(|e| panic!("{}: {e}; run with KAIRO_UPDATE_GOLDENS=1", path.display()));
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file))
        .read_info()
        .unwrap();
    let mut stored = vec![0; decoder.output_buffer_size().unwrap()];
    let info = decoder.next_frame(&mut stored).unwrap();
    assert_eq!((info.width, info.height), (width, height), "{name} size");
    let off = rgba
        .chunks(4)
        .zip(stored.chunks(4))
        .filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 3))
        .count();
    let allowed = (width * height) as usize / 200;
    assert!(
        off <= allowed,
        "{name}: {off} pixels differ (allowed {allowed})"
    );
}

#[test]
fn an_empty_program_is_transparent() {
    let Some(mut r) = renderer() else { return };
    let frame = render(&mut r, &Scene::default(), 1080, Instant::now());
    assert!(frame.iter().all(|b| *b == 0));
}

#[test]
fn a_crossfade_is_linear_with_the_new_node_above() {
    let Some(mut r) = renderer() else { return };
    let t0 = Instant::now();
    let mut scene = Scene::default();
    scene.stack.swap(Slot::Background, Some(solid(RED)), 0, t0);
    scene
        .stack
        .swap(Slot::Background, Some(solid(BLUE)), 1000, t0);
    for (at, p) in [
        (0, 0.0f32),
        (250, 0.25),
        (500, 0.5),
        (750, 0.75),
        (1000, 1.0),
    ] {
        let frame = render(&mut r, &scene, 1080, t0 + ms(at));
        // Old red at 1 - p, new blue at p drawn over it.
        let old = [1.0 - p, 0.0, 0.0, 1.0 - p];
        let expected = [old[0] * (1.0 - p), 0.0, p, p + old[3] * (1.0 - p)];
        close(
            pixel(&frame, 1920, 960, 540),
            bytes(expected),
            &format!("{at} ms"),
        );
        if at == 500 {
            golden("crossfade-50", 1920, 1080, &frame);
        }
    }
    // Removed 50 ms after its fade ends, as `retire`'s timeout does.
    scene.stack.prune(t0 + ms(1049));
    assert_eq!(scene.stack.nodes(Slot::Background).len(), 2);
    scene.stack.prune(t0 + ms(1050));
    assert_eq!(scene.stack.nodes(Slot::Background).len(), 1);
    assert_eq!(
        scene.stack.next_change(t0 + ms(1050)),
        None,
        "still afterwards"
    );
}

#[test]
fn an_instant_swap_cuts_and_removes_the_old_node() {
    let Some(mut r) = renderer() else { return };
    let t0 = Instant::now();
    let mut scene = Scene::default();
    scene.stack.swap(Slot::Text, Some(solid(RED)), 0, t0);
    scene.stack.swap(Slot::Text, Some(solid(GREEN)), 0, t0);
    assert_eq!(scene.stack.nodes(Slot::Text).len(), 1);
    close(
        pixel(&render(&mut r, &scene, 1080, t0), 1920, 5, 5),
        bytes(GREEN),
        "cut",
    );
    assert!(!scene.stack.animating(t0));
}

#[test]
fn a_node_retired_mid_fade_fades_from_where_it_was() {
    let t0 = Instant::now();
    let mut stack = Stack::default();
    stack.swap(Slot::Message, Some(solid(RED)), 1000, t0);
    stack.swap(Slot::Message, None, 1000, t0 + ms(400));
    let node = &stack.nodes(Slot::Message)[0];
    assert!(node.is_leaving());
    assert!((node.fade(t0 + ms(400)) - 0.4).abs() < 1e-3);
    assert!((node.fade(t0 + ms(900)) - 0.2).abs() < 1e-3);
    assert_eq!(node.fade(t0 + ms(1400)), 0.0);
    assert!(
        stack.current(Slot::Message).is_none(),
        "a leaving node is never current"
    );
}

#[test]
fn changing_the_logo_does_not_re_fade_the_text() {
    let Some(mut r) = renderer() else { return };
    let t0 = Instant::now();
    let mut scene = Scene::default();
    let text = Rect::new(100.0, 100.0, 400.0, 200.0);
    let logo = Rect::new(1700.0, 50.0, 150.0, 150.0);
    scene
        .stack
        .swap(Slot::Text, Some(boxed(GREEN, text)), 1000, t0);
    let settled = t0 + ms(2000);
    let text_id = scene.stack.current(Slot::Text).unwrap().id;
    scene
        .stack
        .swap(Slot::Logo, Some(boxed(WHITE, logo)), 1000, settled);
    for at in [0, 300, 700] {
        let now = settled + ms(at);
        let frame = render(&mut r, &scene, 1080, now);
        close(
            pixel(&frame, 1920, 300, 200),
            bytes(GREEN),
            "text stays opaque",
        );
        let fade = at as f32 / 1000.0;
        close(
            pixel(&frame, 1920, 1775, 125),
            bytes([fade; 4]),
            "logo fades in",
        );
    }
    assert_eq!(scene.stack.current(Slot::Text).unwrap().id, text_id);
    assert_eq!(scene.stack.nodes(Slot::Text).len(), 1);
}

#[test]
fn a_keyed_background_with_the_same_key_is_kept_without_a_fade() {
    let t0 = Instant::now();
    let mut stack = Stack::default();
    assert!(!stack.set_keyed(
        Slot::Background,
        Some((solid(RED), "VIDEO:a.mp4:".into())),
        800,
        t0
    ));
    let id = stack.current(Slot::Background).unwrap().id;
    let mut dim = solid(RED);
    dim.opacity = 0.5;
    let later = t0 + ms(2000);
    assert!(stack.set_keyed(
        Slot::Background,
        Some((dim, "VIDEO:a.mp4:".into())),
        800,
        later
    ));
    let node = stack.current(Slot::Background).unwrap();
    assert_eq!((node.id, node.content.opacity), (id, 0.5));
    assert_eq!(node.fade(later), 1.0);
    // A different file crossfades; the old node is then out of reach.
    assert!(!stack.set_keyed(
        Slot::Background,
        Some((solid(BLUE), "VIDEO:b.mp4:".into())),
        800,
        later
    ));
    assert_eq!(stack.nodes(Slot::Background).len(), 2);
    assert!(!stack.set_keyed(
        Slot::Background,
        Some((solid(RED), "VIDEO:a.mp4:".into())),
        800,
        later
    ));
    assert_eq!(
        stack.nodes(Slot::Background).len(),
        3,
        "a leaving node is not revived"
    );
}

#[test]
fn the_text_slot_sits_between_elements_and_above_the_camera_in_full_mode() {
    let Some(mut r) = renderer() else { return };
    let t0 = Instant::now();
    let mut scene = Scene::default();
    let middle = Rect::new(800.0, 400.0, 320.0, 280.0);
    scene.stack.swap(Slot::Background, Some(solid(RED)), 0, t0);
    scene.stack.swap(Slot::Camera, Some(solid(WHITE)), 0, t0);
    scene.restyle_camera(1080);
    scene
        .stack
        .swap(Slot::Text, Some(boxed(GREEN, middle)), 0, t0);
    scene.stack.swap(
        Slot::Logo,
        Some(boxed(BLUE, Rect::new(900.0, 500.0, 50.0, 50.0))),
        0,
        t0,
    );
    scene.layout = video_layout(Some(&json!({"layout": "full"}))).unwrap();
    let frame = render(&mut r, &scene, 1080, t0);
    close(
        pixel(&frame, 1920, 10, 10),
        bytes(WHITE),
        "camera covers the background",
    );
    close(
        pixel(&frame, 1920, 820, 420),
        bytes(GREEN),
        "words above the camera",
    );
    close(pixel(&frame, 1920, 920, 520), bytes(BLUE), "logo on top");
}

#[test]
fn pip_puts_the_camera_above_everything_in_its_box_with_radius_and_shadow() {
    let Some(mut r) = renderer() else { return };
    let t0 = Instant::now();
    let mut scene = Scene {
        layout: video_layout(Some(&json!({
            "layout": "pip", "radiusPx": 24,
            "box": {"xPct": 60, "yPct": 55, "widthPct": 30, "heightPct": 30}
        })))
        .unwrap(),
        ..Scene::default()
    };
    scene
        .stack
        .swap(Slot::Background, Some(solid([0.5, 0.5, 0.5, 1.0])), 0, t0);
    scene.stack.swap(Slot::Text, Some(solid(GREEN)), 0, t0);
    scene.stack.swap(Slot::Camera, Some(solid(WHITE)), 0, t0);
    scene.restyle_camera(1080);
    let placed = scene.stack.current(Slot::Camera).unwrap().content.clone();
    assert_eq!(placed.rect, Some(Rect::new(1152.0, 594.0, 576.0, 324.0)));
    assert_eq!(placed.radius, 24.0);
    assert!(placed.shadow.is_some());
    let frame = render(&mut r, &scene, 1080, t0);
    close(
        pixel(&frame, 1920, 1440, 756),
        bytes(WHITE),
        "camera above the words",
    );
    close(
        pixel(&frame, 1920, 100, 100),
        bytes(GREEN),
        "words elsewhere",
    );
    // The rounded corner leaves the words showing at the box's very corner.
    let corner = pixel(&frame, 1920, 1153, 595);
    assert!(
        corner[0] < 128 && corner[1] > 100,
        "rounded corner {corner:?}"
    );
    // The shadow darkens the words just below the box.
    let below = pixel(&frame, 1920, 1440, 924);
    assert!(below[1] < 250, "shadow below {below:?}");
    golden("pip", 1920, 1080, &frame);
}

#[test]
fn split_shrinks_the_presentation_beside_the_camera_on_black() {
    let Some(mut r) = renderer() else { return };
    let t0 = Instant::now();
    let mut scene = Scene {
        layout: video_layout(Some(
            &json!({"layout": "split", "splitPct": 40, "side": "right"}),
        ))
        .unwrap(),
        ..Scene::default()
    };
    scene.stack.swap(Slot::Background, Some(solid(RED)), 0, t0);
    scene.stack.swap(Slot::Camera, Some(solid(WHITE)), 0, t0);
    scene.restyle_camera(1080);
    let frame = render(&mut r, &scene, 1080, t0);
    // Presentation: 60% size at the left, centred vertically (216 to 864).
    close(pixel(&frame, 1920, 500, 540), bytes(RED), "presentation");
    close(
        pixel(&frame, 1920, 500, 100),
        bytes([0.0, 0.0, 0.0, 1.0]),
        "backdrop above it",
    );
    close(
        pixel(&frame, 1920, 1500, 100),
        bytes(WHITE),
        "camera on the right",
    );
    golden("split", 1920, 1080, &frame);
    // With the camera gone the presentation takes the frame again.
    scene.stack.swap(Slot::Camera, None, 0, t0);
    scene.restyle_camera(1080);
    let frame = render(&mut r, &scene, 1080, t0);
    close(
        pixel(&frame, 1920, 1500, 100),
        bytes(RED),
        "full frame without a camera",
    );
}

#[test]
fn documents_have_an_opaque_black_program_background() {
    let Some(mut r) = renderer() else { return };
    let scene = Scene {
        program_background: Some([0.0, 0.0, 0.0, 1.0]),
        ..Scene::default()
    };
    let frame = render(&mut r, &scene, 1080, Instant::now());
    close(pixel(&frame, 1920, 0, 0), [0, 0, 0, 255], "black");
}

#[test]
fn display_windows_letterbox_the_frame_on_black() {
    let Some(mut r) = renderer() else { return };
    let mut scene = Scene::default();
    let half_red = [0.5, 0.0, 0.0, 0.5];
    scene
        .stack
        .swap(Slot::Background, Some(solid(half_red)), 0, Instant::now());
    let target = r.target(1080);
    let window = r.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("window"),
        size: wgpu::Extent3d {
            width: 800,
            height: 800,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = window.create_view(&Default::default());
    let mut encoder = r.device().create_command_encoder(&Default::default());
    r.draw_frame(&mut encoder, &scene, &target, Instant::now());
    r.present(
        &mut encoder,
        &target,
        &view,
        wgpu::TextureFormat::Rgba8Unorm,
        (800, 800),
    );
    r.queue().submit([encoder.finish()]);
    let shown = r.read_texture(&window);
    // 800 wide: the frame is 450 tall, from y 175 to 625, on black.
    close(pixel(&shown, 800, 400, 100), [0, 0, 0, 255], "bar above");
    close(pixel(&shown, 800, 400, 700), [0, 0, 0, 255], "bar below");
    // Half-transparent red over the body's black.
    close(pixel(&shown, 800, 400, 400), [128, 0, 0, 255], "frame");
    golden("letterbox", 800, 800, &shown);
}

#[test]
fn frame_targets_follow_the_frame_height_limits() {
    let Some(r) = renderer() else { return };
    assert_eq!((r.target(1080).width, r.target(1080).height), (1920, 1080));
    assert_eq!(r.target(100).height, 360);
    assert_eq!(r.target(9000).height, 4320);
    assert_eq!(r.target(0).height, 1080);
}

#[test]
fn transition_times_are_clamped() {
    assert_eq!(clamp_transition(-5.0), 0);
    assert_eq!(clamp_transition(f64::NAN), 0);
    assert_eq!(clamp_transition(250.7), 250);
    assert_eq!(clamp_transition(9000.0), 5000);
}

#[test]
fn an_image_raster_is_drawn_into_its_box() {
    let Some(mut r) = renderer() else { return };
    let raster = Raster::new(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 255]);
    let mut scene = Scene::default();
    scene.stack.swap(
        Slot::Props,
        Some(Content {
            rect: Some(Rect::new(0.0, 0.0, 200.0, 100.0)),
            ..Content::new(Fill::Image(raster))
        }),
        0,
        Instant::now(),
    );
    let frame = render(&mut r, &scene, 1080, Instant::now());
    close(pixel(&frame, 1920, 10, 50), [255, 0, 0, 255], "left texel");
    close(
        pixel(&frame, 1920, 190, 50),
        [0, 0, 255, 255],
        "right texel",
    );
    close(
        pixel(&frame, 1920, 300, 50),
        [0, 0, 0, 0],
        "outside the box",
    );
}
