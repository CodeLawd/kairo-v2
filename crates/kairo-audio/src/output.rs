//! Where the speaker mix goes, and the clock that runs the mixer.
//!
//! The device is chosen as `sinkIdFor` chose it: the output whose label is
//! exactly `outputLabel`, else the system default (`setSinkId('')`). The
//! choice is made again on every device change (`devicechange` in
//! `TracksPlayer`), when the stream fails (unplugged), and when the label
//! changes. With no device wanted (this process makes no sound) or none
//! present, the mixer runs on the wall clock, so NDI taps keep their pace.
//!
//! The audio thread renders the mixer into a ring that the device callback
//! drains. The callback only copies (and, at a rate other than 48 kHz,
//! interpolates): it never locks or allocates. A callback that finds the
//! ring short plays silence and counts the frames as underruns: a glitch.

use crate::mixer::{MAX_CHUNK, Mixer, RATE};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How often devices are listed again for hot-plug.
pub const DEVICE_POLL: Duration = Duration::from_secs(1);
/// Frames kept queued for the device: about 85 ms, enough to ride out a
/// busy machine (a 43 ms cushion underran during builds in the soak).
const TARGET_FILL: usize = 4096;
const RING_FRAMES: usize = 8192;
const CHUNK: usize = 512;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputDevice {
    pub id: String,
    pub label: String,
    pub is_default: bool,
}

/// `sinkIdFor(label)`: the output with exactly this label. An empty label,
/// or none that matches, is the system default.
pub fn match_label<'a>(devices: &'a [OutputDevice], label: &str) -> Option<&'a OutputDevice> {
    if label.is_empty() {
        return None;
    }
    devices.iter().find(|d| d.label == label)
}

/// The device to open for `label`: its match, else the default.
pub fn choose<'a>(devices: &'a [OutputDevice], label: &str) -> Option<&'a OutputDevice> {
    match_label(devices, label).or_else(|| devices.iter().find(|d| d.is_default))
}

/// What a device stream drains: 48 kHz interleaved stereo.
pub struct DeviceFeed {
    consumer: rtrb::Consumer<f32>,
    underruns: Arc<AtomicU64>,
    primed: Arc<AtomicBool>,
    /// Interpolation between the two frames around the read position.
    step: f64,
    pos: f64,
    prev: [f32; 2],
    next: [f32; 2],
}

impl DeviceFeed {
    fn pop(&mut self) -> Option<[f32; 2]> {
        let l = self.consumer.pop().ok()?;
        let r = self.consumer.pop().unwrap_or(l);
        Some([l, r])
    }

    /// One output frame at the device rate; silence (and an underrun once
    /// primed) when the ring is short.
    fn frame(&mut self) -> [f32; 2] {
        if self.step == 1.0 {
            return match self.pop() {
                Some(f) => f,
                None => {
                    self.missed();
                    [0.0, 0.0]
                }
            };
        }
        while self.pos >= 1.0 {
            self.pos -= 1.0;
            self.prev = self.next;
            match self.pop() {
                Some(f) => self.next = f,
                None => {
                    self.missed();
                    self.next = [0.0, 0.0];
                }
            }
        }
        let t = self.pos as f32;
        self.pos += self.step;
        [
            self.prev[0] + (self.next[0] - self.prev[0]) * t,
            self.prev[1] + (self.next[1] - self.prev[1]) * t,
        ]
    }

