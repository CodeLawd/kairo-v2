//! A camera's audio input, played with its picture.
//!
//! `overlay.html` asked for the input with echo cancellation, noise
//! suppression and gain control off ("program sound, not a call"). Core
//! Audio input through cpal applies none of those, so the sound arrives as
//! the device sends it. It is converted to 48 kHz stereo and pushed into a
//! mixer source. The callback never locks or allocates: the conversion
//! buffer is made before the stream starts.
//!
//! A live input runs on its own clock, so the queue is kept short: when
//! more than [`MAX_QUEUE`] frames are waiting, new input is dropped until
//! it drains, which bounds the delay instead of letting it grow.

use crate::backend::CameraError;
use crate::label::{self, DeviceInfo};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SizedSample};
use kairo_audio::mixer::{RATE, SourceInput};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// At most this much camera sound waits in the mixer: 60 ms.
pub const MAX_QUEUE: usize = RATE as usize * 6 / 100;
/// The callback's buffer, in output frames.
const SCRATCH: usize = 16_384;

/// Audio inputs as `enumerateDevices` listed them.
pub fn input_devices() -> Vec<DeviceInfo> {
    let host = cpal::default_host();
    let Ok(devices) = host.input_devices() else {
        return Vec::new();
    };
    devices
        .filter_map(|device| {
            let id = device.id().ok()?.to_string();
            let name = device.description().ok()?.name().to_owned();
            (!name.is_empty()).then(|| DeviceInfo {
                id,
                label: name.clone(),
                name,
            })
        })
        .collect()
}

/// An input being captured. Dropping it stops the capture.
pub struct Mic {
    _stream: SendStream,
    failed: Arc<AtomicBool>,
    pub name: String,
}

struct SendStream(#[allow(dead_code)] cpal::Stream);
// SAFETY: the stream is only created and dropped, never used across
// threads; Core Audio streams may be dropped from any thread (as the
// output side already does in `kairo-audio`).
unsafe impl Send for SendStream {}

impl Mic {
    /// The device stopped (unplugged, taken).
    pub fn failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }
}

/// Opens the input `saved` names (a `"Default - "` label is the system
/// default input) and feeds `input`.
pub fn open(saved: &str, input: SourceInput) -> Result<Mic, CameraError> {
    let host = cpal::default_host();
    let device = if label::is_default(saved) {
        host.default_input_device()
    } else {
        let devices = input_devices();
        let found =
            label::find(&devices, saved).ok_or_else(|| CameraError::NotFound(saved.to_owned()))?;
        let parsed: cpal::DeviceId = found
            .id
            .parse()
            .map_err(|_| CameraError::Failed(format!("bad input id {}", found.id)))?;
        host.device_by_id(&parsed)
    }
    .ok_or_else(|| CameraError::NotFound(saved.to_owned()))?;
    let name = device
        .description()
        .map(|d| d.name().to_owned())
        .unwrap_or_default();
    let default = device.default_input_config().map_err(map_error)?;
    let config = device
        .supported_input_configs()
        .ok()
        .and_then(|mut configs| {
            configs.find(|c| {
                c.sample_format() == default.sample_format()
                    && c.channels() == default.channels()
                    && (c.min_sample_rate()..=c.max_sample_rate()).contains(&RATE)
            })
        })
        .map(|c| c.with_sample_rate(RATE))
        .unwrap_or(default);
    let format = config.sample_format();
    let stream_config: cpal::StreamConfig = config.into();
    let failed = Arc::new(AtomicBool::new(false));
    let feed = Feeder::new(
        input,
        stream_config.channels as usize,
        stream_config.sample_rate,
    );
    let stream = match format {
        cpal::SampleFormat::F32 => build::<f32>(&device, stream_config, feed, failed.clone()),
        cpal::SampleFormat::I16 => build::<i16>(&device, stream_config, feed, failed.clone()),
        cpal::SampleFormat::I32 => build::<i32>(&device, stream_config, feed, failed.clone()),
        cpal::SampleFormat::U16 => build::<u16>(&device, stream_config, feed, failed.clone()),
        cpal::SampleFormat::F64 => build::<f64>(&device, stream_config, feed, failed.clone()),
        other => Err(CameraError::Failed(format!(
            "unsupported input format {other:?}"
        ))),
    }?;
    stream
        .play()
        .map_err(|e| CameraError::Failed(e.to_string()))?;
    tracing::info!(input = %name, "camera sound live");
    Ok(Mic {
        _stream: SendStream(stream),
        failed,
        name,
    })
}

fn map_error(error: cpal::Error) -> CameraError {
    match error.kind() {
        cpal::ErrorKind::DeviceBusy => CameraError::Busy,
        cpal::ErrorKind::DeviceNotAvailable => CameraError::Unplugged,
        cpal::ErrorKind::PermissionDenied => CameraError::Denied,
        _ => CameraError::Failed(error.to_string()),
    }
}

fn build<T: SizedSample>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    mut feed: Feeder,
    failed: Arc<AtomicBool>,
) -> Result<cpal::Stream, CameraError>
where
    f32: FromSample<T>,
{
    device
        .build_input_stream::<T, _, _>(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| feed.take(data),
            move |error: cpal::Error| {
                if !matches!(error.kind(), cpal::ErrorKind::DeviceChanged) {
                    failed.store(true, Ordering::Release);
                }
            },
            None,
        )
        .map_err(map_error)
}

