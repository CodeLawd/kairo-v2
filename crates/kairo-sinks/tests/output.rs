//! The NDI feed's behaviour against a fake sender, and against the real NDI
//! runtime when one is present.

use kairo_audio::AudioBlock;
use kairo_compositor::uyva::{UyvaConverter, UyvaFrame};
use kairo_sinks::ndi::{BGRA, Runtime, UYVA};
use kairo_sinks::{Make, NdiOutput, Timing, Transmit};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Width, height, FourCC, bytes and when.
type Sent = (u32, u32, u32, usize, Instant);

#[derive(Default)]
struct Log {
    video: Mutex<Vec<Sent>>,
    audio: Mutex<Vec<(u32, u32)>>,
}

struct Fake {
    log: Arc<Log>,
    delay: Duration,
}

impl Transmit for Fake {
    fn video(&self, w: u32, h: u32, fourcc: u32, data: &[u8]) -> Result<(), String> {
        std::thread::sleep(self.delay);
        self.log
            .video
            .lock()
            .unwrap()
            .push((w, h, fourcc, data.len(), Instant::now()));
        Ok(())
    }
    fn audio(&self, b: &AudioBlock) -> Result<(), String> {
        self.log.audio.lock().unwrap().push((b.channels, b.samples));
        Ok(())
    }
}

struct Maker {
    log: Arc<Log>,
    fail_first: AtomicU32,
    attempts: AtomicU32,
    delay: Duration,
}

impl Make for Maker {
    fn make(&self, _: &str) -> Result<Arc<dyn Transmit>, String> {
        self.attempts.fetch_add(1, Ordering::AcqRel);
        if self.fail_first.load(Ordering::Acquire) > 0 {
            self.fail_first.fetch_sub(1, Ordering::AcqRel);
            return Err("name in use".into());
        }
        Ok(Arc::new(Fake {
            log: self.log.clone(),
            delay: self.delay,
        }))
    }
}

fn maker(fail_first: u32, delay: Duration) -> (Arc<Maker>, Arc<Log>) {
    let log = Arc::new(Log::default());
    (
        Arc::new(Maker {
            log: log.clone(),
            fail_first: AtomicU32::new(fail_first),
            attempts: AtomicU32::new(0),
            delay,
        }),
        log,
    )
}

const FAST: Timing = Timing {
    interval: Duration::from_millis(40),
    cooldown: Duration::from_millis(150),
};

