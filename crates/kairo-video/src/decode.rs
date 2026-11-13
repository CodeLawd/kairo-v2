//! FFmpeg decoding into YUV planes (from spike 004): hardware first
//! (VideoToolbox, D3D11VA, VAAPI), software when the hardware cannot open
//! the stream or fails during it.

use crate::transport::{Decoded, Source};
use ff::{codec, ffi, format, frame::Video, software::scaling, util::color};
use ffmpeg_next as ff;
use kairo_compositor::{YuvColor, YuvFrame};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::{Arc, Once};

/// Starts FFmpeg once and keeps its own logging to errors.
pub fn init() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = ff::init();
        ff::util::log::set_level(ff::util::log::Level::Error);
    });
}

/// How a stream is being decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Hardware(&'static str),
    Software,
}

pub struct Decoder {
    input: format::context::Input,
    codec: codec::decoder::Video,
    index: usize,
    time_base: f64,
    start: f64,
    interval: f64,
    duration: Option<f64>,
    backend: Backend,
    draining: bool,
    next_pts: f64,
    scaler: Option<scaling::Context>,
    path: PathBuf,
}

/// Opens with `hardware` first when asked; a hardware decoder that cannot
/// produce the first frame is dropped for software, and the reason logged.
pub fn open(path: &Path, hardware: bool) -> Result<Decoder, String> {
    init();
    if hardware {
        match Decoder::create(path, true).and_then(|mut d| {
            d.decode()?.ok_or("no video frames")?;
            d.seek(0.0)?;
            Ok(d)
        }) {
            Ok(decoder) => return Ok(decoder),
            Err(error) => tracing::info!(
                path = %path.display(),
                "hardware video decode unavailable, using software: {error}"
            ),
        }
    }
    Decoder::create(path, false)
}

fn err(context: &str) -> impl Fn(ff::Error) -> String + '_ {
    move |e| format!("{context}: {e}")
}

impl Decoder {
    pub fn backend(&self) -> Backend {
        self.backend
    }

    pub fn size(&self) -> (u32, u32) {
        (self.codec.width(), self.codec.height())
    }

    fn create(path: &Path, hardware: bool) -> Result<Self, String> {
        if !path.is_file() {
            return Err(format!("{} is not a file", path.display()));
        }
        let input = format::input(path).map_err(err("cannot open"))?;
        let stream = input
            .streams()
            .best(ff::media::Type::Video)
            .ok_or("the file has no video stream")?;
        let index = stream.index();
        let time_base = f64::from(stream.time_base());
        if !(time_base.is_finite() && time_base > 0.0) {
            return Err("the video stream has no time base".into());
        }
        let start = if stream.start_time() == ffi::AV_NOPTS_VALUE {
            0.0
        } else {
            stream.start_time() as f64 * time_base
        };
        let rate = f64::from(stream.avg_frame_rate());
        let interval = if rate.is_finite() && rate > 0.0 {
            1.0 / rate
        } else {
            1.0 / 30.0
        };
        let duration = if stream.duration() > 0 {
            Some(stream.duration() as f64 * time_base)
        } else if input.duration() > 0 {
            Some(input.duration() as f64 / f64::from(ffi::AV_TIME_BASE) - start.max(0.0))
        } else {
            None
        }
        .filter(|d| d.is_finite() && *d > 0.0);
        let parameters = stream.parameters();
        let id = parameters.id();
        let alpha_vp9 = id == codec::Id::VP9 && stream.metadata().get("alpha_mode") == Some("1");
        if hardware && (id == codec::Id::PRORES || alpha_vp9) {
            return Err("software decode keeps the alpha channel".into());
        }
        // VP9 alpha is in Matroska BlockAdditional, which only libvpx reads.
        let selected = if alpha_vp9 {
            codec::decoder::find_by_name("libvpx-vp9").or_else(|| {
                tracing::warn!(
                    path = %path.display(),
                    "this FFmpeg has no libvpx: the video's transparency is lost"
                );
                codec::decoder::find(id)
            })
        } else {
            codec::decoder::find(id)
        }
        .ok_or_else(|| format!("no decoder for {id:?}"))?;
        let mut context =
            codec::context::Context::from_parameters(parameters).map_err(err("codec"))?;
        let backend = if hardware {
            Backend::Hardware(configure_hardware(&mut context, selected)?)
        } else {
            Backend::Software
        };
        let codec = context
            .decoder()
            .open_as(selected)
            .and_then(|o| o.video())
            .map_err(err("cannot open the decoder"))?;
        Ok(Self {
            input,
            codec,
            index,
            time_base,
            start,
            interval,
            duration,
            backend,
            draining: false,
            next_pts: 0.0,
            scaler: None,
            path: path.to_owned(),
        })
    }

