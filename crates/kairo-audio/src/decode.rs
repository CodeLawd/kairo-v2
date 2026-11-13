//! A file's sound as 48 kHz interleaved stereo float, through FFmpeg.
//!
//! Samples are converted here rather than through libswresample (whose
//! channel-layout API differs across the FFmpeg versions `ffmpeg-next`
//! builds against). Mono plays on both sides; 5.1 is folded to stereo with
//! the Web Audio "speakers" coefficients, as Chromium mixes it; other
//! layouts keep their first two channels. A playback rate other than 1
//! resamples the sound to `48000 / rate`, so it plays faster or slower
//! with its pitch (Chromium keeps the pitch; DECISIONS 056).

use crate::mixer::RATE;
use ff::codec;
use ff::ffi;
use ff::format;
use ff::util::format::sample::{Sample, Type};
use ffmpeg_next as ff;
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};
use std::path::Path;
use std::sync::Once;

fn init() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = ff::init();
        ff::util::log::set_level(ff::util::log::Level::Error);
    });
}

fn err(context: &str) -> impl Fn(ff::Error) -> String + '_ {
    move |e| format!("{context}: {e}")
}

/// The resampler's input block, in source frames.
const CHUNK: usize = 1024;
const SQRT_HALF: f32 = std::f32::consts::FRAC_1_SQRT_2;

pub struct AudioFile {
    input: format::context::Input,
    codec: codec::decoder::Audio,
    index: usize,
    time_base: f64,
    start: f64,
    duration: Option<f64>,
    draining: bool,
    finished: bool,
    /// Frames before this media time are dropped (after a seek).
    skip_until: f64,
    src_rate: u32,
    resampler: Option<Fft<f32>>,
    pending: Vec<f32>,
    out: Vec<f32>,
}

impl AudioFile {
    /// Opens the best audio stream of `path`; `Ok(None)` when it has none
    /// (a silent clip). `rate` is the playback rate, 0.5 to 2.
    pub fn open(path: &Path, rate: f64) -> Result<Option<Self>, String> {
        init();
        if !path.is_file() {
            return Err(format!("{} is not a file", path.display()));
        }
        let input = format::input(path).map_err(err("cannot open"))?;
        let Some(stream) = input.streams().best(ff::media::Type::Audio) else {
            return Ok(None);
        };
        let index = stream.index();
        let time_base = f64::from(stream.time_base());
        if !(time_base.is_finite() && time_base > 0.0) {
            return Err("the audio stream has no time base".into());
        }
        let start = if stream.start_time() == ffi::AV_NOPTS_VALUE {
            0.0
        } else {
            stream.start_time() as f64 * time_base
        };
        let duration = if stream.duration() > 0 {
            Some(stream.duration() as f64 * time_base)
        } else if input.duration() > 0 {
            Some(input.duration() as f64 / f64::from(ffi::AV_TIME_BASE) - start.max(0.0))
        } else {
            None
        }
        .filter(|d| d.is_finite() && *d > 0.0);
        let context =
            codec::context::Context::from_parameters(stream.parameters()).map_err(err("codec"))?;
        let codec = context
            .decoder()
            .audio()
            .map_err(err("cannot open the audio decoder"))?;
        let src_rate = codec.rate();
        if !(8_000..=384_000).contains(&src_rate) {
            return Err(format!("unsupported sample rate {src_rate}"));
        }
        let rate = if rate.is_finite() && rate > 0.0 {
            rate.clamp(0.5, 2.0)
        } else {
            1.0
        };
        let target = (f64::from(RATE) / rate).round() as usize;
        let resampler = if src_rate as usize == target {
            None
        } else {
            Some(
                Fft::<f32>::new(src_rate as usize, target, CHUNK, 2, FixedSync::Input)
                    .map_err(|e| e.to_string())?,
            )
        };
        let out = vec![0.0; resampler.as_ref().map_or(0, |r| r.output_frames_max() * 2)];
        Ok(Some(Self {
            input,
            codec,
            index,
            time_base,
            start,
            duration,
            draining: false,
            finished: false,
            skip_until: 0.0,
            src_rate,
            resampler,
            pending: Vec::with_capacity(CHUNK * 4),
            out,
        }))
    }