    fn missed(&self) {
        if self.primed.load(Ordering::Relaxed) {
            self.underruns.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Fills a device buffer of `channels` interleaved channels. Realtime:
    /// no locks, no allocation.
    pub fn fill<T: SizedSample + FromSample<f32>>(&mut self, data: &mut [T], channels: usize) {
        for frame in data.chunks_exact_mut(channels.max(1)) {
            let [l, r] = self.frame();
            match frame.len() {
                1 => frame[0] = T::from_sample((l + r) * 0.5),
                _ => {
                    frame[0] = T::from_sample(l);
                    frame[1] = T::from_sample(r);
                    for extra in &mut frame[2..] {
                        *extra = T::from_sample(0.0);
                    }
                }
            }
        }
    }
}

/// An open stream; dropping it closes the device.
pub trait DeviceStream: Send {}

/// What lists devices and opens them: the system, or a fake in tests.
pub trait Host: Send + 'static {
    fn devices(&self) -> Vec<OutputDevice>;
    /// Opens `id`, draining `feed`; sets `failed` when the stream dies.
    fn open(
        &self,
        id: &str,
        feed: DeviceFeedMaker,
        failed: Arc<AtomicBool>,
    ) -> Result<Box<dyn DeviceStream>, String>;
}

/// Makes the feed once the device's rate is known.
pub struct DeviceFeedMaker {
    consumer: rtrb::Consumer<f32>,
    underruns: Arc<AtomicU64>,
    primed: Arc<AtomicBool>,
}

impl DeviceFeedMaker {
    pub fn make(self, device_rate: u32) -> DeviceFeed {
        DeviceFeed {
            consumer: self.consumer,
            underruns: self.underruns,
            primed: self.primed,
            step: f64::from(RATE) / f64::from(device_rate.max(1)),
            pos: 1.0,
            prev: [0.0; 2],
            next: [0.0; 2],
        }
    }
}

#[cfg(test)]
pub(crate) struct DeviceFeedMakerForTests;

#[cfg(test)]
impl DeviceFeedMakerForTests {
    pub fn maker(consumer: rtrb::Consumer<f32>) -> DeviceFeedMaker {
        DeviceFeedMaker {
            consumer,
            underruns: Arc::new(AtomicU64::new(0)),
            primed: Arc::new(AtomicBool::new(true)),
        }
    }
}

/// The system's outputs through cpal.
pub struct SystemHost;

struct CpalStream(#[allow(dead_code)] cpal::Stream);
// SAFETY: the stream is only held (to keep it open) and dropped; it is
// never used from two threads at once.
unsafe impl Send for CpalStream {}
impl DeviceStream for CpalStream {}

/// The outputs the system has, the default marked.
pub fn list_output_devices() -> Vec<OutputDevice> {
    SystemHost.devices()
}

impl Host for SystemHost {
    fn devices(&self) -> Vec<OutputDevice> {
        let host = cpal::default_host();
        let default = host
            .default_output_device()
            .and_then(|d| d.id().ok())
            .map(|id| id.to_string());
        let Ok(outputs) = host.output_devices() else {
            return Vec::new();
        };
        outputs
            .enumerate()
            .filter_map(|(i, device)| {
                let id = device.id().ok()?.to_string();
                let label = device
                    .description()
                    .map(|d| d.name().to_owned())
                    .ok()
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| format!("Speaker {}", i + 1));
                Some(OutputDevice {
                    is_default: Some(&id) == default.as_ref(),
                    id,
                    label,
                })
            })
            .collect()
    }

    fn open(
        &self,
        id: &str,
        feed: DeviceFeedMaker,
        failed: Arc<AtomicBool>,
    ) -> Result<Box<dyn DeviceStream>, String> {
        let host = cpal::default_host();
        let parsed: cpal::DeviceId = id.parse().map_err(|_| format!("bad device id {id}"))?;
        let device = host
            .device_by_id(&parsed)
            .ok_or("the output device is gone")?;
        let default = device.default_output_config().map_err(|e| e.to_string())?;
        // 48 kHz when the device can, to skip the interpolation.
        let config = device
            .supported_output_configs()
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
        let channels = stream_config.channels as usize;
        let feed = feed.make(stream_config.sample_rate);
        let stream = match format {
            cpal::SampleFormat::F32 => build::<f32>(&device, stream_config, channels, feed, failed),
            cpal::SampleFormat::I16 => build::<i16>(&device, stream_config, channels, feed, failed),
            cpal::SampleFormat::I32 => build::<i32>(&device, stream_config, channels, feed, failed),
            cpal::SampleFormat::U16 => build::<u16>(&device, stream_config, channels, feed, failed),
            cpal::SampleFormat::F64 => build::<f64>(&device, stream_config, channels, feed, failed),
            other => Err(format!("unsupported output format {other:?}")),
        }?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Box::new(CpalStream(stream)))
    }
}

