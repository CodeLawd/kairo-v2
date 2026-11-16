//! `soak`: a scripted service against the real `kairo-engine`, with chaos
//! (prompt 108). See `tools/soak/README.md`.
//!
//! ```text
//! cargo build --release -p kairo-engine -p soak
//! cargo build --release -p kairo-sinks --example decklink_sim
//! target/release/soak --label clean
//! target/release/soak --label chaos --chaos
//! ```

mod chaos;
mod deepgram;
mod monitor;
mod procs;
mod report;
mod service;
mod web;

use kairo_core::AppCore;
use kairo_core::engine::{EngineHandle, EngineOptions, EngineStatus};
use kairo_core::resilience::{ResilienceActor, SttEvent as HealthEvent};
use kairo_engine_proto::*;
use kairo_stt::deepgram::{DeepgramStt, SttEvent, Timings};
use kairo_stt::relay::AudioRelay;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

/// Owner limit: no run longer than 10 minutes (DECISIONS 2026-11-14).
const MAX_MINUTES: f64 = 10.0;
const HELPER_NAME: &str = "decklink_sim";
const NDI_SDK_RUNTIME: &str = "/Library/NDI SDK for Apple/lib/macOS/libndi.dylib";

#[derive(Debug, Clone, PartialEq)]
struct Args {
    minutes: f64,
    chaos: bool,
    os_sleep: bool,
    label: String,
    out: Option<PathBuf>,
    engine: Option<PathBuf>,
    helper: Option<PathBuf>,
    sample_every: Duration,
}

fn parse(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut parsed = Args {
        minutes: MAX_MINUTES,
        chaos: false,
        os_sleep: false,
        label: "run".into(),
        out: None,
        engine: None,
        helper: None,
        sample_every: Duration::from_secs(5),
    };
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{name} needs a value"));
        match arg.as_str() {
            "--minutes" => {
                parsed.minutes = value("--minutes")?
                    .parse()
                    .map_err(|_| "--minutes needs a number")?;
            }
            "--chaos" => parsed.chaos = true,
            "--os-sleep" => parsed.os_sleep = true,
            "--label" => parsed.label = value("--label")?,
            "--out" => parsed.out = Some(value("--out")?.into()),
            "--engine" => parsed.engine = Some(value("--engine")?.into()),
            "--helper" => parsed.helper = Some(value("--helper")?.into()),
            "--sample-secs" => {
                let secs: u64 = value("--sample-secs")?
                    .parse()
                    .map_err(|_| "--sample-secs needs a whole number")?;
                if secs == 0 {
                    return Err("--sample-secs must be at least 1".into());
                }
                parsed.sample_every = Duration::from_secs(secs);
            }
            "-h" | "--help" => return Err(USAGE.into()),
            other => return Err(format!("unknown argument {other}\n{USAGE}")),
        }
    }
    if !(parsed.minutes > 0.0 && parsed.minutes <= MAX_MINUTES) {
        return Err(format!(
            "--minutes must be above 0 and at most {MAX_MINUTES} (owner limit); loop the tool for longer runs"
        ));
    }
    if parsed.os_sleep && !parsed.chaos {
        return Err("--os-sleep is a chaos event; add --chaos".into());
    }
    if parsed.label.is_empty() || parsed.label.contains(['/', '\\']) {
        return Err("--label must be a plain name".into());
    }
    Ok(parsed)
}

const USAGE: &str =
    "usage: soak [--minutes M (≤10)] [--chaos [--os-sleep]] [--label NAME] [--out DIR]
            [--engine PATH] [--helper PATH] [--sample-secs N]";

fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
        .unwrap_or_default()
}

/// UTC `YYYY-MM-DDTHH:MM:SSZ` from the system clock.
fn utc_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

