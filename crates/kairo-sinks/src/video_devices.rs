//! Video card outputs: the device service, from Electron's
//! `src/main/services/video-devices/index.ts`, `provider.ts` and
//! `provider-loader.ts`.
//!
//! The service owns acquisition and lifecycle; providers own conversion and
//! hardware clocks. Work is serialized on the service's own thread, so a slow
//! card driver never holds up the engine's main loop (screens keep drawing
//! while a helper starts). The engine reads state and sinks through a shared
//! snapshot without waiting.

use kairo_domain::video_devices::{
    NamedVideoProvider, VideoDeviceSnapshot, VideoOutputDiagnostics, VideoOutputState,
    VideoPlaybackDiagnostics, VideoPort, VideoProviderStatus,
};
#[cfg(unix)]
pub use kairo_engine_proto::ring::RingWriter;
/// No frame ring off Unix: cards take BGRA only there.
#[cfg(not(unix))]
pub enum RingWriter {}
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Electron's `NO_CARDS`.
pub const NO_CARDS: &str = "No video cards detected. Check the card in Blackmagic Desktop Video Setup and that its driver is allowed in System Settings → Privacy & Security, then refresh.";

/// One video card output as configured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub id: String,
    pub enabled: bool,
    pub port_id: String,
    pub mode_id: String,
    pub key_fill: bool,
}

impl Binding {
    fn same(&self, other: &Binding) -> bool {
        self.port_id == other.port_id
            && self.mode_id == other.mode_id
            && self.key_fill == other.key_fill
    }
}

/// Called with an asynchronous driver or helper failure.
pub type ErrorListener = Box<dyn Fn(String) + Send + Sync>;

/// A native output. Repeated close must be safe.
pub trait OutputSink: Send + Sync {
    fn diagnostics(&self) -> Option<VideoPlaybackDiagnostics>;
    /// The GPU path: UYVY frames at the mode raster that the helper repeats
    /// on the hardware clock. `None`: BGRA through `set_frame` only.
    fn frame_ring(&self) -> Option<Arc<RingWriter>>;
    /// Replaces the frame repeated on the hardware clock: top-down BGRA.
    /// The sink copies it before returning.
    fn set_frame(&self, bgra: &[u8], width: u32, height: u32) -> Result<(), String>;
    /// Calls `listener` on a later failure, or at once if one happened.
    fn on_error(&self, listener: ErrorListener);
    /// Releases scheduled frames and the port. Blocks until released.
    fn close(&self);
}

pub trait Provider: Send {
    fn name(&self) -> &str;
    fn status(&self) -> VideoProviderStatus;
    fn list_ports(&mut self) -> Result<Vec<VideoPort>, String>;
    /// Polled every 2 s: whether the inventory changed since the last poll
    /// (hotplug, profile change). Unscanned providers answer false.
    fn ports_changed(&mut self) -> bool {
        false
    }
    fn open_output(
        &mut self,
        port_id: &str,
        mode_id: &str,
        key_fill: bool,
    ) -> Result<Box<dyn OutputSink>, String>;
}

/// `VideoProviderLoadCode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadCode {
    UnsupportedTuple,
    RuntimeUnavailable,
    LoadFailed,
}

/// A provider registration: a lazy factory for the platforms it supports.
pub struct Registration {
    pub name: &'static str,
    pub tuples: &'static [&'static str],
    pub load: Box<dyn FnOnce() -> Result<Box<dyn Provider>, String>>,
}

