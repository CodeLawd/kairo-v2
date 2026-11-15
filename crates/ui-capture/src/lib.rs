//! The header's Live popover (capture and Syphon) and the Capture Settings
//! window (prompt 091): `LiveMenu`/`CaptureSection` in `AppShell.tsx`,
//! `CaptureSettings.tsx` and `useCaptureStore.ts`.

pub mod model;
mod view;

use std::sync::Arc;

use gpui_kit::{Context, EventEmitter, Task};
use kairo_domain::capture::{CaptureAudioInput, CaptureSettings, CaptureStatus};
use kairo_domain::overlay::{OverlayOutput, OverlayOutputKind};

pub use view::{CaptureSettingsWindow, CaptureWindowEvent, LiveMenu, LiveMenuEvent, ids};

/// One output's readiness (`ndi.getStatus().outputs[]`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputReady {
    pub id: String,
    pub ready: Option<bool>,
    pub reason: Option<String>,
}

/// What the views read at start.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CaptureSnapshot {
    pub status: CaptureStatus,
    pub settings: CaptureSettings,
    pub outputs: Vec<OverlayOutput>,
    pub ready: Vec<OutputReady>,
}

/// What the application pushes.
#[derive(Clone, Debug, PartialEq)]
pub enum CaptureFeed {
    /// `capture.onStatus`.
    Status(CaptureStatus),
    /// `settings.capture` and `settings.overlay.outputs` changed.
    Settings(CaptureSettings, Vec<OverlayOutput>),
    /// Output readiness, polled.
    Ready(Vec<OutputReady>),
}

pub type FeedSink = Arc<dyn Fn(CaptureFeed) + Send + Sync>;
/// Dropping it stops the feed.
pub type Listening = Box<dyn Send>;
pub type Reply<T> = Box<dyn FnOnce(Result<T, String>) + Send>;

/// `window.api.capture.*`, `settings.set('capture')` and the outputs' switches.
pub trait CaptureBackend: Send + Sync + 'static {
    fn snapshot(&self) -> CaptureSnapshot;
    fn listen(&self, sink: FeedSink) -> Listening;
    fn start(&self, reply: Reply<CaptureStatus>);
    fn stop(&self, reply: Reply<CaptureStatus>);
    fn save(&self, settings: CaptureSettings);
    /// The folder dialog; `None` when cancelled. Saves the choice.
    fn choose_folder(&self, reply: Reply<Option<String>>);
    /// Show in Finder: the last recording, else the folder.
    fn reveal(&self);
    fn audio_inputs(&self, reply: Reply<Vec<CaptureAudioInput>>);
    /// A Syphon switch (`saveOutputSettings`).
    fn set_output_enabled(&self, id: String, enabled: bool);
}

/// The capture state the Live popover and the settings window share
/// (`useCaptureStore`).
pub struct Capture {
    backend: Arc<dyn CaptureBackend>,
    pub status: CaptureStatus,
    pub settings: CaptureSettings,
    pub outputs: Vec<OverlayOutput>,
    pub ready: Vec<OutputReady>,
    /// Start or Stop is on its way.
    pub busy: bool,
    _listening: Listening,
    _feed: Task<()>,
}

/// The state changed.
pub struct CaptureChanged;
impl EventEmitter<CaptureChanged> for Capture {}

impl Capture {
    pub fn new(backend: Arc<dyn CaptureBackend>, cx: &mut Context<Self>) -> Self {
        let snapshot = backend.snapshot();
        let (tx, mut rx) = futures_channel::mpsc::unbounded();
        let sink: FeedSink = Arc::new(move |f| {
            let _ = tx.unbounded_send(f);
        });
        let listening = backend.listen(sink);
        let feed = cx.spawn(async move |this, cx| {
            use futures_util::StreamExt;
            while let Some(f) = rx.next().await {
                if this.update(cx, |t, cx| t.apply(f, cx)).is_err() {
                    break;
                }
            }
        });
        Self {
            backend,
            status: snapshot.status,
            settings: snapshot.settings,
            outputs: snapshot.outputs,
            ready: snapshot.ready,
            busy: false,
            _listening: listening,
            _feed: feed,
        }
    }

    pub fn backend(&self) -> &Arc<dyn CaptureBackend> {
        &self.backend
    }

    pub fn apply(&mut self, feed: CaptureFeed, cx: &mut Context<Self>) {
        match feed {
            CaptureFeed::Status(status) => self.status = status,
            CaptureFeed::Settings(settings, outputs) => {
                self.settings = settings;
                self.outputs = outputs;
            }
            CaptureFeed::Ready(ready) => self.ready = ready,
        }
        cx.emit(CaptureChanged);
        cx.notify();
    }

    pub fn syphons(&self) -> Vec<&OverlayOutput> {
        self.outputs
            .iter()
            .filter(|o| o.kind == OverlayOutputKind::Syphon)
            .collect()
    }

    pub fn readiness(&self, id: &str) -> Option<&OutputReady> {
        self.ready.iter().find(|r| r.id == id)
    }

    /// Start when idle, Stop when recording; one at a time.
    pub fn toggle(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        let (tx, rx) = futures_channel::oneshot::channel();
        let reply: Reply<CaptureStatus> = Box::new(move |r| {
            let _ = tx.send(r);
        });
        if self.status.recording {
            self.backend.stop(reply);
        } else {
            self.backend.start(reply);
        }
        cx.spawn(async move |this, cx| {
            let result = rx
                .await
                .unwrap_or_else(|_| Err("The recording engine did not respond.".into()));
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(status) => this.status = status,
                    // Shown where a refusal is shown.
                    Err(error) => this.status.error = Some(error),
                }
                cx.emit(CaptureChanged);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// `save(patch)`: normalised, kept here and persisted.
    pub fn save(&mut self, edit: impl FnOnce(&mut CaptureSettings), cx: &mut Context<Self>) {
        let mut next = self.settings.clone();
        edit(&mut next);
        let raw = serde_json::to_value(&next).unwrap_or_default();
        self.settings = kairo_domain::capture::normalize_capture_settings(&raw);
        self.backend.save(self.settings.clone());
        cx.notify();
    }

    pub fn set_output_enabled(&mut self, id: &str, enabled: bool, cx: &mut Context<Self>) {
        if let Some(output) = self.outputs.iter_mut().find(|o| o.id == id) {
            output.enabled = enabled;
        }
        self.backend.set_output_enabled(id.to_owned(), enabled);
        cx.notify();
    }
}