    fn seek_to(&mut self, seconds: f64) -> Result<(), String> {
        if !(seconds.is_finite() && seconds >= 0.0) {
            return Err("invalid seek time".into());
        }
        let ts = ((self.start + seconds) * f64::from(ffi::AV_TIME_BASE)).round() as i64;
        self.input.seek(ts, ..ts).map_err(err("seek"))?;
        self.codec.flush();
        self.draining = false;
        self.next_pts = seconds;
        Ok(())
    }

    fn decode(&mut self) -> Result<Option<Decoded<Arc<YuvFrame>>>, String> {
        let mut decoded = Video::empty();
        loop {
            match self.codec.receive_frame(&mut decoded) {
                Ok(()) => break,
                Err(ff::Error::Eof) => return Ok(None),
                Err(ff::Error::Other { errno }) if errno == libc::EAGAIN => {}
                Err(e) => return Err(format!("decode: {e}")),
            }
            if self.draining {
                return Err("the decoder stalled at the end of the stream".into());
            }
            let mut packet = ff::Packet::empty();
            match packet.read(&mut self.input) {
                Ok(()) if packet.stream() == self.index => {
                    match self.codec.send_packet(&packet) {
                        Ok(()) => {}
                        // A damaged packet: skip it, as players do.
                        Err(ff::Error::InvalidData) => {
                            tracing::debug!(path = %self.path.display(), "skipped a damaged packet")
                        }
                        Err(e) => return Err(format!("decode: {e}")),
                    }
                }
                Ok(()) => {}
                Err(ff::Error::Eof) => {
                    self.codec.send_eof().map_err(err("decode"))?;
                    self.draining = true;
                }
                Err(e) => return Err(format!("read: {e}")),
            }
        }
        let pts = decoded
            .timestamp()
            .or_else(|| decoded.pts())
            .map_or(self.next_pts, |v| v as f64 * self.time_base - self.start);
        self.next_pts = pts + self.interval;
        let picture = self.planes(decoded)?;
        Ok(Some(Decoded {
            pts,
            picture: Arc::new(picture),
        }))
    }

