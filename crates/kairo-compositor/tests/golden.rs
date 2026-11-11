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

// ── Prompt 053: pictures, filters, blur, gradients, groups, animation ──────

/// A 20 × 20 card of four flat quadrants: red, green over blue, yellow.
fn card() -> std::sync::Arc<Raster> {
    let mut pixels = Vec::new();
    for y in 0..20 {
        for x in 0..20 {
            pixels.extend_from_slice(match (x < 10, y < 10) {
                (true, true) => &[255, 0, 0, 255],
                (false, true) => &[0, 255, 0, 255],
                (true, false) => &[0, 0, 255, 255],
                (false, false) => &[255, 255, 0, 255],
            });
        }
    }
    Raster::new(20, 20, pixels)
}

fn picture(rect: Rect, look: Look) -> Content {
    Content {
        rect: Some(rect),
        look,
        ..Content::new(Fill::Image(card()))
    }
}

fn scene_with(slot: Slot, content: Content) -> Scene {
    let mut scene = Scene::default();
    scene.stack.swap(slot, Some(content), 0, Instant::now());
    scene
}

fn frame_of(scene: &Scene) -> Option<Vec<u8>> {
    let mut r = renderer()?;
    Some(render(&mut r, scene, 1080, Instant::now()))
}

#[test]
fn object_fit_places_a_picture_in_its_box() {
    let box_ = Rect::new(100.0, 100.0, 400.0, 200.0);
    // contain: the square card is 200 square in the middle of a 400 × 200 box.
    let Some(f) = frame_of(&scene_with(
        Slot::Props,
        picture(
            box_,
            Look {
                fit: Fit::Contain,
                ..Look::default()
            },
        ),
    )) else {
        return;
    };
    close(pixel(&f, 1920, 150, 150), [0, 0, 0, 0], "letterbox left");
    close(
        pixel(&f, 1920, 220, 150),
        [255, 0, 0, 255],
        "top left texel",
    );
    close(
        pixel(&f, 1920, 380, 150),
        [0, 255, 0, 255],
        "top right texel",
    );
    close(
        pixel(&f, 1920, 220, 250),
        [0, 0, 255, 255],
        "bottom left texel",
    );
    // cover: 400 square, cut to the box: the middle band of the card.
    let f = frame_of(&scene_with(
        Slot::Props,
        picture(
            box_,
            Look {
                fit: Fit::Cover,
                ..Look::default()
            },
        ),
    ))
    .unwrap();
    close(
        pixel(&f, 1920, 150, 150),
        [255, 0, 0, 255],
        "cover top left",
    );
    close(
        pixel(&f, 1920, 450, 280),
        [255, 255, 0, 255],
        "cover: the middle band shows",
    );
    close(pixel(&f, 1920, 300, 320), [0, 0, 0, 0], "outside the box");
    // fill: stretched.
    let f = frame_of(&scene_with(
        Slot::Props,
        picture(
            box_,
            Look {
                fit: Fit::Fill,
                ..Look::default()
            },
        ),
    ))
    .unwrap();
    close(pixel(&f, 1920, 150, 150), [255, 0, 0, 255], "fill top left");
    close(
        pixel(&f, 1920, 450, 280),
        [255, 255, 0, 255],
        "fill bottom right",
    );
}

#[test]
fn flips_and_turns_move_the_picture_about_the_box_centre() {
    let box_ = Rect::new(100.0, 100.0, 200.0, 200.0);
    let f = frame_of(&scene_with(
        Slot::Props,
        picture(
            box_,
            Look {
                flip_x: true,
                ..Look::default()
            },
        ),
    ))
    .unwrap();
    close(
        pixel(&f, 1920, 120, 120),
        [0, 255, 0, 255],
        "x flipped: green top left",
    );
    let f = frame_of(&scene_with(
        Slot::Props,
        picture(
            box_,
            Look {
                flip_y: true,
                ..Look::default()
            },
        ),
    ))
    .unwrap();
    close(
        pixel(&f, 1920, 120, 120),
        [0, 0, 255, 255],
        "y flipped: blue top left",
    );
    // A quarter turn clockwise: the top-left red texel ends top right.
    let f = frame_of(&scene_with(
        Slot::Props,
        picture(
            box_,
            Look {
                rotation_deg: 90.0,
                ..Look::default()
            },
        ),
    ))
    .unwrap();
    close(
        pixel(&f, 1920, 280, 120),
        [255, 0, 0, 255],
        "red moved top right",
    );
    close(
        pixel(&f, 1920, 120, 120),
        [0, 0, 255, 255],
        "blue moved top left",
    );
    // Flip then turn: flip x, then 90 clockwise puts green (top right after
    // the flip's red/green swap) bottom... checked against the CSS order.
    let f = frame_of(&scene_with(
        Slot::Props,
        picture(
            box_,
            Look {
                flip_x: true,
                rotation_deg: 90.0,
                ..Look::default()
            },
        ),
    ))
    .unwrap();
    close(
        pixel(&f, 1920, 280, 120),
        [0, 255, 0, 255],
        "flipped, then turned",
    );
}