fn wait(what: &str, f: impl Fn() -> bool) {
    let t = Instant::now();
    while !f() {
        assert!(t.elapsed() < Duration::from_secs(5), "timed out: {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// A real UYVA frame from the GPU converter (needs an adapter).
fn uyva(w: u32, h: u32) -> Option<UyvaFrame> {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
    let (d, q) = pollster::block_on(adapter.request_device(&Default::default())).ok()?;
    let t = d.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let c = UyvaConverter::new(&d, w, h, true).ok()?;
    let (tx, rx) = std::sync::mpsc::channel();
    c.convert(&d, &q, &t, Box::new(move |f| tx.send(f).unwrap()));
    d.poll(wgpu::PollType::wait_indefinitely()).ok()?;
    rx.recv().ok()?.ok()
}

#[test]
fn starts_transparent_then_sends_new_pixels_at_once_and_repeats_them() {
    let (make, log) = maker(0, Duration::ZERO);
    let out = NdiOutput::start_with(make, "Kairo Scripture", FAST);
    wait("clear frame", || !log.video.lock().unwrap().is_empty());
    let first = log.video.lock().unwrap()[0];
    assert_eq!(
        (first.0, first.1, first.2, first.3),
        (1920, 1080, BGRA, 1920 * 1080 * 4)
    );
    assert!(out.status().sending);
    let Some(frame) = uyva(1920, 1080) else {
        return;
    };
    let sent = Instant::now();
    out.frame(frame);
    wait("uyva", || {
        log.video.lock().unwrap().iter().any(|v| v.2 == UYVA)
    });
    let at = log
        .video
        .lock()
        .unwrap()
        .iter()
        .find(|v| v.2 == UYVA)
        .unwrap()
        .4;
    assert!(
        at.duration_since(sent) < Duration::from_millis(30),
        "sent without waiting for the tick"
    );
    let repeated_frames = || {
        log.video
            .lock()
            .unwrap()
            .iter()
            .filter(|v| v.2 == UYVA && v.3 == 1920 * 1080 * 3)
            .count()
    };
    if std::env::var("KAIRO_PERF_HOSTED").as_deref() == Ok("1") {
        wait("repeated UYVA frames", || repeated_frames() >= 4);
    } else {
        std::thread::sleep(Duration::from_millis(300));
        let uyvas = repeated_frames();
        assert!(
            (4..=10).contains(&uyvas),
            "repeated at the cadence: {uyvas}"
        );
    }
    out.clear();
    wait("cleared", || {
        log.video.lock().unwrap().last().unwrap().2 == BGRA
    });
}

#[test]
fn a_failed_sender_reports_why_and_retries_after_the_cooldown() {
    let (make, log) = maker(2, Duration::ZERO);
    let out = NdiOutput::start_with(make.clone(), "Kairo Scripture", FAST);
    wait("first failure", || out.status().sender_error.is_some());
    let status = out.status();
    assert!(!status.sending);
    assert_eq!(status.sender_error.as_deref(), Some("name in use"));
    // Not hammered: one attempt per cooldown.
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(make.attempts.load(Ordering::Acquire), 1);
    wait("sending", || out.status().sending);
    assert_eq!(make.attempts.load(Ordering::Acquire), 3);
    assert_eq!(out.status().sender_error, None);
    wait("a frame", || !log.video.lock().unwrap().is_empty());
}

#[test]
fn a_slow_sender_never_builds_a_backlog() {
    // Each send takes longer than the repeat tick.
    let (make, log) = maker(0, Duration::from_millis(60));
    let out = NdiOutput::start_with(make, "x", FAST);
    let start = Instant::now();
    std::thread::sleep(Duration::from_millis(600));
    let n = log.video.lock().unwrap().len();
    // At most one send per (send + interval), never a queue of ticks.
    assert!(n <= 7, "{n} sends in {:?}", start.elapsed());
    drop(out);
    let after = log.video.lock().unwrap().len();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(log.video.lock().unwrap().len(), after, "stopped on drop");
}

#[test]
fn audio_goes_out_once_there_is_a_sender_and_bad_blocks_are_refused() {
    let (make, log) = maker(0, Duration::ZERO);
    let out = NdiOutput::start_with(make, "x", FAST);
    wait("sending", || out.status().sending);
    out.audio(&AudioBlock {
        sample_rate: 48_000.0,
        channels: 2,
        samples: 1024,
        data: vec![0.0; 2048],
    });
    out.audio(&AudioBlock {
        sample_rate: 48_000.0,
        channels: 9,
        samples: 1024,
        data: vec![0.0; 9 * 1024],
    });
    out.audio(&AudioBlock {
        sample_rate: 48_000.0,
        channels: 2,
        samples: 1024,
        data: vec![0.0; 10],
    });
    assert_eq!(*log.audio.lock().unwrap(), vec![(2, 1024)]);
}

/// The real runtime, when the app folder or `KAIRO_NDI_RUNTIME` has one.
fn runtime() -> Option<Arc<Runtime>> {
    let spike = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../spikes/sinks/app-folder/libndi.dylib");
    if std::env::var_os("KAIRO_NDI_RUNTIME").is_none() && spike.is_file() {
        return Runtime::load(&spike).ok().map(Arc::new);
    }
    match Runtime::find() {
        Ok(r) => Some(r),
        Err(e) => {
            assert!(
                std::env::var_os("KAIRO_REQUIRE_NDI").is_none(),
                "no NDI runtime: {e}"
            );
            eprintln!("no NDI runtime ({e}); skipped");
            None
        }
    }
}

#[test]
fn ten_start_stop_cycles_on_the_real_runtime_and_a_receiver_sees_the_frame() {
    let Some(rt) = runtime() else { return };
    eprintln!("NDI {}", rt.version());
    let name = format!("Kairo test {}", std::process::id());
    for cycle in 0..10 {
        let out = NdiOutput::start(Arc::new(rt.clone()), &name);
        wait("sending", || out.status().sending);
        if let Some(frame) = uyva(1920, 1080) {
            out.frame(frame);
        }
        out.audio(&AudioBlock {
            sample_rate: 48_000.0,
            channels: 2,
            samples: 1024,
            data: vec![0.0; 2048],
        });
        wait("frames out", || out.counts().0 >= 2);
        if cycle == 9 {
            // A receiver on this machine finds the source and gets a frame.
            let mut seen = None;
            let got = rt
                .receive_one(&name, 5000, |f| seen = Some((f.xres, f.yres, f.fourcc)))
                .unwrap();
            assert!(got, "no receiver frame from {name}");
            let (w, h, _) = seen.unwrap();
            assert_eq!((w, h), (1920, 1080));
        }
        assert_eq!(out.counts().1, 0, "cycle {cycle}: failed sends");
        drop(out);
    }
}
