//! DeckLink cards through Electron's out-of-process helper
//! (`native/decklink`, copied unchanged), from
//! `providers/decklink/{index,client,paths}.ts`. Enumeration and playback
//! run in the helper, so an SDK failure cannot take the engine down. The
//! engine renders UYVY at the mode raster into the shared frame ring the
//! helper reads on the hardware clock; BGRA down stdin stays as the fallback.

use crate::video_devices::{ErrorListener, OutputSink, Provider};
use kairo_domain::video_devices::{
    DriverState, VideoDriverStatus, VideoPlaybackDiagnostics, VideoPort, VideoProviderStatus,
};
use kairo_engine_proto::ring::RingWriter;
use serde::Deserialize;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// Oldest Desktop Video whose interfaces the helper negotiates.
const MIN_DRIVER: [u64; 3] = [15, 3, 1];
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
/// After stdin closes, the helper has this long to release the port.
const CLEANUP: Duration = Duration::from_secs(4);
const LIST_TIMEOUT: Duration = Duration::from_secs(8);
const VERSION_TIMEOUT: Duration = Duration::from_secs(3);
pub const RUNTIME: &str = "/Library/Frameworks/DeckLinkAPI.framework";
/// `videoDevices:openDriverDownloads` opens this page.
pub const DRIVER_DOWNLOADS: &str = "https://www.blackmagicdesign.com/support";

/// Card outputs are on in every macOS build; `KAIRO_ENABLE_DECKLINK=0`
/// turns them off for diagnosis. No other platform has the provider yet.
pub fn enabled(os: &str, flag: Option<&str>) -> bool {
    os == "macos" && flag != Some("0")
}

/// The helper: bundled in `Contents/Resources/decklink/` when packaged
/// (beside the engine at `Contents/MacOS`), else a local build, else the
/// committed prebuilt helper. The first that exists wins; the provider says
/// "not included in this build" if none does.
pub fn helper_path(exe_dir: &Path, repo: Option<&Path>, exists: impl Fn(&Path) -> bool) -> PathBuf {
    let mut candidates = vec![
        exe_dir.join("../Resources/decklink/decklink-helper"),
        exe_dir.join("../../../../Resources/decklink/decklink-helper"),
    ];
    if let Some(repo) = repo {
        candidates.push(repo.join("native/decklink/build/decklink-helper"));
        candidates.push(repo.join("native/decklink/prebuilt/decklink-helper"));
    }
    candidates
        .iter()
        .find(|p| exists(p))
        .cloned()
        .unwrap_or_else(|| candidates.last().cloned().expect("candidates"))
}

/// True when `version` ("16.4", "15.3.1") is older than 15.3.1;
/// unparseable versions pass.
pub fn driver_too_old(version: Option<&str>) -> bool {
    let Some(version) = version else {
        return false;
    };
    let digits: String = version
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let mut parts = digits.split('.');
    let Some(Ok(major)) = parts.next().map(str::parse::<u64>) else {
        return false;
    };
    let rest: Vec<u64> = parts.take(2).map_while(|p| p.parse::<u64>().ok()).collect();
    let numbers = [
        major,
        rest.first().copied().unwrap_or(0),
        rest.get(1).copied().unwrap_or(0),
    ];
    for (part, min) in numbers.iter().zip(MIN_DRIVER) {
        if *part != min {
            return *part < min;
        }
    }
    false
}

/// The helper's own message (last stderr line), not a command-line wrapper.
fn helper_error(stderr: &str, fallback: String) -> String {
    stderr
        .trim()
        .lines()
        .last()
        .map(str::to_owned)
        .filter(|l| !l.is_empty())
        .unwrap_or(fallback)
}

struct Ran {
    status: Option<i32>,
    stdout: String,
    stderr: String,
}

/// The helper is a console program: on Windows it must not open a terminal.
fn no_window(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    command
}