fn main() -> ExitCode {
    if let Some(code) = procs::relaunch_disclaimed() {
        return ExitCode::from(code.clamp(0, 255) as u8);
    }
    tracing_subscriber::fmt()
        .with_target(false)
        .with_max_level(tracing::Level::INFO)
        .init();
    let args = match parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(64);
        }
    };
    let engine_binary = args
        .engine
        .clone()
        .unwrap_or_else(|| exe_dir().join("kairo-engine"));
    if !engine_binary.exists() {
        eprintln!(
            "no engine at {}; build it with `cargo build --release -p kairo-engine` or pass --engine",
            engine_binary.display()
        );
        return ExitCode::from(66);
    }
    let helper = args
        .helper
        .clone()
        .unwrap_or_else(|| exe_dir().join("examples").join(HELPER_NAME))
        .canonicalize()
        .ok();
    // The engine inherits these. Set before any other thread exists.
    // SAFETY: single-threaded at this point.
    unsafe {
        if std::env::var_os("KAIRO_NDI_RUNTIME").is_none()
            && std::path::Path::new(NDI_SDK_RUNTIME).exists()
        {
            std::env::set_var("KAIRO_NDI_RUNTIME", NDI_SDK_RUNTIME);
        }
        if let Some(helper) = &helper {
            std::env::set_var("KAIRO_ENABLE_DECKLINK", "1");
            std::env::set_var("KAIRO_DECKLINK_HELPER", helper);
        }
    }
    let core = match AppCore::new() {
        Ok(core) => core,
        Err(error) => {
            eprintln!("could not start the core runtime: {error}");
            return ExitCode::from(70);
        }
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("could not start the runtime: {error}");
            return ExitCode::from(70);
        }
    };
    let result = runtime.block_on(soak(args, engine_binary, helper.is_some(), core.clone()));
    drop(runtime);
    drop(core);
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(message) => {
            eprintln!("soak could not run: {message}");
            ExitCode::from(2)
        }
    }
}

fn health_event(event: &SttEvent) -> Option<HealthEvent> {
    Some(match event {
        SttEvent::Connected => HealthEvent::Connected,
        SttEvent::Disconnected => HealthEvent::Disconnected,
        SttEvent::Reconnecting { attempt, delay_ms } => HealthEvent::Reconnecting {
            attempt: *attempt,
            delay_ms: *delay_ms,
        },
        SttEvent::Error { message, .. } => HealthEvent::Error(message.clone()),
        _ => return None,
    })
}

