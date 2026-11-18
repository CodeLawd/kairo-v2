//! The UYVA conversion against its CPU reference (Electron's `uyvy.h`).

use kairo_compositor::uyva::{SLOTS, UyvaConverter, preview_reference, reference, uyva_len};
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok();
    let Some(adapter) = adapter else {
        assert!(std::env::var_os("KAIRO_REQUIRE_GPU").is_none(), "no GPU");
        eprintln!("no GPU adapter; skipped");
        return None;
    };
    pollster::block_on(adapter.request_device(&Default::default())).ok()
}

fn texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    rgba: &[u8],
    w: u32,
    h: u32,
) -> wgpu::Texture {
    let t = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        t.as_image_copy(),
        rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(w * 4),
            rows_per_image: Some(h),
        },
        t.size(),
    );
    t
}

fn convert(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    rgba: &[u8],
    src: (u32, u32),
    dst: (u32, u32),
    alpha: bool,
) -> Vec<u8> {
    let t = texture(device, queue, rgba, src.0, src.1);
    let c = UyvaConverter::new(device, dst.0, dst.1, alpha).unwrap();
    let (tx, rx) = mpsc::channel();
    assert!(c.convert(
        device,
        queue,
        &t,
        Box::new(move |r| tx.send(r.map(|f| f.data().to_vec())).unwrap())
    ));
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let out = rx.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
    // Only the UYVY plane is compared when there is no alpha plane.
    let n = dst.0 as usize * dst.1 as usize * if alpha { 3 } else { 2 };
    out[..n].to_vec()
}

/// Premultiplied, as Chromium paints it; Electron's test gradient in RGBA order.
fn gradient(w: u32, h: u32) -> Vec<u8> {
    let mut out = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let a = (x * 7 + y * 3) % 256;
            let b = a.min(x * 255 / w);
            let g = a.min(y * 255 / h);
            let r = a.min(((x + y) * 13) % 256);
            out[i..i + 4].copy_from_slice(&[r as u8, g as u8, b as u8, a as u8]);
        }
    }
    out
}

/// BGRA pixels as Electron's vectors give them, to RGBA.
fn from_bgra(px: &[[u8; 4]]) -> Vec<u8> {
    px.iter().flat_map(|p| [p[2], p[1], p[0], p[3]]).collect()
}

#[test]
fn the_helper_self_test_vectors_match_byte_for_byte() {
    let Some((d, q)) = device() else { return };
    // Width 4: the shader writes four pixels at a time; pairs are repeated.
    let black = from_bgra(&[[0, 0, 0, 255]; 4]);
    let white = from_bgra(&[[255, 255, 255, 255]; 4]);
    let red = from_bgra(&[[0, 0, 255, 255]; 4]);
    assert_eq!(
        &reference(&black, (4, 1), (4, 1), false)[..4],
        &[128, 16, 128, 16]
    );
    assert_eq!(
        &reference(&red, (4, 1), (4, 1), false)[..3],
        &[102, 63, 240]
    );
    let w = reference(&white, (4, 1), (4, 1), false);
    assert_eq!((w[1], w[3]), (235, 235));
    for v in [&black, &white, &red] {
        assert_eq!(
            convert(&d, &q, v, (4, 1), (4, 1), false),
            reference(v, (4, 1), (4, 1), false)
        );
    }
}

#[test]
fn a_1080p_gradient_matches_the_cpu_path_including_scaling() {
    let Some((d, q)) = device() else { return };
    let src = gradient(1920, 1080);
    assert_eq!(
        convert(&d, &q, &src, (1920, 1080), (1920, 1080), false),
        reference(&src, (1920, 1080), (1920, 1080), false)
    );
    assert_eq!(
        convert(&d, &q, &src, (1920, 1080), (1280, 720), false),
        reference(&src, (1920, 1080), (1280, 720), false)
    );
    let retina = gradient(3840, 2160);
    assert_eq!(
        convert(&d, &q, &retina, (3840, 2160), (1920, 1080), false),
        reference(&retina, (3840, 2160), (1920, 1080), false)
    );
}

#[test]
fn uyva_carries_the_source_alpha_after_the_uyvy_plane() {
    let Some((d, q)) = device() else { return };
    let src = gradient(64, 4);
    let out = convert(&d, &q, &src, (64, 4), (64, 4), true);
    assert_eq!(out.len(), 64 * 4 * 3);
    assert_eq!(out, reference(&src, (64, 4), (64, 4), true));
    for i in 0..64 * 4 {
        assert_eq!(out[64 * 4 * 2 + i], src[i * 4 + 3]);
    }
}