/// CSS `hue-rotate`, `saturate`, `brightness`, `contrast` on straight colour.
fn css_filter(rgb: [f32; 3], hue: f32, sat: f32, bri: f32, con: f32) -> [u8; 3] {
    let mut c = rgb;
    let clamp = |v: f32| v.clamp(0.0, 1.0);
    if hue != 0.0 {
        let (s, co) = hue.to_radians().sin_cos();
        let m = [
            [
                0.213 + co * 0.787 - s * 0.213,
                0.715 - co * 0.715 - s * 0.715,
                0.072 - co * 0.072 + s * 0.928,
            ],
            [
                0.213 - co * 0.213 + s * 0.143,
                0.715 + co * 0.285 + s * 0.140,
                0.072 - co * 0.072 - s * 0.283,
            ],
            [
                0.213 - co * 0.213 - s * 0.787,
                0.715 - co * 0.715 + s * 0.715,
                0.072 + co * 0.928 + s * 0.072,
            ],
        ];
        c = [0, 1, 2].map(|i| clamp(m[i][0] * c[0] + m[i][1] * c[1] + m[i][2] * c[2]));
    }
    if sat != 1.0 {
        let m = [
            [
                0.213 + 0.787 * sat,
                0.715 - 0.715 * sat,
                0.072 - 0.072 * sat,
            ],
            [
                0.213 - 0.213 * sat,
                0.715 + 0.285 * sat,
                0.072 - 0.072 * sat,
            ],
            [
                0.213 - 0.213 * sat,
                0.715 - 0.715 * sat,
                0.072 + 0.928 * sat,
            ],
        ];
        c = [0, 1, 2].map(|i| clamp(m[i][0] * c[0] + m[i][1] * c[1] + m[i][2] * c[2]));
    }
    c = c.map(|v| clamp(v * bri));
    c = c.map(|v| clamp((v - 0.5) * con + 0.5));
    c.map(|v| (v * 255.0).round() as u8)
}

#[test]
fn colour_filters_match_the_css_definitions_in_order() {
    let rgb = [0.8_f32, 0.4, 0.2];
    let color = [rgb[0], rgb[1], rgb[2], 1.0];
    for (hue, sat, bri, con) in [
        (90.0, 1.0, 1.0, 1.0),
        (-140.0, 1.0, 1.0, 1.0),
        (0.0, 1.8, 1.0, 1.0),
        (0.0, 0.2, 1.0, 1.0),
        (0.0, 1.0, 1.5, 1.0),
        (0.0, 1.0, 0.4, 1.0),
        (0.0, 1.0, 1.0, 1.6),
        (0.0, 1.0, 1.0, 0.4),
        (40.0, 1.4, 1.2, 0.8),
        (180.0, 2.0, 1.75, 1.75),
    ] {
        let raster = Raster::new(1, 1, color.map(|v| (v * 255.0).round() as u8).to_vec());
        let look = Look {
            filter: ColorFilter {
                hue_deg: hue,
                saturation: sat,
                brightness: bri,
                contrast: con,
            },
            ..Look::default()
        };
        let content = Content {
            rect: Some(Rect::new(0.0, 0.0, 100.0, 100.0)),
            look,
            ..Content::new(Fill::Image(raster))
        };
        let Some(f) = frame_of(&scene_with(Slot::Props, content)) else {
            return;
        };
        let want = css_filter(
            [204.0 / 255.0, 102.0 / 255.0, 51.0 / 255.0],
            hue,
            sat,
            bri,
            con,
        );
        let got = pixel(&f, 1920, 50, 50);
        for i in 0..3 {
            assert!(
                (i32::from(got[i]) - i32::from(want[i])).abs() <= 2,
                "hue {hue} sat {sat} bri {bri} con {con}: {got:?} vs {want:?}"
            );
        }
        assert_eq!(got[3], 255);
    }
}

