//! What a filtered 1080p video layer costs per frame (prompt 053).
//!
//! There is no video decoder until prompt 055, so each frame is a fresh
//! 1920 × 1080 upload into a source texture, as a decoder's output would be,
//! then a draw with the look under test and a wait for the GPU to finish.
//! Ignored by default: it wants a real adapter and prints numbers.
//!
//!     cargo test -p kairo-compositor --release --test perf -- --ignored --nocapture

use kairo_compositor::*;
use std::time::Instant;

fn measure(name: &str, look: Look, frames: usize) {
    let mut r = Renderer::headless(false).expect("a GPU");
    let info = r.adapter_info().clone();
    let frame = {
        let pixels: Vec<u8> = (0..1920 * 1080 * 4).map(|i| (i % 253) as u8).collect();
        Raster::new(1920, 1080, pixels)
    };
    let mut scene = Scene::default();
    scene.stack.swap(
        Slot::Background,
        Some(Content {
            look,
            ..Content::new(Fill::Source {
                id: 1,
                backdrop: [0.0; 4],
            })
        }),
        0,
        Instant::now(),
    );
    let target = r.target(1080);
    let mut ms = Vec::new();
    for i in 0..frames + 20 {
        let started = Instant::now();
        r.update_source(1, &frame);
        let mut encoder = r.device().create_command_encoder(&Default::default());
        r.draw_frame(&mut encoder, &scene, &target, Instant::now());
        r.queue().submit([encoder.finish()]);
        r.device()
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll");
        if i >= 20 {
            ms.push(started.elapsed().as_secs_f64() * 1000.0);
        }
    }
    ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p = |q: f64| ms[((ms.len() - 1) as f64 * q) as usize];
    eprintln!(
        "{name:<34} p50 {:.2} ms  p95 {:.2} ms  max {:.2} ms   ({})",
        p(0.5),
        p(0.95),
        ms[ms.len() - 1],
        info.name
    );
}

#[test]
#[ignore = "needs a GPU; prints numbers"]
fn filtered_1080p_video_costs_per_frame() {
    measure("plain (upload + draw)", Look::default(), 300);
    let colour = ColorFilter {
        hue_deg: 40.0,
        saturation: 1.4,
        brightness: 1.2,
        contrast: 0.8,
    };
    measure(
        "colour filter",
        Look {
            filter: colour,
            ..Look::default()
        },
        300,
    );
    for blur in [4.0, 12.0, 40.0] {
        measure(
            &format!("blur {blur} + colour (background)"),
            Look {
                filter: colour,
                blur_px: blur,
                blur_first: true,
                scale: 1.0 + blur * 4.0 / 1080.0,
                ..Look::default()
            },
            200,
        );
    }
}