    /// The decoded frame as NV12 or P010, with alpha when it has one.
    fn planes(&mut self, decoded: Video) -> Result<YuvFrame, String> {
        use format::Pixel;
        let color = color_of(&decoded);
        let mut transferred = Video::empty();
        // SAFETY: both frames are live; transfer only a hardware frame, and
        // FFmpeg allocates the destination planes.
        let hardware = unsafe { !(*decoded.as_ptr()).hw_frames_ctx.is_null() };
        if hardware {
            let r = unsafe {
                ffi::av_hwframe_transfer_data(transferred.as_mut_ptr(), decoded.as_ptr(), 0)
            };
            if r < 0 {
                return Err(format!("download hardware frame: {}", ff::Error::from(r)));
            }
        }
        let source = if hardware { &transferred } else { &decoded };
        let (width, height) = (source.width(), source.height());
        if width == 0 || height == 0 {
            return Err("a decoded frame has no size".into());
        }
        // SAFETY: av_pix_fmt_desc_get returns a static descriptor or null.
        let descriptor = unsafe { ffi::av_pix_fmt_desc_get(source.format().into()).as_ref() }
            .ok_or("unknown decoded pixel format")?;
        let has_alpha = descriptor.flags & (ffi::AV_PIX_FMT_FLAG_ALPHA as u64) != 0;
        let bits = if !has_alpha && descriptor.comp[0].depth > 8 {
            10
        } else {
            8
        };
        if has_alpha {
            let planar = self.scale(source, Pixel::YUVA420P)?;
            let (cw, ch) = (width.div_ceil(2) as usize, height.div_ceil(2) as usize);
            let mut chroma = vec![0u8; cw * ch * 2];
            for row in 0..ch {
                let (u, v) = (
                    &planar.data(1)[row * planar.stride(1)..],
                    &planar.data(2)[row * planar.stride(2)..],
                );
                for col in 0..cw {
                    chroma[(row * cw + col) * 2] = u[col];
                    chroma[(row * cw + col) * 2 + 1] = v[col];
                }
            }
            return Ok(YuvFrame {
                width,
                height,
                bits: 8,
                color,
                luma: tight(&planar, 0, width as usize, height as usize),
                luma_stride: width,
                chroma,
                chroma_stride: (cw * 2) as u32,
                alpha: Some(tight(&planar, 3, width as usize, height as usize)),
            });
        }
        let target = if bits == 10 {
            Pixel::P010LE
        } else {
            Pixel::NV12
        };
        let scaled;
        let frame = if source.format() == target {
            source
        } else {
            scaled = self.scale(source, target)?;
            &scaled
        };
        let sample = if bits == 10 { 2 } else { 1 };
        let (cw, ch) = (width.div_ceil(2) as usize, height.div_ceil(2) as usize);
        Ok(YuvFrame {
            width,
            height,
            bits,
            color,
            luma: tight(frame, 0, width as usize * sample, height as usize),
            luma_stride: width * sample as u32,
            chroma: tight(frame, 1, cw * 2 * sample, ch),
            chroma_stride: (cw * 2 * sample) as u32,
            alpha: None,
        })
    }

    fn scale(&mut self, input: &Video, target: format::Pixel) -> Result<Video, String> {
        let (w, h) = (input.width(), input.height());
        if self.scaler.as_ref().is_none_or(|s| {
            s.input().format != input.format()
                || s.output().format != target
                || s.input().width != w
                || s.input().height != h
        }) {
            self.scaler = Some(
                scaling::Context::get(input.format(), w, h, target, w, h, scaling::Flags::BILINEAR)
                    .map_err(err("convert"))?,
            );
        }
        let mut output = Video::empty();
        self.scaler
            .as_mut()
            .ok_or("no converter")?
            .run(input, &mut output)
            .map_err(err("convert"))?;
        Ok(output)
    }
}

/// A plane's rows packed `row` bytes apart.
fn tight(frame: &Video, plane: usize, row: usize, rows: usize) -> Vec<u8> {
    let stride = frame.stride(plane);
    let data = frame.data(plane);
    let mut out = Vec::with_capacity(row * rows);
    for r in 0..rows {
        out.extend_from_slice(&data[r * stride..r * stride + row]);
    }
    out
}

fn color_of(frame: &Video) -> YuvColor {
    let (kr, kb) = match frame.color_space() {
        color::Space::BT709 => (0.2126, 0.0722),
        color::Space::BT470BG | color::Space::SMPTE170M => (0.299, 0.114),
        color::Space::SMPTE240M => (0.212, 0.087),
        color::Space::BT2020NCL => (0.2627, 0.0593),
        // Unstated: SD sizes are BT.601, the rest BT.709.
        _ if frame.width() < 1280 && frame.height() <= 576 => (0.299, 0.114),
        _ => (0.2126, 0.0722),
    };
    YuvColor {
        kr,
        kb,
        full_range: frame.color_range() == color::Range::JPEG,
    }
}

