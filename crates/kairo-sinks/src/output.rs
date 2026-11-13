//! One NDI feed: Electron's `NdiService`, per output.
//!
//! - The sender is created on its own thread. If that fails (most often a
//!   second copy of the app holding the name), the reason is kept and
//!   creation is tried again after [`RETRY_COOLDOWN`], never disabling NDI
//!   for the rest of the run.
//! - New pixels go out at once; the current frame is repeated every
//!   [`FRAME_INTERVAL`] so receivers keep a picture. The next send is only
//!   started once the previous one returned, so a slow network never
//!   builds a backlog.
//! - Until a frame arrives, and after [`NdiOutput::clear`], the frame is
//!   1920 × 1080 transparent black BGRA, as Electron's cleared frame.
//! - Audio blocks go straight to the sender; failures are logged at most
//!   once every 10 s.

use crate::ndi::{BGRA, Runtime, Sender, UYVA};
use kairo_audio::AudioBlock;
use kairo_compositor::uyva::UyvaFrame;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// The repeat cadence of the current frame (about 10 fps).
pub const FRAME_INTERVAL: Duration = Duration::from_millis(100);
/// Between sender-creation attempts.
pub const RETRY_COOLDOWN: Duration = Duration::from_secs(5);
/// The cleared frame's size (Electron's `WIDTH` × `HEIGHT`).
pub const CLEAR_SIZE: (u32, u32) = (1920, 1080);

/// The feed's cadence; tests shorten it.
#[derive(Clone, Copy, Debug)]
pub struct Timing {
    pub interval: Duration,
    pub cooldown: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            interval: FRAME_INTERVAL,
            cooldown: RETRY_COOLDOWN,
        }
    }
}

/// What sends: the NDI runtime, or a fake under test.
pub trait Transmit: Send + Sync {
    fn video(&self, width: u32, height: u32, fourcc: u32, data: &[u8]) -> Result<(), String>;
    fn audio(&self, block: &AudioBlock) -> Result<(), String>;
}

impl Transmit for Sender {
    fn video(&self, width: u32, height: u32, fourcc: u32, data: &[u8]) -> Result<(), String> {
        self.send_video(width, height, fourcc, data)
    }

    fn audio(&self, block: &AudioBlock) -> Result<(), String> {
        self.send_audio(
            block.sample_rate.round() as u32,
            block.channels,
            block.samples,
            &block.data,
        )
    }
}

/// Makes senders by name.
pub trait Make: Send + Sync {
    fn make(&self, name: &str) -> Result<Arc<dyn Transmit>, String>;
}

impl Make for Arc<Runtime> {
    fn make(&self, name: &str) -> Result<Arc<dyn Transmit>, String> {
        Ok(Arc::new(self.sender(name)?))
    }
}

/// `getStatus()`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub available: bool,
    pub sending: bool,
    pub sender_error: Option<String>,
}

enum Frame {
    Clear,
    Uyva(Arc<UyvaFrame>),
}

struct State {
    frame: Frame,
    dirty: bool,
    stop: bool,
    sender: Option<Arc<dyn Transmit>>,
    error: Option<String>,
}

struct Shared {
    name: String,
    state: Mutex<State>,
    wake: Condvar,
    sent: AtomicU64,
    failed: AtomicU64,
    audio_warned: Mutex<Option<Instant>>,
}

pub struct NdiOutput {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl NdiOutput {
    /// Starts the feed named `name`.
    pub fn start(make: Arc<dyn Make>, name: &str) -> Self {
        Self::start_with(make, name, Timing::default())
    }

    pub fn start_with(make: Arc<dyn Make>, name: &str, timing: Timing) -> Self {
        let shared = Arc::new(Shared {
            name: name.to_owned(),
            state: Mutex::new(State {
                frame: Frame::Clear,
                dirty: true,
                stop: false,
                sender: None,
                error: None,
            }),
            wake: Condvar::new(),
            sent: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            audio_warned: Mutex::new(None),
        });
        let looped = shared.clone();
        let thread = std::thread::Builder::new()
            .name(format!("kairo-ndi {name}"))
            .spawn(move || run(&looped, &*make, timing))
            .ok();
        Self { shared, thread }
    }

