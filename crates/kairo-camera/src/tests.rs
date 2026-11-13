//! The camera manager against a synthetic capture system.

use crate::backend::{Backend, CameraError, Capture, FrameSink, Health};
use crate::cameras::{CameraEvent, Cameras, FIRST_ID, Feed};
use crate::format::Request;
use crate::label::{DeviceInfo, chromium_label};
use kairo_compositor::YuvFrame;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

/// A camera that draws its frame number into the luma plane.
struct SynthCapture {
    stop: Arc<AtomicBool>,
    health: Arc<Mutex<Health>>,
    closed: Arc<AtomicU64>,
}

impl Capture for SynthCapture {
    fn health(&self) -> Health {
        *self.health.lock().unwrap()
    }
    fn mode(&self) -> (u32, u32, f64) {
        (64, 36, 30.0)
    }
}

impl Drop for SynthCapture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.closed.fetch_add(1, Ordering::AcqRel);
    }
}

#[derive(Default)]
struct Synth {
    devices: Mutex<Vec<DeviceInfo>>,
    deny: AtomicBool,
    busy: AtomicBool,
    /// Opens of these labels wait for a message before finishing.
    gates: Mutex<HashMap<String, mpsc::Receiver<()>>>,
    health: Arc<Mutex<Health>>,
    opened: AtomicU64,
    closed: Arc<AtomicU64>,
}

impl Synth {
    fn with(names: &[(&str, &str)]) -> Arc<Self> {
        let s = Self {
            health: Arc::new(Mutex::new(Health::Running)),
            ..Self::default()
        };
        *s.devices.lock().unwrap() = names
            .iter()
            .map(|(name, id)| DeviceInfo {
                id: (*id).into(),
                name: (*name).into(),
                label: chromium_label(name, id),
            })
            .collect();
        Arc::new(s)
    }

    fn gate(&self, label: &str) -> mpsc::Sender<()> {
        let (tx, rx) = mpsc::channel();
        self.gates.lock().unwrap().insert(label.into(), rx);
        tx
    }
}

impl Backend for Synth {
    fn video_devices(&self) -> Result<Vec<DeviceInfo>, CameraError> {
        if self.deny.load(Ordering::Acquire) {
            return Err(CameraError::Denied);
        }
        Ok(self.devices.lock().unwrap().clone())
    }

    fn open(
        &self,
        device: &DeviceInfo,
        request: Request,
        sink: FrameSink,
    ) -> Result<Box<dyn Capture>, CameraError> {
        assert_eq!(request, Request::PROGRAM);
        let gate = self.gates.lock().unwrap().remove(&device.label);
        if let Some(gate) = gate {
            let _ = gate.recv_timeout(Duration::from_secs(5));
        }
        if self.busy.load(Ordering::Acquire) {
            return Err(CameraError::Busy);
        }
        self.opened.fetch_add(1, Ordering::AcqRel);
        let stop = Arc::new(AtomicBool::new(false));
        let running = stop.clone();
        std::thread::spawn(move || {
            let mut n = 0u8;
            while !running.load(Ordering::Acquire) {
                sink(YuvFrame::solid(64, 36, n, 128, 128));
                n = n.wrapping_add(1);
                std::thread::sleep(Duration::from_millis(5));
            }
        });
        Ok(Box::new(SynthCapture {
            stop,
            health: self.health.clone(),
            closed: self.closed.clone(),
        }))
    }
}

#[derive(Default)]
struct Frames {
    by_id: Mutex<HashMap<u64, u64>>,
    removed: Mutex<Vec<u64>>,
}

impl Feed for Frames {
    fn frame(&self, id: u64, frame: Arc<YuvFrame>) {
        assert!(frame.is_valid());
        *self.by_id.lock().unwrap().entry(id).or_default() += 1;
    }
    fn remove(&self, id: u64) {
        self.removed.lock().unwrap().push(id);
    }
}

