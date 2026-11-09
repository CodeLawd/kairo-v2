//! Runs the NDI, ring and converter probes and prints measured results.

use sinks_spike::{gpu, ndi, ring::Ring};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

fn uyva_test(width: u32, height: u32) -> Vec<u8> {
    let mut bgra = Vec::with_capacity((width * height * 4) as usize);
    for _ in 0..width * height {
        bgra.extend_from_slice(&[0, 0, 255, 255]);
    }
    let mut out = vec![0; (width * height * 3) as usize];
    assert_eq!(
        gpu::convert(&bgra, width, height, width, height, true, &mut out),
        0
    );
    out
}

fn ndi_probe(library: &Path) -> Result<(), String> {
    let source = format!("Kairo Spike {}", std::process::id());
    let (width, height) = (1280, 720);
    let uyva = uyva_test(width, height);
    let bgra: Vec<u8> = [0u8, 0, 255, 255].repeat((width * height) as usize);
    let audio = vec![0.0f32; 2 * 1600];
    for cycle in 1..=10 {
        let ndi = ndi::Ndi::load(library)?;
        if cycle == 1 {
            println!("ndi version {}", ndi.version()?);
        }
        let sender = ndi.sender(&source)?;
        sender.send_video(width, height, ndi::UYVA, width * 2, &uyva)?;
        sender.send_video(width, height, ndi::BGRA, width * 4, &bgra)?;
        sender.send_audio(48000, 2, 1600, &audio)?;
        drop(sender);
        drop(ndi);
    }
    println!("ndi 10 load/send/destroy cycles ok");

    let ndi = ndi::Ndi::load(library)?;
    let sender = ndi.sender(&source)?;
    assert!(
        sender
            .send_video(width, height, ndi::UYVA, width * 2, &uyva[1..])
            .is_err()
    );
    assert!(sender.send_audio(48000, 2, 1600, &audio[1..]).is_err());
    println!("ndi malformed frames rejected ok");
    let stop = std::sync::atomic::AtomicBool::new(false);
    std::thread::scope(|scope| -> Result<(), String> {
        let handle = scope.spawn(|| {
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                let _ = sender.send_video(width, height, ndi::UYVA, width * 2, &uyva);
                std::thread::sleep(Duration::from_millis(33));
            }
        });
        let found = ndi.find(15_000, &source)?;
        println!("ndi discovery saw {found:?}");
        let received = ndi.receive_one(&source, 15_000, |frame| {
            let fourcc = frame
                .fourcc
                .to_le_bytes()
                .map(char::from)
                .iter()
                .collect::<String>();
            // SAFETY: NDI guarantees stride * height bytes in a captured frame.
            let pixel = unsafe {
                std::slice::from_raw_parts(
                    frame
                        .data
                        .add((frame.yres / 2 * frame.stride + frame.xres * 2) as usize),
                    4,
                )
            };
            println!(
                "ndi loopback frame {}x{} fourcc={fourcc} stride={} centre={pixel:?}",
                frame.xres, frame.yres, frame.stride
            );
        })?;
        println!("ndi loopback received={received}");
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = handle.join();
        Ok(())
    })
}

fn ring_probe(native: &Path, out: &Path) -> Result<(), String> {
    let stub = out.join("ring_stub");
    let status = Command::new("xcrun")
        .args(["clang++", "-std=c++17", "-O2"])
        .arg(native.join("ring_stub.cpp"))
        .arg("-o")
        .arg(&stub)
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("stub build failed".into());
    }
    let name = format!("/kr-{}", std::process::id());
    let (width, height, frames) = (1920, 1080, 600u32);
    let ring = Ring::create(&name, width, height)?;
    let reader = Command::new(&stub)
        .args([
            &name,
            &width.to_string(),
            &height.to_string(),
            &frames.to_string(),
        ])
        .output();
    let child = reader;
    println!(
        "ring premature reader (no frames, quiet exit): {}",
        String::from_utf8_lossy(&child.map_err(|e| e.to_string())?.stdout).trim()
    );

    let mut reader = Command::new(&stub)
        .args([
            &name,
            &width.to_string(),
            &height.to_string(),
            &frames.to_string(),
        ])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let start = Instant::now();
    for frame in 0..frames {
        ring.write(|data| data.fill(frame as u8));
        std::thread::sleep(Duration::from_micros(4_000));
    }
    println!(
        "ring wrote {frames} 1080p frames in {:.2}s",
        start.elapsed().as_secs_f64()
    );
    let output = {
        let mut text = String::new();
        use std::io::Read;
        reader
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut text)
            .map_err(|e| e.to_string())?;
        let _ = reader.wait();
        text
    };
    println!("ring stub reader: {}", output.trim());
    let wrong = Command::new(&stub)
        .args([&name, "1280", "720", "1"])
        .output()
        .map_err(|e| e.to_string())?;
    println!(
        "ring wrong raster: {}",
        String::from_utf8_lossy(&wrong.stdout).trim()
    );
    drop(ring);
    let gone = Command::new(&stub)
        .args([&name, "1920", "1080", "1"])
        .output()
        .map_err(|e| e.to_string())?;
    println!(
        "ring after close: {}",
        String::from_utf8_lossy(&gone.stdout).trim()
    );
    Ok(())
}

