//! Syphon end to end, ported from Electron's `native/syphon/tests/syphon.test.mjs`:
//! a frame goes in as an IOSurface or CPU pixels, is published, and a Syphon
//! client reads it back the way OBS or Resolume would see it.
#![cfg(target_os = "macos")]

use kairo_native_sys::syphon::{IoSurface, Server, available};
use std::sync::mpsc;
use std::time::Duration;

/// Left half opaque red, right half half-transparent blue (premultiplied),
/// one green pixel at (1, 1).
fn frame(width: u32, height: u32) -> Vec<u8> {
    let mut bgra = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let i = ((y * width + x) * 4) as usize;
            let px = if x < width / 2 {
                [0, 0, 255, 255]
            } else {
                [128, 0, 0, 128]
            };
            bgra[i..i + 4].copy_from_slice(&px);
        }
    }
    let i = ((width + 1) * 4) as usize;
    bgra[i..i + 4].copy_from_slice(&[0, 255, 0, 255]);
    bgra
}

/// Publishes one surface; true once the GPU copy is done, false when dropped.
fn publish(server: &Server, surface: &IoSurface) -> bool {
    let (tx, rx) = mpsc::channel();
    let started = server
        .publish(surface, move |error| {
            let _ = tx.send(error);
        })
        .expect("publish");
    if !started {
        return false;
    }
    let error = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("copy finished");
    assert_eq!(error, None);
    true
}

#[test]
fn syphon_is_available_on_this_mac() {
    assert_eq!(available(), Ok(()));
}

#[test]
fn a_published_frame_reaches_a_client_pixel_for_pixel_alpha_included() {
    let (width, height) = (64, 36);
    let bgra = frame(width, height);
    let surface = IoSurface::from_bgra(&bgra, width, height).expect("surface");
    let server = Server::open("Kairo test").expect("open");
    assert!(publish(&server, &surface));
    let (w, h, seen) = server.read_latest(3000.0).unwrap().expect("a frame");
    assert_eq!((w, h), (width, height));
    assert!(
        seen == bgra,
        "every pixel, transparency included, unchanged"
    );
    assert_eq!(server.stats().published, 1);
}

#[test]
fn a_frame_sent_while_the_previous_one_copies_is_dropped_not_queued() {
    let surface = IoSurface::from_bgra(&frame(1920, 1080), 1920, 1080).unwrap();
    let server = Server::open("Kairo busy test").unwrap();
    let (tx, rx) = mpsc::channel();
    let mut results = Vec::new();
    for _ in 0..5 {
        let tx = tx.clone();
        results.push(
            server
                .publish(&surface, move |_| {
                    let _ = tx.send(());
                })
                .unwrap(),
        );
    }
    let started = results.iter().filter(|r| **r).count();
    for _ in 0..started {
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
    }
    assert!(results[0]);
    let stats = server.stats();
    assert_eq!(stats.published + stats.skipped, 5);
    assert!(
        stats.skipped >= 1,
        "back-to-back frames dropped while one is in flight"
    );
}

#[test]
fn a_closed_server_refuses_frames_instead_of_crashing() {
    let surface = IoSurface::from_bgra(&frame(16, 16), 16, 16).unwrap();
    let server = Server::open("Kairo closed test").unwrap();
    server.close();
    server.close();
    let error = server.publish(&surface, |_| {}).unwrap_err();
    assert!(error.contains("closed"), "{error}");
    assert!(
        server
            .publish_pixels(&frame(16, 16), 16, 16, 0)
            .unwrap_err()
            .contains("closed")
    );
    assert!(server.surface(16, 16).unwrap_err().contains("closed"));
    assert!(server.publish_surface().unwrap_err().contains("closed"));
    assert!(server.read_latest(10.0).unwrap_err().contains("closed"));
    assert!(!server.stats().has_clients);
}

#[test]
fn a_size_change_between_frames_is_followed() {
    let server = Server::open("Kairo resize test").unwrap();
    let small = IoSurface::from_bgra(&frame(32, 18), 32, 18).unwrap();
    let large = IoSurface::from_bgra(&frame(64, 36), 64, 36).unwrap();
    assert!(publish(&server, &small));
    assert_eq!(server.read_latest(3000.0).unwrap().unwrap().0, 32);
    assert!(publish(&server, &large));
    assert_eq!(server.read_latest(3000.0).unwrap().unwrap().0, 64);
}

#[test]
fn pixels_from_the_cpu_path_are_published_as_they_are() {
    let (width, height) = (40, 20);
    let bgra = frame(width, height);
    let server = Server::open("Kairo pixels test").unwrap();
    assert_eq!(server.publish_pixels(&bgra, width, height, 0), Ok(true));
    let seen = server.read_latest(3000.0).unwrap().unwrap();
    assert!(seen.2 == bgra, "every pixel unchanged");
    let error = server
        .publish_pixels(&[0; 10], width, height, 0)
        .unwrap_err();
    assert!(error.contains("width × height"), "{error}");
    assert!(server.publish_pixels(&bgra, 0, height, 0).is_err());
    // Padded rows are read at their stride.
    let mut padded = vec![0u8; (width as usize * 4 + 16) * height as usize];
    for y in 0..height as usize {
        let row = &bgra[y * width as usize * 4..(y + 1) * width as usize * 4];
        padded[y * (width as usize * 4 + 16)..][..row.len()].copy_from_slice(row);
    }
    assert_eq!(
        server.publish_pixels(&padded, width, height, width as usize * 4 + 16),
        Ok(true)
    );
    assert!(server.read_latest(3000.0).unwrap().unwrap().2 == bgra);
}

#[test]
fn the_server_surface_keeps_its_id_until_the_size_changes() {
    let server = Server::open("Kairo surface test").unwrap();
    let a = server.surface(32, 18).unwrap();
    let b = server.surface(32, 18).unwrap();
    assert_eq!(a.id(), b.id());
    let c = server.surface(64, 36).unwrap();
    assert_ne!(a.id(), c.id());
    assert!(server.surface(0, 10).unwrap_err().contains("empty"));
    assert_eq!(server.publish_surface(), Ok(()));
    assert_eq!(server.stats().published, 1);
}

#[test]
fn a_name_with_a_nul_is_refused_and_an_empty_one_falls_back() {
    assert!(Server::open("Kairo\0x").is_err());
    let server = Server::open("").unwrap();
    assert_eq!(server.publish_pixels(&frame(4, 2), 4, 2, 0), Ok(true));
}