/// `loadVideoDeviceProvider`: never panics on a provider that fails to load
/// or reports itself unavailable. `tuple` is `"<os>:<arch>"` in Node's names
/// (`darwin:arm64`).
pub fn load_provider(
    registration: Registration,
    tuple: &str,
) -> Result<Box<dyn Provider>, (String, LoadCode, String)> {
    let name = registration.name.to_owned();
    if !registration.tuples.contains(&tuple) {
        let (os, arch) = tuple.split_once(':').unwrap_or((tuple, ""));
        return Err((
            name,
            LoadCode::UnsupportedTuple,
            format!("Unsupported platform/architecture: {os}/{arch}"),
        ));
    }
    let loaded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(registration.load));
    let provider = match loaded {
        Ok(Ok(provider)) => provider,
        Ok(Err(reason)) => return Err((name, LoadCode::LoadFailed, reason)),
        Err(_) => {
            return Err((
                name,
                LoadCode::LoadFailed,
                "Video device provider failed to load".into(),
            ));
        }
    };
    let status = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| provider.status()));
    match status {
        Ok(status) if status.available => Ok(provider),
        Ok(status) => Err((
            name,
            LoadCode::RuntimeUnavailable,
            status
                .reason
                .filter(|r| !r.is_empty())
                .unwrap_or_else(|| "Video device runtime unavailable".into()),
        )),
        Err(_) => Err((
            name,
            LoadCode::LoadFailed,
            "Video device provider failed to load".into(),
        )),
    }
}

/// The node Node's `process.platform:process.arch` would name this build.
pub fn this_tuple() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => other,
    };
    format!("{os}:{arch}")
}

/// The text of a caught panic, as Electron kept a thrown error's message.
fn panic_text(payload: Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "Video device operation failed".into())
}

/// A sink held for one output, with capture counters for the snapshot.
pub struct Managed {
    native: Box<dyn OutputSink>,
    binding: Binding,
    active: AtomicBool,
    closed: AtomicBool,
    captures: AtomicU64,
    sampled: Mutex<(u64, Instant, f64)>,
    failure: Mutex<Option<String>>,
    pub frame_rate: Option<f64>,
    pub raster: (u32, u32),
    fault: mpsc::Sender<Msg>,
}

impl Managed {
    /// A copy of each valid frame goes to the card; a delivery failure
    /// faults the output (retried only on refresh).
    pub fn set_frame(&self, bgra: &[u8], width: u32, height: u32) -> Result<(), String> {
        if !self.active.load(Ordering::SeqCst) {
            return Ok(());
        }
        if width == 0
            || height == 0
            || bgra.len() as u64 != u64::from(width) * u64::from(height) * 4
        {
            return Err("Invalid BGRA frame dimensions or byte length".into());
        }
        self.captures.fetch_add(1, Ordering::Relaxed);
        if let Err(error) = self.native.set_frame(bgra, width, height) {
            self.fail(error);
        }
        Ok(())
    }

