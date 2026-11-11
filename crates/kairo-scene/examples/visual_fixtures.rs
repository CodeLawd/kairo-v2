//! Writes the pictures the visual corpus (prompt 053) uses: one 4:3 test
//! card in every format the renderer shows. They are small, committed, and
//! regenerated with
//! `cargo run -p kairo-scene --example visual_fixtures -- tests/fixtures/visuals`.

use image::codecs::gif::{GifEncoder, Repeat};
use image::{Delay, Frame, ImageFormat, Rgba, RgbaImage};
use std::path::Path;

/// Four quadrants, a diagonal, and an off-centre disc: nothing is symmetric,
/// so a flip, a turn or a wrong fit shows.
fn card(alpha_disc: bool) -> RgbaImage {
    let (w, h) = (640u32, 480u32);
    RgbaImage::from_fn(w, h, |x, y| {
        let base = match (x < w / 2, y < h / 2) {
            (true, true) => [220, 50, 50],
            (false, true) => [50, 200, 90],
            (true, false) => [50, 90, 220],
            (false, false) => [235, 200, 40],
        };
        let mut p = [base[0], base[1], base[2], 255u8];
        if (x as i32 - y as i32 * 4 / 3).abs() < 6 {
            p = [255, 255, 255, 255];
        }
        let (dx, dy) = (x as f32 - 150.0, y as f32 - 120.0);
        if dx * dx + dy * dy < 50.0 * 50.0 {
            p = if alpha_disc {
                [0, 0, 0, 90]
            } else {
                [20, 20, 20, 255]
            };
        }
        // A frame, so cropping by `cover` shows.
        if x < 8 || y < 8 || x >= w - 8 || y >= h - 8 {
            p = [255, 0, 255, 255];
        }
        Rgba(p)
    })
}

fn main() {
    let dir = std::env::args().nth(1).expect("output directory");
    let dir = Path::new(&dir);
    std::fs::create_dir_all(dir).unwrap();
    let opaque = card(false);
    opaque
        .save_with_format(dir.join("card.png"), ImageFormat::Png)
        .unwrap();
    card(true)
        .save_with_format(dir.join("card-alpha.png"), ImageFormat::Png)
        .unwrap();
    image::imageops::resize(&opaque, 160, 120, image::imageops::FilterType::Nearest)
        .save_with_format(dir.join("card.bmp"), ImageFormat::Bmp)
        .unwrap();
    image::DynamicImage::ImageRgba8(card(true))
        .save_with_format(dir.join("card.webp"), ImageFormat::WebP)
        .unwrap();
    image::DynamicImage::ImageRgba8(opaque.clone())
        .save_with_format(dir.join("card.avif"), ImageFormat::Avif)
        .unwrap();
    // An animation that sits on its first frame for ten seconds.
    let mut gif = Vec::new();
    {
        let mut enc = GifEncoder::new(&mut gif);
        enc.set_repeat(Repeat::Infinite).unwrap();
        for tint in [0u8, 120, 240] {
            let mut frame = card(false);
            for p in frame.pixels_mut() {
                if p.0[..3] == [255, 0, 255] {
                    p.0 = [tint, 255 - tint, 255, 255];
                }
            }
            let frame =
                image::imageops::resize(&frame, 320, 240, image::imageops::FilterType::Nearest);
            enc.encode_frame(Frame::from_parts(
                frame,
                0,
                0,
                Delay::from_numer_denom_ms(10_000, 1),
            ))
            .unwrap();
        }
    }
    std::fs::write(dir.join("anim.gif"), gif).unwrap();
    std::fs::write(
        dir.join("card.svg"),
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="640" height="480" viewBox="0 0 640 480">
  <rect width="320" height="240" fill="#dc3232"/>
  <rect x="320" width="320" height="240" fill="#32c85a"/>
  <rect y="240" width="320" height="240" fill="#325adc"/>
  <rect x="320" y="240" width="320" height="240" fill="#ebc828"/>
  <circle cx="150" cy="120" r="50" fill="#141414"/>
  <path d="M0 0 L640 480" stroke="#ffffff" stroke-width="8" fill="none"/>
  <rect x="4" y="4" width="632" height="472" fill="none" stroke="#ff00ff" stroke-width="8"/>
</svg>
"##,
    )
    .unwrap();
}
