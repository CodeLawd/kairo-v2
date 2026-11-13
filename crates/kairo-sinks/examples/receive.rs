//! Receives one frame from an NDI source, as a receiver on the network
//! would, and writes it as a PNG for checking by eye.
//!
//! ```text
//! cargo run -p kairo-sinks --example receive -- "Kairo Scripture" out.png [TIMEOUT_MS]
//! ```
//!
//! The runtime comes from `KAIRO_NDI_RUNTIME`. UYVA becomes RGBA with the
//! inverse Rec.709 limited-range matrix, composited on a grey checkerboard
//! so transparency shows.

use kairo_sinks::ndi::{Runtime, UYVA};

fn main() {
    let mut args = std::env::args().skip(1);
    let name = args.next().unwrap_or_else(|| "Kairo Scripture".into());
    let out = args.next().unwrap_or_else(|| "ndi-frame.png".into());
    let timeout: u32 = args.next().and_then(|t| t.parse().ok()).unwrap_or(8000);
    let runtime = Runtime::find().unwrap_or_else(|e| panic!("no NDI runtime: {e}"));
    println!("NDI {}", runtime.version());
    let (names, _) = runtime.find_sources(3000, "").expect("finder");
    println!("sources: {names:?}");
    let mut image = None;
    let got = runtime
        .receive_one(&name, timeout, |f| {
            println!(
                "frame {}x{} fourcc {:#x} stride {}",
                f.xres, f.yres, f.fourcc, f.stride
            );
            if f.fourcc != UYVA {
                return;
            }
            let (w, h, stride) = (f.xres as usize, f.yres as usize, f.stride as usize);
            // SAFETY: the receiver's frame holds stride × h bytes and the alpha plane.
            let data = unsafe { std::slice::from_raw_parts(f.data, stride * h + w * h) };
            let mut rgba = vec![0u8; w * h * 4];
            for y in 0..h {
                for x in 0..w {
                    let g = y * stride + (x / 2) * 4;
                    let (u, yy, v) = (
                        data[g] as f32 - 128.0,
                        data[g + 1 + (x % 2) * 2] as f32 - 16.0,
                        data[g + 2] as f32 - 128.0,
                    );
                    let yl = yy * 255.0 / 219.0;
                    let (cb, cr) = (u * 255.0 / 224.0, v * 255.0 / 224.0);
                    let r = yl + 1.5748 * cr;
                    let gg = yl - 0.1873 * cb - 0.4681 * cr;
                    let b = yl + 1.8556 * cb;
                    let a = data[stride * h + y * w + x] as f32 / 255.0;
                    let check = if (x / 32 + y / 32) % 2 == 0 {
                        90.0
                    } else {
                        140.0
                    };
                    let o = (y * w + x) * 4;
                    // Premultiplied colour over the checkerboard.
                    for (i, c) in [r, gg, b].into_iter().enumerate() {
                        rgba[o + i] = (c + check * (1.0 - a)).clamp(0.0, 255.0) as u8;
                    }
                    rgba[o + 3] = 255;
                }
            }
            image = Some((w as u32, h as u32, rgba));
        })
        .expect("receiver");
    assert!(got, "no frame from {name}");
    if let Some((w, h, rgba)) = image {
        let file = std::fs::File::create(&out).expect("png file");
        let mut encoder = png::Encoder::new(file, w, h);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&rgba)
            .unwrap();
        println!("wrote {out}");
    }
}
