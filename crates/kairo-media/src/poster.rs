//! Video thumbnails and durations.
//!
//! Electron's media dock shows a `<video preload="metadata">` per tile, so
//! the operator sees the clip's first frame and Chromium reports its length.
//! Here FFmpeg decodes the frame at or after 0.1 s (the first frame of a
//! shorter clip), scales it to fit [`POSTER_MAX_WIDTH`] and writes a PNG.
//! Results are cached on disk under a key of path, mtime and size, so an
//! edited file gets a fresh poster and an unchanged one costs a stat.

use ffmpeg_next as ff;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Once;
use std::time::UNIX_EPOCH;

/// Where the poster frame is taken, in seconds from the start.
pub const POSTER_AT_SECONDS: f64 = 0.1;
/// Posters are dock thumbnails; wider frames are scaled down to this.
pub const POSTER_MAX_WIDTH: u32 = 640;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Poster {
    /// The PNG in the cache folder.
    pub image: PathBuf,
    pub width: u32,
    pub height: u32,
    /// `None` when the container does not say.
    pub duration_seconds: Option<f64>,
}

#[derive(Debug)]
pub enum PosterError {
    /// The file could not be read or the cache could not be written.
    Io(std::io::Error),
    /// FFmpeg could not open or decode it. The dock flags the tile as broken.
    Decode(String),
}

impl std::fmt::Display for PosterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Decode(e) => write!(f, "Could not decode the video — {e}"),
        }
    }
}

impl std::error::Error for PosterError {}

impl From<std::io::Error> for PosterError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<ff::Error> for PosterError {
    fn from(e: ff::Error) -> Self {
        Self::Decode(e.to_string())
    }
}

/// The cache key: SHA-256 over the resolved path, mtime in nanoseconds and size.
pub fn cache_key(path: &Path) -> std::io::Result<String> {
    let meta = std::fs::metadata(path)?;
    let mtime = meta
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let mut hasher = Sha256::new();
    hasher.update(crate::paths::resolve(path).to_string_lossy().as_bytes());
    hasher.update([0]);
    hasher.update(mtime.to_le_bytes());
    hasher.update(meta.len().to_le_bytes());
    let mut hex = format!("{:x}", hasher.finalize());
    hex.truncate(32);
    Ok(hex)
}

/// The poster for `video`, from `cache_dir` when present, else decoded and cached.
/// Blocking; call it off the UI thread.
pub fn poster_for(cache_dir: &Path, video: &Path) -> Result<Poster, PosterError> {
    let key = cache_key(video)?;
    let meta_path = cache_dir.join(format!("{key}.json"));
    if let Ok(text) = std::fs::read_to_string(&meta_path)
        && let Ok(poster) = serde_json::from_str::<Poster>(&text)
        && poster.image.is_file()
    {
        return Ok(poster);
    }
    std::fs::create_dir_all(cache_dir)?;
    let image = cache_dir.join(format!("{key}.png"));
    let (frame, duration_seconds) = decode_poster(video)?;
    write_png(&image, &frame)?;
    let poster = Poster {
        image,
        width: frame.width,
        height: frame.height,
        duration_seconds,
    };
    let text = serde_json::to_string(&poster).map_err(std::io::Error::other)?;
    kairo_store::fs_atomic::write_atomic(&meta_path, text.as_bytes())?;
    Ok(poster)
}

/// Removes cached posters whose key is not in `keep`. Returns how many went.
pub fn prune_cache(cache_dir: &Path, keep: &std::collections::HashSet<String>) -> usize {
    let Ok(entries) = std::fs::read_dir(cache_dir) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some((key, "png" | "json")) = name.rsplit_once('.') else {
            continue;
        };
        if !keep.contains(key) && std::fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

pub struct RgbaFrame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

pub(crate) fn init() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let _ = ff::init();
        ff::util::log::set_level(ff::util::log::Level::Error);
    });
}

