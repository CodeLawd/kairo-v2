//! Still and animated pictures for backgrounds and elements.
//!
//! The renderer showed whatever Chromium could decode: gif (animated, with
//! Chromium's delay rule), webp (animated too), avif, bmp, svg, and the png
//! and jpeg everything ships. The format comes from the bytes, not the file
//! name, as in a browser. Decoding is bounded in size, frame count and
//! memory, because a file the operator picks is still untrusted input.
//!
//! The result is premultiplied RGBA at no more than the size it will be
//! drawn: a 12-megapixel photo behind a 1920-wide frame is reduced here, once,
//! with a proper filter, rather than minified by the GPU every frame.

use image::codecs::gif::GifDecoder;
use image::codecs::png::PngDecoder;
use image::codecs::webp::WebPDecoder;
use image::imageops::FilterType;
use image::{AnimationDecoder, DynamicImage, ImageDecoder, Limits, RgbaImage};
use std::io::Cursor;
use std::sync::{Arc, OnceLock};

/// Widest or tallest picture accepted, and the most pixels one frame holds.
pub const MAX_SIDE: u32 = 32_768;
pub const MAX_PIXELS: u64 = 150_000_000;
/// Frames of one animation, and all their pixels together.
pub const MAX_FRAMES: usize = 4_000;
pub const MAX_ANIMATION_BYTES: usize = 384 << 20;
/// The largest side a vector picture is drawn at.
pub const MAX_VECTOR_SIDE: u32 = 8_192;

/// How a picture sits in its box (`object-fit`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FitMode {
    Cover,
    Contain,
    Fill,
}

/// The box a picture will be drawn in, in frame pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BoxFit {
    pub width: u32,
    pub height: u32,
    pub fit: FitMode,
}

#[derive(Clone, Debug)]
pub struct PictureFrame {
    /// Premultiplied RGBA, `width * height * 4` bytes.
    pub pixels: Vec<u8>,
    /// How long it shows, at least one millisecond; a still has one frame.
    pub delay_ms: u32,
}

#[derive(Clone, Debug)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub frames: Vec<PictureFrame>,
    /// How many times an animation plays; `None` is forever.
    pub plays: Option<u32>,
}

impl Picture {
    pub fn is_animated(&self) -> bool {
        self.frames.len() > 1
    }

    pub fn bytes(&self) -> usize {
        self.frames.iter().map(|f| f.pixels.len()).sum()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaError {
    /// Nothing to decode.
    Empty,
    /// Not a picture format a browser would show here.
    Unsupported,
    /// Larger than the limits allow.
    TooLarge(String),
    /// Damaged, or a feature the decoder lacks.
    Corrupt(String),
}

impl std::fmt::Display for MediaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MediaError::Empty => write!(f, "the file is empty"),
            MediaError::Unsupported => write!(f, "not a picture format that can be shown"),
            MediaError::TooLarge(what) => write!(f, "too large: {what}"),
            MediaError::Corrupt(what) => write!(f, "cannot be decoded: {what}"),
        }
    }
}

impl std::error::Error for MediaError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Gif,
    Webp,
    Png,
    Avif,
    Svg,
    Other,
}

/// What the bytes are, by their content.
fn sniff(bytes: &[u8]) -> Kind {
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Kind::Gif;
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Kind::Webp;
    }
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Kind::Png;
    }
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        let head = &bytes[8..bytes.len().min(64)];
        if head.windows(4).any(|w| w == b"avif" || w == b"avis") {
            return Kind::Avif;
        }
    }
    let head = &bytes[..bytes.len().min(4096)];
    let text = head.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(head);
    let trimmed = text
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .map_or(&text[0..0], |i| &text[i..]);
    if trimmed.first() == Some(&b'<') && trimmed.windows(4).any(|w| w.eq_ignore_ascii_case(b"<svg"))
    {
        return Kind::Svg;
    }
    Kind::Other
}