/// Runs the helper with a deadline; killed if it overruns.
fn run(helper: &Path, args: &[&str], timeout: Duration) -> Result<Ran, String> {
    let mut child = no_window(&mut Command::new(helper))
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("DeckLink helper could not start: {e}"))?;
    let mut stdout = child.stdout.take().expect("piped");
    let mut stderr = child.stderr.take().expect("piped");
    let out = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        text
    });
    let err = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code(),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("DeckLink helper timed out ({})", args.join(" ")));
            }
            Err(e) => return Err(e.to_string()),
        }
    };
    Ok(Ran {
        status,
        stdout: out.join().unwrap_or_default(),
        stderr: err.join().unwrap_or_default(),
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Inventory {
    version: Option<serde_json::Value>,
    compatibility_mode: Option<bool>,
    ports: Option<Vec<VideoPort>>,
}

/// The qualified native boundary: enumeration and playback out of process.
pub struct DeckLinkProvider {
    helper: PathBuf,
    runtime: PathBuf,
    scanned: bool,
    version: Option<String>,
    driver: Option<VideoDriverStatus>,
    ports: Vec<VideoPort>,
    previous: Option<String>,
    rings: AtomicU64,
}

impl DeckLinkProvider {
    pub fn new(helper: PathBuf, runtime: PathBuf) -> Self {
        Self {
            helper,
            runtime,
            scanned: false,
            version: None,
            driver: None,
            ports: Vec::new(),
            previous: None,
            rings: AtomicU64::new(0),
        }
    }

    fn describe_driver(
        &mut self,
        version: &str,
        compatibility: Option<bool>,
    ) -> Option<VideoDriverStatus> {
        self.version = (!version.is_empty()).then(|| version.to_owned());
        self.driver = if driver_too_old(Some(version)) {
            Some(VideoDriverStatus {
                state: DriverState::Unsupported,
                message: format!(
                    "Desktop Video {version} is too old. Update Blackmagic Desktop Video to 15.3.1 or later, then restart the Mac."
                ),
            })
        } else if compatibility.unwrap_or_else(|| version.starts_with("15.")) {
            Some(VideoDriverStatus {
                state: DriverState::Compatibility,
                message: format!(
                    "Desktop Video {} detected. Kairo is using compatibility mode.",
                    if version.is_empty() {
                        "(version unknown)"
                    } else {
                        version
                    }
                ),
            })
        } else if version.is_empty() {
            None
        } else {
            Some(VideoDriverStatus {
                state: DriverState::Current,
                message: format!("Desktop Video {version} detected."),
            })
        };
        self.driver.clone()
    }

    fn ring_name(&self) -> String {
        // `shm_open` names stay under 31 bytes on macOS; the helper takes 30.
        let n = self.rings.fetch_add(1, Ordering::Relaxed);
        format!("/kairo-dl-{}-{n}", std::process::id())
    }
}

impl Provider for DeckLinkProvider {
    fn name(&self) -> &str {
        "decklink"
    }

    fn status(&self) -> VideoProviderStatus {
        if !cfg!(target_os = "macos") {
            return VideoProviderStatus {
                available: false,
                version: None,
                reason: Some("DeckLink is supported on macOS in this build".into()),
                driver: None,
            };
        }
        if !self.runtime.exists() {
            return VideoProviderStatus {
                available: false,
                version: None,
                reason: Some("Install Blackmagic Desktop Video to use DeckLink cards".into()),
                driver: Some(VideoDriverStatus {
                    state: DriverState::Missing,
                    message:
                        "Install Blackmagic Desktop Video to use this card, then restart the Mac."
                            .into(),
                }),
            };
        }
        if !self.helper.exists() {
            return VideoProviderStatus {
                available: false,
                version: None,
                reason: Some("DeckLink native helper is not included in this build".into()),
                driver: None,
            };
        }
        VideoProviderStatus {
            available: true,
            version: self.version.clone(),
            reason: None,
            driver: self.driver.clone(),
        }
    }

    fn list_ports(&mut self) -> Result<Vec<VideoPort>, String> {
        if !self.status().available {
            return Ok(Vec::new());
        }
        self.driver = None;
        self.version = None;
        self.ports = Vec::new();
        let listed = run(&self.helper, &["--list"], LIST_TIMEOUT).and_then(|ran| {
            if ran.status == Some(0) {
                Ok(ran.stdout)
            } else {
                Err(helper_error(
                    &ran.stderr,
                    format!("DeckLink helper failed ({:?})", ran.status),
                ))
            }
        });
        let stdout = match listed {
            Ok(stdout) => stdout,
            Err(error) => {
                // Enumeration may fail before an older driver can answer;
                // version discovery must not depend on those interfaces.
                if let Ok(ran) = run(&self.helper, &["--version"], VERSION_TIMEOUT)
                    && let Ok(info) = serde_json::from_str::<serde_json::Value>(&ran.stdout)
                    && let Some(version) = info.get("version").and_then(|v| v.as_str())
                {
                    self.describe_driver(version, Some(false));
                }
                if let Some(driver) = &self.driver
                    && driver.state == DriverState::Unsupported
                {
                    return Err(driver.message.clone());
                }
                return Err(error);
            }
        };
        let inventory: Inventory = serde_json::from_str(stdout.trim())
            .map_err(|_| "Invalid DeckLink inventory".to_owned())?;
        let Some(ports) = inventory.ports else {
            return Err("Invalid DeckLink inventory".into());
        };
        // Older drivers list cards but answer none of the output interfaces.
        let version = inventory
            .version
            .as_ref()
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned();
        if let Some(driver) = self.describe_driver(&version, inventory.compatibility_mode)
            && driver.state == DriverState::Unsupported
        {
            return Err(driver.message);
        }
        self.scanned = true;
        self.ports = ports
            .into_iter()
            .map(|mut port| {
                if port.unavailable_reason.as_deref().unwrap_or("").is_empty() {
                    port.unavailable_reason = port.modes.is_empty().then(|| {
                        "No output modes available in the current device profile".to_owned()
                    });
                }
                port
            })
            .collect();
        Ok(self.ports.clone())
    }

    fn ports_changed(&mut self) -> bool {
        if !self.scanned || !self.status().available {
            return false;
        }
        let key = match self.list_ports() {
            Ok(ports) => serde_json::to_string(&ports).unwrap_or_default(),
            Err(_) => "unavailable".into(),
        };
        let changed = self.previous.as_ref() != Some(&key);
        self.previous = Some(key);
        changed
    }

    fn open_output(
        &mut self,
        port_id: &str,
        mode_id: &str,
        key_fill: bool,
    ) -> Result<Box<dyn OutputSink>, String> {
        if key_fill {
            return Err("DeckLink key/fill is not qualified in this build".into());
        }
        let port = self
            .ports
            .iter()
            .find(|p| p.id == port_id)
            .ok_or("Device disconnected")?;
        let mode = port
            .modes
            .iter()
            .find(|m| m.id == mode_id)
            .ok_or("Unsupported video mode")?;
        // The GPU path: a shared ring at the mode's raster that the helper
        // reads on the hardware clock. Without it, frames go down stdin.
        let name = self.ring_name();
        let ring = match RingWriter::create(&name, mode.width as u32, mode.height as u32) {
            Ok(ring) => Some(Arc::new(ring)),
            Err(error) => {
                tracing::warn!(%error, "DeckLink frame ring unavailable, using CPU frames");
                None
            }
        };
        let mut args = vec![];
        match &ring {
            Some(_) => args.extend(["--play-shm", port_id, mode_id, &name]),
            None => args.extend(["--play", port_id, mode_id]),
        }
        let playback = Playback::spawn(
            no_window(&mut Command::new(&self.helper)).args(&args),
            STARTUP_TIMEOUT,
        )?;
        let ring = match (ring, playback.ring_status()) {
            (Some(ring), Some(Ok(()))) => Some(ring),
            (Some(_), status) => {
                let reason = match status {
                    Some(Err(reason)) => reason,
                    _ => "no ring response".into(),
                };
                tracing::warn!(
                    reason,
                    "helper did not attach the frame ring, using CPU frames"
                );
                None
            }
            (None, _) => None,
        };
        Ok(Box::new(Card { playback, ring }))
    }
}

/// An open card port: the helper and, when attached, its frame ring. The
/// helper stops reading before the ring is unmapped and unlinked.
struct Card {
    playback: Playback,
    ring: Option<Arc<RingWriter>>,
}

impl OutputSink for Card {
    fn diagnostics(&self) -> Option<VideoPlaybackDiagnostics> {
        self.playback.diagnostics()
    }
    fn frame_ring(&self) -> Option<Arc<RingWriter>> {
        self.ring.clone()
    }
    fn set_frame(&self, bgra: &[u8], width: u32, height: u32) -> Result<(), String> {
        self.playback.set_frame(bgra, width, height)
    }
    fn on_error(&self, listener: ErrorListener) {
        self.playback.on_error(listener)
    }
    fn close(&self) {
        self.playback.close();
    }
}

#[derive(Default)]
struct State {
    started: bool,
    exited: bool,
    closing: bool,
    failure: Option<String>,
    ring: Option<Result<(), String>>,
    diagnostics: Option<VideoPlaybackDiagnostics>,
    stderr: String,
    /// At most one unsent frame: the newest.
    newest: Option<Vec<u8>>,
    /// stdin is closed once this is set (EOF asks the helper to release).
    eof: bool,
}

struct Inner {
    state: Mutex<State>,
    changed: Condvar,
    listeners: Mutex<Vec<ErrorListener>>,
    pid: u32,
}

impl Inner {
    fn fail(&self, error: String) {
        let first = {
            let mut state = self.state.lock().unwrap();
            if state.closing || state.failure.is_some() {
                false
            } else {
                state.failure = Some(error.clone());
                state.newest = None;
                true
            }
        };
        self.changed.notify_all();
        if !first {
            return;
        }
        for listener in self.listeners.lock().unwrap().iter() {
            // Isolate subscribers from one another.
            let _ =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| listener(error.clone())));
        }
        self.terminate();
    }

    /// SIGTERM, then SIGKILL after the cleanup window if still running.
    fn terminate(&self) {
        if self.state.lock().unwrap().exited {
            return;
        }
        signal(self.pid, libc::SIGTERM);
        let state = self.state.lock().unwrap();
        let (state, _) = self
            .changed
            .wait_timeout_while(state, CLEANUP, |s| !s.exited)
            .unwrap();
        if !state.exited {
            signal(self.pid, libc::SIGKILL);
        }
    }
}