    /// Counts a frame that went to the ring instead.
    pub fn count_capture(&self) {
        if self.active.load(Ordering::SeqCst) {
            self.captures.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn frame_ring(&self) -> Option<Arc<RingWriter>> {
        self.native.frame_ring()
    }

    pub fn active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    fn fail(&self, reason: String) {
        if self.active.swap(false, Ordering::SeqCst) {
            *self.failure.lock().unwrap() = Some(reason.clone());
            let _ = self.fault.send(Msg::Fault {
                binding: self.binding.clone(),
                reason,
            });
        }
    }

    /// Closes the output: the port is released once and the output stops
    /// being ready ("Output closed"). Repeated close is safe.
    pub fn close(&self) {
        if self.active.swap(false, Ordering::SeqCst) {
            let _ = self.fault.send(Msg::Closed(self.binding.id.clone()));
        }
        self.release();
    }

    fn release(&self) {
        self.active.store(false, Ordering::SeqCst);
        if !self.closed.swap(true, Ordering::SeqCst) {
            self.native.close();
        }
    }

    fn diagnostics(&self) -> VideoOutputDiagnostics {
        let captures = self.captures.load(Ordering::Relaxed);
        let mut sampled = self.sampled.lock().unwrap();
        let elapsed = sampled.1.elapsed();
        if elapsed >= Duration::from_millis(500) {
            sampled.2 = (captures - sampled.0) as f64 / elapsed.as_secs_f64();
            sampled.0 = captures;
            sampled.1 = Instant::now();
        }
        VideoOutputDiagnostics {
            captured_frames: captures as f64,
            capture_fps: sampled.2,
            hardware: self.native.diagnostics(),
        }
    }
}

#[derive(Default)]
struct Shared {
    ports: Vec<VideoPort>,
    providers: Vec<NamedVideoProvider>,
    states: BTreeMap<String, VideoOutputState>,
    entries: HashMap<String, Arc<Managed>>,
    acquiring: usize,
}

enum Msg {
    Reconcile(Vec<Binding>),
    Refresh,
    Fault {
        binding: Binding,
        reason: String,
    },
    /// A sink closed by its holder.
    Closed(String),
    /// Answers once everything queued before it is done.
    Barrier(mpsc::Sender<()>),
    Close(mpsc::Sender<()>),
}

/// The device service. Cheap to share; all work runs on its own thread.
pub struct VideoDevices {
    tx: mpsc::Sender<Msg>,
    shared: Arc<Mutex<Shared>>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

/// Inventory polling cadence (`onPortsChanged`).
const POLL: Duration = Duration::from_secs(2);

impl VideoDevices {
    /// Starts the service. `changed` runs on the service thread after any
    /// pass that changed ports, providers or output states.
    pub fn start(
        providers: Vec<Box<dyn Provider>>,
        debounce: Duration,
        changed: impl Fn() + Send + 'static,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let shared = Arc::new(Mutex::new(Shared::default()));
        let mut core = Core {
            providers,
            bindings: Vec::new(),
            faults: HashMap::new(),
            scanned: false,
            closed: false,
            shared: shared.clone(),
            tx: tx.clone(),
        };
        let thread = std::thread::Builder::new()
            .name("video-devices".into())
            .spawn(move || core.run(rx, debounce, changed))
            .expect("video device thread");
        Self {
            tx,
            shared,
            thread: Mutex::new(Some(thread)),
        }
    }

    /// Queues a reconcile against `bindings` (all card outputs, enabled or not).
    pub fn reconcile(&self, bindings: Vec<Binding>) {
        let _ = self.tx.send(Msg::Reconcile(bindings));
    }

    /// Forgets faults and re-enumerates on the next pass.
    pub fn refresh(&self) {
        let _ = self.tx.send(Msg::Refresh);
    }

    /// Waits until everything queued so far has run.
    pub fn settle(&self) {
        let (reply, done) = mpsc::channel();
        if self.tx.send(Msg::Barrier(reply)).is_ok() {
            let _ = done.recv();
        }
    }

    pub fn snapshot(&self) -> VideoDeviceSnapshot {
        let shared = self.shared.lock().unwrap();
        let mut outputs = shared.states.clone();
        for (id, entry) in &shared.entries {
            let state = outputs.entry(id.clone()).or_insert(VideoOutputState {
                ready: false,
                reason: String::new(),
                test_pattern: None,
                diagnostics: None,
            });
            state.diagnostics = Some(entry.diagnostics());
        }
        VideoDeviceSnapshot {
            ports: shared.ports.clone(),
            providers: shared.providers.clone(),
            outputs,
        }
    }

    pub fn output_status(&self, id: &str) -> VideoOutputState {
        let shared = self.shared.lock().unwrap();
        shared.states.get(id).cloned().unwrap_or(VideoOutputState {
            ready: false,
            reason: "Output unavailable".into(),
            test_pattern: None,
            diagnostics: None,
        })
    }

    pub fn sink_for(&self, id: &str) -> Option<Arc<Managed>> {
        self.shared.lock().unwrap().entries.get(id).cloned()
    }

    /// True while hardware is held or an acquisition is in flight: quit must
    /// release it first.
    pub fn holds_hardware(&self) -> bool {
        let shared = self.shared.lock().unwrap();
        !shared.entries.is_empty() || shared.acquiring > 0
    }

    /// Releases every output; true when that finished within `timeout`
    /// (quit's 2 s bound). No further outputs open afterwards.
    pub fn close(&self, timeout: Duration) -> bool {
        let (reply, done) = mpsc::channel();
        let _ = self.tx.send(Msg::Close(reply));
        let released = done.recv_timeout(timeout).is_ok();
        if released && let Some(thread) = self.thread.lock().unwrap().take() {
            let _ = thread.join();
        }
        released
    }
}

impl Drop for VideoDevices {
    fn drop(&mut self) {
        self.close(Duration::from_secs(2));
    }
}

struct Core {
    providers: Vec<Box<dyn Provider>>,
    bindings: Vec<Binding>,
    faults: HashMap<String, (Binding, String)>,
    /// Ports are re-enumerated only when this is false: refresh, hotplug,
    /// or an acquisition that needs fresh state.
    scanned: bool,
    closed: bool,
    shared: Arc<Mutex<Shared>>,
    tx: mpsc::Sender<Msg>,
}

impl Core {
    fn run(&mut self, rx: mpsc::Receiver<Msg>, debounce: Duration, changed: impl Fn()) {
        let mut next_poll = Instant::now() + POLL;
        let mut refresh_at: Option<Instant> = None;
        loop {
            let wake = refresh_at.map_or(next_poll, |r| r.min(next_poll));
            match rx.recv_timeout(wake.saturating_duration_since(Instant::now())) {
                Ok(msg) => {
                    let before = self.key();
                    match msg {
                        Msg::Reconcile(bindings) => {
                            if !self.closed {
                                self.bindings = bindings;
                                self.reconcile();
                            }
                        }
                        Msg::Refresh => {
                            if !self.closed {
                                self.faults.clear();
                                self.scanned = false;
                                self.reconcile();
                            }
                        }
                        Msg::Fault { binding, reason } => {
                            // A newer acquisition of the output is not this fault's.
                            let stale = self
                                .shared
                                .lock()
                                .unwrap()
                                .entries
                                .get(&binding.id)
                                .is_some_and(|e| e.active());
                            if !self.closed && !stale {
                                self.state(&binding.id, false, &reason);
                                self.faults
                                    .insert(binding.id.clone(), (binding.clone(), reason));
                                self.drop_entry(&binding.id);
                            }
                        }
                        Msg::Closed(id) => {
                            let mut shared = self.shared.lock().unwrap();
                            if shared.entries.get(&id).is_some_and(|e| !e.active()) {
                                shared.entries.remove(&id);
                                shared.states.insert(
                                    id,
                                    VideoOutputState {
                                        ready: false,
                                        reason: "Output closed".into(),
                                        test_pattern: None,
                                        diagnostics: None,
                                    },
                                );
                            }
                        }
                        Msg::Barrier(reply) => {
                            let _ = reply.send(());
                        }
                        Msg::Close(reply) => {
                            self.closed = true;
                            let ids: Vec<String> = self
                                .shared
                                .lock()
                                .unwrap()
                                .entries
                                .keys()
                                .cloned()
                                .collect();
                            for id in ids {
                                self.drop_entry(&id);
                            }
                            self.shared.lock().unwrap().states.clear();
                            let _ = reply.send(());
                            return;
                        }
                    }
                    if !self.closed && before != self.key() {
                        changed();
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    let now = Instant::now();
                    if refresh_at.is_some_and(|at| at <= now) {
                        refresh_at = None;
                        let _ = self.tx.send(Msg::Refresh);
                    }
                    if next_poll <= now {
                        next_poll = now + POLL;
                        // Hotplug and profile changes, debounced into one refresh.
                        let mut any = false;
                        for provider in &mut self.providers {
                            let polled =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    provider.ports_changed()
                                }));
                            any |= polled.unwrap_or(false);
                        }
                        if any && !self.closed {
                            refresh_at = Some(now + debounce);
                        }
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    /// Change detection: ports, providers and states.
    fn key(&self) -> String {
        let shared = self.shared.lock().unwrap();
        format!(
            "{:?}{:?}{:?}",
            shared.ports, shared.providers, shared.states
        )
    }

    fn state(&self, id: &str, ready: bool, reason: &str) {
        self.shared.lock().unwrap().states.insert(
            id.to_owned(),
            VideoOutputState {
                ready,
                reason: reason.to_owned(),
                test_pattern: None,
                diagnostics: None,
            },
        );
    }

    fn drop_entry(&self, id: &str) {
        let entry = self.shared.lock().unwrap().entries.remove(id);
        if let Some(entry) = entry {
            entry.release();
        }
    }

    fn enabled(&self, id: &str) -> Option<&Binding> {
        self.bindings.iter().find(|b| b.id == id && b.enabled)
    }

    /// Whether this pass must enumerate hardware. Steady-state reconciles
    /// reuse the cached ports; only a binding about to acquire a port it can
    /// see gets fresh busy and mode state.
    fn needs_scan(&self) -> bool {
        if !self.scanned {
            return true;
        }
        let shared = self.shared.lock().unwrap();
        self.bindings.iter().any(|binding| {
            if !binding.enabled
                || binding.port_id.is_empty()
                || self.faults.contains_key(&binding.id)
            {
                return false;
            }
            if shared
                .entries
                .get(&binding.id)
                .is_some_and(|e| e.binding.same(binding))
            {
                return false;
            }
            shared.ports.iter().any(|p| p.id == binding.port_id)
        })
    }

    fn scan(&mut self) {
        let mut ports = Vec::new();
        let mut states = Vec::new();
        for provider in &mut self.providers {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let status = provider.status();
                let listed = if status.available {
                    provider.list_ports()?
                } else {
                    Vec::new()
                };
                Ok::<_, String>((provider.status(), listed))
            }));
            let name = provider.name().to_owned();
            match result {
                Ok(Ok((status, listed))) => {
                    states.push(NamedVideoProvider {
                        name: name.clone(),
                        status,
                    });
                    ports.extend(listed.into_iter().filter(|p| p.provider == name));
                }
                Ok(Err(reason)) => {
                    // A broken provider must not prevent other scans.
                    let diagnostic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        provider.status()
                    }))
                    .unwrap_or(VideoProviderStatus {
                        available: false,
                        version: None,
                        reason: None,
                        driver: None,
                    });
                    states.push(NamedVideoProvider {
                        name,
                        status: VideoProviderStatus {
                            available: false,
                            reason: Some(reason),
                            ..diagnostic
                        },
                    });
                }
                Err(panic) => states.push(NamedVideoProvider {
                    name,
                    status: VideoProviderStatus {
                        available: false,
                        version: None,
                        reason: Some(panic_text(panic)),
                        driver: None,
                    },
                }),
            }
        }
        let mut shared = self.shared.lock().unwrap();
        shared.ports = ports;
        shared.providers = states;
        self.scanned = true;
    }