/// Device samples to 48 kHz stereo, by linear interpolation.
pub struct Feeder {
    input: SourceInput,
    channels: usize,
    /// Input frames per output frame.
    step: f64,
    /// Position of the next output frame, in input frames after `last`.
    pos: f64,
    last: [f32; 2],
    scratch: Vec<f32>,
    dropping: bool,
}

impl Feeder {
    pub fn new(input: SourceInput, channels: usize, rate: u32) -> Self {
        Self {
            input,
            channels: channels.max(1),
            step: f64::from(rate.max(1)) / f64::from(RATE),
            pos: 0.0,
            last: [0.0; 2],
            scratch: vec![0.0; SCRATCH * 2],
            dropping: false,
        }
    }

    fn frame<T: Sample>(&self, data: &[T], i: usize) -> [f32; 2]
    where
        f32: FromSample<T>,
    {
        let at = i * self.channels;
        let l = f32::from_sample(data[at]);
        // Mono on both sides, else the first two channels (Chromium's
        // stereo down-mix of a multichannel input takes these too).
        let r = if self.channels > 1 {
            f32::from_sample(data[at + 1])
        } else {
            l
        };
        [l, r]
    }

    /// One callback's samples.
    pub fn take<T: Sample>(&mut self, data: &[T])
    where
        f32: FromSample<T>,
    {
        let frames = data.len() / self.channels;
        if frames == 0 {
            return;
        }
        let mut n = 0;
        // Output frames between `last` (input index -1) and input frame k.
        while n < SCRATCH {
            let k = self.pos.floor();
            if k >= frames as f64 {
                break;
            }
            let t = (self.pos - k) as f32;
            let k = k as usize;
            let a = if k == 0 {
                self.last
            } else {
                self.frame(data, k - 1)
            };
            let b = self.frame(data, k);
            self.scratch[n * 2] = a[0] + (b[0] - a[0]) * t;
            self.scratch[n * 2 + 1] = a[1] + (b[1] - a[1]) * t;
            n += 1;
            self.pos += self.step;
        }
        self.pos -= frames as f64;
        if self.pos < 0.0 {
            self.pos = 0.0;
        }
        self.last = self.frame(data, frames - 1);
        // Bound the delay: past the limit, drop until half has drained.
        let queued = self.input.queued();
        if queued > MAX_QUEUE {
            self.dropping = true;
        } else if queued < MAX_QUEUE / 2 {
            self.dropping = false;
        }
        if !self.dropping {
            self.input.push(&self.scratch[..n * 2]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairo_audio::mixer::{SourceKind, mixer};

    #[test]
    fn mono_44k1_becomes_48k_stereo_with_the_same_tone_and_length() {
        let (mut mix, mut control, _) = mixer(0);
        let (_, input) = control.add(SourceKind::Camera).unwrap();
        input.control.set_gain(1.0);
        let mut feed = Feeder::new(input, 1, 44_100);
        // One second of 441 Hz in 441-frame callbacks.
        let tone: Vec<f32> = (0..44_100)
            .map(|i| (i as f32 * 441.0 * std::f32::consts::TAU / 44_100.0).sin() * 0.5)
            .collect();
        let mut out = vec![0.0f32; 2 * 512];
        let mut heard = Vec::new();
        for chunk in tone.chunks(441) {
            feed.take(chunk);
            // The mixer drains as a device would, so the queue limit holds.
            mix.render(&mut out);
            heard.extend(out.chunks(2).map(|f| (f[0], f[1])));
        }
        assert!(heard.iter().all(|(l, r)| (l - r).abs() < 1e-6));
        // 100 callbacks of ~480 frames: 48,000 give or take one.
        let produced: usize = heard.iter().filter(|(l, _)| l.abs() > 0.0).count();
        assert!(produced > 40_000, "{produced}");
        // The tone survives: zero crossings near 2 × 441 a second.
        let crossings = heard
            .windows(2)
            .filter(|w| (w[0].0 < 0.0) != (w[1].0 < 0.0) && w[0].0 != 0.0 && w[1].0 != 0.0)
            .count();
        assert!((800..=960).contains(&crossings), "{crossings}");
    }

    #[test]
    fn a_runaway_input_is_bounded() {
        let (_mix, mut control, _) = mixer(0);
        let (_, input) = control.add(SourceKind::Camera).unwrap();
        let control = input.control.clone();
        let _ = control;
        let mut feed = Feeder::new(input, 2, 48_000);
        let block = vec![0.25f32; 2 * 480];
        for _ in 0..200 {
            feed.take(&block);
        }
        // Nothing drains, so at most the limit plus one block waits.
        assert!(
            feed.input.queued() <= MAX_QUEUE + 480,
            "{}",
            feed.input.queued()
        );
    }

    #[test]
    fn the_callback_does_not_allocate() {
        let (_mix, mut control, _) = mixer(0);
        let (_, input) = control.add(SourceKind::Camera).unwrap();
        let mut feed = Feeder::new(input, 2, 44_100);
        let cap = feed.scratch.capacity();
        let block = vec![0i16; 2 * 1024];
        for _ in 0..10 {
            feed.take(&block);
        }
        assert_eq!(feed.scratch.capacity(), cap);
    }
}
