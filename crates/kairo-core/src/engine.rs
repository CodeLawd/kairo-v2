//! The client and supervisor of `kairo-engine` (architecture section 2).
//!
//! [`EngineHandle`] keeps the intended state of every surface, answers each
//! push from it at once, and forwards accepted commands to the engine. A
//! supervisor thread launches the engine, checks it with heartbeats, and
//! relaunches it after a crash or hang with every surface's state replayed,
//! within the failure budget of `ManagedWindow.allowFailure`: three
//! unplanned relaunches per rolling minute, then it stops trying.

use interprocess::local_socket::{GenericNamespaced, Stream, ToNsName, prelude::*};
use kairo_engine_proto::*;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command as Process, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tokio::sync::{Notify, broadcast, oneshot, watch};

/// Automatic relaunches after a crash or hang allowed per `FAILURE_WINDOW`.
pub const MAX_FAILURES: usize = 3;
pub const FAILURE_WINDOW: Duration = Duration::from_secs(60);
/// Longest a page turn waits for its own frame (`PAINT_WAIT_MS`).
pub const PAINT_WAIT: Duration = Duration::from_millis(400);
/// Longest a query waits for the engine's answer.
pub const QUERY_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug)]
pub struct EngineOptions {
    pub binary: PathBuf,
    /// How long the engine holds its picture once this process is gone.
    pub grace: Duration,
    pub heartbeat_every: Duration,
    /// No answer to heartbeats for this long counts as a hang.
    pub heartbeat_timeout: Duration,
    /// From launch to a completed handshake.
    pub connect_timeout: Duration,
}