#[test]
fn a_burst_beyond_the_slots_is_dropped_and_latency_stays_in_budget() {
    let Some((d, q)) = device() else { return };
    let t = texture(&d, &q, &gradient(1920, 1080), 1920, 1080);
    let c = UyvaConverter::new(&d, 1920, 1080, true).unwrap();
    let (tx, rx) = mpsc::channel();
    let accepted: Vec<bool> = (0..6)
        .map(|_| {
            let tx = tx.clone();
            c.convert(
                &d,
                &q,
                &t,
                Box::new(move |r| tx.send(r.map(|f| f.data().len())).unwrap()),
            )
        })
        .collect();
    // Queue submission can also deliver completed mapping callbacks. A
    // fast backend may free staging slots before the burst is over.
    assert!(accepted[..SLOTS].iter().all(|accepted| *accepted));
    let converted = accepted.iter().filter(|accepted| **accepted).count();
    d.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let burst: Vec<usize> = (0..converted)
        .map(|_| rx.recv_timeout(Duration::from_secs(5)).unwrap().unwrap())
        .collect();
    assert_eq!(burst, vec![uyva_len(1920, 1080); converted]);
    assert_eq!(
        c.stats.skipped.load(std::sync::atomic::Ordering::Relaxed),
        (accepted.len() - converted) as u64
    );

    // Submit to bytes in hand, one frame at a time.
    let mut times = Vec::new();
    for _ in 0..120 {
        let start = Instant::now();
        let tx = tx.clone();
        assert!(c.convert(
            &d,
            &q,
            &t,
            Box::new(move |r| tx.send(r.map(|f| f.data().len())).unwrap())
        ));
        d.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        rx.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
        times.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(f64::total_cmp);
    let (p50, p99) = (times[60], times[118]);
    eprintln!("1080p UYVA convert+readback: p50 {p50:.2} ms, p99 {p99:.2} ms");
    let budget = if cfg!(windows) && std::env::var("KAIRO_PERF_HOSTED").as_deref() == Ok("1") {
        // WARP exercises conversion and readback on the CPU.
        500.0
    } else {
        33.0
    };
    assert!(p99 < budget, "p99 {p99:.2} ms exceeds {budget:.2} ms");
}

#[test]
fn held_frames_make_the_converter_drop_rather_than_allocate() {
    let Some((d, q)) = device() else { return };
    let t = texture(&d, &q, &gradient(64, 4), 64, 4);
    let c = UyvaConverter::new(&d, 64, 4, true).unwrap();
    let held = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut accepted = 0;
    for _ in 0..20 {
        let h = held.clone();
        if c.convert(
            &d,
            &q,
            &t,
            Box::new(move |r| h.lock().unwrap().push(r.unwrap())),
        ) {
            accepted += 1;
        }
        d.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    }
    assert_eq!(accepted, kairo_compositor::uyva::MAX_BUFFERS);
    held.lock().unwrap().clear();
    let h = held.clone();
    assert!(c.convert(
        &d,
        &q,
        &t,
        Box::new(move |r| h.lock().unwrap().push(r.unwrap()))
    ));
    assert!(UyvaConverter::new(&d, 1918, 1080, true).is_err());
}

#[test]
fn the_bgra_tap_is_the_frame_with_red_and_blue_swapped() {
    let Some((d, q)) = device() else { return };
    let src = gradient(64, 8);
    let t = texture(&d, &q, &src, 64, 8);
    let c = UyvaConverter::bgra(&d, 64, 8).unwrap();
    let (tx, rx) = mpsc::channel();
    assert!(c.convert(
        &d,
        &q,
        &t,
        Box::new(move |r| tx.send(r.map(|f| (f.bgra, f.data().to_vec()))).unwrap())
    ));
    d.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let (bgra, out) = rx.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
    assert!(bgra);
    let expected: Vec<u8> = src
        .chunks(4)
        .flat_map(|p| [p[2], p[1], p[0], p[3]])
        .collect();
    assert_eq!(out, expected);
}

fn preview(
    d: &wgpu::Device,
    q: &wgpu::Queue,
    rgba: &[u8],
    src: (u32, u32),
    dst: (u32, u32),
) -> Vec<u8> {
    let t = texture(d, q, rgba, src.0, src.1);
    let c = UyvaConverter::preview(d, dst.0, dst.1).unwrap();
    let (tx, rx) = mpsc::channel();
    assert!(c.convert(
        d,
        q,
        &t,
        Box::new(move |r| tx.send(r.map(|f| (f.bgra, f.data().to_vec()))).unwrap())
    ));
    d.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let (bgra, out) = rx.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
    assert!(bgra);
    out
}

#[test]
fn the_preview_matches_its_cpu_reference_at_every_operator_size() {
    let Some((d, q)) = device() else { return };
    let src = gradient(1920, 1080);
    // 1080p down to the sizes an operator window asks for, a 4K frame, and
    // a non-integer ratio.
    for (from, to) in [
        ((1920, 1080), (640, 360)),
        ((1920, 1080), (960, 540)),
        ((1920, 1080), (1920, 1080)),
        ((1920, 1080), (332, 188)),
    ] {
        assert_eq!(
            preview(&d, &q, &src, from, to),
            preview_reference(&src, from, to),
            "{from:?} to {to:?}"
        );
    }
    let big = gradient(3840, 2160);
    assert_eq!(
        preview(&d, &q, &big, (3840, 2160), (640, 360)),
        preview_reference(&big, (3840, 2160), (640, 360))
    );
}

#[test]
fn the_preview_averages_and_unpremultiplies() {
    // A 2 × 2 box of red and black averages to dark red, not to nearest
    // neighbour's red; half-transparent white comes out as white at half
    // alpha, which is what GPUI expects.
    let red = [200u8, 0, 0, 255];
    let black = [0u8, 0, 0, 255];
    let mut src = Vec::new();
    for _ in 0..2 {
        src.extend_from_slice(&red);
        src.extend_from_slice(&black);
        src.extend_from_slice(&[128, 128, 128, 128]);
        src.extend_from_slice(&[128, 128, 128, 128]);
        src.extend_from_slice(&[0; 4]);
        src.extend_from_slice(&[0; 4]);
        src.extend_from_slice(&red);
        src.extend_from_slice(&red);
    }
    let out = preview_reference(&src, (8, 2), (4, 1));
    assert_eq!(&out[0..4], &[0, 0, 100, 255], "averaged");
    assert_eq!(&out[4..8], &[255, 255, 255, 128], "straight alpha");
    assert_eq!(&out[8..12], &[0, 0, 0, 0], "clear stays clear");
    assert_eq!(&out[12..16], &[0, 0, 200, 255]);
    let Some((d, q)) = device() else { return };
    assert_eq!(preview(&d, &q, &src, (8, 2), (4, 1)), out);
}
