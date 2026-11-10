//! Microphone capture. From `src/renderer/src/audio/capture.ts` and the PCM
//! worklet, moved out of the renderer.
//!
//! The cpal callback runs on the OS realtime thread. It only downmixes to
//! mono and pushes into a lock-free SPSC ring: no allocation, no locks. A
//! capture thread drains the ring every 10 ms, resamples to 16 kHz, cuts
//! 100 ms PCM16 chunks and meters at 10 Hz. Nothing here runs on the UI
//! thread, so a frozen window does not interrupt capture.

use crate::pcm::{Meter, PcmChunker};
use crate::resample::MonoResampler;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use kairo_domain::status::{AudioErrorCode, AudioLevel};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

const DRAIN_INTERVAL: Duration = Duration::from_millis(10);
/// How often a default-device capture checks whether the default moved, on
/// hosts where cpal does not follow it.
const DEFAULT_POLL: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, PartialEq)]
pub struct CaptureError {
    pub code: AudioErrorCode,
    pub message: String,
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CaptureError {}

fn capture_error(code: AudioErrorCode, message: impl Into<String>) -> CaptureError {
    CaptureError {
        code,
        message: message.into(),
    }
}

impl From<cpal::Error> for CaptureError {
    fn from(error: cpal::Error) -> Self {
        let code = match error.kind() {
            cpal::ErrorKind::PermissionDenied => AudioErrorCode::PermissionDenied,
            cpal::ErrorKind::DeviceNotAvailable => AudioErrorCode::DeviceNotFound,
            _ => AudioErrorCode::CaptureFailed,
        };
        capture_error(code, error.to_string())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum CaptureEvent {
    /// 100 ms of 16 kHz mono PCM16, little-endian.
    Pcm(Vec<u8>),
    Level(AudioLevel),
    /// The saved device was unavailable; capture uses the system default.
    FellBackToDefault {
        requested: String,
    },
    /// Capture stopped. `Audio input disconnected` on unplug.
    Error(CaptureError),
}

/// The pipeline the capture thread runs on drained samples: device rate to
/// 16 kHz, then chunking and metering. Public so it can be exercised
/// without hardware.
pub struct Pipeline {
    resampler: MonoResampler,
    chunker: PcmChunker,
}

impl Pipeline {
    pub fn new(input_rate: u32) -> Result<Self, CaptureError> {
        Ok(Self {
            resampler: MonoResampler::new(input_rate)
                .map_err(|e| capture_error(AudioErrorCode::CaptureFailed, e))?,
            chunker: PcmChunker::new(),
        })
    }

    pub fn process(
        &mut self,
        mono: &[f32],
        on_chunk: &mut dyn FnMut(Vec<u8>),
        on_meter: &mut dyn FnMut(Meter),
    ) -> Result<(), CaptureError> {
        let chunker = &mut self.chunker;
        self.resampler
            .process(mono, |samples| {
                chunker.process(samples, &mut *on_chunk, &mut *on_meter);
            })
            .map_err(|e| capture_error(AudioErrorCode::CaptureFailed, e))
    }
}

pub fn level_from(meter: Meter) -> AudioLevel {
    AudioLevel {
        rms: meter.rms,
        peak: meter.peak,
        clipping: meter.peak > 0.99,
        timestamp: now_ms(),
    }
}

use kairo_domain::js::now_ms;

/// A running capture. Dropping it stops capture.
pub struct CaptureHandle {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    overruns: Arc<AtomicU64>,
}

impl CaptureHandle {
    /// Samples the realtime callback dropped because the ring was full.
    pub fn overruns(&self) -> u64 {
        self.overruns.load(Ordering::Relaxed)
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for CaptureHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Start capturing from `device_id` (`""` or `"default"` for the system
/// default). Returns once the stream is playing; `sink` then receives every
/// event on the capture thread. A saved device that is gone falls back to
/// the default, as Electron does.
pub fn start_capture(
    device_id: &str,
    sink: impl FnMut(CaptureEvent) + Send + 'static,
) -> Result<CaptureHandle, CaptureError> {
    let stop = Arc::new(AtomicBool::new(false));
    let overruns = Arc::new(AtomicU64::new(0));
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
    let requested = device_id.to_owned();
    let thread = {
        let stop = stop.clone();
        let overruns = overruns.clone();
        std::thread::Builder::new()
            .name("kairo-audio-capture".into())
            .spawn(move || run(&requested, sink, &stop, &overruns, ready_tx))
            .map_err(|e| capture_error(AudioErrorCode::CaptureFailed, e.to_string()))?
    };
    match ready_rx.recv() {
        Ok(Ok(())) => Ok(CaptureHandle {
            stop,
            thread: Some(thread),
            overruns,
        }),
        Ok(Err(error)) => {
            let _ = thread.join();
            Err(error)
        }
        Err(_) => {
            let _ = thread.join();
            Err(capture_error(
                AudioErrorCode::CaptureFailed,
                "Audio capture thread stopped",
            ))
        }
    }
}

struct Open {
    stream: cpal::Stream,
    consumer: rtrb::Consumer<f32>,
    errors: std::sync::mpsc::Receiver<cpal::ErrorKind>,
    rate: u32,
    device_id: Option<String>,
}

fn is_default(device_id: &str) -> bool {
    device_id.is_empty() || device_id == crate::devices::DEFAULT_DEVICE_ID
}

fn find_device(host: &cpal::Host, device_id: &str) -> Option<cpal::Device> {
    let id: cpal::DeviceId = device_id.parse().ok()?;
    host.device_by_id(&id)
}

fn open(
    device_id: &str,
    overruns: &Arc<AtomicU64>,
    sink: &mut dyn FnMut(CaptureEvent),
) -> Result<Open, CaptureError> {
    let host = cpal::default_host();
    let device = if is_default(device_id) {
        None
    } else {
        let found = find_device(&host, device_id);
        if found.is_none() {
            // Unplugged, or a saved id from another enumerator: retry on the
            // default input rather than failing the whole session.
            tracing::warn!(
                device_id,
                "audio device unavailable; using the system default"
            );
            sink(CaptureEvent::FellBackToDefault {
                requested: device_id.to_owned(),
            });
        }
        found
    };
    let device = match device {
        Some(device) => device,
        None => host.default_input_device().ok_or_else(|| {
            capture_error(AudioErrorCode::DeviceNotFound, "No audio input device")
        })?,
    };
    let config = device.default_input_config()?;
    let rate = config.sample_rate();
    let channels = usize::from(config.channels()).max(1);
    // One second of headroom between the callback and the drain.
    let (producer, consumer) = rtrb::RingBuffer::new(rate as usize);
    let (error_tx, errors) = std::sync::mpsc::channel();
    let stream_config: cpal::StreamConfig = config.config();
    let stream = match config.sample_format() {
        SampleFormat::F32 => build::<f32>(
            &device,
            stream_config,
            channels,
            producer,
            overruns,
            error_tx,
        ),
        SampleFormat::I16 => build::<i16>(
            &device,
            stream_config,
            channels,
            producer,
            overruns,
            error_tx,
        ),
        SampleFormat::I32 => build::<i32>(
            &device,
            stream_config,
            channels,
            producer,
            overruns,
            error_tx,
        ),
        SampleFormat::U16 => build::<u16>(
            &device,
            stream_config,
            channels,
            producer,
            overruns,
            error_tx,
        ),
        SampleFormat::F64 => build::<f64>(
            &device,
            stream_config,
            channels,
            producer,
            overruns,
            error_tx,
        ),
        SampleFormat::I8 => build::<i8>(
            &device,
            stream_config,
            channels,
            producer,
            overruns,
            error_tx,
        ),
        SampleFormat::U8 => build::<u8>(
            &device,
            stream_config,
            channels,
            producer,
            overruns,
            error_tx,
        ),
        other => Err(capture_error(
            AudioErrorCode::CaptureFailed,
            format!("Unsupported audio sample format {other}"),
        )),
    }?;
    stream.play()?;
    Ok(Open {
        stream,
        consumer,
        errors,
        rate,
        device_id: device.id().ok().map(|id| id.to_string()),
    })
}

fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channels: usize,
    mut producer: rtrb::Producer<f32>,
    overruns: &Arc<AtomicU64>,
    errors: std::sync::mpsc::Sender<cpal::ErrorKind>,
) -> Result<cpal::Stream, CaptureError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let overruns = overruns.clone();
    let scale = 1.0 / channels as f32;
    let stream = device.build_input_stream::<T, _, _>(
        config,
        // Realtime: no allocation, no locks, no logging.
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            for frame in data.chunks_exact(channels) {
                let mut sum = 0.0f32;
                for &sample in frame {
                    sum += f32::from_sample(sample);
                }
                if producer.push(sum * scale).is_err() {
                    overruns.fetch_add(1, Ordering::Relaxed);
                }
            }
        },
        move |error: cpal::Error| {
            let _ = errors.send(error.kind());
        },
        None,
    )?;
    Ok(stream)
}

fn run(
    requested: &str,
    mut sink: impl FnMut(CaptureEvent),
    stop: &AtomicBool,
    overruns: &Arc<AtomicU64>,
    ready: std::sync::mpsc::SyncSender<Result<(), CaptureError>>,
) {
    let mut current = match open(requested, overruns, &mut sink) {
        Ok(open) => open,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let mut pipeline = match Pipeline::new(current.rate) {
        Ok(p) => p,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let _ = ready.send(Ok(()));
    tracing::info!(rate = current.rate, device = ?current.device_id, "audio capture started");

    let follows_default = is_default(requested);
    let mut last_default_check = Instant::now();
    let mut scratch: Vec<f32> = Vec::with_capacity(current.rate as usize);
    while !stop.load(Ordering::Acquire) {
        std::thread::sleep(DRAIN_INTERVAL);
        let mut rebuild = false;
        while let Ok(kind) = current.errors.try_recv() {
            match kind {
                cpal::ErrorKind::DeviceNotAvailable => {
                    sink(CaptureEvent::Error(capture_error(
                        AudioErrorCode::DeviceNotFound,
                        "Audio input disconnected",
                    )));
                    return;
                }
                // The host rerouted a default-device stream itself.
                cpal::ErrorKind::DeviceChanged => tracing::info!("audio route changed"),
                cpal::ErrorKind::StreamInvalidated => rebuild = true,
                other => tracing::warn!(?other, "audio stream error"),
            }
        }
        if follows_default
            && !cfg!(target_os = "macos")
            && last_default_check.elapsed() >= DEFAULT_POLL
        {
            last_default_check = Instant::now();
            let now_default = cpal::default_host()
                .default_input_device()
                .and_then(|d| d.id().ok())
                .map(|id| id.to_string());
            if now_default != current.device_id {
                tracing::info!(?now_default, "default audio input changed");
                rebuild = true;
            }
        }
        if rebuild {
            drop(std::mem::take(&mut scratch));
            match open(requested, overruns, &mut sink)
                .and_then(|open| Pipeline::new(open.rate).map(|p| (open, p)))
            {
                Ok((open, p)) => {
                    current = open;
                    pipeline = p;
                    scratch = Vec::with_capacity(current.rate as usize);
                }
                Err(error) => {
                    sink(CaptureEvent::Error(error));
                    return;
                }
            }
        }

        scratch.clear();
        let available = current.consumer.slots();
        if available == 0 {
            continue;
        }
        if let Ok(chunk) = current.consumer.read_chunk(available) {
            let (a, b) = chunk.as_slices();
            scratch.extend_from_slice(a);
            scratch.extend_from_slice(b);
            chunk.commit_all();
        }
        let events = std::cell::RefCell::new(Vec::new());
        let result = pipeline.process(
            &scratch,
            &mut |pcm| events.borrow_mut().push(CaptureEvent::Pcm(pcm)),
            &mut |meter| {
                events
                    .borrow_mut()
                    .push(CaptureEvent::Level(level_from(meter)))
            },
        );
        for event in events.into_inner() {
            sink(event);
        }
        if let Err(error) = result {
            sink(CaptureEvent::Error(error));
            return;
        }
    }
    drop(current.stream);
    tracing::info!("audio capture stopped");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(chunks: &[Vec<u8>]) -> Vec<f64> {
        chunks
            .iter()
            .flat_map(|c| {
                c.chunks(2)
                    .map(|b| f64::from(i16::from_le_bytes([b[0], b[1]])) / 32768.0)
            })
            .collect()
    }

    /// Frequency from upward zero crossings over the steady middle.
    fn frequency(samples: &[f64], rate: f64) -> f64 {
        let crossings: Vec<usize> = samples
            .windows(2)
            .enumerate()
            .filter(|(_, w)| w[0] < 0.0 && w[1] >= 0.0)
            .map(|(i, _)| i)
            .collect();
        let (first, last) = (crossings[1], crossings[crossings.len() - 2]);
        (crossings.len() - 3) as f64 * rate / (last - first) as f64
    }

    #[test]
    fn a_sine_keeps_its_frequency_and_level_after_resampling() {
        for rate in [44_100u32, 48_000, 16_000] {
            let mut pipeline = Pipeline::new(rate).unwrap();
            let (mut chunks, mut meters) = (Vec::new(), Vec::new());
            let tone = 440.0;
            let amplitude = 0.5;
            // Two seconds in 480-frame callbacks.
            let total = rate as usize * 2;
            let mut n = 0;
            while n < total {
                let block: Vec<f32> = (n..(n + 480).min(total))
                    .map(|i| {
                        (amplitude
                            * (2.0 * std::f64::consts::PI * tone * i as f64 / f64::from(rate))
                                .sin()) as f32
                    })
                    .collect();
                n += block.len();
                pipeline
                    .process(&block, &mut |c| chunks.push(c), &mut |m| meters.push(m))
                    .unwrap();
            }
            assert!(chunks.iter().all(|c| c.len() == 3200));
            let samples = decode(&chunks);
            assert!(samples.len() >= 31_000, "{rate}: {}", samples.len());
            let steady = &samples[1600..samples.len() - 1600];
            let f = frequency(steady, 16_000.0);
            assert!((f - tone).abs() < 1.0, "{rate}: {f} Hz");
            let rms = (steady.iter().map(|s| s * s).sum::<f64>() / steady.len() as f64).sqrt();
            let expected = amplitude / 2f64.sqrt();
            assert!((rms - expected).abs() < 0.01, "{rate}: rms {rms}");
            // Meters after the resampler's start-up show the same level.
            let late = &meters[5..];
            assert!(
                late.iter()
                    .all(|m| (m.rms - expected).abs() < 0.02 && (m.peak - amplitude).abs() < 0.03)
            );
            assert!(
                (18..=22).contains(&meters.len()),
                "{rate}: {} meters",
                meters.len()
            );
        }
    }

    #[test]
    fn clipping_is_flagged_above_099() {
        assert!(
            level_from(Meter {
                rms: 0.7,
                peak: 1.0
            })
            .clipping
        );
        assert!(
            !level_from(Meter {
                rms: 0.7,
                peak: 0.99
            })
            .clipping
        );
    }
}