impl Frames {
    fn count(&self, id: u64) -> u64 {
        self.by_id.lock().unwrap().get(&id).copied().unwrap_or(0)
    }
}

fn start(backend: Arc<Synth>) -> (Cameras, Arc<Frames>, mpsc::Receiver<CameraEvent>) {
    let frames = Arc::new(Frames::default());
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let cams = Cameras::new(backend, frames.clone(), move |e| {
        let _ = tx.lock().unwrap().send(e);
    });
    (cams, frames, rx)
}

fn next(rx: &mpsc::Receiver<CameraEvent>) -> CameraEvent {
    rx.recv_timeout(Duration::from_secs(5))
        .expect("a camera event")
}

fn wait(what: &str, f: impl Fn() -> bool) {
    let t = Instant::now();
    while !f() {
        assert!(t.elapsed() < Duration::from_secs(5), "timed out: {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

const BRIO: (&str, &str) = ("Logitech BRIO", "0x14100000046d085e");
const FACETIME: (&str, &str) = ("FaceTime HD Camera", "47B4B64B-7067-4B9C-AD2B-AE273A71F4B5");

#[test]
fn outputs_share_one_capture_and_frames_flow_until_nobody_wants_it() {
    let synth = Synth::with(&[BRIO, FACETIME]);
    let (cams, frames, rx) = start(synth.clone());
    // The label Electron saved, model id and all.
    cams.want("screen", Some("Logitech BRIO (046d:085e)"));
    let CameraEvent::Live { id, width, .. } = next(&rx) else {
        panic!("not live");
    };
    assert_eq!(id, FIRST_ID);
    assert_eq!(width, 64);
    cams.want("ndi", Some("Logitech BRIO (046d:085e)"));
    assert_eq!(cams.live_id("Logitech BRIO (046d:085e)"), Some(id));
    wait("frames", || frames.count(id) > 5);
    assert_eq!(synth.opened.load(Ordering::Acquire), 1);
    cams.want("screen", None);
    assert_eq!(
        synth.closed.load(Ordering::Acquire),
        0,
        "ndi still shows it"
    );
    cams.want("ndi", None);
    assert_eq!(synth.closed.load(Ordering::Acquire), 1);
    assert_eq!(*frames.removed.lock().unwrap(), vec![id]);
    let n = frames.count(id);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(frames.count(id), n, "no frames after it is dropped");
    assert!(rx.try_recv().is_err());
}

#[test]
fn a_camera_change_while_opening_discards_the_stale_stream() {
    let synth = Synth::with(&[BRIO, FACETIME]);
    let release = synth.gate("Logitech BRIO (046d:085e)");
    let (cams, frames, rx) = start(synth.clone());
    cams.want("screen", Some("Logitech BRIO (046d:085e)"));
    cams.want("screen", Some("FaceTime HD Camera"));
    let CameraEvent::Live { label, id, .. } = next(&rx) else {
        panic!("not live");
    };
    assert_eq!(label, "FaceTime HD Camera");
    release.send(()).unwrap();
    wait("stale capture closed", || {
        synth.closed.load(Ordering::Acquire) == 1
    });
    assert!(
        rx.recv_timeout(Duration::from_millis(200)).is_err(),
        "no event for the stale one"
    );
    assert_eq!(cams.live_id("Logitech BRIO (046d:085e)"), None);
    wait("frames", || frames.count(id) > 0);
    // The stale stream's id never received frames after it was dropped.
    assert!(frames.removed.lock().unwrap().contains(&FIRST_ID));
}

#[test]
fn not_found_denied_and_busy_fail_without_touching_the_screens() {
    let synth = Synth::with(&[BRIO]);
    let (cams, frames, rx) = start(synth.clone());
    cams.want("screen", Some("Elgato Cam Link 4K"));
    let event = next(&rx);
    assert_eq!(
        event,
        CameraEvent::Failed {
            label: "Elgato Cam Link 4K".into(),
            error: CameraError::NotFound("Elgato Cam Link 4K".into()),
        }
    );
    if let CameraEvent::Failed { error, .. } = &event {
        assert_eq!(error.to_string(), "Camera not found: Elgato Cam Link 4K");
    }
    synth.deny.store(true, Ordering::Release);
    cams.want("screen", Some("Logitech BRIO"));
    assert!(matches!(
        next(&rx),
        CameraEvent::Failed {
            error: CameraError::Denied,
            ..
        }
    ));
    synth.deny.store(false, Ordering::Release);
    synth.busy.store(true, Ordering::Release);
    // Asking again for a failed label tries again.
    cams.want("screen", Some("Logitech BRIO"));
    assert!(matches!(
        next(&rx),
        CameraEvent::Failed {
            error: CameraError::Busy,
            ..
        }
    ));
    synth.busy.store(false, Ordering::Release);
    cams.want("screen", Some("Logitech BRIO"));
    assert!(matches!(next(&rx), CameraEvent::Live { .. }));
    assert!(
        frames
            .removed
            .lock()
            .unwrap()
            .iter()
            .all(|id| *id != cams.live_id("Logitech BRIO").unwrap())
    );
    // An empty label is no camera.
    cams.want("screen", Some(""));
    assert_eq!(cams.live_id("Logitech BRIO"), None);
}

#[test]
fn an_unplugged_camera_keeps_its_id_and_comes_back() {
    let synth = Synth::with(&[BRIO]);
    let (cams, frames, rx) = start(synth.clone());
    cams.want("screen", Some("Logitech BRIO"));
    let CameraEvent::Live { id, .. } = next(&rx) else {
        panic!("not live");
    };
    *synth.health.lock().unwrap() = Health::Unplugged;
    let removed = synth.devices.lock().unwrap().remove(0);
    assert_eq!(
        next(&rx),
        CameraEvent::Lost {
            label: "Logitech BRIO".into(),
            error: CameraError::Unplugged
        }
    );
    assert_eq!(
        cams.live_id("Logitech BRIO"),
        Some(id),
        "the last frame stays"
    );
    assert!(frames.removed.lock().unwrap().is_empty());
    std::thread::sleep(Duration::from_millis(1200));
    assert_eq!(
        synth.opened.load(Ordering::Acquire),
        1,
        "not reopened while missing"
    );
    *synth.health.lock().unwrap() = Health::Running;
    synth.devices.lock().unwrap().push(removed);
    assert_eq!(
        next(&rx),
        CameraEvent::Restored {
            label: "Logitech BRIO".into()
        }
    );
    let n = frames.count(id);
    wait("frames again", || frames.count(id) > n + 3);
}

#[test]
fn a_camera_taken_by_another_program_is_reported_and_resumes() {
    let synth = Synth::with(&[BRIO]);
    let (cams, _frames, rx) = start(synth.clone());
    cams.want("screen", Some("Logitech BRIO"));
    assert!(matches!(next(&rx), CameraEvent::Live { .. }));
    *synth.health.lock().unwrap() = Health::Busy;
    assert_eq!(
        next(&rx),
        CameraEvent::Lost {
            label: "Logitech BRIO".into(),
            error: CameraError::Busy
        }
    );
    *synth.health.lock().unwrap() = Health::Running;
    assert_eq!(
        next(&rx),
        CameraEvent::Restored {
            label: "Logitech BRIO".into()
        }
    );
    assert_eq!(
        synth.opened.load(Ordering::Acquire),
        1,
        "the capture was kept"
    );
}

#[test]
fn dropping_the_manager_closes_everything() {
    let synth = Synth::with(&[BRIO, FACETIME]);
    let (cams, frames, rx) = start(synth.clone());
    cams.want("a", Some("Logitech BRIO"));
    cams.want("b", Some("FaceTime HD Camera"));
    let _ = (next(&rx), next(&rx));
    drop(cams);
    assert_eq!(synth.closed.load(Ordering::Acquire), 2);
    assert_eq!(frames.removed.lock().unwrap().len(), 2);
}
