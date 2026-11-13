//! One stream on its own thread: it opens the file, decodes ahead into the
//! transport's queue, and hands each frame to the compositor when the
//! clock reaches it. Idle (paused, ended, queue full and nothing due) it
//! sleeps until a control arrives or the next frame is due.

use crate::decode::{self, Backend};
use crate::transport::{Status, Timing, Transport};
use kairo_compositor::YuvFrame;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

/// Where a player's frames go.
pub trait Feed: Send + Sync {
    fn frame(&self, id: u64, frame: Arc<YuvFrame>);
    fn remove(&self, id: u64);
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Control {
    Pause(bool),
    Seek(f64),
    Loop(bool),
}

/// What a player is asked to play.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub path: PathBuf,
    pub timing: Timing,
    pub looped: bool,
    /// Try hardware decode first.
    pub hardware: bool,
}

struct Inbox {
    controls: Vec<Control>,
    stop: bool,
    status: Status,
    backend: Option<Backend>,
    presented: u64,
    dropped: u64,
}

struct Shared {
    inbox: Mutex<Inbox>,
    wake: Condvar,
}

/// Which player owns a source id, so a player that was replaced neither
/// draws nor removes the texture its successor uses.
#[derive(Default)]
pub(crate) struct Owners {
    current: Mutex<std::collections::HashMap<u64, u64>>,
}

pub struct Player {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

pub(crate) type OnError = Arc<dyn Fn(u64, &str, &str) + Send + Sync>;

impl Player {
    pub(crate) fn start(
        id: u64,
        generation: u64,
        request: Request,
        feed: Arc<dyn Feed>,
        owners: Arc<Owners>,
        on_error: OnError,
    ) -> Self {
        let shared = Arc::new(Shared {
            inbox: Mutex::new(Inbox {
                controls: Vec::new(),
                stop: false,
                status: Status {
                    position: request.timing.start.unwrap_or(0.0),
                    end: f64::INFINITY,
                    paused: false,
                    ended: false,
                    error: None,
                    shown: false,
                },
                backend: None,
                presented: 0,
                dropped: 0,
            }),
            wake: Condvar::new(),
        });
        let inner = shared.clone();
        let thread = std::thread::Builder::new()
            .name(format!("video {id:016x}"))
            .spawn(move || run(id, generation, request, inner, feed, owners, on_error))
            .map_err(|e| tracing::error!("cannot start a video thread: {e}"))
            .ok();
        Self { shared, thread }
    }

    pub fn control(&self, control: Control) {
        let mut inbox = self.shared.inbox.lock().expect("video inbox");
        inbox.controls.push(control);
        self.shared.wake.notify_one();
    }

    pub fn status(&self) -> Status {
        self.shared
            .inbox
            .lock()
            .expect("video inbox")
            .status
            .clone()
    }

    pub fn backend(&self) -> Option<Backend> {
        self.shared.inbox.lock().expect("video inbox").backend
    }

    /// Frames shown and frames skipped because they were late.
    pub fn counts(&self) -> (u64, u64) {
        let inbox = self.shared.inbox.lock().expect("video inbox");
        (inbox.presented, inbox.dropped)
    }

    /// Asks the thread to stop; it removes its texture on the way out unless
    /// another player took the source over.
    pub fn stop(mut self) {
        self.signal_stop();
        // Detached: a decode in progress must not hold up the caller.
        self.thread.take();
    }

    fn signal_stop(&self) {
        let mut inbox = self.shared.inbox.lock().expect("video inbox");
        inbox.stop = true;
        self.shared.wake.notify_one();
    }

    /// Stops and waits for the thread (tests, shutdown).
    pub fn join(mut self) {
        self.signal_stop();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.signal_stop();
    }
}

fn run(
    id: u64,
    generation: u64,
    request: Request,
    shared: Arc<Shared>,
    feed: Arc<dyn Feed>,
    owners: Arc<Owners>,
    on_error: OnError,
) {
    let path_text = request.path.display().to_string();
    let mut t = Transport::new(request.timing, request.looped, Instant::now());
    match decode::open(&request.path, request.hardware) {
        Ok(decoder) => {
            let backend = decoder.backend();
            let (w, h) = decoder.size();
            tracing::info!(path = %path_text, ?backend, width = w, height = h, "video opened");
            shared.inbox.lock().expect("video inbox").backend = Some(backend);
            t.open(decoder, Instant::now());
        }
        Err(message) => t.fail(message),
    }
    let mut reported = false;
    loop {
        let controls = {
            let mut inbox = shared.inbox.lock().expect("video inbox");
            if inbox.stop {
                break;
            }
            std::mem::take(&mut inbox.controls)
        };
        let now = Instant::now();
        for control in controls {
            match control {
                Control::Pause(p) => t.set_paused(p, now),
                Control::Seek(s) => t.seek(s, now),
                Control::Loop(l) => t.set_loop(l, now),
            }
        }
        let mut worked = false;
        loop {
            let now = Instant::now();
            if let Some(frame) = t.present(now) {
                // Only the current owner of the id draws into it.
                let owners = owners.current.lock().expect("video owners");
                if owners.get(&id) == Some(&generation) {
                    feed.frame(id, frame);
                }
            }
            if !t.decode_step(Instant::now()) {
                break;
            }
            worked = true;
            // Controls stay responsive while the decoder fills the queue.
            if !shared
                .inbox
                .lock()
                .expect("video inbox")
                .controls
                .is_empty()
            {
                break;
            }
        }
        if !reported && let Some(error) = t.error() {
            reported = true;
            tracing::warn!(path = %path_text, "video failed: {error}");
            on_error(id, &path_text, error);
        }
        let mut inbox = shared.inbox.lock().expect("video inbox");
        inbox.status = t.status(Instant::now());
        inbox.presented = t.presented;
        inbox.dropped = t.dropped;
        if inbox.stop || !inbox.controls.is_empty() || (worked && t.wants_decode()) {
            continue;
        }
        match t.next_wake() {
            Some(at) => {
                let wait = at.saturating_duration_since(Instant::now());
                if !wait.is_zero() {
                    drop(shared.wake.wait_timeout(inbox, wait));
                }
            }
            None if t.wants_decode() => {}
            None => {
                drop(shared.wake.wait(inbox));
            }
        }
    }
    let mut owners = owners.current.lock().expect("video owners");
    if owners.get(&id) == Some(&generation) {
        owners.remove(&id);
        feed.remove(id);
    }
}

impl Owners {
    pub(crate) fn claim(&self, id: u64, generation: u64) {
        self.current
            .lock()
            .expect("video owners")
            .insert(id, generation);
    }
}