#[test]
fn a_filter_leaves_transparent_pixels_transparent() {
    let raster = Raster::new(1, 1, vec![0, 0, 0, 0]);
    let content = Content {
        rect: Some(Rect::new(0.0, 0.0, 50.0, 50.0)),
        look: Look {
            filter: ColorFilter {
                brightness: 1.5,
                contrast: 0.4,
                ..ColorFilter::IDENTITY
            },
            ..Look::default()
        },
        ..Content::new(Fill::Image(raster))
    };
    let Some(f) = frame_of(&scene_with(Slot::Props, content)) else {
        return;
    };
    close(
        pixel(&f, 1920, 25, 25),
        [0, 0, 0, 0],
        "transparent stays transparent",
    );
}

#[test]
fn blur_has_the_css_sigma_and_fades_the_edge_into_transparent() {
    // A white 200 square, blurred by 10: the edge sits at half, one sigma
    // inside is 84% (the Gaussian CDF), far inside is solid.
    let content = Content {
        rect: Some(Rect::new(400.0, 400.0, 200.0, 200.0)),
        look: Look {
            blur_px: 10.0,
            ..Look::default()
        },
        ..Content::new(Fill::Image(Raster::new(1, 1, vec![255, 255, 255, 255])))
    };
    let Some(f) = frame_of(&scene_with(Slot::Props, content)) else {
        return;
    };
    let at = |x: u32| pixel(&f, 1920, x, 500)[3] as f32 / 255.0;
    assert!((at(400) - 0.5).abs() < 0.03, "edge {}", at(400));
    assert!((at(410) - 0.841).abs() < 0.03, "one sigma in {}", at(410));
    // The blurred picture is cut to its box, as `overflow: hidden` cuts it.
    assert!(at(390) < 0.01, "outside the box {}", at(390));
    assert!(at(500) > 0.99, "centre {}", at(500));
    assert!(at(360) < 0.01, "far out {}", at(360));
}

#[test]
fn a_blurred_background_scales_past_its_edges_and_stays_inside_the_frame() {
    // The cover picture is blurred, then scaled by 1 + blur * 4 / 1080 about
    // the centre and cut to the frame, so the frame's own edges stay solid.
    let blur = 20.0;
    let content = Content {
        rect: None,
        look: Look {
            fit: Fit::Fill,
            blur_px: blur,
            blur_first: true,
            scale: 1.0 + blur * 4.0 / 1080.0,
            ..Look::default()
        },
        ..Content::new(Fill::Image(Raster::new(1, 1, vec![200, 100, 50, 255])))
    };
    let Some(f) = frame_of(&scene_with(Slot::Background, content)) else {
        return;
    };
    // The corner is 2 sigma inside the scaled edge vertically: 97.7% covered.
    let corner = pixel(&f, 1920, 0, 0);
    assert!(corner[3] >= 246, "corner {corner:?}");
    assert!(
        pixel(&f, 1920, 960, 3)[3] >= 250,
        "top edge middle is nearly solid"
    );
    close(pixel(&f, 1920, 960, 540), [200, 100, 50, 255], "centre");
}

#[test]
fn colour_then_blur_and_blur_then_colour_differ_where_colours_mix() {
    let base = |first: bool| Content {
        rect: Some(Rect::new(100.0, 100.0, 400.0, 400.0)),
        look: Look {
            blur_px: 12.0,
            blur_first: first,
            filter: ColorFilter {
                contrast: 2.0,
                ..ColorFilter::IDENTITY
            },
            ..Look::default()
        },
        ..Content::new(Fill::Image(card()))
    };
    let Some(a) = frame_of(&scene_with(Slot::Props, base(false))) else {
        return;
    };
    let b = frame_of(&scene_with(Slot::Props, base(true))).unwrap();
    // Away from any boundary both orders give the filtered flat colour.
    close(
        pixel(&a, 1920, 180, 180),
        pixel(&b, 1920, 180, 180),
        "interior",
    );
    // On the boundary between red and green, mixing then contrasting is not
    // contrasting then mixing.
    assert_ne!(pixel(&a, 1920, 300, 180), pixel(&b, 1920, 300, 180));
}