/// Decodes `bytes` for a box. Raster pictures larger than they will be drawn
/// are reduced; vector pictures are drawn at the size they will have.
pub fn decode_picture(bytes: &[u8], target: BoxFit) -> Result<Picture, MediaError> {
    if bytes.is_empty() {
        return Err(MediaError::Empty);
    }
    let picture = match sniff(bytes) {
        Kind::Gif => decode_gif(bytes)?,
        Kind::Webp => decode_webp(bytes)?,
        Kind::Png => decode_png(bytes)?,
        Kind::Avif => decode_avif(bytes)?,
        Kind::Svg => return decode_svg(bytes, target),
        Kind::Other => decode_still(bytes)?,
    };
    shrink(picture, target)
}

fn limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(1 << 30);
    limits
}

fn corrupt(error: impl std::fmt::Display) -> MediaError {
    MediaError::Corrupt(error.to_string())
}

fn check_dimensions(width: u32, height: u32) -> Result<(), MediaError> {
    if width == 0 || height == 0 {
        return Err(MediaError::Corrupt("the picture has no pixels".into()));
    }
    if width > MAX_SIDE || height > MAX_SIDE || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(MediaError::TooLarge(format!("{width} × {height} pixels")));
    }
    Ok(())
}

fn premultiply(rgba: &mut [u8]) {
    for p in rgba.as_chunks_mut::<4>().0 {
        let a = u32::from(p[3]);
        if a != 255 {
            for c in &mut p[..3] {
                *c = ((u32::from(*c) * a + 127) / 255) as u8;
            }
        }
    }
}

fn still(rgba: RgbaImage) -> Result<Picture, MediaError> {
    let (width, height) = rgba.dimensions();
    check_dimensions(width, height)?;
    let mut pixels = rgba.into_raw();
    premultiply(&mut pixels);
    Ok(Picture {
        width,
        height,
        frames: vec![PictureFrame {
            pixels,
            delay_ms: 1,
        }],
        plays: Some(1),
    })
}

/// png, jpeg, bmp (and ico and the rest of what the `image` crate sniffs), in
/// the orientation the file asks for.
fn decode_still(bytes: &[u8]) -> Result<Picture, MediaError> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(corrupt)?;
    reader.limits(limits());
    if reader.format().is_none() {
        return Err(MediaError::Unsupported);
    }
    let mut decoder = reader.into_decoder().map_err(corrupt)?;
    let orientation = decoder.orientation().ok();
    let (w, h) = decoder.dimensions();
    check_dimensions(w, h)?;
    let mut image = DynamicImage::from_decoder(decoder).map_err(corrupt)?;
    if let Some(orientation) = orientation {
        image.apply_orientation(orientation);
    }
    still(image.to_rgba8())
}

/// Chromium plays a still frame of at most 10 ms (and 0) for 100 ms.
fn browser_delay(ms: u32) -> u32 {
    if ms <= 10 { 100 } else { ms }
}

fn frames_of(
    frames: image::Frames<'_>,
    min_delay: bool,
) -> Result<(u32, u32, Vec<PictureFrame>), MediaError> {
    let mut out = Vec::new();
    let mut total = 0usize;
    let mut size = (0, 0);
    for frame in frames {
        let frame = frame.map_err(corrupt)?;
        let (n, d) = frame.delay().numer_denom_ms();
        let ms = n.checked_div(d).unwrap_or(0);
        let buffer = frame.into_buffer();
        let (w, h) = buffer.dimensions();
        check_dimensions(w, h)?;
        if out.is_empty() {
            size = (w, h);
        }
        total += (w * h * 4) as usize;
        if out.len() >= MAX_FRAMES || total > MAX_ANIMATION_BYTES {
            return Err(MediaError::TooLarge(format!(
                "an animation of more than {MAX_FRAMES} frames or {} MiB",
                MAX_ANIMATION_BYTES >> 20
            )));
        }
        let mut pixels = buffer.into_raw();
        premultiply(&mut pixels);
        out.push(PictureFrame {
            pixels,
            delay_ms: if min_delay {
                browser_delay(ms)
            } else {
                ms.max(1)
            },
        });
    }
    if out.is_empty() {
        return Err(MediaError::Corrupt("no frames".into()));
    }
    Ok((size.0, size.1, out))
}