impl EngineOptions {
    pub fn new(binary: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
            grace: Duration::from_secs(10),
            heartbeat_every: Duration::from_secs(1),
            heartbeat_timeout: Duration::from_secs(3),
            connect_timeout: Duration::from_secs(5),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineStatus {
    Starting,
    Running {
        pid: u32,
    },
    /// Between a failure and the next launch.
    Restarting {
        reason: String,
    },
    /// The failure budget is spent; no further relaunches.
    Failed {
        reason: String,
    },
    Stopped,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EngineEvent {
    /// The engine is up and every surface's state has been replayed into it.
    Launched {
        pid: u32,
        relaunch: bool,
    },
    /// The engine went away. `planned` relaunches never count as failures.
    Lost {
        reason: String,
        planned: bool,
    },
    Engine(Event),
}

/// The result of [`EngineHandle::command`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sent {
    /// What `ProgramSurface` would have returned. A refused command is not sent.
    pub accepted: bool,
    pub seq: u64,
}

/// `allowFailure`.
#[derive(Default, Debug)]
pub struct FailureBudget {
    failures: Vec<Instant>,
}

impl FailureBudget {
    /// Records a failure at `now`. True while under the budget.
    pub fn allow(&mut self, now: Instant) -> bool {
        self.failures
            .retain(|at| now.saturating_duration_since(*at) < FAILURE_WINDOW);
        if self.failures.len() >= MAX_FAILURES {
            return false;
        }
        self.failures.push(now);
        true
    }
}

struct ClientSurface {
    state: SurfaceState,
    /// What the engine last said about the sink.
    status: Option<SurfaceStatus>,
}

#[derive(Default)]
struct Mirror {
    surfaces: Vec<ClientSurface>,
    next_seq: u64,
    displays: Vec<DisplayInfo>,
    /// The commands' wire, while an engine is connected.
    link: Option<mpsc::Sender<ToEngine>>,
}

impl Mirror {
    fn surface(&self, output: &str) -> Option<&ClientSurface> {
        self.surfaces.iter().find(|s| s.state.spec.output == output)
    }

    fn send(&mut self, message: ToEngine) {
        if let Some(link) = &self.link
            && link.send(message).is_err()
        {
            self.link = None;
        }
    }
}

struct Inner {
    options: EngineOptions,
    mirror: Mutex<Mirror>,
    status: watch::Sender<EngineStatus>,
    events: broadcast::Sender<EngineEvent>,
    pending: Mutex<HashMap<u64, oneshot::Sender<Reply>>>,
    pings: Mutex<HashMap<u64, oneshot::Sender<()>>>,
    committed: Mutex<HashMap<String, u64>>,
    frame: Notify,
    next_id: AtomicU64,
    last_pong: Mutex<Instant>,
    stopping: AtomicBool,
    restart: AtomicBool,
    child: Mutex<Option<u32>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panic while holding one of these leaves plain data; keep going.
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The engine, from `kairo`'s side. Cheap to clone.
#[derive(Clone)]
pub struct EngineHandle {
    inner: Arc<Inner>,
    supervisor: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl EngineHandle {
    /// Starts supervising a new engine process.
    pub fn spawn(options: EngineOptions) -> Self {
        let (status, _) = watch::channel(EngineStatus::Starting);
        let (events, _) = broadcast::channel(256);
        let inner = Arc::new(Inner {
            options,
            mirror: Mutex::new(Mirror::default()),
            status,
            events,
            pending: Mutex::new(HashMap::new()),
            pings: Mutex::new(HashMap::new()),
            committed: Mutex::new(HashMap::new()),
            frame: Notify::new(),
            next_id: AtomicU64::new(1),
            last_pong: Mutex::new(Instant::now()),
            stopping: AtomicBool::new(false),
            restart: AtomicBool::new(false),
            child: Mutex::new(None),
        });
        let thread_inner = inner.clone();
        let supervisor = std::thread::Builder::new()
            .name("engine-supervisor".into())
            .spawn(move || supervise(thread_inner))
            .expect("spawn the engine supervisor");
        Self {
            inner,
            supervisor: Arc::new(Mutex::new(Some(supervisor))),
        }
    }

    pub fn status(&self) -> EngineStatus {
        self.inner.status.borrow().clone()
    }

    pub fn watch_status(&self) -> watch::Receiver<EngineStatus> {
        self.inner.status.subscribe()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.inner.events.subscribe()
    }

    /// The engine's process id while it runs.
    pub fn pid(&self) -> Option<u32> {
        *lock(&self.inner.child)
    }

    fn failed(&self) -> bool {
        matches!(
            *self.inner.status.borrow(),
            EngineStatus::Failed { .. } | EngineStatus::Stopped
        )
    }

    /// Applies `command` to the intended state and, if accepted, sends it.
    /// While the engine is relaunching, accepted commands reach it through
    /// the replay instead.
    pub fn command(&self, command: Command) -> Sent {
        let failed = self.failed();
        let mut mirror = lock(&self.inner.mirror);
        mirror.next_seq += 1;
        let seq = mirror.next_seq;
        let accepted = match &command {
            Command::Shutdown => true,
            Command::RetainSurfaces { outputs } => {
                mirror
                    .surfaces
                    .retain(|s| outputs.contains(&s.state.spec.output));
                true
            }
            Command::OpenSurface(spec) => {
                match mirror
                    .surfaces
                    .iter_mut()
                    .find(|s| s.state.spec.output == spec.output)
                {
                    Some(surface) => {
                        surface.state.apply(&command, true);
                    }
                    None => mirror.surfaces.push(ClientSurface {
                        state: SurfaceState::new(spec.clone()),
                        status: None,
                    }),
                }
                true
            }
            Command::CloseSurface {
                output,
                destroy: true,
            } => {
                mirror.surfaces.retain(|s| &s.state.spec.output != output);
                true
            }
            _ => {
                let output = command.output().expect("surface command");
                match mirror
                    .surfaces
                    .iter_mut()
                    .find(|s| s.state.spec.output == output)
                {
                    Some(surface) => {
                        let available = !failed && sink_available(surface.status.as_ref());
                        surface.state.apply(&command, available)
                    }
                    None => false,
                }
            }
        };
        if accepted {
            mirror.send(ToEngine::Command { seq, command });
        }
        Sent { accepted, seq }
    }

    /// Whether `output` has a surface that takes pushes now.
    pub fn is_available(&self, output: &str) -> bool {
        if self.failed() {
            return false;
        }
        let mirror = lock(&self.inner.mirror);
        mirror
            .surface(output)
            .is_some_and(|s| !s.state.parked && sink_available(s.status.as_ref()))
    }

    /// Why `output` cannot take a push, or `None` when it can.
    pub fn unavailable_reason(&self, output: &str) -> Option<String> {
        match &*self.inner.status.borrow() {
            EngineStatus::Failed { .. } => return Some("Output engine stopped".into()),
            EngineStatus::Stopped => return Some("Output engine is not running".into()),
            _ => {}
        }
        let mirror = lock(&self.inner.mirror);
        let Some(surface) = mirror.surface(output) else {
            return Some("Display not connected".into());
        };
        if surface.state.parked {
            return Some("Display not connected".into());
        }
        match &surface.status {
            Some(SurfaceStatus::Parked { reason } | SurfaceStatus::Unavailable { reason }) => {
                Some(reason.clone())
            }
            Some(SurfaceStatus::Ready) | None => None,
        }
    }

    pub fn surface_state(&self, output: &str) -> Option<SurfaceState> {
        lock(&self.inner.mirror)
            .surface(output)
            .map(|s| s.state.clone())
    }

    pub fn surface_status(&self, output: &str) -> Option<SurfaceStatus> {
        lock(&self.inner.mirror)
            .surface(output)
            .and_then(|s| s.status.clone())
    }

    /// Every surface id, in the order they were opened.
    pub fn surfaces(&self) -> Vec<String> {
        lock(&self.inner.mirror)
            .surfaces
            .iter()
            .map(|s| s.state.spec.output.clone())
            .collect()
    }

    pub fn displays(&self) -> Vec<DisplayInfo> {
        lock(&self.inner.mirror).displays.clone()
    }

    /// Asks the engine; `None` when it is not running or does not answer.
    pub async fn query(&self, query: Query) -> Option<Reply> {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        lock(&self.inner.pending).insert(id, tx);
        {
            let mut mirror = lock(&self.inner.mirror);
            if mirror.link.is_none() {
                lock(&self.inner.pending).remove(&id);
                return None;
            }
            mirror.send(ToEngine::Query { id, query });
        }
        let reply = tokio::time::timeout(QUERY_TIMEOUT, rx).await.ok()?.ok();
        lock(&self.inner.pending).remove(&id);
        reply
    }

    /// One heartbeat round trip through the engine's main loop.
    pub async fn ping(&self) -> Option<Duration> {
        let nonce = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        lock(&self.inner.pings).insert(nonce, tx);
        let start = Instant::now();
        {
            let mut mirror = lock(&self.inner.mirror);
            if mirror.link.is_none() {
                lock(&self.inner.pings).remove(&nonce);
                return None;
            }
            mirror.send(ToEngine::Ping { nonce });
        }
        let answered = tokio::time::timeout(QUERY_TIMEOUT, rx).await;
        lock(&self.inner.pings).remove(&nonce);
        matches!(answered, Ok(Ok(()))).then(|| start.elapsed())
    }

    /// Waits until `output` has shown a frame carrying command `seq`, or
    /// `timeout` passes (`nextPaint`). True when the frame arrived.
    pub async fn wait_frame(&self, output: &str, seq: u64, timeout: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let notified = self.inner.frame.notified();
            if lock(&self.inner.committed)
                .get(output)
                .is_some_and(|committed| *committed >= seq)
            {
                return true;
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return false;
            }
        }
    }

    /// A planned relaunch: it replays state and never counts as a failure.
    pub fn restart(&self) {
        self.inner.restart.store(true, Ordering::SeqCst);
    }

    /// Closes every output and stops the engine. Blocks until it has exited.
    pub fn shutdown(&self) {
        self.inner.stopping.store(true, Ordering::SeqCst);
        if let Some(supervisor) = lock(&self.supervisor).take() {
            let _ = supervisor.join();
        }
    }
}

fn sink_available(status: Option<&SurfaceStatus>) -> bool {
    matches!(status, None | Some(SurfaceStatus::Ready))
}

// ─── Supervision ─────────────────────────────────────────────────────────────

enum Exit {
    /// Stop requested; do not relaunch.
    Stopped,
    /// `restart()`: relaunch without spending the budget.
    Planned,
    Failure(String),
}

fn supervise(inner: Arc<Inner>) {
    let mut budget = FailureBudget::default();
    let mut launches = 0u32;
    loop {
        if inner.stopping.load(Ordering::SeqCst) {
            break;
        }
        let exit = match launch(&inner) {
            Ok(mut running) => {
                launches += 1;
                let pid = running.child.id();
                inner.status.send_replace(EngineStatus::Running { pid });
                let _ = inner.events.send(EngineEvent::Launched {
                    pid,
                    relaunch: launches > 1,
                });
                tracing::info!(pid, relaunch = launches > 1, "output engine running");
                let exit = run(&inner, &mut running);
                running.finish(&inner);
                exit
            }
            Err(reason) => Exit::Failure(reason),
        };
        match exit {
            Exit::Stopped => break,
            Exit::Planned => {
                tracing::info!("output engine restarting (planned)");
                let _ = inner.events.send(EngineEvent::Lost {
                    reason: "restart".into(),
                    planned: true,
                });
                inner.status.send_replace(EngineStatus::Restarting {
                    reason: "restart".into(),
                });
            }
            Exit::Failure(reason) => {
                tracing::error!("output engine failed: {reason}");
                let _ = inner.events.send(EngineEvent::Lost {
                    reason: reason.clone(),
                    planned: false,
                });
                if !budget.allow(Instant::now()) {
                    tracing::error!(
                        "output engine keeps failing — not relaunching again ({reason})"
                    );
                    inner.status.send_replace(EngineStatus::Failed { reason });
                    return;
                }
                inner
                    .status
                    .send_replace(EngineStatus::Restarting { reason });
            }
        }
    }
    inner.status.send_replace(EngineStatus::Stopped);
}

struct Running {
    child: Child,
    reader: JoinHandle<()>,
    reader_done: Arc<AtomicBool>,
}

impl Running {
    /// Drops the link and fails everything waiting on this engine.
    fn finish(mut self, inner: &Inner) {
        lock(&inner.mirror).link = None;
        *lock(&inner.child) = None;
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = self.reader.join();
        lock(&inner.pending).clear();
        lock(&inner.pings).clear();
    }
}

fn random_hex() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn launch(inner: &Arc<Inner>) -> Result<Running, String> {
    let options = &inner.options;
    let socket = format!("kairo-engine-{}.sock", random_hex());
    let token = format!("{}{}", random_hex(), random_hex());
    let mut child = Process::new(&options.binary)
        .args(["--socket", &socket, "--grace-ms"])
        .arg(options.grace.as_millis().to_string())
        .env(TOKEN_ENV, &token)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot start {}: {e}", options.binary.display()))?;
    if let Some(stderr) = child.stderr.take() {
        let pid = child.id();
        let _ = std::thread::Builder::new()
            .name("engine-log".into())
            .spawn(move || forward_log(pid, stderr));
    }
    match connect(inner, &mut child, &socket, &token) {
        Ok((reader, reader_done)) => Ok(Running {
            child,
            reader,
            reader_done,
        }),
        Err(reason) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(reason)
        }
    }
}

/// Writes the engine's stderr into this process's log at the same level.
fn forward_log(pid: u32, stderr: std::process::ChildStderr) {
    for line in BufReader::new(stderr).lines() {
        let Ok(line) = line else { break };
        let line = line.trim_start();
        let (level, message) = line.split_once(' ').unwrap_or(("INFO", line));
        let message = message.trim_start();
        match level {
            "ERROR" => tracing::error!(target: "kairo_engine", pid, "{message}"),
            "WARN" => tracing::warn!(target: "kairo_engine", pid, "{message}"),
            "DEBUG" | "TRACE" => tracing::debug!(target: "kairo_engine", pid, "{message}"),
            "INFO" => tracing::info!(target: "kairo_engine", pid, "{message}"),
            _ => tracing::info!(target: "kairo_engine", pid, "{line}"),
        }
    }
}

fn connect(
    inner: &Arc<Inner>,
    child: &mut Child,
    socket: &str,
    token: &str,
) -> Result<(JoinHandle<()>, Arc<AtomicBool>), String> {
    let deadline = Instant::now() + inner.options.connect_timeout;
    let name = socket
        .to_ns_name::<GenericNamespaced>()
        .map_err(|e| format!("bad socket name: {e}"))?;
    let stream = loop {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!("engine exited before listening ({status})"));
        }
        if let Ok(stream) = Stream::connect(name.borrow()) {
            break stream;
        }
        if Instant::now() >= deadline {
            return Err("engine did not start listening in time".into());
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    let remaining = deadline.saturating_duration_since(Instant::now());
    let _ = stream.set_recv_timeout(Some(remaining.max(Duration::from_millis(100))));
    write_frame(
        &mut &stream,
        &ToEngine::Hello(Hello {
            protocol: PROTOCOL_VERSION,
            token: token.into(),
        }),
    )
    .map_err(|e| format!("handshake failed: {e}"))?;
    let mut reader = BufReader::new(stream);
    match read_frame::<FromEngine>(&mut reader) {
        Ok(FromEngine::Welcome(welcome)) if welcome.protocol == PROTOCOL_VERSION => {
            tracing::info!(
                "output engine {} handshake, protocol {}",
                welcome.engine_version,
                welcome.protocol
            );
        }
        Ok(FromEngine::Welcome(welcome)) => {
            return Err(format!(
                "protocol version mismatch: engine {}, client {}",
                welcome.protocol, PROTOCOL_VERSION
            ));
        }
        Ok(FromEngine::Rejected { reason }) => {
            return Err(format!("engine refused the handshake: {reason}"));
        }
        Ok(other) => return Err(format!("unexpected first message from engine: {other:?}")),
        Err(error) => return Err(format!("no handshake from engine: {error}")),
    }
    let _ = reader.get_ref().set_recv_timeout(None);

    let stream = Arc::new(reader.into_inner());
    let (tx, rx) = mpsc::channel::<ToEngine>();
    let writer_stream = stream.clone();
    std::thread::Builder::new()
        .name("engine-writer".into())
        .spawn(move || {
            // Ends when the link is dropped or a write fails; the reader
            // and heartbeat notice the dead engine.
            for message in rx {
                if let Err(error) = write_frame(&mut &*writer_stream, &message) {
                    tracing::warn!("write to engine failed: {error}");
                    break;
                }
            }
        })
        .map_err(|e| format!("cannot start the engine writer: {e}"))?;

    let reader_done = Arc::new(AtomicBool::new(false));
    let reader = {
        let inner = inner.clone();
        let done = reader_done.clone();
        std::thread::Builder::new()
            .name("engine-reader".into())
            .spawn(move || {
                let mut reader = BufReader::new(&*stream);
                loop {
                    match read_frame::<FromEngine>(&mut reader) {
                        Ok(message) => dispatch(&inner, message),
                        Err(error) => {
                            if error.kind() != std::io::ErrorKind::UnexpectedEof {
                                tracing::error!("engine stream failed: {error}");
                            }
                            break;
                        }
                    }
                }
                done.store(true, Ordering::SeqCst);
            })
            .map_err(|e| format!("cannot start the engine reader: {e}"))?
    };

    *lock(&inner.last_pong) = Instant::now();
    *lock(&inner.child) = Some(child.id());
    // Replay, under the same lock that orders commands, so nothing sent
    // after this point can overtake the state it builds on.
    {
        let mut mirror = lock(&inner.mirror);
        let replay: Vec<Command> = std::iter::once(Command::RetainSurfaces {
            outputs: mirror
                .surfaces
                .iter()
                .map(|s| s.state.spec.output.clone())
                .collect(),
        })
        .chain(mirror.surfaces.iter().flat_map(|s| s.state.replay()))
        .collect();
        for surface in &mut mirror.surfaces {
            surface.status = None;
        }
        mirror.link = Some(tx);
        for command in replay {
            mirror.next_seq += 1;
            let seq = mirror.next_seq;
            mirror.send(ToEngine::Command { seq, command });
        }
    }
    Ok((reader, reader_done))
}

fn dispatch(inner: &Inner, message: FromEngine) {
    match message {
        FromEngine::Pong { nonce } => {
            *lock(&inner.last_pong) = Instant::now();
            if let Some(waiter) = lock(&inner.pings).remove(&nonce) {
                let _ = waiter.send(());
            }
        }
        FromEngine::Reply { id, reply } => {
            if let Some(waiter) = lock(&inner.pending).remove(&id) {
                let _ = waiter.send(reply);
            }
        }
        FromEngine::Event(event) => {
            match &event {
                Event::SurfaceStatus { output, status } => {
                    let mut mirror = lock(&inner.mirror);
                    if let Some(surface) = mirror
                        .surfaces
                        .iter_mut()
                        .find(|s| &s.state.spec.output == output)
                    {
                        surface.status = Some(status.clone());
                    }
                }
                Event::Displays(displays) => lock(&inner.mirror).displays = displays.clone(),
                Event::FrameCommitted { output, seq } => {
                    let mut committed = lock(&inner.committed);
                    let entry = committed.entry(output.clone()).or_insert(0);
                    *entry = (*entry).max(*seq);
                    drop(committed);
                    inner.frame.notify_waiters();
                }
                Event::Error { output, message } => {
                    tracing::warn!(output = output.as_deref(), "engine: {message}");
                }
                Event::WebNavigated { output, url } => {
                    let mut mirror = lock(&inner.mirror);
                    if let Some(web) = mirror
                        .surfaces
                        .iter_mut()
                        .find(|s| &s.state.spec.output == output)
                        .and_then(|s| s.state.layers.web.as_mut())
                    {
                        web.url = url.clone();
                    }
                }
                Event::Health(_) | Event::Stats(_) => {}
            }
            let _ = inner.events.send(EngineEvent::Engine(event));
        }
        FromEngine::Welcome(_) | FromEngine::Rejected { .. } => {
            tracing::warn!("unexpected handshake message from a connected engine");
        }
    }
}

/// Watches the running engine until it exits, hangs, or is stopped.
fn run(inner: &Inner, running: &mut Running) -> Exit {
    let options = &inner.options;
    let mut next_ping = Instant::now();
    let mut nonce = u64::MAX / 2;
    loop {
        if inner.stopping.load(Ordering::SeqCst) {
            stop(inner, running);
            return Exit::Stopped;
        }
        if inner.restart.swap(false, Ordering::SeqCst) {
            return Exit::Planned;
        }
        match running.child.try_wait() {
            Ok(Some(status)) => return Exit::Failure(format!("engine exited ({status})")),
            Ok(None) => {}
            Err(error) => return Exit::Failure(format!("cannot check the engine: {error}")),
        }
        if running.reader_done.load(Ordering::SeqCst) {
            return Exit::Failure("connection to the engine lost".into());
        }
        let now = Instant::now();
        if now.duration_since(*lock(&inner.last_pong)) > options.heartbeat_timeout {
            return Exit::Failure("engine unresponsive".into());
        }
        if now >= next_ping {
            nonce += 1;
            lock(&inner.mirror).send(ToEngine::Ping { nonce });
            next_ping = now + options.heartbeat_every;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Asks the engine to close its outputs and exit; kills it after 2 s.
fn stop(inner: &Inner, running: &mut Running) {
    {
        let mut mirror = lock(&inner.mirror);
        mirror.next_seq += 1;
        let seq = mirror.next_seq;
        mirror.send(ToEngine::Command {
            seq,
            command: Command::Shutdown,
        });
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if let Ok(Some(_)) = running.child.try_wait() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    tracing::warn!("output engine did not exit after shutdown; killing it");
}