    /// Why this binding cannot hold its port now, or `None` when it can.
    fn problem(
        &self,
        binding: &Binding,
        port: Option<&VideoPort>,
        owners: &HashMap<String, String>,
    ) -> Option<String> {
        if let Some((_, reason)) = self.faults.get(&binding.id) {
            return Some(reason.clone());
        }
        let shared = self.shared.lock().unwrap();
        if binding.port_id.is_empty() {
            // The driver answered but listed nothing: picking a port is not the problem.
            return Some(
                if shared.ports.is_empty() && shared.providers.iter().any(|p| p.status.available) {
                    NO_CARDS.into()
                } else {
                    "Choose a video port".into()
                },
            );
        }
        let Some(port) = port else {
            let failed = shared.providers.iter().find(|p| {
                !p.status.available && binding.port_id.starts_with(&format!("{}:", p.name))
            });
            if let Some(reason) = failed.and_then(|p| p.status.reason.clone()) {
                return Some(reason);
            }
            return Some(if self.providers.is_empty() {
                "No video device provider available".into()
            } else {
                "Device disconnected".into()
            });
        };
        let owner = owners.get(&port.id);
        if owner.is_some_and(|o| o != &binding.id) {
            return Some("Port assigned to another output".into());
        }
        // A provider may report a port Kairo itself holds as busy: that is this output.
        if let Some(reason) = &port.unavailable_reason
            && owner.is_none()
        {
            return Some(reason.clone());
        }
        if !port.modes.iter().any(|m| m.id == binding.mode_id) {
            return Some("Choose a supported video mode".into());
        }
        if binding.key_fill && !port.supports_key_fill {
            return Some("Key/fill unavailable for this port".into());
        }
        None
    }

