//! Known sounds through the real decoder, a clip player and the mixer, on
//! a null sink clocked in real time.

use crate::clip::{ClipControl, ClipPlayer, ClipSettings};
use crate::mixer::{RATE, SourceKind, mixer};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub(crate) fn dir() -> &'static Path {
    static DIR: OnceLock<tempfile::TempDir> = OnceLock::new();
    DIR.get_or_init(|| tempfile::tempdir().expect("temp dir"))
        .path()
}

/// A sound made by `ffmpeg -f lavfi -i <graph>`.
pub(crate) fn fixture(name: &str, graph: &str, extra: &[&str]) -> PathBuf {
    let path = dir().join(name);
    if path.exists() {
        return path;
    }
    let status = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", graph])
        .args(extra)
        .arg(&path)
        .status()
        .expect("the ffmpeg command makes the fixtures");
    assert!(status.success(), "ffmpeg failed for {name}");
    path
}

/// A ramp whose value is a tenth of its own time: a sample says where it is.
pub(crate) fn ramp(name: &str, seconds: f64) -> PathBuf {
    fixture(
        name,
        &format!("aevalsrc=exprs=0.1*t|0.1*t:s=48000:d={seconds}"),
        &["-c:a", "pcm_f32le"],
    )
}

/// A null sink: renders the mixer in real time and keeps what it heard.
pub(crate) struct NullSink {
    pub heard: Arc<Mutex<Vec<f32>>>,
    stop: Arc<AtomicBool>,
}

impl NullSink {
    pub fn start(mut mixer: crate::mixer::Mixer) -> Self {
        let heard = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (h, s) = (heard.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut buf = vec![0.0; 480 * 2];
            let start = Instant::now();
            let mut n = 0u32;
            while !s.load(Ordering::Acquire) {
                mixer.render(&mut buf);
                h.lock().unwrap().extend_from_slice(&buf);
                n += 1;
                let next = start + Duration::from_millis(10) * n;
                std::thread::sleep(next.saturating_duration_since(Instant::now()));
            }
        });
        Self { heard, stop }
    }

    pub fn take(&self) -> Vec<f32> {
        std::mem::take(&mut *self.heard.lock().unwrap())
    }
}

