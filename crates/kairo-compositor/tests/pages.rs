//! Document pages kept on the GPU (prompt 058): a page uploaded ahead stays
//! resident however many frames pass without drawing it, and is released
//! once its owner stops keeping it.

use kairo_compositor::*;
use std::time::Instant;

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

fn draw(r: &mut Renderer, scene: &Scene) {
    let target = r.target(1080);
    let mut encoder = r.device().create_command_encoder(&Default::default());
    r.draw_frame(&mut encoder, scene, &target, Instant::now());
    r.queue().submit([encoder.finish()]);
}

#[test]
fn resident_pages_survive_idle_frames_and_are_released_after() {
    let Some(mut r) = renderer() else {
        return;
    };
    let page = Raster::new(4, 4, vec![255; 64]);
    let other = Raster::new(4, 4, vec![128; 64]);
    r.keep_resident("main", std::slice::from_ref(&page));
    assert!(r.has_texture(&page), "uploaded before any frame shows it");
    let scene = Scene::default();
    for _ in 0..300 {
        draw(&mut r, &scene);
    }
    assert!(r.has_texture(&page), "kept past the idle expiry");
    // The owner now keeps another page: the first falls to the expiry.
    r.keep_resident("main", std::slice::from_ref(&other));
    for _ in 0..300 {
        draw(&mut r, &scene);
    }
    assert!(
        !r.has_texture(&page),
        "released after it stopped being kept"
    );
    assert!(r.has_texture(&other));
    r.keep_resident("main", &[]);
    for _ in 0..300 {
        draw(&mut r, &scene);
    }
    assert!(!r.has_texture(&other), "an empty list releases everything");
}