/// The Netscape application extension's repeat count; Chromium plays a file
/// without one once, `0` forever, and `n` one more time than it says.
fn gif_plays(bytes: &[u8]) -> Option<u32> {
    const MARK: &[u8] = b"NETSCAPE2.0";
    let at = bytes.windows(MARK.len()).position(|w| w == MARK)?;
    let rest = bytes.get(at + MARK.len()..)?;
    match rest {
        [3, 1, lo, hi, ..] => Some(u32::from(*lo) | u32::from(*hi) << 8),
        _ => None,
    }
}

fn decode_gif(bytes: &[u8]) -> Result<Picture, MediaError> {
    let decoder = GifDecoder::new(Cursor::new(bytes)).map_err(corrupt)?;
    let (w, h) = decoder.dimensions();
    check_dimensions(w, h)?;
    let (width, height, frames) = frames_of(decoder.into_frames(), true)?;
    let plays = match gif_plays(bytes) {
        None => Some(1),
        Some(0) => None,
        Some(n) => Some(n + 1),
    };
    Ok(Picture {
        width,
        height,
        frames,
        plays,
    })
}

/// The ANIM chunk's loop count: `0` forever, otherwise the plays.
fn webp_plays(bytes: &[u8]) -> Option<u32> {
    let at = bytes.windows(4).position(|w| w == b"ANIM")?;
    let data = bytes.get(at + 8..at + 14)?;
    Some(u32::from(u16::from_le_bytes([data[4], data[5]])))
}

fn decode_webp(bytes: &[u8]) -> Result<Picture, MediaError> {
    let decoder = WebPDecoder::new(Cursor::new(bytes)).map_err(corrupt)?;
    let (w, h) = decoder.dimensions();
    check_dimensions(w, h)?;
    if !decoder.has_animation() {
        let mut decoder = decoder;
        let orientation = decoder.orientation().ok();
        let mut image = DynamicImage::from_decoder(decoder).map_err(corrupt)?;
        if let Some(orientation) = orientation {
            image.apply_orientation(orientation);
        }
        return still(image.to_rgba8());
    }
    let (width, height, frames) = frames_of(decoder.into_frames(), true)?;
    let plays = match webp_plays(bytes) {
        Some(0) | None => None,
        Some(n) => Some(n),
    };
    Ok(Picture {
        width,
        height,
        frames,
        plays,
    })
}

/// The acTL chunk's play count: `0` forever.
fn apng_plays(bytes: &[u8]) -> Option<u32> {
    let at = bytes.windows(4).position(|w| w == b"acTL")?;
    let data = bytes.get(at + 4..at + 12)?;
    Some(u32::from_be_bytes([data[4], data[5], data[6], data[7]]))
}

fn decode_png(bytes: &[u8]) -> Result<Picture, MediaError> {
    let decoder = PngDecoder::new(Cursor::new(bytes)).map_err(corrupt)?;
    if !decoder.is_apng().unwrap_or(false) {
        return decode_still(bytes);
    }
    let (w, h) = decoder.dimensions();
    check_dimensions(w, h)?;
    let apng = decoder.apng().map_err(corrupt)?;
    let (width, height, frames) = frames_of(apng.into_frames(), true)?;
    let plays = match apng_plays(bytes) {
        Some(0) | None => None,
        Some(n) => Some(n),
    };
    Ok(Picture {
        width,
        height,
        frames,
        plays,
    })
}