async fn soak(
    args: Args,
    engine_binary: PathBuf,
    card: bool,
    core: AppCore,
) -> Result<bool, String> {
    let length = Duration::from_secs_f64(args.minutes * 60.0);
    let scratch = tempfile_dir()?;
    let user_data = scratch.join("profile");
    std::fs::create_dir_all(&user_data).map_err(|e| e.to_string())?;
    tracing::info!("making fixtures in {}", scratch.display());
    let fixtures = {
        let dir = scratch.clone();
        tokio::task::spawn_blocking(move || service::make_fixtures(&dir))
            .await
            .map_err(|e| e.to_string())??
    };

    // Resilience, transcription and its stand-in servers.
    ResilienceActor::register(&core, &user_data, NonZeroUsize::new(64).expect("non-zero"))
        .await
        .map_err(|e| format!("resilience: {e}"))?;
    let server = Arc::new(
        deepgram::FakeDeepgram::start()
            .await
            .map_err(|e| e.to_string())?,
    );
    let proxy = Arc::new(
        deepgram::Proxy::start(server.addr)
            .await
            .map_err(|e| e.to_string())?,
    );
    let access = Arc::new(deepgram::Access::new(proxy.cut_flag()));
    let stt = DeepgramStt::with_endpoint(
        access,
        format!("ws://{}/v1/listen", proxy.addr),
        Timings::default(),
    );
    let start = Instant::now();
    let collector = monitor::Collector::new(start);
    {
        let mut events = stt.subscribe();
        let (core, collector) = (core.clone(), collector.clone());
        tokio::spawn(async move {
            loop {
                match events.recv().await {
                    Ok(event) => {
                        if let Some(health) = health_event(&event) {
                            collector.record_stt(format!("{event:?}"));
                            let _ = core.call(health).await;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(_) => return,
                }
            }
        });
    }
    let relay = AudioRelay::default();
    stt.configure("en", Vec::new());
    stt.attach_stream(relay.create_stream());
    stt.connect().await;
    // 16 kHz mono PCM16 in 100 ms chunks, as the capture path sends.
    let feeder = tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_millis(100));
        loop {
            tick.tick().await;
            relay.feed(vec![0u8; 3_200]);
        }
    });
    let web_url = web::serve().await.map_err(|e| e.to_string())?;

    // The engine and its outputs.
    let engine = EngineHandle::spawn(EngineOptions::new(&engine_binary));
    monitor::follow(&engine, collector.clone());
    let running = procs::until(Duration::from_secs(20), || {
        matches!(
            engine.status(),
            EngineStatus::Running { .. } | EngineStatus::Failed { .. }
        )
    })
    .await;
    if !running || !matches!(engine.status(), EngineStatus::Running { .. }) {
        let status = engine.status();
        tokio::task::spawn_blocking(move || engine.shutdown())
            .await
            .ok();
        return Err(format!("the engine did not start: {status:?}"));
    }
    procs::until(Duration::from_secs(3), || !engine.displays().is_empty()).await;
    let display = engine.displays().first().cloned();
    let candidates = service::candidate_outputs(display.as_ref(), card, &args.label);
    for (_, spec) in &candidates {
        engine.command(Command::OpenSurface(spec.clone()));
    }
    // "Output unavailable" is the engine's interim answer while a sink
    // (a card's helper, say) is still being acquired.
    let settled = |status: Option<SurfaceStatus>| match status {
        None => false,
        Some(SurfaceStatus::Unavailable { reason }) => reason != "Output unavailable",
        Some(_) => true,
    };
    procs::until(Duration::from_secs(15), || {
        candidates
            .iter()
            .all(|(o, _)| settled(engine.surface_status(&o.id)))
    })
    .await;
    let mut ready = Vec::new();
    let mut unavailable = Vec::new();
    for (output, _) in &candidates {
        match engine.surface_status(&output.id) {
            Some(SurfaceStatus::Ready) => ready.push(output.clone()),
            other => {
                let reason = match other {
                    Some(
                        SurfaceStatus::Unavailable { reason } | SurfaceStatus::Parked { reason },
                    ) => reason,
                    _ => "no status from the engine within 15 s".into(),
                };
                unavailable.push((output.id.clone(), reason));
                engine.command(Command::CloseSurface {
                    output: output.id.clone(),
                    destroy: true,
                });
            }
        }
    }
    if ready.is_empty() {
        tokio::task::spawn_blocking(move || engine.shutdown())
            .await
            .ok();
        return Err(format!("no output is available: {unavailable:?}"));
    }
    let web_output = service::web_output(&ready);
    let ids: Vec<String> = ready.iter().map(|o| o.id.clone()).collect();
    tracing::info!("outputs ready: {ids:?}; unavailable: {unavailable:?}; web on {web_output:?}");

    // The script (UI) thread.
    let counters = Arc::new(service::Counters::default());
    let (script_tx, script_rx) = mpsc::channel();
    let script = service::Script {
        engine: engine.clone(),
        outputs: ids.clone(),
        web: web_output.clone().map(|o| (o, web_url.clone())),
        fixtures,
        counters: counters.clone(),
        start,
    };
    let script_thread = std::thread::Builder::new()
        .name("soak-script".into())
        .spawn(move || script.run(script_rx))
        .map_err(|e| e.to_string())?;

    let samples: Arc<Mutex<Vec<monitor::Sample>>> = Arc::default();
    let sampler = {
        let server = server.clone();
        tokio::spawn(monitor::sampler(
            collector.clone(),
            engine.clone(),
            HELPER_NAME.into(),
            move || server.received(),
            args.sample_every,
            samples.clone(),
        ))
    };

    let ctx = Arc::new(chaos::Ctx {
        engine: engine.clone(),
        collector: collector.clone(),
        core: core.clone(),
        user_data: user_data.clone(),
        stt: stt.clone(),
        server: server.clone(),
        proxy: proxy.clone(),
        script: script_tx.clone(),
        outputs: ids.clone(),
        display: ready
            .iter()
            .find(|o| o.kind == service::OutputKind::Display)
            .map(|o| o.id.clone()),
        card: ready
            .iter()
            .find(|o| o.kind == service::OutputKind::Card)
            .map(|o| o.id.clone()),
        web: web_output.clone(),
        helper_name: HELPER_NAME.into(),
    });
    let results: Arc<Mutex<Vec<chaos::Result>>> = Arc::default();
    let chaos_task = args.chaos.then(|| {
        let (ctx, results) = (ctx.clone(), results.clone());
        let events = chaos::schedule(length, args.os_sleep);
        tokio::spawn(async move {
            for (at, kind) in events {
                tokio::time::sleep_until((start + at).into()).await;
                let result = chaos::run(&ctx, kind).await;
                results
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push(result);
            }
        })
    });

    tokio::time::sleep_until((start + length).into()).await;
    if let Some(task) = chaos_task {
        // Every event fits the run; give a late recovery check a moment.
        let _ = tokio::time::timeout(Duration::from_secs(30), task).await;
    }

    // Wind down and report.
    let _ = script_tx.send(service::Control::Stop);
    let _ = tokio::task::spawn_blocking(move || script_thread.join()).await;
    sampler.abort();
    let last = {
        let (c, e) = (collector.clone(), engine.clone());
        let bytes = server.received();
        tokio::task::spawn_blocking(move || monitor::sample(&c, &e, HELPER_NAME, bytes))
            .await
            .map_err(|e| e.to_string())?
    };
    let samples = {
        let mut all = samples.lock().unwrap_or_else(|p| p.into_inner()).clone();
        all.push(last);
        all
    };
    feeder.abort();
    stt.disconnect();
    drop(ctx);
    {
        let engine = engine.clone();
        tokio::task::spawn_blocking(move || engine.shutdown())
            .await
            .map_err(|e| e.to_string())?;
    }
    let results = results.lock().unwrap_or_else(|p| p.into_inner()).clone();
    let state = collector.state();
    let verdict = report::verdict(&samples, &results, state.engine_failed.as_ref(), args.chaos);
    let script_counts = [
        ("commands", counters.sent.load(Ordering::Relaxed)),
        ("refused", counters.refused.load(Ordering::Relaxed)),
        ("slides", counters.slides.load(Ordering::Relaxed)),
        ("page turns", counters.page_turns.load(Ordering::Relaxed)),
        (
            "layer updates",
            counters.layer_updates.load(Ordering::Relaxed),
        ),
    ]
    .into_iter()
    .collect();
    let report = report::Report {
        label: &args.label,
        started: utc_now(),
        minutes: start.elapsed().as_secs_f64() / 60.0,
        chaos: args.chaos,
        engine: engine_binary.display().to_string(),
        outputs_ready: ids,
        outputs_unavailable: unavailable,
        web_output,
        verdict: &verdict,
        chaos_results: &results,
        sinks: state.sinks.clone(),
        script: script_counts,
        engine_errors: state.engine_errors.clone(),
        launches: state.launches.clone(),
        losses: state.losses.clone(),
        statuses: state.statuses.clone(),
        stt: state.stt.clone(),
        swaps_refused: state.swaps_refused,
        samples: &samples,
    };
    drop(state);
    let out = args
        .out
        .clone()
        .unwrap_or_else(|| PathBuf::from("native-migration/evidence/108").join(&args.label));
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let json = serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?;
    std::fs::write(out.join("report.json"), json).map_err(|e| e.to_string())?;
    std::fs::write(out.join("report.md"), report::markdown(&report)).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(&scratch);
    println!(
        "{} — report in {}",
        if verdict.pass { "PASS" } else { "FAIL" },
        out.display()
    );
    Ok(verdict.pass)
}

