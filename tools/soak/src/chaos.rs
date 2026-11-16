//! Failures injected during the service, each with the recovery the
//! product promises (architecture section 4 of the testing strategy,
//! `resilience.ts`, `ManagedWindow.allowFailure`), checked end to end.

use crate::deepgram::{FakeDeepgram, Proxy};
use crate::monitor::Collector;
use crate::procs;
use crate::service::Control;
use kairo_core::AppCore;
use kairo_core::engine::{EngineHandle, EngineStatus};
use kairo_core::resilience::{
    DetectorFailed, DetectorStateChange, DiscardRecovery, GetStatus, InternetConnectivity,
    LoadRecovery, RecoveryFile,
};
use kairo_domain::status::{DetectorFallbackKind, HealthStatus, OverallHealth, ServiceName};
use kairo_engine_proto::*;
use kairo_stt::deepgram::DeepgramStt;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Kind {
    DeepgramDisconnect,
    EngineKill,
    WebContentKill,
    HelperKill,
    UiBlock,
    NetworkLoss,
    EngineHang,
    DisplayReplug,
    DiskNearlyFull,
    RecoveryFile,
    DetectorFailure,
    OsSleep,
}

impl Kind {
    /// The longest the event and its recovery take.
    pub fn span(self) -> Duration {
        Duration::from_secs(match self {
            Kind::UiBlock => 40,
            Kind::NetworkLoss => 60,
            Kind::DiskNearlyFull => 45,
            Kind::DetectorFailure => 45,
            Kind::OsSleep => 45,
            _ => 30,
        })
    }
}

/// When each event fires in a 10-minute run (seconds from the start).
pub const SCHEDULE: [(u64, Kind); 12] = [
    (60, Kind::DeepgramDisconnect),
    // The stall falls in the verses segment, while the background video
    // plays, so every output must keep drawing at the video's rate.
    (95, Kind::UiBlock),
    (140, Kind::EngineKill),
    (175, Kind::WebContentKill),
    (205, Kind::HelperKill),
    (240, Kind::NetworkLoss),
    (305, Kind::EngineHang),
    (340, Kind::DisplayReplug),
    (375, Kind::DiskNearlyFull),
    (425, Kind::RecoveryFile),
    (455, Kind::DetectorFailure),
    (505, Kind::EngineKill),
];