/// Decodes the poster frame and reads the container's duration.
pub fn decode_poster(path: &Path) -> Result<(RgbaFrame, Option<f64>), PosterError> {
    init();
    let mut input = ff::format::input(path)?;
    let duration =
        (input.duration() > 0).then(|| input.duration() as f64 / f64::from(ff::ffi::AV_TIME_BASE));
    let stream = input
        .streams()
        .best(ff::media::Type::Video)
        .ok_or_else(|| PosterError::Decode("no video stream".into()))?;
    let index = stream.index();
    let time_base = f64::from(stream.time_base());
    let start = if stream.start_time() == ff::ffi::AV_NOPTS_VALUE {
        0
    } else {
        stream.start_time()
    };
    let context = ff::codec::context::Context::from_parameters(stream.parameters())?;
    let mut decoder = context.decoder().video()?;
    let target = start as f64 * time_base + POSTER_AT_SECONDS;
    // Land on the keyframe at or before the target; decode forward from there.
    let seek_to = (target * f64::from(ff::ffi::AV_TIME_BASE)) as i64;
    if input.seek(seek_to, ..seek_to).is_err() {
        let _ = input.seek(0, ..0);
    }
    let mut decoded = ff::frame::Video::empty();
    let mut last: Option<ff::frame::Video> = None;
    let mut draining = false;
    let mut packets = input.packets();
    loop {
        match decoder.receive_frame(&mut decoded) {
            Ok(()) => {
                let pts = decoded.timestamp().or_else(|| decoded.pts());
                let at = pts.map(|p| p as f64 * time_base);
                let reached = at.is_none_or(|t| t + 1e-6 >= target);
                last = Some(decoded.clone());
                if reached {
                    break;
                }
                continue;
            }
            Err(ff::Error::Eof) => break,
            Err(ff::Error::Other { errno }) if errno == ff::util::error::EAGAIN => {}
            Err(e) => return Err(e.into()),
        }
        if draining {
            break;
        }
        match packets.next() {
            Some((stream, packet)) if stream.index() == index => decoder.send_packet(&packet)?,
            Some(_) => {}
            None => {
                decoder.send_eof()?;
                draining = true;
            }
        }
    }
    // A clip shorter than the target shows its last decoded frame.
    let frame = last.ok_or_else(|| PosterError::Decode("no frames".into()))?;
    Ok((to_rgba(&frame)?, duration))
}

fn to_rgba(frame: &ff::frame::Video) -> Result<RgbaFrame, PosterError> {
    to_rgba_max(frame, POSTER_MAX_WIDTH)
}

/// A decoded frame as tightly packed RGBA, no wider than `max_width`.
pub(crate) fn to_rgba_max(
    frame: &ff::frame::Video,
    max_width: u32,
) -> Result<RgbaFrame, PosterError> {
    let (w, h) = (frame.width(), frame.height());
    if w == 0 || h == 0 {
        return Err(PosterError::Decode("empty frame".into()));
    }
    let (out_w, out_h) = if w > max_width {
        let out_h = (u64::from(h) * u64::from(max_width) / u64::from(w)).max(1);
        (max_width, out_h as u32)
    } else {
        (w, h)
    };
    let mut scaler = ff::software::scaling::Context::get(
        frame.format(),
        w,
        h,
        ff::format::Pixel::RGBA,
        out_w,
        out_h,
        ff::software::scaling::Flags::BILINEAR,
    )?;
    let mut rgba = ff::frame::Video::empty();
    scaler.run(frame, &mut rgba)?;
    let stride = rgba.stride(0);
    let row = out_w as usize * 4;
    let data = rgba.data(0);
    let mut pixels = Vec::with_capacity(row * out_h as usize);
    for y in 0..out_h as usize {
        pixels.extend_from_slice(&data[y * stride..y * stride + row]);
    }
    Ok(RgbaFrame {
        width: out_w,
        height: out_h,
        pixels,
    })
}

fn write_png(path: &Path, frame: &RgbaFrame) -> std::io::Result<()> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, frame.width, frame.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(std::io::Error::other)?;
        writer
            .write_image_data(&frame.pixels)
            .map_err(std::io::Error::other)?;
    }
    kairo_store::fs_atomic::write_atomic(path, &bytes)
}