    pub fn name(&self) -> &str {
        &self.shared.name
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.shared.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// New pixels: sent now (or as soon as the send in flight returns).
    pub fn frame(&self, frame: UyvaFrame) {
        if !frame.alpha
            || frame.data().len() != kairo_compositor::uyva::uyva_len(frame.width, frame.height)
        {
            tracing::warn!(name = self.shared.name, "NDI frame size mismatch; ignored");
            return;
        }
        let mut state = self.lock();
        state.frame = Frame::Uyva(Arc::new(frame));
        state.dirty = true;
        drop(state);
        self.shared.wake.notify_all();
    }

    /// Back to fully transparent (Electron's `clearFrame`).
    pub fn clear(&self) {
        let mut state = self.lock();
        state.frame = Frame::Clear;
        state.dirty = true;
        drop(state);
        self.shared.wake.notify_all();
    }

    /// One block of program sound; dropped when there is no sender yet.
    pub fn audio(&self, block: &AudioBlock) {
        if let Err(error) = block.validate() {
            tracing::warn!(name = self.shared.name, ?error, "NDI audio block refused");
            return;
        }
        let sender = self.lock().sender.clone();
        let Some(sender) = sender else { return };
        if let Err(error) = sender.audio(block) {
            let mut warned = self
                .shared
                .audio_warned
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if warned.is_none_or(|t| t.elapsed() >= Duration::from_secs(10)) {
                *warned = Some(Instant::now());
                tracing::warn!(name = self.shared.name, %error, "NDI audio frame failed");
            }
        }
    }

    pub fn status(&self) -> Status {
        let state = self.lock();
        Status {
            available: true,
            sending: state.sender.is_some(),
            sender_error: state.error.clone(),
        }
    }

    /// Frames sent and failed so far.
    pub fn counts(&self) -> (u64, u64) {
        (
            self.shared.sent.load(Ordering::Relaxed),
            self.shared.failed.load(Ordering::Relaxed),
        )
    }
}

impl Drop for NdiOutput {
    fn drop(&mut self) {
        self.lock().stop = true;
        self.shared.wake.notify_all();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(shared: &Shared, make: &dyn Make, timing: Timing) {
    let clear = vec![0u8; (CLEAR_SIZE.0 * CLEAR_SIZE.1 * 4) as usize];
    let mut retry_at = Instant::now();
    let mut next_repeat = Instant::now();
    loop {
        // Sender: create it, or try again after the cooldown.
        let has_sender = shared
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .sender
            .is_some();
        if !has_sender && Instant::now() >= retry_at {
            match make.make(&shared.name) {
                Ok(sender) => {
                    let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
                    state.sender = Some(sender);
                    state.error = None;
                    state.dirty = true;
                    tracing::info!(name = shared.name, "NDI sender created");
                }
                Err(error) => {
                    retry_at = Instant::now() + timing.cooldown;
                    tracing::error!(
                        name = shared.name,
                        %error,
                        "NDI sender creation failed; will retry. Is a second copy of Kairo running?"
                    );
                    shared.state.lock().unwrap_or_else(|e| e.into_inner()).error = Some(error);
                }
            }
        }
        // Wait for new pixels, the repeat tick or the next retry.
        let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if state.stop {
                return;
            }
            let now = Instant::now();
            let due = if state.sender.is_some() {
                next_repeat
            } else {
                retry_at
            };
            if (state.sender.is_some() && state.dirty) || now >= due {
                break;
            }
            state = shared
                .wake
                .wait_timeout(state, due - now)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        let Some(sender) = state.sender.clone() else {
            continue;
        };
        state.dirty = false;
        // Sent outside the lock: with the video clock a send waits for its
        // frame's turn, and new frames must not wait behind it.
        let frame = match &state.frame {
            Frame::Clear => None,
            Frame::Uyva(f) => Some(f.clone()),
        };
        drop(state);
        let result = match &frame {
            None => sender.video(CLEAR_SIZE.0, CLEAR_SIZE.1, BGRA, &clear),
            Some(f) => sender.video(f.width, f.height, UYVA, f.data()),
        };
        drop(frame);
        next_repeat = Instant::now() + timing.interval;
        record(shared, result);
    }
}

fn record(shared: &Shared, result: Result<(), String>) {
    match result {
        Ok(()) => {
            shared.sent.fetch_add(1, Ordering::Relaxed);
        }
        Err(error) => {
            shared.failed.fetch_add(1, Ordering::Relaxed);
            tracing::error!(name = shared.name, %error, "NDI frame push failed");
        }
    }
}