/// The schedule for a run of `length`, with the opt-in sleep last.
pub fn schedule(length: Duration, os_sleep: bool) -> Vec<(Duration, Kind)> {
    let mut events: Vec<(Duration, Kind)> = SCHEDULE
        .iter()
        .map(|(at, kind)| (Duration::from_secs(*at), *kind))
        .collect();
    if os_sleep {
        events.push((Duration::from_secs(545), Kind::OsSleep));
        events.sort_by_key(|(at, _)| *at);
    }
    events.retain(|(at, kind)| *at + kind.span() <= length);
    events
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Outcome {
    Recovered,
    Failed,
    /// Could not be exercised here; says why.
    NotVerified,
}

#[derive(Clone, Debug, Serialize)]
pub struct Result {
    pub kind: Kind,
    pub at_s: f64,
    pub outcome: Outcome,
    pub recovery_ms: Option<f64>,
    pub detail: String,
}

pub struct Ctx {
    pub engine: EngineHandle,
    pub collector: Arc<Collector>,
    pub core: AppCore,
    pub user_data: PathBuf,
    pub stt: DeepgramStt,
    pub server: Arc<FakeDeepgram>,
    pub proxy: Arc<Proxy>,
    pub script: mpsc::Sender<Control>,
    /// Outputs carrying the service, and which of them is which.
    pub outputs: Vec<String>,
    pub display: Option<String>,
    pub card: Option<String>,
    pub web: Option<String>,
    pub helper_name: String,
}

fn ms(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1_000.0
}

impl Ctx {
    async fn running_pid(&self, within: Duration, not: Option<u32>) -> Option<u32> {
        let deadline = Instant::now() + within;
        loop {
            if let EngineStatus::Running { pid } = self.engine.status()
                && Some(pid) != not
            {
                return Some(pid);
            }
            if Instant::now() >= deadline {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// The engine's own state equals what the client intends.
    async fn state_matches(&self, within: Duration) -> bool {
        let deadline = Instant::now() + within;
        loop {
            let expected: Vec<SurfaceState> = self
                .engine
                .surfaces()
                .iter()
                .filter_map(|o| self.engine.surface_state(o))
                .collect();
            if let Some(Reply::State(actual)) = self.engine.query(Query::State).await
                && actual == expected
            {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// A new command on `output` reaches its sink as a frame.
    async fn picture(&self, output: &str, within: Duration) -> bool {
        let sent = self.engine.command(Command::SetLayer {
            output: output.into(),
            kind: LayerKind::Logo,
            layer: Some(Json(
                r##"{"kind":"logo","logo":{"mediaPath":"","background":"#000000"}}"##.into(),
            )),
        });
        let shown = sent.accepted && self.engine.wait_frame(output, sent.seq, within).await;
        self.engine.command(Command::SetLayer {
            output: output.into(),
            kind: LayerKind::Logo,
            layer: None,
        });
        shown
    }

    /// The output used to prove a picture: the display, else the first.
    fn picture_output(&self) -> Option<&str> {
        self.display
            .as_deref()
            .or_else(|| self.outputs.first().map(String::as_str))
    }

    async fn resilience(&self) -> Option<kairo_domain::status::ResilienceStatus> {
        self.core.call(GetStatus).await.ok()
    }

    async fn resilience_until(
        &self,
        within: Duration,
        check: impl Fn(&kairo_domain::status::ResilienceStatus) -> bool,
    ) -> bool {
        let deadline = Instant::now() + within;
        loop {
            if self.resilience().await.is_some_and(|s| check(&s)) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

pub async fn run(ctx: &Ctx, kind: Kind) -> Result {
    let at_s = ctx.collector.at();
    tracing::info!("chaos: {kind:?}");
    let (outcome, recovery_ms, detail) = match kind {
        Kind::EngineKill => engine_crash(ctx, libc::SIGKILL).await,
        Kind::EngineHang => engine_crash(ctx, libc::SIGSTOP).await,
        Kind::DeepgramDisconnect => deepgram_disconnect(ctx).await,
        Kind::WebContentKill => web_content_kill(ctx).await,
        Kind::HelperKill => helper_kill(ctx).await,
        Kind::UiBlock => ui_block(ctx).await,
        Kind::NetworkLoss => network_loss(ctx).await,
        Kind::DisplayReplug => display_replug(ctx).await,
        Kind::DiskNearlyFull => disk_nearly_full(ctx).await,
        Kind::RecoveryFile => recovery_file(ctx).await,
        Kind::DetectorFailure => detector_failure(ctx).await,
        Kind::OsSleep => os_sleep(ctx).await,
    };
    tracing::info!("chaos: {kind:?} -> {outcome:?} {detail}");
    Result {
        kind,
        at_s,
        outcome,
        recovery_ms,
        detail,
    }
}

type Checked = (Outcome, Option<f64>, String);

fn failed(detail: impl Into<String>) -> Checked {
    (Outcome::Failed, None, detail.into())
}

fn not_verified(detail: impl Into<String>) -> Checked {
    (Outcome::NotVerified, None, detail.into())
}

/// SIGKILL (crash) or SIGSTOP (hang): the supervisor relaunches the engine,
/// replays every surface and the picture returns.
async fn engine_crash(ctx: &Ctx, signal: i32) -> Checked {
    let Some(pid) = ctx.engine.pid() else {
        return failed("no engine running before the event");
    };
    let started = Instant::now();
    if !procs::signal(pid, signal) {
        return failed(format!("could not signal engine {pid}"));
    }
    // A hang is noticed after the heartbeat timeout (3 s) and then killed.
    let Some(next) = ctx.running_pid(Duration::from_secs(15), Some(pid)).await else {
        if signal == libc::SIGSTOP && procs::alive(pid) {
            procs::signal(pid, libc::SIGCONT);
        }
        return failed(format!("engine not relaunched: {:?}", ctx.engine.status()));
    };
    let relaunched_ms = ms(started);
    if !ctx.state_matches(Duration::from_secs(10)).await {
        return failed(format!(
            "relaunched as {next} in {relaunched_ms:.0} ms but its state never matched the client's"
        ));
    }
    let Some(output) = ctx.picture_output() else {
        return failed("no output to check the picture on");
    };
    if !ctx.picture(output, Duration::from_secs(5)).await {
        return failed(format!("relaunched as {next}, but no frame on {output}"));
    }
    let picture_ms = ms(started);
    (
        Outcome::Recovered,
        Some(picture_ms),
        format!(
            "{} pid {pid}; relaunched as {next} after {relaunched_ms:.0} ms, state replayed, frame on {output} at {picture_ms:.0} ms",
            if signal == libc::SIGKILL {
                "SIGKILL"
            } else {
                "SIGSTOP (hang)"
            }
        ),
    )
}

async fn stt_connected_after(ctx: &Ctx, connections_before: u64, within: Duration) -> bool {
    procs::until(within, || {
        ctx.stt.is_connected() && ctx.server.connections() > connections_before
    })
    .await
}

async fn deepgram_disconnect(ctx: &Ctx) -> Checked {
    if !ctx.stt.is_connected() {
        return failed("transcription was not connected before the event");
    }
    let (connections, bytes) = (ctx.server.connections(), ctx.server.received());
    let started = Instant::now();
    ctx.server.disconnect_all();
    if !stt_connected_after(ctx, connections, Duration::from_secs(15)).await {
        return failed("transcription did not reconnect within 15 s");
    }
    let reconnected_ms = ms(started);
    if !ctx
        .resilience_until(Duration::from_secs(5), |s| {
            s.health
                .iter()
                .any(|h| h.service == ServiceName::Stt && h.status == HealthStatus::Ok)
        })
        .await
    {
        return failed("speech health never returned to ok");
    }
    if !procs::until(Duration::from_secs(5), || {
        ctx.server.received() > bytes + 32_000
    })
    .await
    {
        return failed("audio did not resume after reconnecting");
    }
    let buffered = ctx.stt.replay_buffer_bytes();
    (
        Outcome::Recovered,
        Some(reconnected_ms),
        format!(
            "server closed the socket; reconnected in {reconnected_ms:.0} ms, speech health ok, audio flowing, replay buffer {buffered} B"
        ),
    )
}

async fn web_content_kill(ctx: &Ctx) -> Checked {
    let Some(web) = &ctx.web else {
        return not_verified("no output could carry the web layer");
    };
    if !procs::can_attribute_web_content() {
        return not_verified(
            "web content processes cannot be attributed to this soak (it is not its own responsible process)",
        );
    }
    let Some(engine_pid) = ctx.engine.pid() else {
        return failed("no engine running");
    };
    let victims = procs::own_web_content();
    if victims.is_empty() {
        return failed(format!(
            "the web layer on {web} has no content process attributed to this soak"
        ));
    }
    let started = Instant::now();
    for victim in &victims {
        procs::signal(victim.pid, libc::SIGKILL);
    }
    let killed: Vec<u32> = victims.iter().map(|v| v.pid).collect();
    let replaced = procs::until(Duration::from_secs(15), || {
        procs::own_web_content()
            .iter()
            .any(|p| !killed.contains(&p.pid))
    })
    .await;
    if !replaced {
        return failed(format!(
            "killed {killed:?}; no new content process within 15 s"
        ));
    }
    let reloaded_ms = ms(started);
    if ctx.engine.pid() != Some(engine_pid) {
        return failed("the engine itself went down with its web view");
    }
    let Some(output) = ctx.picture_output() else {
        return failed("no output to check the picture on");
    };
    if !ctx.picture(output, Duration::from_secs(5)).await {
        return failed(format!("no frame on {output} after the web view crash"));
    }
    (
        Outcome::Recovered,
        Some(reloaded_ms),
        format!(
            "SIGKILL web content {killed:?}; page reloaded in a new content process after {reloaded_ms:.0} ms; engine {engine_pid} kept running, other outputs drew"
        ),
    )
}

async fn helper_kill(ctx: &Ctx) -> Checked {
    let Some(card) = &ctx.card else {
        return not_verified("no card output (the DeckLink helper stand-in is not built)");
    };
    let Some(engine_pid) = ctx.engine.pid() else {
        return failed("no engine running");
    };
    let helpers = procs::children_named(engine_pid, &ctx.helper_name);
    let Some(helper) = helpers.first() else {
        return failed(format!(
            "no {} child of engine {engine_pid}",
            ctx.helper_name
        ));
    };
    let started = Instant::now();
    procs::signal(helper.pid, libc::SIGKILL);
    let faulted = procs::until(Duration::from_secs(10), || {
        matches!(
            ctx.engine.surface_status(card),
            Some(SurfaceStatus::Unavailable { ref reason }) if reason.contains("helper exited")
        )
    })
    .await;
    if !faulted {
        return failed(format!(
            "card status after the helper died: {:?}",
            ctx.engine.surface_status(card)
        ));
    }
    let fault_ms = ms(started);
    // The operator's "refresh" acquires the port again (`refresh` in Electron).
    ctx.engine.command(Command::RefreshVideoDevices);
    let ready = procs::until(Duration::from_secs(10), || {
        ctx.engine.surface_status(card) == Some(SurfaceStatus::Ready)
    })
    .await;
    if !ready {
        return failed(format!(
            "refresh did not bring the card back: {:?}",
            ctx.engine.surface_status(card)
        ));
    }
    if !ctx.picture(card, Duration::from_secs(5)).await {
        return failed("card ready again but no frame reached it");
    }
    let recovered_ms = ms(started);
    if ctx.engine.pid() != Some(engine_pid) {
        return failed("the engine went down with its helper");
    }
    (
        Outcome::Recovered,
        Some(recovered_ms),
        format!(
            "SIGKILL helper {}; card faulted with the helper's reason in {fault_ms:.0} ms; operator refresh restored frames at {recovered_ms:.0} ms; engine unaffected",
            helper.pid
        ),
    )
}

async fn ui_block(ctx: &Ctx) -> Checked {
    const BLOCK: Duration = Duration::from_secs(30);
    let engine_pid = ctx.engine.pid();
    let produced: Vec<(String, u64)> = ctx
        .outputs
        .iter()
        .filter(|o| Some(*o) != ctx.web.as_ref())
        .map(|o| (o.clone(), ctx.collector.produced(o)))
        .collect();
    let bytes = ctx.server.received();
    if ctx.script.send(Control::Block(BLOCK)).is_err() {
        return failed("the script thread is gone");
    }
    tokio::time::sleep(BLOCK).await;
    let audio = ctx.server.received().saturating_sub(bytes);
    let mut stalled = Vec::new();
    let mut detail = Vec::new();
    for (output, before) in &produced {
        let delta = ctx.collector.produced(output).saturating_sub(*before);
        detail.push(format!("{output} +{delta}"));
        // A playing 30 fps background draws ~900 frames in 30 s.
        if delta < 300 {
            stalled.push(output.clone());
        }
    }
    // 16 kHz PCM16 is 32,000 B/s; allow for scheduling slack.
    let audio_ok = audio as f64 >= 0.8 * 32_000.0 * BLOCK.as_secs_f64();
    if !stalled.is_empty() || !audio_ok || ctx.engine.pid() != engine_pid {
        return failed(format!(
            "during the 30 s stall: frames {} (stalled {stalled:?}); audio to Deepgram {audio} B; engine {:?} -> {:?}",
            detail.join(", "),
            engine_pid,
            ctx.engine.pid()
        ));
    }
    let Some(output) = ctx.picture_output() else {
        return failed("no output to check the picture on");
    };
    let started = Instant::now();
    if !ctx.picture(output, Duration::from_secs(5)).await {
        return failed("after the stall, a new command did not reach the screen");
    }
    (
        Outcome::Recovered,
        Some(ms(started)),
        format!(
            "script (UI) thread blocked 30 s: frames kept flowing ({}), {audio} B of audio reached Deepgram; commands resumed afterwards",
            detail.join(", ")
        ),
    )
}

async fn network_loss(ctx: &Ctx) -> Checked {
    const DOWN: Duration = Duration::from_secs(20);
    let connections = ctx.server.connections();
    let engine_pid = ctx.engine.pid();
    ctx.proxy.cut();
    if !ctx.proxy.is_cut() {
        return failed("the network stand-in could not be cut");
    }
    // The connectivity check (`check_internet_connection`) would now fail;
    // its DNS probe cannot be cut without taking this machine offline, so
    // its result is fed in.
    let _ = ctx.core.call(InternetConnectivity(false)).await;
    let degraded = ctx
        .resilience_until(Duration::from_secs(10), |s| {
            !s.internet_connected
                && s.overall_health == OverallHealth::Degraded
                && s.claude_fallback_active
                && s.detector_fallback_reason
                    .as_ref()
                    .is_some_and(|r| r.kind == DetectorFallbackKind::Offline)
        })
        .await;
    let stt_down = procs::until(Duration::from_secs(10), || !ctx.stt.is_connected()).await;
    tokio::time::sleep(DOWN).await;
    let buffered = ctx.stt.replay_buffer_bytes();
    ctx.proxy.restore();
    let started = Instant::now();
    let _ = ctx.core.call(InternetConnectivity(true)).await;
    if !degraded || !stt_down {
        return failed(format!(
            "while offline: resilience degraded with offline fallback={degraded}, transcription noticed={stt_down}"
        ));
    }
    // Reconnect backoff (1, 2, 4, 8, 16 s, Electron's) can leave a wait of
    // up to 16 s after the network returns.
    if !stt_connected_after(ctx, connections, Duration::from_secs(40)).await {
        return failed("transcription did not reconnect within 40 s of the network returning");
    }
    let reconnected_ms = ms(started);
    let drained = procs::until(Duration::from_secs(10), || {
        ctx.stt.replay_buffer_bytes() == 0
    })
    .await;
    let healthy = ctx
        .resilience_until(Duration::from_secs(10), |s| {
            s.internet_connected
                && !s.claude_fallback_active
                && s.overall_health == OverallHealth::AllGood
        })
        .await;
    if !drained || !healthy || ctx.engine.pid() != engine_pid {
        return failed(format!(
            "after the network returned: replay drained={drained}, all good={healthy}, engine {engine_pid:?} -> {:?}",
            ctx.engine.pid()
        ));
    }
    (
        Outcome::Recovered,
        Some(reconnected_ms),
        format!(
            "20 s offline (proxy cut, licence server unreachable): health degraded with offline detector fallback, {buffered} B of audio held; reconnected {reconnected_ms:.0} ms after the network returned, held audio replayed, health all good; outputs unaffected"
        ),
    )
}

/// The client half of a display going away and coming back: the surface is
/// parked (closed, not destroyed), then restored with its state.
async fn display_replug(ctx: &Ctx) -> Checked {
    let Some(display) = &ctx.display else {
        return not_verified("no display output on this machine");
    };
    ctx.engine.command(Command::CloseSurface {
        output: display.clone(),
        destroy: false,
    });
    let parked = procs::until(Duration::from_secs(5), || {
        ctx.engine.surface_state(display).is_some_and(|s| s.parked)
    })
    .await;
    if !parked {
        return failed("closing the display surface did not park it");
    }
    tokio::time::sleep(Duration::from_secs(3)).await;
    let started = Instant::now();
    ctx.engine.command(Command::RestoreSurface {
        output: display.clone(),
    });
    if !ctx.picture(display, Duration::from_secs(5)).await {
        return failed("the restored display surface showed no frame");
    }
    let restored_ms = ms(started);
    if !ctx.state_matches(Duration::from_secs(5)).await {
        return failed("restored, but the engine's state differs from the client's");
    }
    (
        Outcome::Recovered,
        Some(restored_ms),
        format!(
            "surface parked 3 s and restored; frame in {restored_ms:.0} ms with state intact. The OS display-change event itself was not exercised (no second display to unplug)"
        ),
    )
}

#[cfg(target_os = "macos")]
struct RamDisk {
    device: String,
    mount: PathBuf,
}

#[cfg(target_os = "macos")]
impl RamDisk {
    /// A RAM-backed HFS+ volume of `bytes`; needs no administrator rights.
    fn create(bytes: u64, name: &str) -> std::result::Result<Self, String> {
        let sectors = bytes / 512;
        let attach = std::process::Command::new("hdiutil")
            .args(["attach", "-nomount", &format!("ram://{sectors}")])
            .output()
            .map_err(|e| format!("hdiutil: {e}"))?;
        let device = String::from_utf8_lossy(&attach.stdout).trim().to_owned();
        if !attach.status.success() || !device.starts_with("/dev/disk") {
            return Err(format!("hdiutil attach failed: {device}"));
        }
        let disk = Self {
            device: device.clone(),
            mount: PathBuf::from("/Volumes").join(name),
        };
        let erase = std::process::Command::new("diskutil")
            .args(["erasevolume", "HFS+", name, &device])
            .output()
            .map_err(|e| format!("diskutil: {e}"))?;
        if !erase.status.success() || !disk.mount.exists() {
            return Err(format!(
                "diskutil erasevolume failed: {}",
                String::from_utf8_lossy(&erase.stderr)
            ));
        }
        Ok(disk)
    }

    fn free(&self) -> Option<u64> {
        let path = std::ffi::CString::new(self.mount.to_str()?).ok()?;
        // SAFETY: a zeroed statfs filled by the call for a NUL-terminated path.
        unsafe {
            let mut stat: libc::statfs = std::mem::zeroed();
            (libc::statfs(path.as_ptr(), &mut stat) == 0)
                .then(|| stat.f_bavail * u64::from(stat.f_bsize))
        }
    }
}

#[cfg(target_os = "macos")]
impl Drop for RamDisk {
    fn drop(&mut self) {
        let _ = std::process::Command::new("hdiutil")
            .args(["detach", "-force", &self.device])
            .output();
    }
}

#[cfg(target_os = "macos")]
fn fill(path: &std::path::Path, bytes: u64) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::File::create(path)?;
    let chunk = vec![0x5au8; 8 << 20];
    let mut left = bytes;
    while left > 0 {
        let n = left.min(chunk.len() as u64) as usize;
        file.write_all(&chunk[..n])?;
        left -= n as u64;
    }
    file.sync_all()
}

#[cfg(target_os = "macos")]
fn capture_status(ctx: &Ctx, since: f64, check: impl Fn(&serde_json::Value) -> bool) -> bool {
    ctx.collector
        .state()
        .capture
        .iter()
        .any(|(at, value)| *at >= since && check(value))
}

/// A recording on a nearly full disk stops cleanly with the reason, and
/// the outputs keep running.
#[cfg(target_os = "macos")]
async fn disk_nearly_full(ctx: &Ctx) -> Checked {
    use std::path::Path;
    let Some(output) = ctx.picture_output().map(str::to_owned) else {
        return failed("no output to record");
    };
    // Above the 2 GiB needed to start, then filled below the 512 MiB floor.
    let disk =
        match tokio::task::spawn_blocking(|| RamDisk::create(2_600 * 1024 * 1024, "KairoSoakDisk"))
            .await
        {
            Ok(Ok(disk)) => disk,
            Ok(Err(error)) => return not_verified(format!("could not make a small disk: {error}")),
            Err(error) => return failed(format!("disk task failed: {error}")),
        };
    let folder = disk.mount.join("Recordings");
    if let Err(error) = std::fs::create_dir_all(&folder) {
        return failed(format!("could not create {}: {error}", folder.display()));
    }
    let engine_pid = ctx.engine.pid();
    let since = ctx.collector.at();
    ctx.engine.command(Command::StartCapture {
        output: output.clone(),
        source_name: "Soak".into(),
        settings: Json::from_value(&serde_json::json!({
            "sourceOutputId": output, "folder": folder, "codec": "h264",
            "resolution": "1080p", "frameRate": 30, "bitrateMbps": 12,
            "audioDeviceId": "", "audioDeviceName": ""
        })),
    });
    let recording = procs::until(Duration::from_secs(10), || {
        capture_status(ctx, since, |v| v["recording"] == true)
    })
    .await;
    if !recording {
        let last = ctx.collector.state().capture.last().map(|c| c.1.clone());
        ctx.engine.command(Command::StopCapture);
        return failed(format!("the recording did not start: {last:?}"));
    }
    let free = disk.free().unwrap_or(0);
    let filler = disk.mount.join("filler.bin");
    let target = free.saturating_sub(300 * 1024 * 1024);
    let filled = tokio::task::spawn_blocking(move || fill(&filler, target)).await;
    if !matches!(filled, Ok(Ok(()))) {
        ctx.engine.command(Command::StopCapture);
        return failed(format!("could not fill the disk: {filled:?}"));
    }
    let started = Instant::now();
    // The disk is checked every 10 s while recording.
    let stopped = procs::until(Duration::from_secs(25), || {
        capture_status(ctx, since, |v| {
            v["recording"] == false
                && v["error"]
                    .as_str()
                    .is_some_and(|e| e.contains("disk is nearly full"))
        })
    })
    .await;
    let stop_ms = ms(started);
    let last = ctx.collector.state().capture.last().map(|c| c.1.clone());
    if !stopped {
        ctx.engine.command(Command::StopCapture);
        return failed(format!(
            "recording did not stop on a nearly full disk: {last:?}"
        ));
    }
    let file = last
        .as_ref()
        .and_then(|v| v["path"].as_str().map(str::to_owned))
        .filter(|p| Path::new(p).exists());
    let picture = ctx.picture(&output, Duration::from_secs(5)).await;
    drop(disk);
    if !picture || ctx.engine.pid() != engine_pid {
        return failed("the outputs did not survive the full disk");
    }
    (
        Outcome::Recovered,
        Some(stop_ms),
        format!(
            "recording on a 2.6 GB RAM disk filled to ~300 MB free: stopped {stop_ms:.0} ms after filling with \"{}\"; file kept: {}; outputs unaffected",
            last.as_ref()
                .and_then(|v| v["error"].as_str())
                .unwrap_or_default(),
            file.is_some()
        ),
    )
}

#[cfg(not(target_os = "macos"))]
async fn disk_nearly_full(_ctx: &Ctx) -> Checked {
    not_verified("the small-disk setup is macOS only (hdiutil)")
}

/// `resilience.ts`'s recovery file paths: a corrupt file loads as nothing,
/// a failed save is logged and survived, restore needs a service, discard
/// of a missing file is quiet.
async fn recovery_file(ctx: &Ctx) -> Checked {
    let started = Instant::now();
    let file = RecoveryFile::new(&ctx.user_data);
    let path = ctx.user_data.join(RECOVERY_FILE_NAME);
    let mut problems = Vec::new();
    if ctx
        .core
        .call(LoadRecovery {
            active_service: false,
        })
        .await
        .ok()
        .is_some_and(|r| r.is_ok())
    {
        problems.push("restore without a service was allowed");
    }
    if std::fs::write(&path, b"{ not json").is_err() {
        return failed("could not write the corrupt recovery file");
    }
    match ctx
        .core
        .call(LoadRecovery {
            active_service: true,
        })
        .await
    {
        Ok(Ok(None)) => {}
        _ => problems.push("a corrupt recovery file did not load as nothing"),
    }
    let snapshot = serde_json::json!({"surfaces": ctx.engine.surfaces(), "at": ctx.collector.at()});
    // A save into a folder that cannot be written fails without harm.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::set_permissions(&ctx.user_data, std::fs::Permissions::from_mode(0o500));
        let refused = file.save(snapshot.clone()).await.is_err();
        let _ = std::fs::set_permissions(&ctx.user_data, std::fs::Permissions::from_mode(0o700));
        if !refused {
            problems.push("a save into a read-only folder reported success");
        }
    }
    if file.save(snapshot.clone()).await.is_err() {
        problems.push("a save after the folder became writable failed");
    }
    match ctx
        .core
        .call(LoadRecovery {
            active_service: true,
        })
        .await
    {
        Ok(Ok(Some(value))) if value == snapshot => {}
        _ => problems.push("the saved snapshot did not load back"),
    }
    let _ = ctx.core.call(DiscardRecovery).await;
    let _ = ctx.core.call(DiscardRecovery).await;
    let status = ctx.resilience().await;
    if status.as_ref().is_none_or(|s| s.recovery_session_available) || path.exists() {
        problems.push("discard left a recovery session behind");
    }
    if !problems.is_empty() {
        return failed(problems.join("; "));
    }
    (
        Outcome::Recovered,
        Some(ms(started)),
        "restore without a service refused; corrupt file loaded as nothing; save into a read-only folder failed and was survived; save, load and double discard behaved".into(),
    )
}

/// `RECOVERY_FILE` in `resilience.ts` (`kairo_store::paths::RECOVERY_FILE_NAME`).
const RECOVERY_FILE_NAME: &str = "session-recovery.json";

/// Two failed AI detector requests switch to local matching; after the
/// 30 s retry delay a successful request returns to normal.
async fn detector_failure(ctx: &Ctx) -> Checked {
    let first = ctx
        .core
        .call(DetectorFailed("simulated 529 overloaded".into()))
        .await;
    let second = ctx
        .core
        .call(DetectorFailed("simulated 529 overloaded".into()))
        .await;
    let (Ok(None), Ok(Some(ticket))) = (first, second) else {
        return failed("two failures did not schedule a retry");
    };
    let started = Instant::now();
    if !ctx
        .resilience_until(Duration::from_secs(2), |s| {
            s.claude_fallback_active
                && s.detector_fallback_reason
                    .as_ref()
                    .is_some_and(|r| r.kind == DetectorFallbackKind::Provider)
        })
        .await
    {
        return failed("no provider fallback after two failures");
    }
    if kairo_core::resilience::probe_after_delay(&ctx.core, ticket)
        .await
        .is_err()
    {
        return failed("the retry timer did not run");
    }
    let probing = ctx
        .resilience_until(Duration::from_secs(2), |s| {
            !s.claude_fallback_active
                && s.health.iter().any(|h| {
                    h.service == ServiceName::Detector
                        && h.last_error.as_deref() == Some("Testing AI detector connectivity")
                })
        })
        .await;
    let _ = ctx.core.call(DetectorStateChange::Succeeded).await;
    let healthy = ctx
        .resilience_until(Duration::from_secs(2), |s| {
            s.overall_health == OverallHealth::AllGood
        })
        .await;
    if !probing || !healthy {
        return failed(format!(
            "probe began={probing}, all good after success={healthy}"
        ));
    }
    (
        Outcome::Recovered,
        Some(ms(started)),
        "two failures: provider fallback; retry after 30 s; success restored all good".into(),
    )
}

/// Opt-in: puts this Mac to sleep. Someone must wake it within 40 s.
async fn os_sleep(ctx: &Ctx) -> Checked {
    if !cfg!(target_os = "macos") {
        return not_verified("sleep is only scripted on macOS");
    }
    let connections = ctx.server.connections();
    let wall = std::time::SystemTime::now();
    let mono = Instant::now();
    let slept = std::process::Command::new("pmset")
        .arg("sleepnow")
        .status()
        .is_ok_and(|s| s.success());
    if !slept {
        return not_verified("pmset sleepnow failed");
    }
    // Asleep, the monotonic clock stops while the wall clock does not.
    let woke = procs::until(Duration::from_secs(40), || {
        wall.elapsed()
            .unwrap_or_default()
            .saturating_sub(mono.elapsed())
            > Duration::from_secs(5)
    })
    .await;
    if !woke {
        return not_verified("the machine did not sleep (or was not woken)");
    }
    let started = Instant::now();
    let running = ctx
        .running_pid(Duration::from_secs(30), None)
        .await
        .is_some();
    let picture = match ctx.picture_output() {
        Some(output) => ctx.picture(output, Duration::from_secs(10)).await,
        None => false,
    };
    let stt = stt_connected_after(ctx, connections, Duration::from_secs(60)).await
        || ctx.stt.is_connected();
    if !(running && picture && stt) {
        return failed(format!(
            "after wake: engine running={running}, frame={picture}, transcription={stt}"
        ));
    }
    (
        Outcome::Recovered,
        Some(ms(started)),
        "slept and woke: engine running, frames drawn, transcription connected".into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairo_core::engine::MAX_FAILURES;

    impl Kind {
        /// Counts against the supervisor's relaunch budget.
        fn costs_a_relaunch(self) -> bool {
            matches!(self, Kind::EngineKill | Kind::EngineHang)
        }
    }

    #[test]
    fn the_schedule_never_spends_the_relaunch_budget() {
        let events = schedule(Duration::from_secs(600), true);
        for (i, (at, kind)) in events.iter().enumerate() {
            if !kind.costs_a_relaunch() {
                continue;
            }
            // Each event can cost one relaunch; the budget allows
            // MAX_FAILURES in any rolling 60 s, so fewer must fall there.
            let in_window = events[i..]
                .iter()
                .filter(|(other, k)| k.costs_a_relaunch() && *other < *at + Duration::from_secs(60))
                .count();
            assert!(in_window < MAX_FAILURES, "{in_window} at {at:?}");
        }
    }

    #[test]
    fn events_do_not_overlap_and_fit_the_run() {
        let length = Duration::from_secs(600);
        let events = schedule(length, true);
        assert!(events.windows(2).all(|w| w[0].0 + w[0].1.span() <= w[1].0));
        assert!(events.iter().all(|(at, k)| *at + k.span() <= length));
        assert!(events.iter().any(|(_, k)| *k == Kind::OsSleep));
        assert!(
            !schedule(length, false)
                .iter()
                .any(|(_, k)| *k == Kind::OsSleep)
        );
    }

    #[test]
    fn every_required_event_is_scheduled_in_ten_minutes() {
        let events = schedule(Duration::from_secs(600), false);
        for kind in [
            Kind::EngineKill,
            Kind::WebContentKill,
            Kind::HelperKill,
            Kind::UiBlock,
            Kind::NetworkLoss,
            Kind::DisplayReplug,
            Kind::DiskNearlyFull,
            Kind::DeepgramDisconnect,
        ] {
            assert!(events.iter().any(|(_, k)| *k == kind), "{kind:?}");
        }
    }

    #[test]
    fn the_ui_stall_happens_over_a_playing_video() {
        let (at, _) = SCHEDULE.iter().find(|(_, k)| *k == Kind::UiBlock).unwrap();
        for s in [*at, at + 40] {
            assert_eq!(
                crate::service::segment(Duration::from_secs(s)),
                crate::service::Segment::Verses
            );
        }
    }

    #[test]
    fn a_short_run_keeps_only_the_events_that_fit() {
        let events = schedule(Duration::from_secs(120), false);
        assert_eq!(
            events.iter().map(|(_, k)| *k).collect::<Vec<_>>(),
            [Kind::DeepgramDisconnect]
        );
    }
}