    fn reconcile(&mut self) {
        if self.needs_scan() {
            self.scan();
        }
        // Release all changed owners first, so swaps never collide with old bindings.
        let held: Vec<(String, Binding)> = self
            .shared
            .lock()
            .unwrap()
            .entries
            .iter()
            .map(|(id, e)| (id.clone(), e.binding.clone()))
            .collect();
        for (id, binding) in &held {
            if !self.enabled(id).is_some_and(|next| binding.same(next)) {
                self.drop_entry(id);
            }
        }
        let stale: Vec<String> = self
            .faults
            .iter()
            .filter(|(id, (binding, _))| !self.enabled(id).is_some_and(|next| binding.same(next)))
            .map(|(id, _)| id.clone())
            .collect();
        for id in stale {
            self.faults.remove(&id);
        }
        self.shared.lock().unwrap().states.clear();
        // Outputs that kept their binding keep their port; only newcomers can conflict.
        let mut owners: HashMap<String, String> = self
            .shared
            .lock()
            .unwrap()
            .entries
            .iter()
            .map(|(id, e)| (e.binding.port_id.clone(), id.clone()))
            .collect();
        for binding in self.bindings.clone() {
            if !binding.enabled {
                self.state(&binding.id, false, "Output disabled");
                continue;
            }
            let port = self
                .shared
                .lock()
                .unwrap()
                .ports
                .iter()
                .find(|p| p.id == binding.port_id)
                .cloned();
            let problem = self.problem(&binding, port.as_ref(), &owners);
            if problem.is_some() || self.closed {
                if let Some(port) = &port
                    && owners.get(&port.id) == Some(&binding.id)
                {
                    owners.remove(&port.id);
                }
                self.drop_entry(&binding.id);
                self.state(
                    &binding.id,
                    false,
                    problem.as_deref().unwrap_or("Service closed"),
                );
                continue;
            }
            let port = port.expect("checked by problem");
            owners.insert(port.id.clone(), binding.id.clone());
            let held = self
                .shared
                .lock()
                .unwrap()
                .entries
                .contains_key(&binding.id);
            if !held && let Err(reason) = self.acquire(&binding, &port) {
                owners.remove(&port.id);
                self.state(&binding.id, false, &reason);
                continue;
            }
            // A sink can report an already-observed failure while subscribing.
            if let Some((_, reason)) = self.faults.get(&binding.id) {
                let reason = reason.clone();
                self.state(&binding.id, false, &reason);
                continue;
            }
            self.state(&binding.id, true, "");
        }
    }