impl Source for Decoder {
    type Picture = Arc<YuvFrame>;

    fn next(&mut self) -> Result<Option<Decoded<Arc<YuvFrame>>>, String> {
        match self.decode() {
            Err(error) if self.backend != Backend::Software => {
                let target = self.next_pts.max(0.0);
                tracing::warn!(
                    path = %self.path.display(),
                    "hardware video decode failed at {target:.3} s, switching to software: {error}"
                );
                let mut fallback = Self::create(&self.path, false)?;
                fallback.seek_to(target)?;
                *self = fallback;
                // Frames before the failure were shown already.
                while let Some(frame) = self.decode()? {
                    if frame.pts + 1e-6 >= target {
                        return Ok(Some(frame));
                    }
                }
                Ok(None)
            }
            result => result,
        }
    }

    fn seek(&mut self, seconds: f64) -> Result<(), String> {
        self.seek_to(seconds)
    }

    fn duration(&self) -> Option<f64> {
        self.duration
    }

    fn interval(&self) -> f64 {
        self.interval
    }
}

fn configure_hardware(
    context: &mut codec::context::Context,
    codec: ff::Codec,
) -> Result<&'static str, String> {
    use ffi::AVHWDeviceType::*;
    let (kind, name) = if cfg!(target_os = "macos") {
        (AV_HWDEVICE_TYPE_VIDEOTOOLBOX, "videotoolbox")
    } else if cfg!(target_os = "windows") {
        (AV_HWDEVICE_TYPE_D3D11VA, "d3d11va")
    } else if cfg!(target_os = "linux") {
        (AV_HWDEVICE_TYPE_VAAPI, "vaapi")
    } else {
        return Err("no hardware decoder on this system".into());
    };
    // SAFETY: the codec's configuration list ends with null. On success
    // av_hwdevice_ctx_create hands one AVBufferRef to the context, which
    // frees it with the context.
    unsafe {
        let available = (0..)
            .map(|i| ffi::avcodec_get_hw_config(codec.as_ptr(), i))
            .take_while(|c| !c.is_null())
            .any(|c| (*c).device_type == kind && (*c).methods & 1 != 0);
        if !available {
            return Err(format!("{name} does not decode this codec"));
        }
        let raw = context.as_mut_ptr();
        let r = ffi::av_hwdevice_ctx_create(
            &mut (*raw).hw_device_ctx,
            kind,
            ptr::null(),
            ptr::null_mut(),
            0,
        );
        if r < 0 {
            return Err(format!("{name} device: {}", ff::Error::from(r)));
        }
        (*raw).get_format = Some(select_hardware_format);
    }
    Ok(name)
}

unsafe extern "C" fn select_hardware_format(
    context: *mut ffi::AVCodecContext,
    formats: *const ffi::AVPixelFormat,
) -> ffi::AVPixelFormat {
    use ffi::{AVHWDeviceType::*, AVPixelFormat::*};
    // SAFETY: libavcodec calls this with a valid context and a list ending
    // in AV_PIX_FMT_NONE.
    unsafe {
        let device = (*(*context).hw_device_ctx)
            .data
            .cast::<ffi::AVHWDeviceContext>();
        let expected = match (*device).type_ {
            AV_HWDEVICE_TYPE_VIDEOTOOLBOX => AV_PIX_FMT_VIDEOTOOLBOX,
            AV_HWDEVICE_TYPE_D3D11VA => AV_PIX_FMT_D3D11,
            AV_HWDEVICE_TYPE_VAAPI => AV_PIX_FMT_VAAPI,
            _ => AV_PIX_FMT_NONE,
        };
        for i in 0.. {
            let candidate = *formats.add(i);
            if candidate == expected {
                return candidate;
            }
            if candidate == AV_PIX_FMT_NONE {
                break;
            }
        }
        // No hardware format offered: fail, so the stream reopens in
        // software.
        AV_PIX_FMT_NONE
    }
}