#[test]
fn a_gradient_follows_css_angles_and_interpolates_premultiplied() {
    let gradient = |angle: f32| Content {
        rect: Some(Rect::new(0.0, 0.0, 400.0, 200.0)),
        ..Content::new(Fill::Gradient {
            from: RED,
            to: BLUE,
            angle_deg: angle,
        })
    };
    let Some(f) = frame_of(&scene_with(Slot::Props, gradient(90.0))) else {
        return;
    };
    // 90deg: left to right.
    close(pixel(&f, 1920, 2, 100), [255, 0, 0, 255], "start");
    close(pixel(&f, 1920, 200, 100), [128, 0, 128, 255], "middle");
    close(pixel(&f, 1920, 397, 100), [3, 0, 252, 255], "end");
    // 0deg: bottom to top (the first colour at the bottom).
    let f = frame_of(&scene_with(Slot::Props, gradient(0.0))).unwrap();
    close(
        pixel(&f, 1920, 200, 199),
        [254, 0, 1, 255],
        "0deg starts at the bottom",
    );
    close(
        pixel(&f, 1920, 200, 0),
        [1, 0, 254, 255],
        "0deg ends at the top",
    );
    // 45deg on a 400 × 200 box: the corners are exactly the end colours.
    let f = frame_of(&scene_with(Slot::Props, gradient(45.0))).unwrap();
    close(
        pixel(&f, 1920, 1, 198),
        [255, 0, 0, 255],
        "bottom left corner",
    );
    close(
        pixel(&f, 1920, 398, 1),
        [0, 0, 255, 255],
        "top right corner",
    );
    // Premultiplied: red to transparent blue stays red-ish, not purple.
    let alpha = Content {
        rect: Some(Rect::new(0.0, 0.0, 400.0, 200.0)),
        ..Content::new(Fill::Gradient {
            from: RED,
            to: [0.0; 4],
            angle_deg: 90.0,
        })
    };
    let f = frame_of(&scene_with(Slot::Props, alpha)).unwrap();
    close(
        pixel(&f, 1920, 200, 100),
        [128, 0, 0, 128],
        "premultiplied middle",
    );
}

#[test]
fn overlapping_children_fade_as_one_layer() {
    // Two opaque overlapping boxes at 50%: where they overlap the lower one
    // must not show through (group opacity, not per-box opacity).
    let group = Content::new(Fill::Group(std::sync::Arc::new(vec![
        boxed(RED, Rect::new(100.0, 100.0, 200.0, 200.0)),
        boxed(BLUE, Rect::new(200.0, 100.0, 200.0, 200.0)),
    ])));
    let mut scene = Scene::default();
    let t0 = Instant::now();
    scene.stack.swap(Slot::ElementsBelow, Some(group), 1000, t0);
    let Some(mut r) = renderer() else { return };
    let f = render(&mut r, &scene, 1080, t0 + ms(500));
    close(
        pixel(&f, 1920, 150, 200),
        [128, 0, 0, 128],
        "red alone at half",
    );
    close(
        pixel(&f, 1920, 250, 200),
        [0, 0, 128, 128],
        "overlap is blue only, at half",
    );
    close(
        pixel(&f, 1920, 350, 200),
        [0, 0, 128, 128],
        "blue alone at half",
    );
    // Fully in, nothing is isolated and the order holds.
    let f = render(&mut r, &scene, 1080, t0 + ms(1500));
    close(
        pixel(&f, 1920, 250, 200),
        [0, 0, 255, 255],
        "overlap is blue",
    );
}

#[test]
fn a_group_keeps_each_childs_own_opacity() {
    let mut half = boxed(RED, Rect::new(0.0, 0.0, 100.0, 100.0));
    half.opacity = 0.5;
    let group = Content::new(Fill::Group(std::sync::Arc::new(vec![half])));
    let Some(f) = frame_of(&scene_with(Slot::ElementsAbove, group)) else {
        return;
    };
    close(pixel(&f, 1920, 50, 50), [128, 0, 0, 128], "child opacity");
}

#[test]
fn animated_frames_change_with_the_nodes_age() {
    let frames = std::sync::Arc::new(Frames {
        rasters: vec![
            Raster::new(1, 1, vec![255, 0, 0, 255]),
            Raster::new(1, 1, vec![0, 255, 0, 255]),
            Raster::new(1, 1, vec![0, 0, 255, 255]),
        ],
        delays_ms: vec![100, 200, 100],
        plays: None,
    });
    let content = Content {
        rect: Some(Rect::new(0.0, 0.0, 50.0, 50.0)),
        ..Content::new(Fill::Frames(frames))
    };
    let mut scene = Scene::default();
    let t0 = Instant::now();
    scene.stack.swap(Slot::Props, Some(content), 0, t0);
    let Some(mut r) = renderer() else { return };
    for (at, want) in [
        (50, RED),
        (150, GREEN),
        (290, GREEN),
        (320, BLUE),
        (420, RED),
        (1210, RED),
    ] {
        let f = render(&mut r, &scene, 1080, t0 + ms(at));
        close(pixel(&f, 1920, 25, 25), bytes(want), &format!("at {at} ms"));
    }
}

