use anyhow::{Context, Result, anyhow, bail, ensure};
use ff::{codec, ffi, format, frame::Video, software::scaling, util::color};
use ffmpeg_next as ff;
use std::{path::Path, ptr, time::Instant};

#[derive(Clone, Copy)]
pub struct Color {
    pub kr: f32,
    pub kb: f32,
    pub full: bool,
}

impl Color {
    fn from_frame(frame: &Video) -> Self {
        let (kr, kb) = match frame.color_space() {
            color::Space::BT470BG | color::Space::SMPTE170M => (0.299, 0.114),
            color::Space::SMPTE240M => (0.212, 0.087),
            color::Space::BT2020NCL => (0.2627, 0.0593),
            _ if frame.width() < 1280 && frame.height() <= 576 => (0.299, 0.114),
            _ => (0.2126, 0.0722),
        };
        let (kr, kb) = if frame.color_space() == color::Space::BT709 {
            (0.2126, 0.0722)
        } else {
            (kr, kb)
        };
        Self {
            kr,
            kb,
            full: frame.color_range() == color::Range::JPEG,
        }
    }
}

pub struct Frame {
    pub pts: f64,
    pub pixels: Video,
    pub alpha: Option<Vec<u8>>,
    pub color: Color,
    pub bits: u32,
    pub transfer_ms: Option<f64>,
}

pub struct Decoder {
    input: format::context::Input,
    codec: codec::decoder::Video,
    index: usize,
    time_base: f64,
    start: f64,
    pub interval: f64,
    pub backend: &'static str,
    draining: bool,
    next_pts: f64,
    scaler: Option<scaling::Context>,
    path: std::path::PathBuf,
}

impl Decoder {
    pub fn open(path: &Path, hardware: bool) -> Result<Self> {
        if hardware {
            match Self::create(path, true).and_then(|mut decoder| {
                decoder
                    .next()
                    .context("hardware probe")?
                    .context("no video frames")?;
                decoder.seek(0.0)?;
                Ok(decoder)
            }) {
                Ok(decoder) => return Ok(decoder),
                Err(error) => eprintln!("hardware fallback for {}: {error:#}", path.display()),
            }
        }
        Self::create(path, false)
    }

    fn create(path: &Path, hardware: bool) -> Result<Self> {
        let input = format::input(path).with_context(|| format!("open {}", path.display()))?;
        let stream = input
            .streams()
            .best(ff::media::Type::Video)
            .context("input has no video stream")?;
        let index = stream.index();
        let time_base = f64::from(stream.time_base());
        ensure!(
            time_base.is_finite() && time_base > 0.0,
            "invalid time base"
        );
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
        let parameters = stream.parameters();
        let codec_id = parameters.id();
        if hardware
            && (codec_id == codec::Id::PRORES
                || codec_id == codec::Id::VP9 && stream.metadata().get("alpha_mode") == Some("1"))
        {
            bail!("use software decode to preserve possible alpha");
        }
        // VP9 alpha lives in Matroska BlockAdditional. The native VP9 decoder
        // ignores it. Prefer the libvpx decoder when the build provides it.
        let selected =
            if codec_id == codec::Id::VP9 && stream.metadata().get("alpha_mode") == Some("1") {
                Some(
                    codec::decoder::find_by_name("libvpx-vp9")
                        .context("alpha WebM requires a libvpx-enabled FFmpeg build")?,
                )
            } else {
                codec::decoder::find(codec_id)
            }
            .context("no decoder for stream")?;
        let mut context = codec::context::Context::from_parameters(parameters)?;
        let backend = if hardware {
            configure_hardware(&mut context, selected)?
        } else {
            "software"
        };
        let codec = context.decoder().open_as(selected)?.video()?;
        Ok(Self {
            input,
            codec,
            index,
            time_base,
            start,
            interval,
            backend,
            draining: false,
            next_pts: 0.0,
            scaler: None,
            path: path.to_owned(),
        })
    }