/// A still avif. An avif sequence shows its primary picture.
fn decode_avif(bytes: &[u8]) -> Result<Picture, MediaError> {
    let image = avif_decode::Decoder::from_avif(bytes)
        .map_err(corrupt)?
        .to_image()
        .map_err(corrupt)?;
    let to8 = |v: u16| (v >> 8) as u8;
    let (width, height, pixels) = match image {
        avif_decode::Image::Rgba8(img) => {
            let mut out = Vec::with_capacity(img.width() * img.height() * 4);
            for row in img.rows() {
                for p in row {
                    out.extend_from_slice(&[p.r, p.g, p.b, p.a]);
                }
            }
            (img.width(), img.height(), out)
        }
        avif_decode::Image::Rgb8(img) => {
            let mut out = Vec::with_capacity(img.width() * img.height() * 4);
            for row in img.rows() {
                for p in row {
                    out.extend_from_slice(&[p.r, p.g, p.b, 255]);
                }
            }
            (img.width(), img.height(), out)
        }
        avif_decode::Image::Rgba16(img) => {
            let mut out = Vec::with_capacity(img.width() * img.height() * 4);
            for row in img.rows() {
                for p in row {
                    out.extend_from_slice(&[to8(p.r), to8(p.g), to8(p.b), to8(p.a)]);
                }
            }
            (img.width(), img.height(), out)
        }
        avif_decode::Image::Rgb16(img) => {
            let mut out = Vec::with_capacity(img.width() * img.height() * 4);
            for row in img.rows() {
                for p in row {
                    out.extend_from_slice(&[to8(p.r), to8(p.g), to8(p.b), 255]);
                }
            }
            (img.width(), img.height(), out)
        }
        avif_decode::Image::Gray8(img) => {
            let mut out = Vec::with_capacity(img.width() * img.height() * 4);
            for row in img.rows() {
                for p in row {
                    let v = **p;
                    out.extend_from_slice(&[v, v, v, 255]);
                }
            }
            (img.width(), img.height(), out)
        }
        avif_decode::Image::Gray16(img) => {
            let mut out = Vec::with_capacity(img.width() * img.height() * 4);
            for row in img.rows() {
                for p in row {
                    let v = to8(**p);
                    out.extend_from_slice(&[v, v, v, 255]);
                }
            }
            (img.width(), img.height(), out)
        }
    };
    let (width, height) = (width as u32, height as u32);
    check_dimensions(width, height)?;
    let rgba = RgbaImage::from_raw(width, height, pixels)
        .ok_or_else(|| MediaError::Corrupt("the decoded picture has the wrong size".into()))?;
    still(rgba)
}

/// The fonts an svg's text may use: the system's, loaded once.
fn fonts() -> Arc<usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut db = usvg::fontdb::Database::new();
            db.load_system_fonts();
            Arc::new(db)
        })
        .clone()
}

/// An svg, drawn at the size it will have. Nothing outside the file is read:
/// a linked file or address is ignored, as it is for an image in a browser.
fn decode_svg(bytes: &[u8], target: BoxFit) -> Result<Picture, MediaError> {
    let options = usvg::Options {
        fontdb: fonts(),
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: usvg::ImageHrefResolver::default_data_resolver(),
            resolve_string: Box::new(|_, _| None),
        },
        ..usvg::Options::default()
    };
    let tree = usvg::Tree::from_data(bytes, &options).map_err(corrupt)?;
    let size = tree.size();
    let (iw, ih) = (f64::from(size.width()), f64::from(size.height()));
    if !(iw > 0.0 && ih > 0.0 && iw.is_finite() && ih.is_finite()) {
        return Err(MediaError::Corrupt("the picture has no size".into()));
    }
    let (bw, bh) = (
        f64::from(target.width.max(1)),
        f64::from(target.height.max(1)),
    );
    let (dw, dh) = match target.fit {
        FitMode::Fill => (bw, bh),
        FitMode::Contain => {
            let s = (bw / iw).min(bh / ih);
            (iw * s, ih * s)
        }
        FitMode::Cover => {
            let s = (bw / iw).max(bh / ih);
            (iw * s, ih * s)
        }
    };
    let cap = f64::from(MAX_VECTOR_SIDE);
    let shrink = (cap / dw).min(cap / dh).min(1.0);
    let (w, h) = (
        (dw * shrink).round().max(1.0) as u32,
        (dh * shrink).round().max(1.0) as u32,
    );
    let mut pixmap = tiny_skia::Pixmap::new(w, h)
        .ok_or_else(|| MediaError::Corrupt("the picture has no pixels".into()))?;
    let transform =
        tiny_skia::Transform::from_scale(w as f32 / size.width(), h as f32 / size.height());
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    Ok(Picture {
        width: w,
        height: h,
        frames: vec![PictureFrame {
            pixels: pixmap.take(),
            delay_ms: 1,
        }],
        plays: Some(1),
    })
}