    /// The stream's length in seconds, when the file says.
    pub fn duration(&self) -> Option<f64> {
        self.duration
    }

    /// Jumps to `seconds` of media time.
    pub fn seek(&mut self, seconds: f64) -> Result<(), String> {
        if !(seconds.is_finite() && seconds >= 0.0) {
            return Err("invalid seek time".into());
        }
        let ts = ((self.start + seconds) * f64::from(ffi::AV_TIME_BASE)).round() as i64;
        self.input.seek(ts, ..ts).map_err(err("seek"))?;
        self.codec.flush();
        self.draining = false;
        self.finished = false;
        self.skip_until = seconds;
        self.pending.clear();
        if let Some(r) = self.resampler.as_mut() {
            r.reset();
        }
        Ok(())
    }

    /// Appends the next decoded stretch to `out` as 48 kHz interleaved
    /// stereo. Returns false at the end of the stream.
    pub fn read(&mut self, out: &mut Vec<f32>) -> Result<bool, String> {
        if self.finished {
            return Ok(false);
        }
        let mut frame = ff::frame::Audio::empty();
        loop {
            match self.codec.receive_frame(&mut frame) {
                Ok(()) => break,
                Err(ff::Error::Eof) => {
                    self.finished = true;
                    self.flush_tail(out)?;
                    return Ok(false);
                }
                Err(ff::Error::Other { errno }) if errno == libc::EAGAIN => {}
                Err(e) => return Err(format!("decode: {e}")),
            }
            if self.draining {
                self.finished = true;
                self.flush_tail(out)?;
                return Ok(false);
            }
            let mut packet = ff::Packet::empty();
            match packet.read(&mut self.input) {
                Ok(()) if packet.stream() == self.index => match self.codec.send_packet(&packet) {
                    Ok(()) => {}
                    // A damaged packet: skip it, as players do.
                    Err(ff::Error::InvalidData) => {}
                    Err(e) => return Err(format!("decode: {e}")),
                },
                Ok(()) => {}
                Err(ff::Error::Eof) => {
                    self.codec.send_eof().map_err(err("decode"))?;
                    self.draining = true;
                }
                Err(e) => return Err(format!("read: {e}")),
            }
        }
        let pts = frame
            .timestamp()
            .or_else(|| frame.pts())
            .map(|v| v as f64 * self.time_base - self.start);
        let mut stereo = to_stereo(&frame)?;
        // Drop what lies before the seek target.
        if let Some(pts) = pts
            && pts < self.skip_until
        {
            let skip = ((self.skip_until - pts) * f64::from(self.src_rate)).round() as usize;
            let skip = skip.min(stereo.len() / 2);
            stereo.drain(..skip * 2);
        }
        self.skip_until = 0.0;
        self.resample(&stereo, out)?;
        Ok(true)
    }

    fn resample(&mut self, input: &[f32], out: &mut Vec<f32>) -> Result<(), String> {
        let Some(resampler) = self.resampler.as_mut() else {
            out.extend_from_slice(input);
            return Ok(());
        };
        self.pending.extend_from_slice(input);
        let mut offset = 0;
        loop {
            let need = resampler.input_frames_next();
            if (self.pending.len() - offset) / 2 < need {
                break;
            }
            let slice = &self.pending[offset..offset + need * 2];
            let input = InterleavedSlice::new(slice, 2, need).map_err(|e| e.to_string())?;
            let capacity = self.out.len() / 2;
            let mut output =
                InterleavedSlice::new_mut(&mut self.out, 2, capacity).map_err(|e| e.to_string())?;
            let (read, written) = resampler
                .process_into_buffer(&input, &mut output, None)
                .map_err(|e| e.to_string())?;
            offset += read * 2;
            out.extend_from_slice(&self.out[..written * 2]);
        }
        self.pending.drain(..offset);
        Ok(())
    }

