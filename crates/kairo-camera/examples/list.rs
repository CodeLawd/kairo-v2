//! Prints the video and audio inputs with the labels Electron saved for
//! them, then opens one camera for a moment and reports its frames.
//!
//! ```text
//! cargo run -p kairo-camera --example list -- [LABEL]
//! ```

use kairo_camera::{Backend, Request};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

fn main() {
    let backend = kairo_camera::system();
    match backend.video_devices() {
        Ok(devices) => {
            println!("video inputs:");
            for d in &devices {
                println!("  {:?}  (name {:?}, id {})", d.label, d.name, d.id);
            }
            let wanted = std::env::args().nth(1);
            let device = match &wanted {
                Some(l) => kairo_camera::label::find(&devices, l),
                None => devices.first(),
            };
            if let Some(device) = device {
                open(&*backend, device);
            } else if let Some(l) = wanted {
                println!("Camera not found: {l}");
            }
        }
        Err(error) => println!("video inputs: {error}"),
    }
    println!("audio inputs:");
    for d in kairo_camera::mic::input_devices() {
        println!("  {:?}", d.label);
    }
}

fn open(backend: &dyn Backend, device: &kairo_camera::DeviceInfo) {
    let frames = Arc::new(AtomicU64::new(0));
    let first = Arc::new(std::sync::Mutex::new(None::<(u32, u32, Duration)>));
    let start = Instant::now();
    let (count, size) = (frames.clone(), first.clone());
    let sink = Box::new(move |frame: kairo_compositor::YuvFrame| {
        count.fetch_add(1, Ordering::Relaxed);
        let mut s = size.lock().unwrap();
        if s.is_none() {
            *s = Some((frame.width, frame.height, start.elapsed()));
        }
    });
    match backend.open(device, Request::PROGRAM, sink) {
        Ok(capture) => {
            let opened = start.elapsed();
            frames.store(0, Ordering::Relaxed);
            std::thread::sleep(Duration::from_secs(3));
            let n = frames.load(Ordering::Relaxed);
            println!(
                "opened {:?} in {} ms, mode {:?}, first frame {:?}, {:.1} fps over 3 s, health {:?}",
                device.label,
                opened.as_millis(),
                capture.mode(),
                first.lock().unwrap(),
                n as f64 / 3.0,
                capture.health()
            );
        }
        Err(error) => println!("open {:?}: {error}", device.label),
    }
}