    pub fn seek(&mut self, seconds: f64) -> Result<()> {
        ensure!(seconds.is_finite() && seconds >= 0.0, "invalid seek time");
        let timestamp = ((self.start + seconds) * f64::from(ffi::AV_TIME_BASE)).round() as i64;
        self.input
            .seek(timestamp, ..timestamp)
            .context("seek demuxer")?;
        self.codec.flush();
        self.draining = false;
        self.next_pts = seconds;
        Ok(())
    }

    pub fn next(&mut self) -> Result<Option<Frame>> {
        match self.decode() {
            Err(error) if self.backend != "software" => {
                eprintln!(
                    "{} decode failed at {:.3}s: {error:#}; switching to software",
                    self.backend, self.next_pts
                );
                let target = self.next_pts.max(0.0);
                let mut fallback = Self::create(&self.path, false)?;
                fallback.seek(target)?;
                *self = fallback;
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

    fn decode(&mut self) -> Result<Option<Frame>> {
        let mut decoded = Video::empty();
        loop {
            match self.codec.receive_frame(&mut decoded) {
                Ok(()) => break,
                Err(ff::Error::Eof) => return Ok(None),
                Err(ff::Error::Other { errno }) if errno == libc::EAGAIN => {}
                Err(error) => return Err(error).context("receive decoded video frame"),
            }
            ensure!(!self.draining, "decoder stalled while draining");
            let mut packet = ff::Packet::empty();
            match packet.read(&mut self.input) {
                Ok(()) if packet.stream() == self.index => self.codec.send_packet(&packet)?,
                Ok(()) => {}
                Err(ff::Error::Eof) => {
                    self.codec.send_eof()?;
                    self.draining = true;
                }
                Err(error) => return Err(error).context("read video packet"),
            }
        }
        let pts = decoded
            .timestamp()
            .or_else(|| decoded.pts())
            .map_or(self.next_pts, |value| {
                value as f64 * self.time_base - self.start
            });
        let color = Color::from_frame(&decoded);
        let mut transferred = Video::empty();
        let mut transfer_ms = None;
        // SAFETY: both AVFrames are live; transfer only when FFmpeg supplied a
        // hardware frame and let FFmpeg allocate the destination planes.
        let source = unsafe {
            if (*decoded.as_ptr()).hw_frames_ctx.is_null() {
                &decoded
            } else {
                let started = Instant::now();
                let result =
                    ffi::av_hwframe_transfer_data(transferred.as_mut_ptr(), decoded.as_ptr(), 0);
                if result < 0 {
                    return Err(ff::Error::from(result)).context("download hardware frame");
                }
                transfer_ms = Some(started.elapsed().as_secs_f64() * 1000.0);
                &transferred
            }
        };
        use format::Pixel;
        // SAFETY: av_pix_fmt_desc_get returns a static descriptor or null.
        let descriptor = unsafe { ffi::av_pix_fmt_desc_get(source.format().into()).as_ref() }
            .context("unknown decoded pixel format")?;
        let has_alpha = descriptor.flags & (ffi::AV_PIX_FMT_FLAG_ALPHA as u64) != 0;
        let bits = if !has_alpha && descriptor.comp[0].depth > 8 {
            10
        } else {
            8
        };
        let target = if bits == 10 {
            Pixel::P010LE
        } else {
            Pixel::NV12
        };
        let mut pixels;
        let mut alpha = None;
        if has_alpha {
            let planar = self.scale(source, Pixel::YUVA420P)?;
            let (width, height) = (source.width(), source.height());
            pixels = Video::new(Pixel::NV12, width, height);
            copy_plane(&planar, 0, &mut pixels, 0, width as usize, height as usize);
            let stride = pixels.stride(1);
            for row in 0..height.div_ceil(2) as usize {
                for col in 0..width.div_ceil(2) as usize {
                    let u = planar.data(1)[row * planar.stride(1) + col];
                    let v = planar.data(2)[row * planar.stride(2) + col];
                    let offset = row * stride + col * 2;
                    pixels.data_mut(1)[offset..offset + 2].copy_from_slice(&[u, v]);
                }
            }
            alpha = Some(
                planar
                    .data(3)
                    .chunks_exact(planar.stride(3))
                    .take(height as usize)
                    .flat_map(|row| row[..width as usize].iter().copied())
                    .collect(),
            );
        } else if source.format() == target {
            // Return ownership rather than making another full-frame CPU copy.
            pixels = if ptr::eq(source, &decoded) {
                decoded
            } else {
                transferred
            };
            self.next_pts = pts + self.interval;
            return Ok(Some(Frame {
                pts,
                pixels,
                alpha,
                color,
                bits,
                transfer_ms,
            }));
        } else {
            pixels = self.scale(source, target)?;
        }
        self.next_pts = pts + self.interval;
        Ok(Some(Frame {
            pts,
            pixels,
            alpha,
            color,
            bits,
            transfer_ms,
        }))
    }

    fn scale(&mut self, input: &Video, target: format::Pixel) -> Result<Video> {
        let (width, height) = (input.width(), input.height());
        if self.scaler.as_ref().is_none_or(|s| {
            s.input().format != input.format()
                || s.output().format != target
                || s.input().width != width
                || s.input().height != height
        }) {
            self.scaler = Some(scaling::Context::get(
                input.format(),
                width,
                height,
                target,
                width,
                height,
                scaling::Flags::BILINEAR,
            )?);
        }
        let mut output = Video::empty();
        self.scaler
            .as_mut()
            .context("scaler unavailable")?
            .run(input, &mut output)?;
        Ok(output)
    }
}

fn copy_plane(
    src: &Video,
    src_plane: usize,
    dst: &mut Video,
    dst_plane: usize,
    width: usize,
    rows: usize,
) {
    let src_stride = src.stride(src_plane);
    let dst_stride = dst.stride(dst_plane);
    for (from, to) in src
        .data(src_plane)
        .chunks_exact(src_stride)
        .zip(dst.data_mut(dst_plane).chunks_exact_mut(dst_stride))
        .take(rows)
    {
        to[..width].copy_from_slice(&from[..width]);
    }
}

fn configure_hardware(
    context: &mut codec::context::Context,
    codec: ff::Codec,
) -> Result<&'static str> {
    use ffi::AVHWDeviceType::*;
    let (kind, name) = if cfg!(target_os = "macos") {
        (AV_HWDEVICE_TYPE_VIDEOTOOLBOX, "videotoolbox")
    } else if cfg!(target_os = "windows") {
        (AV_HWDEVICE_TYPE_D3D11VA, "d3d11va")
    } else if cfg!(target_os = "linux") {
        (AV_HWDEVICE_TYPE_VAAPI, "vaapi")
    } else {
        bail!("no configured hardware backend on this OS");
    };
    // SAFETY: the FFmpeg codec exposes a terminated configuration list.
    // On success av_hwdevice_ctx_create transfers one AVBufferRef to context.
    unsafe {
        let available = (0..)
            .map(|i| ffi::avcodec_get_hw_config(codec.as_ptr(), i))
            .take_while(|config| !config.is_null())
            .any(|config| (*config).device_type == kind && (*config).methods & 1 != 0);
        ensure!(available, "{name} unsupported for this decoder");
        let raw = context.as_mut_ptr();
        let result = ffi::av_hwdevice_ctx_create(
            &mut (*raw).hw_device_ctx,
            kind,
            ptr::null(),
            ptr::null_mut(),
            0,
        );
        if result < 0 {
            return Err(anyhow!(ff::Error::from(result))).context("create hardware device");
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
    // SAFETY: libavcodec calls this with a valid context and a terminated list.
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
        for index in 0.. {
            let candidate = *formats.add(index);
            if candidate == expected || candidate == AV_PIX_FMT_NONE {
                return candidate;
            }
        }
    }
    ffi::AVPixelFormat::AV_PIX_FMT_NONE
}