#[test]
fn a_node_with_a_pending_fill_keeps_its_picture() {
    let mut stack = Stack::default();
    let now = Instant::now();
    let pic = |look: Look| Content {
        look,
        ..Content::new(Fill::Image(card()))
    };
    assert!(!stack.set_keyed(
        Slot::Background,
        Some((pic(Look::default()), "k".into())),
        0,
        now
    ));
    let id = stack.current(Slot::Background).unwrap().id;
    // The same file again, its pixels still being decoded: kept, new look.
    let pending = Content {
        look: Look {
            blur_px: 5.0,
            ..Look::default()
        },
        opacity: 0.5,
        ..Content::new(Fill::Pending)
    };
    assert!(stack.set_keyed(Slot::Background, Some((pending, "k".into())), 0, now));
    let node = stack.current(Slot::Background).unwrap();
    assert_eq!(node.id, id, "the node is kept");
    assert!(matches!(node.content.fill, Fill::Image(_)), "pixels kept");
    assert_eq!(node.content.look.blur_px, 5.0, "look taken");
    assert_eq!(node.content.opacity, 0.5);
    // A new file: a new node, empty until its pixels arrive.
    let new = Content::new(Fill::Pending);
    assert!(!stack.set_keyed(Slot::Background, Some((new, "other".into())), 0, now));
    assert!(matches!(
        stack.current(Slot::Background).unwrap().content.fill,
        Fill::Pending
    ));
    // Pixels for the old key arrive late: ignored. For the current key: taken.
    assert!(!stack.update_keyed(Slot::Background, "k", pic(Look::default()), now));
    assert!(stack.update_keyed(Slot::Background, "other", pic(Look::default()), now));
    assert!(matches!(
        stack.current(Slot::Background).unwrap().content.fill,
        Fill::Image(_)
    ));
}

#[test]
fn a_leaving_node_takes_no_late_pixels() {
    let mut stack = Stack::default();
    let now = Instant::now();
    stack.set_keyed(
        Slot::Background,
        Some((Content::new(Fill::Pending), "k".into())),
        500,
        now,
    );
    stack.set_keyed(Slot::Background, None, 500, now);
    assert!(!stack.update_keyed(Slot::Background, "k", Content::new(Fill::Solid(RED)), now));
}

#[test]
fn frame_timing_loops_stops_and_holds_the_last_frame() {
    let frames = |plays| Frames {
        rasters: vec![Raster::new(1, 1, vec![0; 4]); 3],
        delays_ms: vec![100, 100, 300],
        plays,
    };
    let f = frames(None);
    assert_eq!(f.index_at(ms(0)), 0);
    assert_eq!(f.index_at(ms(99)), 0);
    assert_eq!(f.index_at(ms(100)), 1);
    assert_eq!(f.index_at(ms(250)), 2);
    assert_eq!(f.index_at(ms(500)), 0, "loops");
    assert_eq!(f.next_after(ms(0)), Some(ms(100)));
    assert_eq!(f.next_after(ms(150)), Some(ms(200)));
    assert_eq!(f.next_after(ms(250)), Some(ms(500)));
    assert_eq!(f.next_after(ms(550)), Some(ms(600)), "next loop");
    let twice = frames(Some(2));
    assert_eq!(twice.index_at(ms(650)), 1, "second play");
    assert_eq!(twice.index_at(ms(1000)), 2, "done: the last frame stays");
    assert_eq!(twice.index_at(ms(99_000)), 2);
    assert_eq!(twice.next_after(ms(1000)), None);
    let still = Frames {
        rasters: vec![Raster::new(1, 1, vec![0; 4])],
        delays_ms: vec![50],
        plays: None,
    };
    assert_eq!(
        still.next_after(ms(0)),
        None,
        "a single frame never changes"
    );
}

#[test]
fn an_animation_reports_a_change_only_when_the_frame_moves() {
    let mut stack = Stack::default();
    let t0 = Instant::now();
    let frames = std::sync::Arc::new(Frames {
        rasters: vec![Raster::new(1, 1, vec![0; 4]); 2],
        delays_ms: vec![100, 100],
        plays: None,
    });
    stack.swap(Slot::Props, Some(Content::new(Fill::Frames(frames))), 0, t0);
    assert!(stack.frames_changed(t0), "the first look");
    assert!(!stack.frames_changed(t0 + ms(50)));
    assert!(stack.frames_changed(t0 + ms(120)));
    assert!(!stack.frames_changed(t0 + ms(150)));
    assert_eq!(stack.next_change(t0 + ms(150)), Some(t0 + ms(200)));
    // A still scene has nothing scheduled.
    let mut still = Stack::default();
    still.swap(Slot::Props, Some(Content::new(Fill::Solid(RED))), 0, t0);
    assert_eq!(still.next_change(t0), None);
}