fn helper_probe(helper: &Path) {
    let name = format!("/kh-{}", std::process::id());
    let ring = Ring::create(&name, 1920, 1080).expect("ring");
    let output = Command::new(helper)
        .args(["--play-shm", "decklink:0:0", "decklink:1080", &name])
        .output();
    match output {
        Ok(o) => println!(
            "helper --play-shm exit={:?} stdout={} stderr={}",
            o.status.code(),
            String::from_utf8_lossy(&o.stdout).trim(),
            String::from_utf8_lossy(&o.stderr).trim()
        ),
        Err(e) => println!("helper failed to start: {e}"),
    }
    drop(ring);
}

fn converter_latency() {
    let (width, height) = (1920u32, 1080u32);
    let bgra: Vec<u8> = (0..width * height)
        .flat_map(|i| [i as u8, (i >> 8) as u8, (i >> 16) as u8, 255])
        .collect();
    let mut out = vec![0; (width * height * 3) as usize];
    let mut samples = Vec::new();
    let mut gpu_only = Vec::new();
    for _ in 0..300 {
        let start = Instant::now();
        assert_eq!(
            gpu::convert(&bgra, width, height, width, height, true, &mut out),
            0
        );
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
        // SAFETY: a plain counter read.
        gpu_only.push(unsafe { gpu::kgf_last_gpu_ns() } as f64 / 1e6);
    }
    samples.sort_by(f64::total_cmp);
    println!(
        "converter 1080p UYVA upload+convert+copy p50={:.2}ms p99={:.2}ms n=300",
        samples[150], samples[296]
    );
    gpu_only.sort_by(f64::total_cmp);
    println!(
        "converter 1080p UYVA GPU commit-to-complete p50={:.2}ms p99={:.2}ms",
        gpu_only[150], gpu_only[296]
    );
}

/// Mirrors the Electron test: submit to completion with the result copied out.
fn iosurface_latency() {
    use sinks_spike::converter::{Converter, FORMAT_BGRA, TestSurface};
    let (width, height) = (1920u32, 1080u32);
    let bgra: Vec<u8> = (0..width * height)
        .flat_map(|i| [i as u8, (i >> 3) as u8, (i >> 7) as u8, 255])
        .collect();
    let surface = TestSurface::new(&bgra, width, height, FORMAT_BGRA).expect("surface");
    let converter = Converter::open(width, height, true, None).expect("converter");
    let mut times = Vec::new();
    for _ in 0..120 {
        let (send, receive) = std::sync::mpsc::channel();
        let start = Instant::now();
        assert_eq!(
            converter.convert(&surface, move |r| send.send(r.is_ok()).unwrap()),
            Ok(true)
        );
        assert!(receive.recv().unwrap());
        times.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(f64::total_cmp);
    println!(
        "iosurface 1080p UYVA convert+readback p50={:.2}ms p99={:.2}ms n=120 stats={:?}",
        times[60],
        times[118],
        converter.stats()
    );
}

fn main() -> Result<(), String> {
    let args: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    let [library, native, out, helper] = args.as_slice() else {
        return Err("usage: probe LIBNDI NATIVE_DIR OUT_DIR HELPER".into());
    };
    println!("metal error: {:?}", gpu::error());
    converter_latency();
    for _ in 0..3 {
        iosurface_latency();
    }
    ring_probe(native, out)?;
    helper_probe(helper);
    ndi_probe(library)
}
