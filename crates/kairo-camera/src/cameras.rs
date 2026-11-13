//! The live cameras of the engine: `__setCamera` for every output at once.
//!
//! Outputs that want the same label share one capture and one source id,
//! as they share a video decoder. Opening runs on its own thread, so a slow
//! device never holds up slides. A camera change while one is opening
//! discards the stale stream when it arrives (`cameraLabel !== wanted`).
//! A failure leaves the screens as they were and is reported, as
//! `__setCamera` resolved `false`. A device that goes away mid-show keeps
//! its last frame on screen (as a `<video>` with an ended track does) and
//! is opened again when it comes back.

use crate::backend::{Backend, CameraError, Capture, Health};
use crate::format::Request;
use crate::label;
use kairo_compositor::YuvFrame;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Source ids of cameras sit far above the video players' ids.
pub const FIRST_ID: u64 = 1 << 48;
/// How often open captures are checked, and lost ones looked for.
pub const CHECK_EVERY: Duration = Duration::from_millis(500);

/// Where camera frames go (the compositor).
pub trait Feed: Send + Sync {
    fn frame(&self, id: u64, frame: Arc<YuvFrame>);
    fn remove(&self, id: u64);
}

#[derive(Clone, Debug, PartialEq)]
pub enum CameraEvent {
    /// Frames for `label` arrive as source `id`.
    Live {
        label: String,
        id: u64,
        width: u32,
        height: u32,
        fps: f64,
    },
    /// `label` could not be opened; the screens keep what they showed.
    Failed { label: String, error: CameraError },
    /// `label` stopped mid-show; its last frame stays.
    Lost { label: String, error: CameraError },
    /// `label` is running again on the same source id.
    Restored { label: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Opening,
    Live,
    Failed,
    /// Unplugged: the capture is closed and looked for again.
    Gone,
    /// Taken by another program: the capture stays and may resume.
    Held,
}

struct Cam {
    id: u64,
    generation: u64,
    status: Status,
    capture: Option<Box<dyn Capture>>,
    /// Cleared when the camera is dropped, so a late frame is not fed.
    active: Arc<AtomicBool>,
}

#[derive(Default)]
struct State {
    /// Each owner's (output's) wanted label.
    wants: HashMap<String, String>,
    cams: HashMap<String, Cam>,
    next_id: u64,
    stop: bool,
}

struct Inner {
    backend: Arc<dyn Backend>,
    feed: Arc<dyn Feed>,
    events: Box<dyn Fn(CameraEvent) + Send + Sync>,
    state: Mutex<State>,
    wake: Condvar,
}

pub struct Cameras {
    inner: Arc<Inner>,
    monitor: Option<JoinHandle<()>>,
}

impl Cameras {
    pub fn new(
        backend: Arc<dyn Backend>,
        feed: Arc<dyn Feed>,
        events: impl Fn(CameraEvent) + Send + Sync + 'static,
    ) -> Self {
        let inner = Arc::new(Inner {
            backend,
            feed,
            events: Box::new(events),
            state: Mutex::new(State {
                next_id: FIRST_ID,
                ..State::default()
            }),
            wake: Condvar::new(),
        });
        let watched = inner.clone();
        let monitor = std::thread::Builder::new()
            .name("kairo-camera-monitor".into())
            .spawn(move || monitor(&watched))
            .ok();
        Self { inner, monitor }
    }

    /// `owner` (an output) wants `label` live, or nothing. Asking again for
    /// a label that failed tries again.
    pub fn want(&self, owner: &str, label: Option<&str>) {
        let label = label.filter(|l| !l.is_empty());
        let mut opens = Vec::new();
        let mut removed = Vec::new();
        {
            let mut state = self.inner.lock();
            match label {
                Some(l) => {
                    state.wants.insert(owner.to_owned(), l.to_owned());
                }
                None => {
                    state.wants.remove(owner);
                }
            }
            if let Some(l) = label {
                let next_id = state.next_id;
                let cam = state.cams.entry(l.to_owned()).or_insert_with(|| Cam {
                    id: next_id,
                    generation: 0,
                    status: Status::Failed,
                    capture: None,
                    active: Arc::new(AtomicBool::new(true)),
                });
                if cam.id == next_id {
                    state.next_id += 1;
                }
                let cam = state.cams.get_mut(l).expect("camera");
                if cam.status == Status::Failed {
                    cam.generation += 1;
                    cam.status = Status::Opening;
                    opens.push((l.to_owned(), cam.generation, cam.id, cam.active.clone()));
                }
            }
            let wanted: Vec<String> = state.wants.values().cloned().collect();
            let unused: Vec<String> = state
                .cams
                .keys()
                .filter(|l| !wanted.contains(l))
                .cloned()
                .collect();
            for l in unused {
                if let Some(cam) = state.cams.remove(&l) {
                    cam.active.store(false, Ordering::Release);
                    removed.push(cam);
                }
            }
        }
        for cam in removed {
            // Closing a device can take a moment; never under the lock.
            drop(cam.capture);
            self.inner.feed.remove(cam.id);
        }
        for (l, generation, id, active) in opens {
            spawn_open(self.inner.clone(), l, generation, id, active);
        }
    }

    /// The source id of `label`'s frames once it is live.
    pub fn live_id(&self, label: &str) -> Option<u64> {
        let state = self.inner.lock();
        state
            .cams
            .get(label)
            .filter(|c| matches!(c.status, Status::Live | Status::Gone | Status::Held))
            .map(|c| c.id)
    }

