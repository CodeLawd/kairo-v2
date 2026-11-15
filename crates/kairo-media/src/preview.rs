//! Private preview playback for the media dock and inspector (prompt 083).
//!
//! Electron played a muted `<video>` in the renderer. Here FFmpeg decodes
//! the file in this process, scaled down to a thumbnail width, on a worker
//! thread the dock owns: a preview never reaches the engine or a screen,
//! and the dock applies colour adjustments to the decoded pixels, so moving
//! a slider never restarts the video.

use ffmpeg_next as ff;
use std::path::Path;

use crate::poster::{PosterError, RgbaFrame, init, to_rgba_max};

/// One decoded preview frame and its time in seconds.
pub struct PreviewFrame {
    pub frame: RgbaFrame,
    pub seconds: f64,
}

/// A file opened for preview, decoded front to back.
pub struct PreviewStream {
    input: ff::format::context::Input,
    decoder: ff::decoder::Video,
    index: usize,
    time_base: f64,
    start: f64,
    max_width: u32,
    duration: Option<f64>,
    interval: f64,
    draining: bool,
}

impl PreviewStream {
    pub fn open(path: &Path, max_width: u32) -> Result<Self, PosterError> {
        init();
        let input = ff::format::input(path)?;
        let duration = (input.duration() > 0)
            .then(|| input.duration() as f64 / f64::from(ff::ffi::AV_TIME_BASE));
        let stream = input
            .streams()
            .best(ff::media::Type::Video)
            .ok_or_else(|| PosterError::Decode("no video stream".into()))?;
        let index = stream.index();
        let time_base = f64::from(stream.time_base());
        let start = if stream.start_time() == ff::ffi::AV_NOPTS_VALUE {
            0.0
        } else {
            stream.start_time() as f64 * time_base
        };
        let rate = f64::from(stream.avg_frame_rate());
        let interval = if rate.is_finite() && rate > 1.0 {
            1.0 / rate
        } else {
            1.0 / 30.0
        };
        let context = ff::codec::context::Context::from_parameters(stream.parameters())?;
        let decoder = context.decoder().video()?;
        Ok(Self {
            input,
            decoder,
            index,
            time_base,
            start,
            max_width: max_width.max(16),
            duration,
            interval,
            draining: false,
        })
    }

    /// The container's duration, when it states one.
    pub fn duration(&self) -> Option<f64> {
        self.duration
    }

    /// Seconds between frames.
    pub fn interval(&self) -> f64 {
        self.interval
    }

    /// Moves to `seconds`: the next frame is the first at or after it.
    pub fn seek(&mut self, seconds: f64) -> Result<(), PosterError> {
        let target = (self.start + seconds.max(0.0)) * f64::from(ff::ffi::AV_TIME_BASE);
        let target = target as i64;
        self.input.seek(target, ..target)?;
        self.decoder.flush();
        self.draining = false;
        Ok(())
    }

    /// The next frame; `None` at the end of the file.
    pub fn next_frame(&mut self) -> Result<Option<PreviewFrame>, PosterError> {
        let mut decoded = ff::frame::Video::empty();
        loop {
            match self.decoder.receive_frame(&mut decoded) {
                Ok(()) => {
                    let pts = decoded.timestamp().or_else(|| decoded.pts()).unwrap_or(0);
                    let seconds = (pts as f64 * self.time_base - self.start).max(0.0);
                    return Ok(Some(PreviewFrame {
                        frame: to_rgba_max(&decoded, self.max_width)?,
                        seconds,
                    }));
                }
                Err(ff::Error::Eof) => return Ok(None),
                Err(ff::Error::Other { errno }) if errno == ff::util::error::EAGAIN => {}
                Err(e) => return Err(e.into()),
            }
            if self.draining {
                return Ok(None);
            }
            let mut sent = false;
            for (stream, packet) in self.input.packets() {
                if stream.index() == self.index {
                    self.decoder.send_packet(&packet)?;
                    sent = true;
                    break;
                }
            }
            if !sent {
                self.decoder.send_eof()?;
                self.draining = true;
            }
        }
    }
}
