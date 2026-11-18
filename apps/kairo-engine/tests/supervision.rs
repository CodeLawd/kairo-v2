//! The real `kairo-engine` under `kairo-core`'s supervisor.
//!
//! These launch the engine binary. Where no window system is available
//! (a headless Linux runner) the engine cannot start its event loop, and
//! each test says so and returns, unless `KAIRO_REQUIRE_ENGINE` is set.
//!
//! Unix only: the crash and hang cases send SIGKILL and SIGSTOP. The engine
//! has not been run on Windows yet (prompt 060).
#![cfg(unix)]

use interprocess::local_socket::{GenericNamespaced, Stream, ToNsName, prelude::*};
use kairo_core::engine::*;
use kairo_engine_proto::*;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command as Process, Stdio};
use std::time::{Duration, Instant};

const ENGINE: &str = env!("CARGO_BIN_EXE_kairo-engine");

fn options() -> EngineOptions {
    let mut options = EngineOptions::new(ENGINE);
    options.grace = Duration::from_secs(2);
    options
}

/// Exit code 70: the engine could not start the window system.
fn headless(reason: &str) -> bool {
    let headless = reason.contains("exit status: 70");
    assert!(
        !(headless && std::env::var_os("KAIRO_REQUIRE_ENGINE").is_some()),
        "engine could not start: {reason}"
    );
    if headless {
        eprintln!("skipped: no window system ({reason})");
    }
    headless
}

async fn running(engine: &EngineHandle, within: Duration) -> Result<u32, String> {
    let mut status = engine.watch_status();
    let deadline = tokio::time::Instant::now() + within;
    loop {
        match status.borrow_and_update().clone() {
            EngineStatus::Running { pid } => return Ok(pid),
            EngineStatus::Failed { reason } => return Err(reason),
            EngineStatus::Restarting { reason } if reason.contains("exit status: 70") => {
                return Err(reason);
            }
            _ => {}
        }
        if tokio::time::timeout_at(deadline, status.changed())
            .await
            .is_err()
        {
            return Err(format!(
                "not running after {within:?}: {:?}",
                engine.status()
            ));
        }
    }
}