fn signal(pid: u32, sig: libc::c_int) {
    // SAFETY: a signal to our own child, which the waiter thread reaps.
    unsafe {
        libc::kill(pid as libc::pid_t, sig);
    }
}

/// The helper in playback mode (`createPlaybackSink`, run on threads instead
/// of a worker). SDK work and frame repeat stay in the helper.
pub struct Playback {
    inner: Arc<Inner>,
}

impl Playback {
    /// Spawns and waits for `started`. Startup failures carry the helper's
    /// own message (busy port, disconnected device).
    pub fn spawn(command: &mut Command, timeout: Duration) -> Result<Self, String> {
        let mut child: Child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("DeckLink helper could not start: {e}"))?;
        let inner = Arc::new(Inner {
            state: Mutex::new(State::default()),
            changed: Condvar::new(),
            listeners: Mutex::new(Vec::new()),
            pid: child.id(),
        });
        let stdin = child.stdin.take().expect("piped");
        let stdout = child.stdout.take().expect("piped");
        let mut stderr = child.stderr.take().expect("piped");
        {
            let inner = inner.clone();
            std::thread::spawn(move || writer(inner, stdin));
        }
        {
            let inner = inner.clone();
            std::thread::spawn(move || {
                let mut buffer = [0u8; 1024];
                while let Ok(n) = stderr.read(&mut buffer) {
                    if n == 0 {
                        break;
                    }
                    let mut state = inner.state.lock().unwrap();
                    state
                        .stderr
                        .push_str(&String::from_utf8_lossy(&buffer[..n]));
                    let len = state.stderr.len();
                    if len > 4096 {
                        let mut cut = len - 4096;
                        while !state.stderr.is_char_boundary(cut) {
                            cut += 1;
                        }
                        state.stderr.drain(..cut);
                    }
                }
            });
        }
        {
            let inner = inner.clone();
            std::thread::spawn(move || reader(inner, stdout));
        }
        {
            let inner = inner.clone();
            std::thread::spawn(move || {
                let status = child.wait();
                // Give stderr a moment to drain into the message.
                std::thread::sleep(Duration::from_millis(20));
                let why = {
                    let mut state = inner.state.lock().unwrap();
                    state.exited = true;
                    let code = match status {
                        Ok(s) => s
                            .code()
                            .map(|c| c.to_string())
                            .or_else(|| {
                                use std::os::unix::process::ExitStatusExt;
                                s.signal().map(|n| format!("signal {n}"))
                            })
                            .unwrap_or_default(),
                        Err(e) => e.to_string(),
                    };
                    let tail = state.stderr.trim().to_owned();
                    if tail.is_empty() {
                        format!("DeckLink helper exited ({code})")
                    } else {
                        tail
                    }
                };
                inner.changed.notify_all();
                inner.fail(why);
            });
        }
        let state = inner.state.lock().unwrap();
        let (state, waited) = inner
            .changed
            .wait_timeout_while(state, timeout, |s| !s.started && s.failure.is_none())
            .unwrap();
        if let Some(failure) = state.failure.clone() {
            drop(state);
            return Err(failure);
        }
        if waited.timed_out() && !state.started {
            drop(state);
            inner.fail("DeckLink helper startup timed out".into());
            return Err("DeckLink helper startup timed out".into());
        }
        drop(state);
        Ok(Self { inner })
    }

    /// `--play-shm` only: whether the helper attached the frame ring.
    pub fn ring_status(&self) -> Option<Result<(), String>> {
        self.inner.state.lock().unwrap().ring.clone()
    }

    pub fn diagnostics(&self) -> Option<VideoPlaybackDiagnostics> {
        self.inner.state.lock().unwrap().diagnostics.clone()
    }

    /// Copies the frame now; keeps at most one unsent frame (the newest).
    pub fn set_frame(&self, bgra: &[u8], width: u32, height: u32) -> Result<(), String> {
        let mut state = self.inner.state.lock().unwrap();
        if let Some(failure) = &state.failure {
            return Err(failure.clone());
        }
        if state.closing || state.exited {
            return Err("DeckLink output closed".into());
        }
        if width < 1
            || height < 1
            || width > 4096
            || height > 2160
            || bgra.len() as u64 != u64::from(width) * u64::from(height) * 4
        {
            return Err("Invalid BGRA frame dimensions or byte length".into());
        }
        let mut packet = Vec::with_capacity(12 + bgra.len());
        packet.extend_from_slice(&width.to_le_bytes());
        packet.extend_from_slice(&height.to_le_bytes());
        packet.extend_from_slice(&(bgra.len() as u32).to_le_bytes());
        packet.extend_from_slice(bgra);
        state.newest = Some(packet);
        drop(state);
        self.inner.changed.notify_all();
        Ok(())
    }

    pub fn on_error(&self, listener: ErrorListener) {
        let failure = self.inner.state.lock().unwrap().failure.clone();
        match failure {
            Some(failure) => listener(failure),
            None => self.inner.listeners.lock().unwrap().push(listener),
        }
    }

    /// EOF lets the helper stop scheduled playback and release the port;
    /// SIGKILL if it has not exited within 4 s. Repeated close is safe.
    pub fn close(&self) {
        {
            let mut state = self.inner.state.lock().unwrap();
            // A frame already handed over still goes out before EOF.
            state.closing = true;
            state.eof = true;
        }
        self.inner.listeners.lock().unwrap().clear();
        self.inner.changed.notify_all();
        let state = self.inner.state.lock().unwrap();
        let (state, _) = self
            .inner
            .changed
            .wait_timeout_while(state, CLEANUP, |s| !s.exited)
            .unwrap();
        if !state.exited {
            drop(state);
            signal(self.inner.pid, libc::SIGKILL);
            let state = self.inner.state.lock().unwrap();
            let _ = self
                .inner
                .changed
                .wait_timeout_while(state, Duration::from_secs(1), |s| !s.exited);
        }
    }
}