    /// The last partial block through the resampler, padded with silence.
    fn flush_tail(&mut self, out: &mut Vec<f32>) -> Result<(), String> {
        if self.resampler.is_none() || self.pending.is_empty() {
            return Ok(());
        }
        let real = self.pending.len() / 2;
        let need = self.resampler.as_ref().map_or(0, |r| r.input_frames_next());
        self.pending.resize(need.max(real) * 2, 0.0);
        let before = out.len();
        let tail = std::mem::take(&mut self.pending);
        self.resample(&tail, out)?;
        // Only as much as the real samples cover.
        let ratio = self.out_rate() / f64::from(self.src_rate);
        let keep = before + ((real as f64 * ratio).round() as usize) * 2;
        out.truncate(keep.min(out.len()));
        Ok(())
    }

    fn out_rate(&self) -> f64 {
        match &self.resampler {
            Some(r) => {
                f64::from(self.src_rate) * r.output_frames_max() as f64
                    / r.input_frames_max() as f64
            }
            None => f64::from(self.src_rate),
        }
    }
}

/// One decoded frame as interleaved stereo at its own rate.
fn to_stereo(frame: &ff::frame::Audio) -> Result<Vec<f32>, String> {
    let n = frame.samples();
    let channels = usize::from(frame.channels()).max(1);
    let format = frame.format();
    let planar = matches!(
        format,
        Sample::U8(Type::Planar)
            | Sample::I16(Type::Planar)
            | Sample::I32(Type::Planar)
            | Sample::I64(Type::Planar)
            | Sample::F32(Type::Planar)
            | Sample::F64(Type::Planar)
    );
    let width = match format {
        Sample::U8(_) => 1,
        Sample::I16(_) => 2,
        Sample::I32(_) | Sample::F32(_) => 4,
        Sample::I64(_) | Sample::F64(_) => 8,
        Sample::None => return Err("no sample format".into()),
    };
    let value = |bytes: &[u8]| -> f32 {
        match format {
            Sample::U8(_) => (f32::from(bytes[0]) - 128.0) / 128.0,
            Sample::I16(_) => f32::from(i16::from_ne_bytes([bytes[0], bytes[1]])) / 32_768.0,
            Sample::I32(_) => {
                i32::from_ne_bytes(bytes[..4].try_into().unwrap_or([0; 4])) as f32 / 2_147_483_648.0
            }
            Sample::I64(_) => {
                (i64::from_ne_bytes(bytes[..8].try_into().unwrap_or([0; 8])) as f64
                    / 9_223_372_036_854_775_808.0) as f32
            }
            Sample::F32(_) => f32::from_ne_bytes(bytes[..4].try_into().unwrap_or([0; 4])),
            Sample::F64(_) => f64::from_ne_bytes(bytes[..8].try_into().unwrap_or([0; 8])) as f32,
            Sample::None => 0.0,
        }
    };
    let sample = |i: usize, c: usize| -> f32 {
        let (plane, at) = if planar {
            (c, i * width)
        } else {
            (0, (i * channels + c) * width)
        };
        let data = frame.data(plane);
        data.get(at..at + width).map_or(0.0, value)
    };
    let mut out = Vec::with_capacity(n * 2);
    for i in 0..n {
        let (l, r) = match channels {
            1 => {
                let s = sample(i, 0);
                (s, s)
            }
            // FL FR FC LFE SL SR: Web Audio's speaker downmix.
            6 => {
                let c = sample(i, 2) * SQRT_HALF;
                (
                    sample(i, 0) + c + sample(i, 4) * SQRT_HALF,
                    sample(i, 1) + c + sample(i, 5) * SQRT_HALF,
                )
            }
            _ => (sample(i, 0), sample(i, 1)),
        };
        out.push(l);
        out.push(r);
    }
    Ok(out)
}