/// Waits for a launch event after `pid` (a relaunch).
async fn relaunched(engine: &EngineHandle, old: u32) -> u32 {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let EngineStatus::Running { pid } = engine.status()
            && pid != old
        {
            return pid;
        }
        assert!(
            Instant::now() < deadline,
            "no relaunch: {:?}",
            engine.status()
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

fn kill(pid: u32, signal: i32) {
    // SAFETY: a plain syscall on a process this test launched.
    assert_eq!(unsafe { libc::kill(pid as i32, signal) }, 0);
}

fn ndi(output: &str) -> Command {
    Command::OpenSurface(SurfaceSpec {
        output: output.into(),
        label: output.into(),
        sink: SinkSpec::Ndi {
            name: output.into(),
        },
    })
}

fn rehearsal(output: &str, display: &DisplayInfo) -> Command {
    Command::OpenSurface(SurfaceSpec {
        output: output.into(),
        label: "Kairo test output".into(),
        sink: SinkSpec::Display(DisplaySink {
            binding: DisplayBinding {
                display_id: Some(display.id),
                label: display.label.clone(),
                size: Some((display.bounds.width, display.bounds.height)),
            },
            rehearsal: true,
            frame_height: 1080,
        }),
    })
}

async fn engine_state(engine: &EngineHandle) -> Option<Vec<SurfaceState>> {
    match engine.query(Query::State).await? {
        Reply::State(states) => Some(states),
        _ => None,
    }
}

fn mirror(engine: &EngineHandle) -> Vec<SurfaceState> {
    engine
        .surfaces()
        .iter()
        .filter_map(|o| engine.surface_state(o))
        .collect()
}

fn percentile(sorted: &[f64], fraction: f64) -> f64 {
    let rank = (fraction * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

#[tokio::test(flavor = "multi_thread")]
async fn commands_reach_the_engine_loop_quickly() {
    let engine = EngineHandle::spawn(options());
    if let Err(reason) = running(&engine, Duration::from_secs(10)).await {
        if headless(&reason) {
            return;
        }
        panic!("{reason}");
    }
    for _ in 0..200 {
        engine.ping().await.expect("warm-up ping");
    }
    let mut samples = Vec::new();
    for _ in 0..5_000 {
        samples.push(engine.ping().await.expect("ping").as_secs_f64() * 1_000.0);
    }
    samples.sort_by(f64::total_cmp);
    let (p50, p99, max) = (
        percentile(&samples, 0.5),
        percentile(&samples, 0.99),
        samples[samples.len() - 1],
    );
    eprintln!("command round trip n=5000 p50={p50:.4}ms p99={p99:.4}ms max={max:.4}ms");
    // The target is 1 ms p99 (prompt 050 section 8). A debug build on a
    // loaded test machine gets headroom here; the release figure is
    // recorded in PROGRESS.md.
    assert!(p99 < 5.0, "p99 {p99} ms");
    engine.shutdown();
    assert_eq!(engine.status(), EngineStatus::Stopped);
}

#[tokio::test(flavor = "multi_thread")]
async fn killing_the_engine_during_a_command_storm_replays_state() {
    let engine = EngineHandle::spawn(options());
    let pid = match running(&engine, Duration::from_secs(10)).await {
        Ok(pid) => pid,
        Err(reason) if headless(&reason) => return,
        Err(reason) => panic!("{reason}"),
    };
    assert!(engine.command(ndi("ndi-a")).accepted);
    assert!(engine.command(ndi("ndi-b")).accepted);
    // A display surface too, when there is a display: its slide is the
    // state that matters most after a relaunch.
    let deadline = Instant::now() + Duration::from_secs(2);
    while engine.displays().is_empty() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let display = engine.displays().first().cloned();
    if let Some(display) = &display {
        assert!(engine.command(rehearsal("screen", display)).accepted);
    }

    let storm = {
        let engine = engine.clone();
        let has_display = display.is_some();
        std::thread::spawn(move || {
            for i in 0..20_000u32 {
                let output = if i % 2 == 0 { "ndi-a" } else { "ndi-b" };
                let kind = LayerKind::ALL[(i % 6) as usize];
                engine.command(Command::SetLayer {
                    output: output.into(),
                    kind,
                    layer: Some(message(&format!("{i}"))),
                });
                if i % 7 == 0 {
                    engine.command(Command::Configure {
                        output: output.into(),
                        config: SurfaceConfig {
                            transition_ms: f64::from(i % 5000),
                            ..SurfaceConfig::default()
                        },
                    });
                }
                if has_display && i % 3 == 0 {
                    engine.command(Command::ShowSlide {
                        output: "screen".into(),
                        source: "screen".into(),
                        slide: SlideContent {
                            reference: "Psalm 23:1".into(),
                            text: format!("slide {i}"),
                            theme: Json(
                                r#"{"background":{"type":"color","color":"black"}}"#.into(),
                            ),
                            colored_lines: None,
                        },
                        instant: i % 2 == 0,
                    });
                }
                if i % 500 == 0 {
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
        })
    };
    tokio::time::sleep(Duration::from_millis(40)).await;
    let killed_at = Instant::now();
    kill(pid, libc::SIGKILL);
    let next = relaunched(&engine, pid).await;
    eprintln!(
        "relaunched as {next} {:.1} ms after SIGKILL",
        killed_at.elapsed().as_secs_f64() * 1_000.0
    );
    storm.join().unwrap();

    let expected = mirror(&engine);
    assert_eq!(expected.len(), if display.is_some() { 3 } else { 2 });
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if engine_state(&engine).await.as_ref() == Some(&expected) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "engine state never matched the client's"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let ndi_a = expected.iter().find(|s| s.spec.output == "ndi-a").unwrap();
    assert!(ndi_a.layers.any());
    if display.is_some() {
        let screen = expected.iter().find(|s| s.spec.output == "screen").unwrap();
        assert!(screen.last_render.is_some(), "the slide survived the crash");
        // The relaunched engine shows it: a frame for the newest command.
        let sent = engine.command(Command::SetLayer {
            output: "screen".into(),
            kind: LayerKind::Logo,
            layer: Some(Json(
                r##"{"kind":"logo","logo":{"mediaPath":"","background":"#000000"}}"##.into(),
            )),
        });
        assert!(
            engine
                // A relaunched engine on a loaded runner can take a while
                // to bring its window back.
                .wait_frame("screen", sent.seq, Duration::from_secs(20))
                .await,
            "no frame; status {:?}",
            engine.surface_status("screen")
        );
    }
    engine.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_failure_budget_stops_relaunching() {
    let engine = EngineHandle::spawn(options());
    let mut pid = match running(&engine, Duration::from_secs(10)).await {
        Ok(pid) => pid,
        Err(reason) if headless(&reason) => return,
        Err(reason) => panic!("{reason}"),
    };
    // Three crashes inside a minute are relaunched; the fourth is not.
    for _ in 0..3 {
        kill(pid, libc::SIGKILL);
        pid = relaunched(&engine, pid).await;
    }
    kill(pid, libc::SIGKILL);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !matches!(engine.status(), EngineStatus::Failed { .. }) {
        assert!(Instant::now() < deadline, "{:?}", engine.status());
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        engine.unavailable_reason("x").as_deref(),
        Some("Output engine stopped")
    );
    assert!(!engine.command(ndi("late")).accepted || !engine.is_available("late"));
    // A planned restart never spends the budget: proven by the separate
    // planned-restart test below.
}

#[tokio::test(flavor = "multi_thread")]
async fn planned_restarts_do_not_spend_the_budget() {
    let engine = EngineHandle::spawn(options());
    let mut pid = match running(&engine, Duration::from_secs(10)).await {
        Ok(pid) => pid,
        Err(reason) if headless(&reason) => return,
        Err(reason) => panic!("{reason}"),
    };
    engine.command(ndi("ndi"));
    engine.command(Command::SetLayer {
        output: "ndi".into(),
        kind: LayerKind::Message,
        layer: Some(message("hello")),
    });
    for _ in 0..5 {
        engine.restart();
        pid = relaunched(&engine, pid).await;
    }
    let state = engine_state(&engine).await.unwrap();
    assert_eq!(state[0].layers.message, Some(message("hello")));
    engine.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hung_engine_is_killed_and_relaunched() {
    let mut options = options();
    options.heartbeat_every = Duration::from_millis(100);
    options.heartbeat_timeout = Duration::from_millis(600);
    let engine = EngineHandle::spawn(options);
    let pid = match running(&engine, Duration::from_secs(10)).await {
        Ok(pid) => pid,
        Err(reason) if headless(&reason) => return,
        Err(reason) => panic!("{reason}"),
    };
    let mut events = engine.subscribe();
    kill(pid, libc::SIGSTOP);
    let next = relaunched(&engine, pid).await;
    assert_ne!(next, pid);
    let mut saw = false;
    while let Ok(event) = events.try_recv() {
        if let EngineEvent::Lost { reason, planned } = event {
            assert_eq!((reason.as_str(), planned), ("engine unresponsive", false));
            saw = true;
        }
    }
    assert!(saw, "the hang was reported");
    engine.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_missing_binary_fails_within_the_budget() {
    let mut options = EngineOptions::new("/nonexistent/kairo-engine");
    options.launch_retry = Duration::from_millis(500);
    let engine = EngineHandle::spawn(options);
    let reason = running(&engine, Duration::from_secs(5)).await.unwrap_err();
    assert!(
        reason.starts_with("cannot start /nonexistent/kairo-engine"),
        "{reason}"
    );
    assert!(matches!(engine.status(), EngineStatus::Failed { .. }));
    assert!(engine.query(Query::State).await.is_none());
    assert!(engine.ping().await.is_none());
}

// ─── The engine on its own ───────────────────────────────────────────────────

struct Bare {
    child: Child,
    socket: String,
    log: std::sync::mpsc::Receiver<String>,
}

fn bare(grace_ms: u64) -> Bare {
    let socket = format!("kairo-engine-test-{}-{grace_ms}.sock", std::process::id());
    let mut child = Process::new(ENGINE)
        .args(["--socket", &socket, "--grace-ms", &grace_ms.to_string()])
        .env(TOKEN_ENV, "secret")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let (tx, log) = std::sync::mpsc::channel();
    let stderr = child.stderr.take().unwrap();
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let _ = tx.send(line);
        }
    });
    Bare { child, socket, log }
}

impl Bare {
    /// `None` when the engine exited first (no window system).
    fn connect(&mut self) -> Option<Stream> {
        let name = self
            .socket
            .clone()
            .to_ns_name::<GenericNamespaced>()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(stream) = Stream::connect(name.borrow()) {
                return Some(stream);
            }
            if let Ok(Some(status)) = self.child.try_wait() {
                // Only a missing window system may stop the engine here.
                let reason = format!("exit status: {}", status.code().unwrap_or(-1));
                assert!(headless(&reason), "engine exited: {reason}");
                return None;
            }
            assert!(Instant::now() < deadline, "engine never listened");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn hello(&mut self, protocol: u32, token: &str) -> Option<FromEngine> {
        let stream = self.connect()?;
        write_frame(
            &mut &stream,
            &ToEngine::Hello(Hello {
                protocol,
                token: token.into(),
            }),
        )
        .unwrap();
        Some(read_frame(&mut BufReader::new(&stream)).unwrap())
    }

    fn logged(&self, needle: &str) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            match self.log.recv_timeout(Duration::from_millis(100)) {
                Ok(line) if line.contains(needle) => return true,
                _ => {}
            }
        }
        false
    }

    fn exits_within(&mut self, limit: Duration) -> Option<Duration> {
        let start = Instant::now();
        while start.elapsed() < limit {
            if self.child.try_wait().unwrap().is_some() {
                return Some(start.elapsed());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        None
    }
}

impl Drop for Bare {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn version_mismatch_and_bad_tokens_are_rejected_with_a_log_entry() {
    let mut engine = bare(10_000);
    let Some(reply) = engine.hello(PROTOCOL_VERSION + 1, "secret") else {
        return;
    };
    let expected = format!(
        "protocol version mismatch: client {}, engine {}",
        PROTOCOL_VERSION + 1,
        PROTOCOL_VERSION
    );
    assert_eq!(
        reply,
        FromEngine::Rejected {
            reason: expected.clone()
        }
    );
    assert!(engine.logged(&expected), "log entry for the mismatch");

    assert_eq!(
        engine.hello(PROTOCOL_VERSION, "wrong").unwrap(),
        FromEngine::Rejected {
            reason: "bad handshake token".into()
        }
    );
    assert!(engine.logged("bad handshake token"));

    // A malformed first frame is refused too, and the engine keeps serving.
    let stream = engine.connect().unwrap();
    use std::io::Write;
    (&stream)
        .write_all(&[3, 0, 0, 0, 0xff, 0xff, 0xff])
        .unwrap();
    assert!(matches!(
        read_frame::<FromEngine>(&mut BufReader::new(&stream)).unwrap(),
        FromEngine::Rejected { .. }
    ));
    assert!(matches!(
        engine.hello(PROTOCOL_VERSION, "secret").unwrap(),
        FromEngine::Welcome(_)
    ));
}

#[test]
fn the_engine_holds_its_picture_for_the_grace_period_then_exits() {
    let mut engine = bare(800);
    let Some(welcome) = engine.hello(PROTOCOL_VERSION, "secret") else {
        return;
    };
    assert!(matches!(welcome, FromEngine::Welcome(_)));
    // `hello` dropped the connection: the UI is gone.
    let exited = engine.exits_within(Duration::from_secs(5)).expect("exits");
    assert!(exited >= Duration::from_millis(600), "held for {exited:?}");
    assert!(engine.logged("holding the picture"));
}

#[test]
fn a_client_reconnecting_within_the_grace_period_keeps_the_engine() {
    let mut engine = bare(1_000);
    let Some(_) = engine.hello(PROTOCOL_VERSION, "secret") else {
        return;
    };
    std::thread::sleep(Duration::from_millis(300));
    let stream = engine.connect().unwrap();
    write_frame(
        &mut &stream,
        &ToEngine::Hello(Hello {
            protocol: PROTOCOL_VERSION,
            token: "secret".into(),
        }),
    )
    .unwrap();
    let mut reader = BufReader::new(&stream);
    assert!(matches!(
        read_frame::<FromEngine>(&mut reader).unwrap(),
        FromEngine::Welcome(_)
    ));
    assert_eq!(engine.exits_within(Duration::from_millis(1_500)), None);
    write_frame(&mut &stream, &ToEngine::Ping { nonce: 5 }).unwrap();
    loop {
        if read_frame::<FromEngine>(&mut reader).unwrap() == (FromEngine::Pong { nonce: 5 }) {
            break;
        }
    }
}

#[test]
fn an_engine_no_client_ever_reaches_exits_after_the_grace_period() {
    let mut engine = bare(500);
    if engine.connect().is_none() {
        return;
    }
    // Connected but never said hello: that is not a client.
    assert!(engine.exits_within(Duration::from_secs(5)).is_some());
}

#[test]
fn missing_arguments_and_token_exit_with_usage_errors() {
    let status = Process::new(ENGINE).stderr(Stdio::null()).status().unwrap();
    assert_eq!(status.code(), Some(64));
    let status = Process::new(ENGINE)
        .args(["--socket", "x"])
        .env_remove(TOKEN_ENV)
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(64));
}

/// A UI that crashes takes the engine's stderr pipe with it. Logging the
/// disconnect into that dead pipe used to panic the client thread, so the
/// grace period never started and the engine stayed up for good.
#[test]
fn the_grace_period_starts_even_when_stderr_is_gone() {
    let mut engine = bare(500);
    // Nobody reads stderr any more: close the pipe's read end.
    let mut child = std::mem::replace(&mut engine.child, Process::new("true").spawn().unwrap());
    let socket = engine.socket.clone();
    drop(engine);
    drop(child.stderr.take());
    let name = socket.to_ns_name::<GenericNamespaced>().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let stream = loop {
        if let Ok(stream) = Stream::connect(name.borrow()) {
            break stream;
        }
        if let Ok(Some(status)) = child.try_wait() {
            if headless(&format!("exit status: {}", status.code().unwrap_or(-1))) {
                return;
            }
            panic!("engine exited: {status}");
        }
        assert!(Instant::now() < deadline, "engine never listened");
        std::thread::sleep(Duration::from_millis(5));
    };
    write_frame(
        &mut &stream,
        &ToEngine::Hello(Hello {
            protocol: PROTOCOL_VERSION,
            token: "secret".into(),
        }),
    )
    .unwrap();
    assert!(matches!(
        read_frame::<FromEngine>(&mut BufReader::new(&stream)).unwrap(),
        FromEngine::Welcome(_)
    ));
    drop(stream);
    let start = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if start.elapsed() > Duration::from_secs(5) {
            let _ = child.kill();
            panic!("engine still running 5 s after its client left");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A message layer as `programLayersFor` sends it.
fn message(text: &str) -> Json {
    Json(
        serde_json::json!({
            "kind": "message",
            "text": text,
            "style": {"position": "bottom", "fontSizePx": 44, "color": "#ffffff",
                      "background": "rgba(0,0,0,0.72)", "scroll": false}
        })
        .to_string(),
    )
}

/// A build can replace the engine binary while the app runs: a binary
/// missing for a moment is waited for and never spends the failure budget.
#[tokio::test(flavor = "multi_thread")]
async fn an_engine_binary_missing_for_a_moment_still_starts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir
        .path()
        .join(format!("kairo-engine{}", std::env::consts::EXE_SUFFIX));
    let mut options = EngineOptions::new(&path);
    options.grace = Duration::from_secs(2);
    let engine = EngineHandle::spawn(options);
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(!matches!(engine.status(), EngineStatus::Failed { .. }));
    std::fs::copy(ENGINE, &path).unwrap();
    match running(&engine, Duration::from_secs(15)).await {
        Ok(_) => {}
        Err(reason) if headless(&reason) => return,
        Err(reason) => panic!("{reason}"),
    }
    engine.shutdown();
}