impl Drop for NullSink {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

fn wait(what: &str, secs: u64, mut ok: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(secs);
    while !ok() {
        assert!(Instant::now() < end, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn player(
    path: PathBuf,
    settings: ClipSettings,
) -> (ClipPlayer, NullSink, Arc<crate::SourceControl>) {
    let (mixer, mut control, _) = mixer(0);
    let (_, input) = control.add(SourceKind::Tracks).unwrap();
    input.control.set_gain(1.0);
    let source = input.control.clone();
    let sink = NullSink::start(mixer);
    // Keep the control side alive with the sink.
    std::mem::forget(control);
    (
        ClipPlayer::start(path, settings, false, input),
        sink,
        source,
    )
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0, |m, v| m.max(v.abs()))
}

#[test]
fn a_mono_tone_plays_on_both_sides_at_its_level_for_its_length() {
    // 440 Hz at 44.1 kHz, `sine`'s default amplitude of 1/8.
    let path = fixture("tone.wav", "sine=f=440:d=1:sample_rate=44100", &[]);
    let (clip, sink, source) = player(path, ClipSettings::default());
    wait("ended", 5, || clip.status().ended);
    std::thread::sleep(Duration::from_millis(50));
    let heard = sink.take();
    let left: Vec<f32> = heard.iter().step_by(2).copied().collect();
    let right: Vec<f32> = heard.iter().skip(1).step_by(2).copied().collect();
    assert!((peak(&left) - 0.125).abs() < 0.01, "{}", peak(&left));
    assert_eq!(left, right);
    let sounding = left.iter().filter(|v| v.abs() > 1e-4).count();
    // About a second at 48 kHz (zero crossings excluded).
    assert!((46_000..=48_500).contains(&sounding), "{sounding}");
    let status = clip.status();
    assert!((status.duration - 1.0).abs() < 0.01);
    assert!(status.paused && status.ended);
    assert_eq!(source.underruns(), 0, "a clip playing out is not a glitch");
}

#[test]
fn a_seek_lands_where_it_was_asked() {
    let path = ramp("ramp.wav", 5.0);
    let (clip, sink, _) = player(path, ClipSettings::default());
    wait("ready", 5, || clip.status().ready);
    clip.control(ClipControl::Seek(2.0));
    std::thread::sleep(Duration::from_millis(100));
    sink.take();
    std::thread::sleep(Duration::from_millis(200));
    let heard = sink.take();
    // Between 2.0 and 2.4 s of media: values 0.20 to 0.24.
    let values: Vec<f32> = heard
        .iter()
        .step_by(2)
        .copied()
        .filter(|v| *v > 0.0)
        .collect();
    assert!(!values.is_empty());
    assert!(
        values.iter().all(|v| (0.199..0.26).contains(v)),
        "{:?}",
        &values[..4]
    );
    let position = clip.status().position;
    assert!((2.2..2.45).contains(&position), "{position}");
}

#[test]
fn a_looping_range_stays_inside_it() {
    let path = ramp("ramp-loop.wav", 5.0);
    let settings = ClipSettings {
        start: Some(1.0),
        end: Some(1.5),
        rate: 1.0,
        looped: true,
    };
    let (clip, sink, source) = player(path, settings);
    std::thread::sleep(Duration::from_millis(1600));
    let heard = sink.take();
    let first = heard.iter().step_by(2).position(|v| *v != 0.0).unwrap();
    // Past the first render, whose gain ramps in from silence.
    let values: Vec<f32> = heard
        .iter()
        .step_by(2)
        .skip(first + 480)
        .copied()
        .filter(|v| *v != 0.0)
        .collect();
    // Three laps of 0.1 to 0.15.
    assert!(values.len() > 48_000);
    assert!(
        values.iter().all(|v| (0.0999..0.1501).contains(v)),
        "{:?}",
        values
            .iter()
            .fold((1.0f32, 0.0f32), |(a, b), v| (a.min(*v), b.max(*v)))
    );
    let position = clip.status().position;
    assert!((1.0..1.5).contains(&position), "{position}");
    assert!(!clip.status().ended);
    assert_eq!(source.underruns(), 0, "the wrap leaves no gap");
}

#[test]
fn pause_holds_and_play_after_the_end_starts_again() {
    let path = ramp("ramp-short.wav", 0.5);
    let (clip, sink, _) = player(path, ClipSettings::default());
    std::thread::sleep(Duration::from_millis(200));
    clip.control(ClipControl::Pause);
    wait("paused", 2, || clip.status().paused);
    let held = clip.status().position;
    std::thread::sleep(Duration::from_millis(200));
    sink.take();
    std::thread::sleep(Duration::from_millis(100));
    assert!(sink.take().iter().all(|v| *v == 0.0), "paused is silent");
    assert!((clip.status().position - held).abs() < 1e-9);
    clip.control(ClipControl::Play);
    wait("ended", 3, || clip.status().ended);
    clip.control(ClipControl::Play);
    wait("restarted", 2, || !clip.status().ended);
    sink.take();
    std::thread::sleep(Duration::from_millis(60));
    let first = sink
        .take()
        .iter()
        .copied()
        .find(|v| *v > 0.0)
        .unwrap_or(1.0);
    assert!(first < 0.03, "from the start again: {first}");
}

#[test]
fn rate_two_plays_a_second_in_half_a_second() {
    let path = fixture("tone-rate.wav", "sine=f=440:d=1:sample_rate=48000", &[]);
    let settings = ClipSettings {
        rate: 2.0,
        ..ClipSettings::default()
    };
    let start = Instant::now();
    let (clip, _sink, _) = player(path, settings);
    wait("ended", 5, || clip.status().ended);
    let took = start.elapsed().as_secs_f64();
    assert!((0.45..0.8).contains(&took), "{took}");
    assert!((clip.status().position - 1.0).abs() < 0.02);
}

#[test]
fn five_one_folds_to_stereo_with_the_speaker_mix() {
    // FL 0.1, FR 0.2, FC 0.3, LFE 0.9 (dropped), SL 0.4, SR 0.5.
    let path = fixture(
        "surround.wav",
        "aevalsrc=exprs=0.1|0.2|0.3|0.9|0.4|0.5:s=48000:d=0.5:c=5.1",
        &["-c:a", "pcm_f32le"],
    );
    let (clip, sink, _) = player(path, ClipSettings::default());
    wait("ended", 5, || clip.status().ended);
    let heard = sink.take();
    // Past the first render, whose gain ramps in from silence.
    let first = heard.chunks(2).position(|f| f[0] != 0.0).unwrap();
    let frame = heard.chunks(2).nth(first + 2000).unwrap().to_vec();
    let h = std::f32::consts::FRAC_1_SQRT_2;
    assert!(
        (frame[0] - (0.1 + h * 0.3 + h * 0.4)).abs() < 1e-4,
        "{frame:?}"
    );
    assert!(
        (frame[1] - (0.2 + h * 0.3 + h * 0.5)).abs() < 1e-4,
        "{frame:?}"
    );
}

#[test]
fn a_file_without_sound_is_silent_and_a_missing_one_fails() {
    let video = fixture(
        "picture-only.mp4",
        "color=c=red:s=64x64:d=1",
        &["-c:v", "mpeg4"],
    );
    let (clip, _sink, _) = player(video, ClipSettings::default());
    wait("ready", 5, || clip.status().ready);
    assert!(clip.status().silent && clip.status().error.is_none());
    let (gone, _sink, _) = player(dir().join("nope.wav"), ClipSettings::default());
    wait("error", 5, || gone.status().error.is_some());
    let broken = dir().join("broken.wav");
    std::fs::write(&broken, b"RIFF....WAVEjunk").unwrap();
    let (bad, _sink, _) = player(broken, ClipSettings::default());
    wait("error", 5, || bad.status().error.is_some());
}

#[test]
fn aac_and_mp3_decode_too() {
    for (name, codec) in [("tone.m4a", "aac"), ("tone.mp3", "libmp3lame")] {
        let encoders = Command::new("ffmpeg")
            .args(["-hide_banner", "-encoders"])
            .output()
            .unwrap();
        if !String::from_utf8_lossy(&encoders.stdout).contains(codec) {
            eprintln!("skipped {name}: no {codec} encoder");
            continue;
        }
        let path = fixture(
            name,
            "sine=f=1000:d=0.5:sample_rate=44100",
            &["-c:a", codec],
        );
        let (clip, sink, _) = player(path, ClipSettings::default());
        wait("ended", 5, || clip.status().ended);
        let p = peak(&sink.take());
        // Lossy codecs ring a little above the source level.
        assert!((p - 0.125).abs() < 0.03, "{name}: {p}");
    }
    let _ = RATE;
}

/// Prompt 056's soak: zero glitches over an hour. The real default output
/// at gain 0 (silent, but the device pulls every buffer), four looping
/// clips, two NDI taps on and drained. `KAIRO_AUDIO_SOAK_SECS` sets the
/// length (default 3600).
#[test]
#[ignore = "soak: KAIRO_AUDIO_SOAK_SECS=3600 cargo test --release -p kairo-audio soak -- --ignored --nocapture"]
fn soak_has_no_glitches() {
    use crate::output::{AudioOut, SystemHost};
    use crate::tap::TapSender;
    let secs: u64 = std::env::var("KAIRO_AUDIO_SOAK_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3600);
    let (mixer, mut control, taps) = mixer(2);
    let blocks = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let counted = blocks.clone();
    let _sender = TapSender::start(taps, move |_, block| {
        assert!(block.validate().is_ok());
        counted.fetch_add(1, Ordering::Relaxed);
    });
    control.tap(0).unwrap().set_on(true);
    control.tap(1).unwrap().set_on(true);
    let mut clips = Vec::new();
    let mut sources = Vec::new();
    for i in 0..4 {
        let path = ramp(&format!("soak-{i}.wav"), 3.0 + i as f64);
        let (_, input) = control.add(SourceKind::Video).unwrap();
        input.control.set_gain(0.0);
        input.control.set_taps(0b11);
        sources.push(input.control.clone());
        let settings = ClipSettings {
            start: Some(0.5),
            end: Some(2.5),
            rate: 1.0,
            looped: true,
        };
        clips.push(ClipPlayer::start(path, settings, false, input));
    }
    let out = AudioOut::start(mixer, SystemHost);
    out.set_output(true, "");
    let start = Instant::now();
    let mut last = Instant::now();
    while start.elapsed() < Duration::from_secs(secs) {
        std::thread::sleep(Duration::from_secs(1));
        if last.elapsed() >= Duration::from_secs((secs / 10).clamp(1, 300)) {
            last = Instant::now();
            eprintln!(
                "{:>5} s: device {:?}, device underruns {}, source underruns {}, tap blocks {}",
                start.elapsed().as_secs(),
                out.device(),
                out.stats().underruns.load(Ordering::Relaxed),
                sources.iter().map(|s| s.underruns()).sum::<u64>(),
                blocks.load(Ordering::Relaxed)
            );
        }
    }
    let device = out.stats().underruns.load(Ordering::Relaxed);
    let source: u64 = sources.iter().map(|s| s.underruns()).sum();
    let rendered = out.stats().rendered.load(Ordering::Relaxed);
    let tapped = blocks.load(Ordering::Relaxed);
    eprintln!(
        "soak {secs} s on {:?}: device underruns {device}, source underruns {source}, \
         frames rendered {rendered}, tap blocks {tapped}, opens {}",
        out.device(),
        out.stats().opens.load(Ordering::Relaxed)
    );
    assert!(out.device().is_some(), "no output device to soak on");
    assert_eq!(device, 0, "device glitches");
    assert_eq!(source, 0, "source glitches");
    // Two taps, a block every 1024 frames each.
    let expected = 2 * secs * u64::from(RATE) / 1024;
    assert!(tapped + 8 >= expected * 99 / 100, "{tapped} of {expected}");
}

/// Counts allocations made on threads that ask to be watched.
mod counting {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    pub struct Counting;

    thread_local! {
        pub static WATCH: Cell<bool> = const { Cell::new(false) };
        pub static COUNT: Cell<u64> = const { Cell::new(0) };
    }

    fn note() {
        let _ = WATCH.try_with(|w| {
            if w.get() {
                let _ = COUNT.try_with(|c| c.set(c.get() + 1));
            }
        });
    }

    // SAFETY: forwards to the system allocator unchanged.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            note();
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            note();
            unsafe { System.dealloc(ptr, layout) }
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            note();
            unsafe { System.realloc(ptr, layout, size) }
        }
    }

    /// Allocations and frees `f` made on this thread.
    pub fn count(f: impl FnOnce()) -> u64 {
        COUNT.with(|c| c.set(0));
        WATCH.with(|w| w.set(true));
        f();
        WATCH.with(|w| w.set(false));
        COUNT.with(Cell::get)
    }
}

#[global_allocator]
static ALLOC: counting::Counting = counting::Counting;

#[test]
fn the_mixer_and_the_device_callback_never_allocate() {
    let (mut mixer, mut control, _taps) = mixer(3);
    let mut inputs = Vec::new();
    for i in 0..4 {
        let (id, mut input) = control.add(SourceKind::Video).unwrap();
        input.control.set_gain(0.25 * i as f32);
        input.control.set_taps(0b101);
        input.push(&vec![0.1; 48_000 * 2]);
        inputs.push((id, input));
    }
    control.tap(0).unwrap().set_on(true);
    control.tap(2).unwrap().set_on(true);
    let mut out = vec![0.0; 1024 * 2];
    // Sources arrive on the first render; removals go back to be dropped.
    mixer.render(&mut out);
    control.remove(inputs[0].0);
    inputs[1].1.control.flush();
    let allocations = counting::count(|| {
        for _ in 0..40 {
            mixer.render(&mut out);
        }
    });
    assert_eq!(allocations, 0, "Mixer::render allocated");
    control.collect();

    let (mut producer, consumer) = rtrb::RingBuffer::new(8192);
    for _ in 0..4096 {
        let _ = producer.push(0.3);
    }
    let maker = crate::output::DeviceFeedMakerForTests::maker(consumer);
    let mut feed = maker.make(44_100);
    let mut device = vec![0i16; 512 * 2];
    let allocations = counting::count(|| {
        for _ in 0..8 {
            feed.fill(&mut device, 2);
        }
    });
    assert_eq!(allocations, 0, "the device callback allocated");
}

#[test]
fn seeks_and_pauses_on_a_playing_clip_are_not_glitches() {
    let path = ramp("ramp-seek.wav", 6.0);
    let (clip, _sink, source) = player(path, ClipSettings::default());
    std::thread::sleep(Duration::from_millis(300));
    for at in [3.0, 1.0, 4.5, 0.2] {
        clip.control(ClipControl::Seek(at));
        std::thread::sleep(Duration::from_millis(250));
    }
    clip.control(ClipControl::Pause);
    std::thread::sleep(Duration::from_millis(100));
    clip.control(ClipControl::Seek(2.0));
    clip.control(ClipControl::Play);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(source.underruns(), 0);
}