fn build<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channels: usize,
    mut feed: DeviceFeed,
    failed: Arc<AtomicBool>,
) -> Result<cpal::Stream, String> {
    device
        .build_output_stream::<T, _, _>(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| feed.fill(data, channels),
            move |error: cpal::Error| {
                // Unplugged, or the route changed under us: reopen.
                let _ = error;
                failed.store(true, Ordering::Release);
            },
            None,
        )
        .map_err(|e| e.to_string())
}

/// What the audio thread reports.
#[derive(Debug, Default)]
pub struct OutputStats {
    /// Device frames that had to be silence: glitches.
    pub underruns: AtomicU64,
    /// Times a device was (re)opened.
    pub opens: AtomicU64,
    /// Frames the mixer rendered.
    pub rendered: AtomicU64,
    /// Frames rendered but not yet played: what is queued for the device.
    pub queued: AtomicU64,
}

struct Wanted {
    /// Make sound at all (this process holds the audio surface, or plays tracks).
    speaker: bool,
    label: String,
    changed: bool,
}

struct Shared {
    wanted: Mutex<Wanted>,
    stop: AtomicBool,
    /// Label of the device playing now, if any.
    current: Mutex<Option<String>>,
    stats: Arc<OutputStats>,
}

/// The audio thread: owns the mixer, picks and reopens the device.
pub struct AudioOut {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl AudioOut {
    pub fn start(mixer: Mixer, host: impl Host) -> Self {
        let stats = Arc::new(OutputStats::default());
        let shared = Arc::new(Shared {
            wanted: Mutex::new(Wanted {
                speaker: false,
                label: String::new(),
                changed: true,
            }),
            stop: AtomicBool::new(false),
            current: Mutex::new(None),
            stats: stats.clone(),
        });
        let inner = shared.clone();
        let thread = std::thread::Builder::new()
            .name("audio".into())
            .spawn(move || run(mixer, host, inner))
            .map_err(|e| tracing::error!("cannot start the audio thread: {e}"))
            .ok();
        Self { shared, thread }
    }

    /// Whether to make sound, and on which device (`outputLabel`).
    pub fn set_output(&self, speaker: bool, label: &str) {
        let mut wanted = self.shared.wanted.lock().unwrap_or_else(|e| e.into_inner());
        if wanted.speaker != speaker || wanted.label != label {
            wanted.speaker = speaker;
            wanted.label = label.to_owned();
            wanted.changed = true;
        }
    }