/// A private scratch folder for fixtures and the profile.
fn tempfile_dir() -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join(format!("kairo-soak-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Result<Args, String> {
        parse(list.iter().map(|s| s.to_string()))
    }

    #[test]
    fn defaults_are_a_ten_minute_clean_run() {
        let a = args(&[]).unwrap();
        assert_eq!(a.minutes, 10.0);
        assert!(!a.chaos && !a.os_sleep);
        assert_eq!(a.sample_every, Duration::from_secs(5));
    }

    #[test]
    fn runs_over_ten_minutes_are_refused() {
        assert!(args(&["--minutes", "10.5"]).is_err());
        assert!(args(&["--minutes", "0"]).is_err());
        assert!(args(&["--minutes", "-1"]).is_err());
        assert!(args(&["--minutes", "NaN"]).is_err());
        assert!(args(&["--minutes", "x"]).is_err());
        assert_eq!(args(&["--minutes", "2.5"]).unwrap().minutes, 2.5);
    }

    #[test]
    fn bad_arguments_are_refused() {
        assert!(args(&["--os-sleep"]).is_err());
        assert!(args(&["--chaos", "--os-sleep"]).unwrap().os_sleep);
        assert!(args(&["--label", "a/b"]).is_err());
        assert!(args(&["--label"]).is_err());
        assert!(args(&["--sample-secs", "0"]).is_err());
        assert!(args(&["--what"]).is_err());
        assert_eq!(
            args(&["--engine", "/x", "--out", "o"]).unwrap().engine,
            Some(PathBuf::from("/x"))
        );
    }

    #[test]
    fn speech_events_map_to_health() {
        assert!(matches!(
            health_event(&SttEvent::Reconnecting {
                attempt: 2,
                delay_ms: 2000
            }),
            Some(HealthEvent::Reconnecting {
                attempt: 2,
                delay_ms: 2000
            })
        ));
        assert!(matches!(
            health_event(&SttEvent::Error {
                message: "x".into(),
                fatal: true
            }),
            Some(HealthEvent::Error(m)) if m == "x"
        ));
        assert!(health_event(&SttEvent::UtteranceEnd(1.0)).is_none());
    }

    #[test]
    fn utc_dates_are_formatted() {
        let now = utc_now();
        assert_eq!(now.len(), 20);
        assert!(now.starts_with("20") && now.ends_with('Z'), "{now}");
    }
}