impl Drop for Playback {
    fn drop(&mut self) {
        self.close();
    }
}

fn writer(inner: Arc<Inner>, mut stdin: ChildStdin) {
    loop {
        let packet = {
            let state = inner.state.lock().unwrap();
            let mut state = inner
                .changed
                .wait_while(state, |s| {
                    s.newest.is_none() && !s.eof && !s.exited && s.failure.is_none()
                })
                .unwrap();
            if state.exited || state.failure.is_some() || (state.eof && state.newest.is_none()) {
                return; // Drops stdin: EOF.
            }
            state.newest.take()
        };
        if let Some(packet) = packet
            && let Err(error) = stdin.write_all(&packet).and_then(|_| stdin.flush())
        {
            inner.fail(format!("DeckLink helper pipe failed: {error}"));
            return;
        }
    }
}

fn reader(inner: Arc<Inner>, stdout: impl Read) {
    let mut lines = BufReader::new(stdout);
    let mut line = String::new();
    loop {
        line.clear();
        match lines.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        if line.len() > 65536 {
            inner.fail("Invalid DeckLink helper response".into());
            return;
        }
        let Ok(event) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
            inner.fail("Invalid DeckLink helper response".into());
            return;
        };
        let kind = event.get("event").and_then(|e| e.as_str()).unwrap_or("");
        let mut state = inner.state.lock().unwrap();
        match kind {
            // Sent before `started` in --play-shm mode.
            "ring" => {
                state.ring = Some(if event.get("ok") == Some(&serde_json::Value::Bool(true)) {
                    Ok(())
                } else {
                    Err(event
                        .get("reason")
                        .and_then(|r| r.as_str())
                        .unwrap_or("Frame ring unavailable")
                        .to_owned())
                });
            }
            "started" => {
                if state.failure.is_none() {
                    state.started = true;
                }
            }
            "stats" => {
                let number = |key: &str| {
                    event
                        .get(key)
                        .and_then(|v| v.as_f64())
                        .filter(|v| v.is_finite() && *v >= 0.0)
                };
                if let (
                    Some(completed),
                    Some(late),
                    Some(dropped),
                    Some(repeated),
                    Some(received),
                    Some(output_fps),
                ) = (
                    number("completed"),
                    number("late"),
                    number("dropped"),
                    number("repeated"),
                    number("received"),
                    number("outputFps"),
                ) {
                    report_health(state.diagnostics.as_ref(), late, dropped, output_fps);
                    state.diagnostics = Some(VideoPlaybackDiagnostics {
                        completed,
                        late,
                        dropped,
                        repeated,
                        received,
                        output_fps,
                    });
                }
            }
            "error" => {
                let reason = event
                    .get("reason")
                    .and_then(|r| r.as_str())
                    .unwrap_or("DeckLink playback failed")
                    .to_owned();
                drop(state);
                inner.fail(reason);
                continue;
            }
            _ => {}
        }
        drop(state);
        inner.changed.notify_all();
    }
}

/// Logs when the card falls behind (late or dropped frames, or under 20 fps
/// out), at most once every 30 s.
fn report_health(
    previous: Option<&VideoPlaybackDiagnostics>,
    late: f64,
    dropped: f64,
    output_fps: f64,
) {
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    let Some(previous) = previous else {
        return;
    };
    let new_late = late - previous.late;
    let new_dropped = dropped - previous.dropped;
    if new_late > 0.0 || new_dropped > 0.0 || (output_fps > 0.0 && output_fps < 20.0) {
        let mut last = LAST.lock().unwrap();
        if last.is_none_or(|t| t.elapsed() > Duration::from_secs(30)) {
            *last = Some(Instant::now());
            tracing::warn!(
                late = new_late,
                dropped = new_dropped,
                output_fps = output_fps.round(),
                "DeckLink output falling behind"
            );
        }
    }
}
