//! A stand-in for `decklink-helper` with one simulated card, for checking
//! the engine's card path on a machine without DeckLink hardware. It speaks
//! the helper's command line and events (`--list`, `--version`, `--play`,
//! `--play-shm`), reads the real frame ring the engine writes, and saves the
//! newest frame as a PNG. It is a development tool: nothing ships it.
//!
//! ```text
//! cargo build -p kairo-sinks --example decklink_sim
//! KAIRO_DECKLINK_HELPER=target/debug/examples/decklink_sim \
//! KAIRO_DECKLINK_SIM_OUT=/tmp/card.png \
//!   cargo run -p kairo-engine --example drive -- --card decklink:sim:0 decklink:1
//! ```

#[cfg(unix)]
fn main() {
    use kairo_engine_proto::ring::{RingReader, frame_bytes};
    use std::io::{Read, Write};
    use std::time::{Duration, Instant};

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = |id: &str| match id {
        "decklink:1" => Some((1920u32, 1080u32, 25.0)),
        "decklink:2" => Some((1280, 720, 50.0)),
        _ => None,
    };
    match args.first().map(String::as_str) {
        Some("--version") => println!(r#"{{"version":"16.4"}}"#),
        Some("--list") => println!(
            r#"{{"version":"16.4","ports":[{{"id":"decklink:sim:0","provider":"decklink","deviceName":"Simulated DeckLink","portName":"SDI 1","supportsKeyFill":false,"modes":[{{"id":"decklink:1","label":"1080p25","width":1920,"height":1080,"frameRateN":25,"frameRateD":1,"interlaced":false}},{{"id":"decklink:2","label":"720p50","width":1280,"height":720,"frameRateN":50,"frameRateD":1,"interlaced":false}}],"unavailableReason":null}}],"compatibilityMode":false}}"#
        ),
        Some(cmd @ ("--play" | "--play-shm")) => {
            if args.get(1).map(String::as_str) != Some("decklink:sim:0") {
                eprintln!("Device disconnected");
                std::process::exit(1);
            }
            let Some((width, height, fps)) = args.get(2).and_then(|m| mode(m)) else {
                eprintln!("Invalid mode, ring name or probe duration");
                std::process::exit(2);
            };
            let ring = if cmd == "--play-shm" {
                match RingReader::open(&args[3], width, height) {
                    Ok(ring) => {
                        println!(r#"{{"event":"ring","ok":true}}"#);
                        Some(ring)
                    }
                    Err(e) => {
                        println!(r#"{{"event":"ring","ok":false,"reason":"{e}"}}"#);
                        None
                    }
                }
            } else {
                None
            };
            println!(r#"{{"event":"started","port":"decklink:sim:0"}}"#);
            let _ = std::io::stdout().flush();
            let out = std::env::var("KAIRO_DECKLINK_SIM_OUT").ok();
            // stdin: EOF means release; BGRA packets are the CPU path.
            let (eof_tx, eof_rx) = std::sync::mpsc::channel::<Option<(u32, u32, Vec<u8>)>>();
            std::thread::spawn(move || {
                let mut stdin = std::io::stdin().lock();
                loop {
                    let mut header = [0u8; 12];
                    if stdin.read_exact(&mut header).is_err() {
                        let _ = eof_tx.send(None);
                        return;
                    }
                    let word = |i: usize| u32::from_le_bytes(header[i..i + 4].try_into().unwrap());
                    let mut bytes = vec![0u8; word(8) as usize];
                    if stdin.read_exact(&mut bytes).is_err() {
                        let _ = eof_tx.send(None);
                        return;
                    }
                    let _ = eof_tx.send(Some((word(0), word(4), bytes)));
                }
            });
            let mut frame = vec![0u8; frame_bytes(width, height)];
            let mut last = 0u64;
            let (mut completed, mut received) = (0u64, 0u64);
            let tick = Duration::from_secs_f64(1.0 / fps);
            let started = Instant::now();
            let mut next_stats = started + Duration::from_secs(1);
            let mut saved = Instant::now() - Duration::from_secs(10);
            let mut bgra: Option<(u32, u32, Vec<u8>)> = None;
            loop {
                match eof_rx.try_recv() {
                    Ok(None) => break,
                    Ok(Some(packet)) => {
                        received += 1;
                        bgra = Some(packet);
                    }
                    Err(_) => {}
                }
                if let Some(ring) = &ring
                    && ring.read_newest(&mut last, &mut frame)
                {
                    received += 1;
                }
                completed += 1;
                if Instant::now() >= next_stats {
                    let secs = started.elapsed().as_secs_f64();
                    println!(
                        r#"{{"event":"stats","completed":{completed},"late":0,"dropped":0,"repeated":{},"received":{received},"outputFps":{:.1}}}"#,
                        completed.saturating_sub(received),
                        completed as f64 / secs
                    );
                    let _ = std::io::stdout().flush();
                    next_stats += Duration::from_secs(1);
                }
                if let Some(path) = &out
                    && saved.elapsed() > Duration::from_millis(500)
                {
                    saved = Instant::now();
                    let rgb = match &bgra {
                        Some((w, h, px)) if ring.is_none() => Some((*w, *h, bgra_to_rgb(px))),
                        _ if ring.is_some() && last > 0 => {
                            Some((width, height, uyvy_to_rgb(&frame)))
                        }
                        _ => None,
                    };
                    if let Some((w, h, rgb)) = rgb {
                        save(path, w, h, &rgb);
                    }
                }
                std::thread::sleep(tick);
            }
            // As the helper: unlink the ring on exit.
            if let Some(name) = args.get(3).filter(|_| ring.is_some()) {
                let name = std::ffi::CString::new(name.as_str()).unwrap();
                // SAFETY: a NUL-terminated name.
                unsafe { libc::shm_unlink(name.as_ptr()) };
            }
            println!(
                r#"{{"event":"stopped","completed":{completed},"late":0,"dropped":0,"flushed":0,"errors":0}}"#
            );
        }
        _ => {
            eprintln!(
                "Invalid arguments: --list, --play PORT MODE, --play-shm PORT MODE RING, or --probe PORT MODE SECONDS"
            );
            std::process::exit(2);
        }
    }

    fn bgra_to_rgb(px: &[u8]) -> Vec<u8> {
        px.as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0]])
            .collect()
    }

    /// Rec.709 limited range back to RGB, for looking at.
    fn uyvy_to_rgb(uyvy: &[u8]) -> Vec<u8> {
        let mut rgb = Vec::with_capacity(uyvy.len() / 2 * 3);
        for q in uyvy.as_chunks::<4>().0 {
            let (u, y0, v, y1) = (
                q[0] as f32 - 128.0,
                q[1] as f32,
                q[2] as f32 - 128.0,
                q[3] as f32,
            );
            for y in [y0, y1] {
                let y = (y - 16.0) * 255.0 / 219.0;
                let (u, v) = (u * 255.0 / 224.0, v * 255.0 / 224.0);
                let r = y + 1.5748 * v;
                let g = y - 0.1873 * u - 0.4681 * v;
                let b = y + 1.8556 * u;
                rgb.extend([r, g, b].map(|c| c.round().clamp(0.0, 255.0) as u8));
            }
        }
        rgb
    }

    fn save(path: &str, w: u32, h: u32, rgb: &[u8]) {
        let Ok(file) = std::fs::File::create(path) else {
            return;
        };
        let mut encoder = png::Encoder::new(file, w, h);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        if let Ok(mut writer) = encoder.write_header() {
            let _ = writer.write_image_data(rgb);
        }
    }
}

#[cfg(not(unix))]
fn main() {}