    /// The label of the device playing, `None` when silent.
    pub fn device(&self) -> Option<String> {
        self.shared
            .current
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn stats(&self) -> &OutputStats {
        &self.shared.stats
    }

    /// Seconds from the mixer to the speaker: what is queued for the
    /// device. Sound taken from a source now is heard this much later.
    pub fn latency(&self) -> f64 {
        self.shared.stats.queued.load(Ordering::Relaxed) as f64 / f64::from(RATE)
    }
}

impl Drop for AudioOut {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Open {
    id: String,
    _stream: Box<dyn DeviceStream>,
    producer: rtrb::Producer<f32>,
    failed: Arc<AtomicBool>,
    primed: Arc<AtomicBool>,
}

/// The audio thread runs ahead of the device: ask the system to schedule
/// it promptly.
fn raise_priority() {
    #[cfg(target_os = "macos")]
    // SAFETY: sets this thread's own QoS class; no pointers involved.
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0);
    }
}

fn run(mut mixer: Mixer, host: impl Host, shared: Arc<Shared>) {
    raise_priority();
    let mut buffer = vec![0.0f32; MAX_CHUNK * 2];
    let mut open: Option<Open> = None;
    let mut last_poll: Option<Instant> = None;
    // Wall-clock pacing when there is no device.
    let mut clock_start = Instant::now();
    let mut clock_frames: u64 = 0;
    while !shared.stop.load(Ordering::Acquire) {
        let (speaker, label, changed) = {
            let mut wanted = shared.wanted.lock().unwrap_or_else(|e| e.into_inner());
            let changed = std::mem::take(&mut wanted.changed);
            (wanted.speaker, wanted.label.clone(), changed)
        };
        let failed = open
            .as_ref()
            .is_some_and(|o| o.failed.load(Ordering::Acquire));
        let due = last_poll.is_none_or(|t| t.elapsed() >= DEVICE_POLL);
        if changed || failed || due {
            last_poll = Some(Instant::now());
            let devices = if speaker { host.devices() } else { Vec::new() };
            let choice = speaker.then(|| choose(&devices, &label).cloned()).flatten();
            let keep = !failed && open.as_ref().map(|o| &o.id) == choice.as_ref().map(|c| &c.id);
            if !keep {
                if open.take().is_some() {
                    tracing::info!("audio output closed");
                }
                *shared.current.lock().unwrap_or_else(|e| e.into_inner()) = None;
                if let Some(device) = choice {
                    let (producer, consumer) = rtrb::RingBuffer::new(RING_FRAMES * 2);
                    let failed = Arc::new(AtomicBool::new(false));
                    let primed = Arc::new(AtomicBool::new(false));
                    let maker = DeviceFeedMaker {
                        consumer,
                        underruns: Arc::new(AtomicU64::new(0)),
                        primed: primed.clone(),
                    };
                    let underruns = maker.underruns.clone();
                    match host.open(&device.id, maker, failed.clone()) {
                        Ok(stream) => {
                            tracing::info!(device = device.label, "audio output opened");
                            shared.stats.opens.fetch_add(1, Ordering::Relaxed);
                            *shared.current.lock().unwrap_or_else(|e| e.into_inner()) =
                                Some(device.label.clone());
                            // Underruns of this stream add to the total.
                            spawn_counter(underruns, shared.stats.clone(), failed.clone());
                            open = Some(Open {
                                id: device.id,
                                _stream: stream,
                                producer,
                                failed,
                                primed,
                            });
                        }
                        Err(error) => {
                            tracing::warn!(device = device.label, %error, "audio output failed to open");
                        }
                    }
                }
                clock_start = Instant::now();
                clock_frames = 0;
            }
        }
        match open.as_mut() {
            Some(device) => {
                // Keep the device's ring full: its clock paces the mixer.
                loop {
                    let queued = RING_FRAMES - device.producer.slots() / 2;
                    shared.stats.queued.store(queued as u64, Ordering::Relaxed);
                    if queued >= TARGET_FILL {
                        device.primed.store(true, Ordering::Relaxed);
                        break;
                    }
                    let out = &mut buffer[..CHUNK * 2];
                    mixer.render(out);
                    shared
                        .stats
                        .rendered
                        .fetch_add(CHUNK as u64, Ordering::Relaxed);
                    if let Ok(mut chunk) = device.producer.write_chunk(CHUNK * 2) {
                        let (a, b) = chunk.as_mut_slices();
                        let (x, y) = out.split_at(a.len());
                        a.copy_from_slice(x);
                        b.copy_from_slice(y);
                        chunk.commit_all();
                    }
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            None => {
                shared.stats.queued.store(0, Ordering::Relaxed);
                let target = (clock_start.elapsed().as_secs_f64() * f64::from(RATE)) as u64;
                while clock_frames + CHUNK as u64 <= target {
                    mixer.render(&mut buffer[..CHUNK * 2]);
                    clock_frames += CHUNK as u64;
                    shared
                        .stats
                        .rendered
                        .fetch_add(CHUNK as u64, Ordering::Relaxed);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

/// Moves a stream's underrun count into the totals until the stream fails
/// or is replaced (its counter is then dropped).
fn spawn_counter(underruns: Arc<AtomicU64>, stats: Arc<OutputStats>, failed: Arc<AtomicBool>) {
    let _ = std::thread::Builder::new()
        .name("audio stats".into())
        .spawn(move || {
            let mut seen = 0;
            loop {
                std::thread::sleep(Duration::from_millis(200));
                let now = underruns.load(Ordering::Relaxed);
                stats.underruns.fetch_add(now - seen, Ordering::Relaxed);
                seen = now;
                // Ours alone once the stream is gone: stop.
                if Arc::strong_count(&underruns) == 1 || failed.load(Ordering::Acquire) {
                    return;
                }
            }
        });
}

#[cfg(test)]
pub(crate) mod fake {
    //! A host whose devices tests plug in and pull out.
    use super::*;

    /// An open fake stream: its device id, what it drained, its failure flag.
    pub type Stream = (String, Arc<Mutex<Vec<f32>>>, Arc<AtomicBool>);

    #[derive(Clone, Default)]
    pub struct FakeHost {
        pub devices: Arc<Mutex<Vec<OutputDevice>>>,
        /// Per open stream: the device id and what it drained so far.
        pub streams: Arc<Mutex<Vec<Stream>>>,
        pub rate: u32,
    }

    struct FakeStream {
        stop: Arc<AtomicBool>,
    }
    impl DeviceStream for FakeStream {}
    impl Drop for FakeStream {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
        }
    }

    impl FakeHost {
        pub fn plug(&self, id: &str, label: &str, is_default: bool) {
            let mut devices = self.devices.lock().unwrap();
            if is_default {
                devices.iter_mut().for_each(|d| d.is_default = false);
            }
            devices.push(OutputDevice {
                id: id.into(),
                label: label.into(),
                is_default,
            });
        }

        /// Pulls a device out: its stream fails, as a real one does.
        pub fn unplug(&self, id: &str) {
            self.devices.lock().unwrap().retain(|d| d.id != id);
            for (sid, _, failed) in self.streams.lock().unwrap().iter() {
                if sid == id {
                    failed.store(true, Ordering::Release);
                }
            }
        }
    }

    impl Host for FakeHost {
        fn devices(&self) -> Vec<OutputDevice> {
            self.devices.lock().unwrap().clone()
        }

        fn open(
            &self,
            id: &str,
            feed: DeviceFeedMaker,
            failed: Arc<AtomicBool>,
        ) -> Result<Box<dyn DeviceStream>, String> {
            if !self.devices.lock().unwrap().iter().any(|d| d.id == id) {
                return Err("gone".into());
            }
            let heard = Arc::new(Mutex::new(Vec::new()));
            self.streams
                .lock()
                .unwrap()
                .push((id.into(), heard.clone(), failed.clone()));
            let stop = Arc::new(AtomicBool::new(false));
            let rate = if self.rate == 0 { RATE } else { self.rate };
            let mut feed = feed.make(rate);
            let done = stop.clone();
            // A device clock: 10 ms buffers in real time.
            std::thread::spawn(move || {
                let frames = rate as usize / 100;
                let mut data = vec![0.0f32; frames * 2];
                let start = Instant::now();
                let mut n = 0u32;
                while !done.load(Ordering::Acquire) && !failed.load(Ordering::Acquire) {
                    feed.fill(&mut data, 2);
                    heard.lock().unwrap().extend_from_slice(&data);
                    n += 1;
                    let next = start + Duration::from_millis(10) * n;
                    std::thread::sleep(next.saturating_duration_since(Instant::now()));
                }
            });
            Ok(Box::new(FakeStream { stop }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeHost;
    use super::*;
    use crate::mixer::{SourceKind, mixer};

    fn devices() -> Vec<OutputDevice> {
        vec![
            OutputDevice {
                id: "a".into(),
                label: "MacBook Pro Speakers".into(),
                is_default: true,
            },
            OutputDevice {
                id: "b".into(),
                label: "USB Audio".into(),
                is_default: false,
            },
        ]
    }

    #[test]
    fn a_label_picks_its_device_and_anything_else_is_the_default() {
        let d = devices();
        assert_eq!(choose(&d, "USB Audio").unwrap().id, "b");
        assert_eq!(choose(&d, "").unwrap().id, "a");
        // Exact labels only, as `sinkIdFor` compares them.
        assert_eq!(choose(&d, "usb audio").unwrap().id, "a");
        assert_eq!(choose(&d, "Gone").unwrap().id, "a");
        assert!(choose(&[], "USB Audio").is_none());
    }

    #[test]
    fn interpolation_keeps_a_steady_level_at_another_rate() {
        let (mut producer, consumer) = rtrb::RingBuffer::new(4096);
        for _ in 0..1000 {
            producer.push(0.5).unwrap();
            producer.push(-0.5).unwrap();
        }
        let maker = DeviceFeedMaker {
            consumer,
            underruns: Arc::new(AtomicU64::new(0)),
            primed: Arc::new(AtomicBool::new(true)),
        };
        let mut feed = maker.make(44_100);
        let mut out = vec![0.0f32; 400 * 2];
        feed.fill(&mut out, 2);
        // After the first frame (interpolated from silence) the level holds.
        assert!(
            out[4..]
                .chunks(2)
                .all(|f| (f[0] - 0.5).abs() < 1e-6 && (f[1] + 0.5).abs() < 1e-6)
        );
        // Mono devices get the average; extra channels silence.
        let mut mono = vec![0.0f32; 4];
        feed.fill(&mut mono, 1);
        assert!(mono.iter().all(|v| v.abs() < 1e-6));
    }

    fn wait(what: &str, mut ok: impl FnMut() -> bool) {
        let end = Instant::now() + Duration::from_secs(5);
        while !ok() {
            assert!(Instant::now() < end, "timed out: {what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn devices_follow_the_label_and_hot_plug() {
        let host = FakeHost::default();
        host.plug("a", "Speakers", true);
        let (mixer, mut control, _) = mixer(0);
        let (_, source) = control.add(SourceKind::Video).unwrap();
        source.control.set_gain(1.0);
        let out = AudioOut::start(mixer, host.clone());
        // No sound wanted: no device.
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(out.device(), None);
        out.set_output(true, "USB Audio");
        // Not plugged in: the default.
        wait("default", || out.device().as_deref() == Some("Speakers"));
        host.plug("b", "USB Audio", false);
        wait("hot-plugged label", || {
            out.device().as_deref() == Some("USB Audio")
        });
        host.unplug("b");
        wait("back to the default", || {
            out.device().as_deref() == Some("Speakers")
        });
        out.set_output(false, "USB Audio");
        wait("silent", || out.device().is_none());
        assert!(out.stats().opens.load(Ordering::Relaxed) >= 3);
    }

    #[test]
    fn a_known_tone_reaches_the_device_unchanged_and_without_glitches() {
        let host = FakeHost::default();
        host.plug("a", "Speakers", true);
        let (mixer, mut control, _) = mixer(0);
        let (_, mut source) = control.add(SourceKind::Tracks).unwrap();
        source.control.set_gain(0.5);
        let out = AudioOut::start(mixer, host.clone());
        out.set_output(true, "");
        // Feed 1 s of a 1 kHz tone in real time.
        let tone: Vec<f32> = (0..RATE as usize)
            .flat_map(|i| {
                let v = (i as f32 * 1000.0 * std::f32::consts::TAU / RATE as f32).sin() * 0.8;
                [v, v]
            })
            .collect();
        let mut sent = 0;
        let start = Instant::now();
        while sent < tone.len() / 2 {
            sent += source.push(&tone[sent * 2..(sent + 4800).min(tone.len() / 2) * 2]);
            std::thread::sleep(Duration::from_millis(20));
        }
        source.control.set_playing(true);
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(200));
        let heard = host.streams.lock().unwrap()[0].1.lock().unwrap().clone();
        // Peak level: 0.8 × 0.5 = 0.4.
        let peak = heard.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((peak - 0.4).abs() < 0.01, "{peak}");
        let underruns = out.stats().underruns.load(Ordering::Relaxed);
        if std::env::var("KAIRO_PERF_HOSTED").as_deref() == Ok("1") {
            assert!(
                underruns <= u64::from(RATE / 100),
                "{underruns} dropped samples"
            );
        } else {
            assert_eq!(underruns, 0);
        }
    }
}