    /// Video input labels as the operator's list showed them.
    pub fn labels(&self) -> Result<Vec<String>, CameraError> {
        Ok(label::labels(&self.inner.backend.video_devices()?))
    }
}

impl Drop for Cameras {
    fn drop(&mut self) {
        let cams: Vec<Cam> = {
            let mut state = self.inner.lock();
            state.stop = true;
            state.wants.clear();
            state.cams.drain().map(|(_, c)| c).collect()
        };
        self.inner.wake.notify_all();
        for cam in cams {
            cam.active.store(false, Ordering::Release);
            drop(cam.capture);
            self.inner.feed.remove(cam.id);
        }
        if let Some(monitor) = self.monitor.take() {
            let _ = monitor.join();
        }
    }
}

impl Inner {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn open(
        &self,
        wanted: &str,
        id: u64,
        active: &Arc<AtomicBool>,
    ) -> Result<Box<dyn Capture>, CameraError> {
        let devices = self.backend.video_devices()?;
        let device = label::find(&devices, wanted)
            .ok_or_else(|| CameraError::NotFound(wanted.to_owned()))?
            .clone();
        let feed = self.feed.clone();
        let active = active.clone();
        let sink = Box::new(move |frame: YuvFrame| {
            if active.load(Ordering::Acquire) && frame.is_valid() {
                feed.frame(id, Arc::new(frame));
            }
        });
        self.backend.open(&device, Request::PROGRAM, sink)
    }
}

fn spawn_open(
    inner: Arc<Inner>,
    wanted: String,
    generation: u64,
    id: u64,
    active: Arc<AtomicBool>,
) {
    let run = move || {
        let result = inner.open(&wanted, id, &active);
        let mut state = inner.lock();
        let current = state
            .cams
            .get(&wanted)
            .is_some_and(|c| c.generation == generation && !state.stop);
        if !current {
            // The operator moved on while this opened: drop it.
            drop(state);
            drop(result);
            return;
        }
        let cam = state.cams.get_mut(&wanted).expect("camera");
        let event = match result {
            Ok(capture) => {
                let (width, height, fps) = capture.mode();
                let restored = cam.status == Status::Gone;
                cam.capture = Some(capture);
                cam.status = Status::Live;
                if restored {
                    CameraEvent::Restored { label: wanted }
                } else {
                    tracing::info!(label = %wanted, width, height, fps, "camera live");
                    CameraEvent::Live {
                        label: wanted,
                        id,
                        width,
                        height,
                        fps,
                    }
                }
            }
            Err(error) => {
                if cam.status == Status::Gone {
                    // Still not back: keep looking.
                    return;
                }
                cam.status = Status::Failed;
                tracing::warn!(label = %wanted, %error, "camera did not start");
                CameraEvent::Failed {
                    label: wanted,
                    error,
                }
            }
        };
        drop(state);
        (inner.events)(event);
    };
    if let Err(error) = std::thread::Builder::new()
        .name("kairo-camera-open".into())
        .spawn(run)
    {
        tracing::error!(%error, "camera open thread could not start");
    }
}

fn monitor(inner: &Arc<Inner>) {
    let mut next_look = Instant::now();
    loop {
        let mut events = Vec::new();
        let mut closed = Vec::new();
        let mut reopen = Vec::new();
        {
            let state = inner.lock();
            let (mut state, _) = inner
                .wake
                .wait_timeout_while(state, CHECK_EVERY, |s| !s.stop)
                .unwrap_or_else(|e| e.into_inner());
            if state.stop {
                return;
            }
            let look = Instant::now() >= next_look;
            for (l, cam) in state.cams.iter_mut() {
                match cam.status {
                    Status::Live | Status::Held => {
                        let health = cam
                            .capture
                            .as_ref()
                            .map_or(Health::Unplugged, |c| c.health());
                        match (cam.status, health) {
                            (Status::Live, Health::Unplugged)
                            | (Status::Held, Health::Unplugged) => {
                                cam.status = Status::Gone;
                                closed.extend(cam.capture.take());
                                events.push(CameraEvent::Lost {
                                    label: l.clone(),
                                    error: CameraError::Unplugged,
                                });
                            }
                            (Status::Live, Health::Busy) => {
                                cam.status = Status::Held;
                                events.push(CameraEvent::Lost {
                                    label: l.clone(),
                                    error: CameraError::Busy,
                                });
                            }
                            (Status::Held, Health::Running) => {
                                cam.status = Status::Live;
                                events.push(CameraEvent::Restored { label: l.clone() });
                            }
                            _ => {}
                        }
                    }
                    Status::Gone if look => {
                        cam.generation += 1;
                        reopen.push((l.clone(), cam.generation, cam.id, cam.active.clone()));
                    }
                    _ => {}
                }
            }
            if look {
                next_look = Instant::now() + Duration::from_secs(1);
            }
        }
        for capture in closed {
            drop(capture);
        }
        for event in events {
            if let CameraEvent::Lost { label, error } = &event {
                tracing::warn!(%label, %error, "camera lost; its last frame stays");
            }
            (inner.events)(event);
        }
        for (l, generation, id, active) in reopen {
            // Only reopen once it is listed again, so a missing device
            // costs one listing a second.
            let listed = inner
                .backend
                .video_devices()
                .is_ok_and(|d| label::find(&d, &l).is_some());
            if listed {
                spawn_open(inner.clone(), l, generation, id, active);
            }
        }
    }
}