    fn acquire(&mut self, binding: &Binding, port: &VideoPort) -> Result<(), String> {
        let Some(provider) = self
            .providers
            .iter_mut()
            .find(|p| p.name() == port.provider)
        else {
            return Err("No video device provider available".into());
        };
        self.shared.lock().unwrap().acquiring += 1;
        let opened = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            provider.open_output(&port.id, &binding.mode_id, binding.key_fill)
        }));
        self.shared.lock().unwrap().acquiring -= 1;
        let native = match opened {
            Ok(result) => result?,
            Err(panic) => return Err(panic_text(panic)),
        };
        if self.closed {
            native.close();
            return Err("Service closed".into());
        }
        let mode = port
            .modes
            .iter()
            .find(|m| m.id == binding.mode_id)
            .expect("mode checked");
        let rate = mode.frame_rate_n / mode.frame_rate_d;
        let managed = Arc::new(Managed {
            native,
            binding: binding.clone(),
            active: AtomicBool::new(true),
            closed: AtomicBool::new(false),
            captures: AtomicU64::new(0),
            sampled: Mutex::new((0, Instant::now(), 0.0)),
            failure: Mutex::new(None),
            frame_rate: (rate.is_finite() && rate > 0.0).then_some(rate),
            raster: (mode.width as u32, mode.height as u32),
            fault: self.tx.clone(),
        });
        self.shared
            .lock()
            .unwrap()
            .entries
            .insert(binding.id.clone(), managed.clone());
        let weak = Arc::downgrade(&managed);
        let subscribed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            managed.native.on_error(Box::new(move |reason| {
                if let Some(managed) = weak.upgrade() {
                    managed.fail(reason);
                }
            }))
        }));
        if let Err(panic) = subscribed {
            self.drop_entry(&binding.id);
            return Err(panic_text(panic));
        }
        // An already-failed sink reported through the subscription: handle it now.
        if !managed.active() {
            let reason = managed
                .failure
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| "Video device operation failed".into());
            self.faults
                .insert(binding.id.clone(), (binding.clone(), reason.clone()));
            self.drop_entry(&binding.id);
            return Err(reason);
        }
        Ok(())
    }
}