/// Reduces a raster picture to the size it will be drawn at. Never enlarges:
/// the GPU does that, with the same bilinear filter a browser uses.
fn shrink(mut picture: Picture, target: BoxFit) -> Result<Picture, MediaError> {
    let (w, h) = (f64::from(picture.width), f64::from(picture.height));
    let (bw, bh) = (
        f64::from(target.width.max(1)),
        f64::from(target.height.max(1)),
    );
    let (sx, sy) = match target.fit {
        FitMode::Fill => (bw / w, bh / h),
        FitMode::Contain => {
            let s = (bw / w).min(bh / h);
            (s, s)
        }
        FitMode::Cover => {
            let s = (bw / w).max(bh / h);
            (s, s)
        }
    };
    let nw = ((w * sx.min(1.0)).round() as u32).max(1);
    let nh = ((h * sy.min(1.0)).round() as u32).max(1);
    if nw >= picture.width && nh >= picture.height {
        return Ok(picture);
    }
    for frame in &mut picture.frames {
        let source = RgbaImage::from_raw(
            picture.width,
            picture.height,
            std::mem::take(&mut frame.pixels),
        )
        .ok_or_else(|| MediaError::Corrupt("a frame has the wrong size".into()))?;
        // Premultiplied channels are filtered like any others, then kept
        // within their alpha (a Lanczos lobe can overshoot).
        let mut out = image::imageops::resize(&source, nw, nh, FilterType::Lanczos3).into_raw();
        for p in out.as_chunks_mut::<4>().0 {
            let a = p[3];
            for c in &mut p[..3] {
                *c = (*c).min(a);
            }
        }
        frame.pixels = out;
    }
    picture.width = nw;
    picture.height = nh;
    Ok(picture)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> BoxFit {
        BoxFit {
            width: 64,
            height: 64,
            fit: FitMode::Cover,
        }
    }

    fn png(w: u32, h: u32, rgba: [u8; 4]) -> Vec<u8> {
        let mut out = Vec::new();
        image::RgbaImage::from_pixel(w, h, image::Rgba(rgba))
            .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn a_png_is_premultiplied_and_reduced_to_its_box() {
        let picture = decode_picture(&png(256, 128, [200, 100, 50, 128]), target()).unwrap();
        // Cover over 64 × 64: the short side becomes 64.
        assert_eq!((picture.width, picture.height), (128, 64));
        let p = &picture.frames[0].pixels[..4];
        assert_eq!(p[3], 128);
        assert!((i32::from(p[0]) - 100).abs() <= 2, "premultiplied: {p:?}");
        assert!(!picture.is_animated());
    }

    #[test]
    fn a_small_picture_is_not_enlarged() {
        let picture = decode_picture(&png(8, 8, [1, 2, 3, 255]), target()).unwrap();
        assert_eq!((picture.width, picture.height), (8, 8));
    }

    #[test]
    fn contain_and_fill_choose_their_sizes() {
        let bytes = png(400, 200, [9, 9, 9, 255]);
        let contain = BoxFit {
            fit: FitMode::Contain,
            ..target()
        };
        let p = decode_picture(&bytes, contain).unwrap();
        assert_eq!((p.width, p.height), (64, 32));
        let fill = BoxFit {
            fit: FitMode::Fill,
            ..target()
        };
        let p = decode_picture(&bytes, fill).unwrap();
        assert_eq!((p.width, p.height), (64, 64));
    }

    #[test]
    fn empty_unknown_and_damaged_files_fail_cleanly() {
        assert_eq!(
            decode_picture(&[], target()).unwrap_err(),
            MediaError::Empty
        );
        assert!(decode_picture(b"hello, not a picture", target()).is_err());
        let mut broken = png(32, 32, [1, 1, 1, 255]);
        broken.truncate(broken.len() / 2);
        assert!(decode_picture(&broken, target()).is_err());
        assert!(decode_picture(&[0x89, b'P', b'N', b'G', 1, 2, 3], target()).is_err());
    }

    #[test]
    fn the_format_comes_from_the_bytes() {
        assert_eq!(sniff(b"GIF89a...."), Kind::Gif);
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Kind::Webp);
        assert_eq!(sniff(b"\0\0\0\x1cftypavif\0\0\0\0avifmif1"), Kind::Avif);
        assert_eq!(
            sniff(b"\xEF\xBB\xBF  <?xml version=\"1.0\"?><svg/>"),
            Kind::Svg
        );
        assert_eq!(sniff(b"<html><body/></html>"), Kind::Other);
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF]), Kind::Other);
    }

    #[test]
    fn an_svg_is_drawn_at_the_size_it_will_have() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="5" viewBox="0 0 10 5"><rect width="10" height="5" fill="#ff0000"/></svg>"##;
        let contain = BoxFit {
            width: 200,
            height: 200,
            fit: FitMode::Contain,
        };
        let p = decode_picture(svg, contain).unwrap();
        assert_eq!((p.width, p.height), (200, 100));
        assert_eq!(&p.frames[0].pixels[..4], &[255, 0, 0, 255]);
        let cover = BoxFit {
            fit: FitMode::Cover,
            ..contain
        };
        let p = decode_picture(svg, cover).unwrap();
        assert_eq!((p.width, p.height), (400, 200));
    }

    #[test]
    fn an_svg_reads_nothing_outside_itself() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="4" height="4"><image xlink:href="/etc/hosts" width="4" height="4"/><rect width="4" height="4" fill="#00ff00" fill-opacity="0.5"/></svg>"##;
        let p = decode_picture(svg, target()).unwrap();
        assert_eq!(p.frames.len(), 1);
    }

    #[test]
    fn gif_delays_and_loops_follow_chromium() {
        // 2 × 2, two frames of 0 and 50 centiseconds, loop count 3.
        let mut gif = Vec::new();
        {
            let mut enc = image::codecs::gif::GifEncoder::new(&mut gif);
            enc.set_repeat(image::codecs::gif::Repeat::Finite(3))
                .unwrap();
            for (i, ms) in [0u32, 500].into_iter().enumerate() {
                let frame = image::Frame::from_parts(
                    RgbaImage::from_pixel(2, 2, image::Rgba([i as u8 * 200, 0, 0, 255])),
                    0,
                    0,
                    image::Delay::from_numer_denom_ms(ms, 1),
                );
                enc.encode_frame(frame).unwrap();
            }
        }
        let p = decode_picture(&gif, target()).unwrap();
        assert!(p.is_animated());
        assert_eq!(
            p.frames[0].delay_ms, 100,
            "a delay of 10 ms or less is 100 ms"
        );
        assert_eq!(p.frames[1].delay_ms, 500);
        assert_eq!(p.plays, Some(4), "a count of n plays n + 1 times");
    }

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/visuals")
                .join(name),
        )
        .unwrap()
    }

    #[test]
    fn every_format_decodes_to_the_same_card() {
        let fit = BoxFit {
            width: 640,
            height: 480,
            fit: FitMode::Contain,
        };
        for name in [
            "card.png",
            "card.webp",
            "card.avif",
            "card.svg",
            "card.bmp",
            "anim.gif",
        ] {
            let p = decode_picture(&fixture(name), fit).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(p.width * 3, p.height * 4, "{name}: 4:3");
            // The top-left quadrant is red whatever the format.
            let f = &p.frames[0];
            let at = ((p.height / 4 * p.width + p.width / 8) * 4) as usize;
            let px = &f.pixels[at..at + 4];
            assert!(
                px[0] > 190 && px[1] < 90 && px[2] < 90 && px[3] == 255,
                "{name}: {px:?}"
            );
        }
        assert!(
            decode_picture(&fixture("anim.gif"), fit)
                .unwrap()
                .is_animated()
        );
    }

    #[test]
    fn pictures_over_the_limits_are_refused() {
        assert!(matches!(
            check_dimensions(MAX_SIDE + 1, 1),
            Err(MediaError::TooLarge(_))
        ));
        assert!(matches!(
            check_dimensions(20_000, 20_000),
            Err(MediaError::TooLarge(_))
        ));
        assert!(check_dimensions(0, 10).is_err());
    }
}
