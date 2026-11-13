//! A Syphon client, standing in for Syphon Simple Client or OBS: finds a
//! server by name in any process, counts its frames for a while, and saves
//! the newest one as a PNG (straight alpha).
//!
//! cargo run -p kairo-native-sys --example syphon_client -- "Kairo Lower Third" out.png [watch-seconds]

#[cfg(target_os = "macos")]
fn main() {
    let mut args = std::env::args().skip(1);
    let name = args.next().expect("server name");
    let path = args.next().expect("output PNG path");
    let watch: f64 = args.next().map_or(2.0, |s| s.parse().expect("seconds"));
    let started = std::time::Instant::now();
    let got = match kairo_native_sys::syphon::receive(&name, 10_000.0, watch * 1000.0) {
        Ok(got) => got,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    println!(
        "server {name:?}: {}x{}, {} frames in {watch:.1} s ({:.1} fps), total {:.1} s",
        got.width,
        got.height,
        got.frames,
        got.frames as f64 / watch,
        started.elapsed().as_secs_f64()
    );
    let w = got.width as usize;
    for (label, x, y) in [
        ("top-left", 0, 0),
        ("centre", w / 2, got.height as usize / 2),
    ] {
        let i = (y * w + x) * 4;
        println!("{label} BGRA {:?}", &got.bgra[i..i + 4]);
    }
    // Premultiplied BGRA to straight RGBA for viewing.
    let mut rgba = got.bgra;
    for p in rgba.as_chunks_mut::<4>().0 {
        p.swap(0, 2);
        let a = p[3] as u32;
        if a > 0 && a < 255 {
            for c in &mut p[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    let file = std::fs::File::create(&path).expect("create PNG");
    let mut encoder = png::Encoder::new(file, got.width, got.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .and_then(|mut w| w.write_image_data(&rgba))
        .expect("write PNG");
    println!("saved {path}");
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("Syphon is only available on a Mac");
    std::process::exit(1);
}
