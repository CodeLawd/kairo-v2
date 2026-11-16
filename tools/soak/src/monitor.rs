//! What the soak records: every engine event that matters, transcription
//! events, and a resource sample every few seconds.

use crate::procs;
use kairo_core::engine::{EngineEvent, EngineHandle, EngineStatus};
use kairo_engine_proto::{Event, SurfaceStatus};
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Cumulative per-sink frame counts across engine relaunches. The engine's
/// counters start again from zero in each new process.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct SinkTotals {
    pub produced: u64,
    pub dropped: u64,
    pub late: u64,
    #[serde(skip)]
    last: (u64, u64, u64),
}

impl SinkTotals {
    pub fn update(&mut self, produced: u64, dropped: u64, late: u64) {
        fn add(total: &mut u64, last: u64, now: u64) {
            // A smaller count is a new engine process: it counts from zero.
            *total += if now >= last { now - last } else { now };
        }
        add(&mut self.produced, self.last.0, produced);
        add(&mut self.dropped, self.last.1, dropped);
        add(&mut self.late, self.last.2, late);
        self.last = (produced, dropped, late);
    }

    /// A relaunched engine counts from zero.
    pub fn engine_restarted(&mut self) {
        self.last = (0, 0, 0);
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Logged {
    pub at_s: f64,
    pub what: String,
}

#[derive(Default)]
pub struct State {
    pub sinks: BTreeMap<String, SinkTotals>,
    pub engine_errors: Vec<Logged>,
    pub launches: Vec<Logged>,
    pub losses: Vec<Logged>,
    pub statuses: Vec<Logged>,
    pub capture: Vec<(f64, serde_json::Value)>,
    pub swaps_refused: u64,
    pub stt: Vec<Logged>,
    /// Set if the supervisor ever gave up.
    pub engine_failed: Option<Logged>,
}

pub struct Collector {
    pub start: Instant,
    state: Mutex<State>,
}

impl Collector {
    pub fn new(start: Instant) -> Arc<Self> {
        Arc::new(Self {
            start,
            state: Mutex::default(),
        })
    }

    pub fn at(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    pub fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn log(&self, what: String) -> Logged {
        Logged {
            at_s: self.at(),
            what,
        }
    }

    pub fn record(&self, event: EngineEvent) {
        let logged_at = self.at();
        let mut state = self.state();
        match event {
            EngineEvent::Launched { pid, relaunch } => {
                for sink in state.sinks.values_mut() {
                    sink.engine_restarted();
                }
                state.launches.push(Logged {
                    at_s: logged_at,
                    what: format!("pid {pid} relaunch={relaunch}"),
                });
            }
            EngineEvent::Lost { reason, planned } => state.losses.push(Logged {
                at_s: logged_at,
                what: format!("{reason} (planned={planned})"),
            }),
            EngineEvent::Engine(Event::Stats(stats)) => {
                for sink in stats {
                    state.sinks.entry(sink.output).or_default().update(
                        sink.produced,
                        sink.dropped,
                        sink.late,
                    );
                }
            }
            EngineEvent::Engine(Event::Error { output, message }) => {
                state.engine_errors.push(Logged {
                    at_s: logged_at,
                    what: format!("{}: {message}", output.as_deref().unwrap_or("engine")),
                });
            }
            EngineEvent::Engine(Event::SurfaceStatus { output, status }) => {
                let text = match status {
                    SurfaceStatus::Ready => "ready".to_owned(),
                    SurfaceStatus::Parked { reason } => format!("parked: {reason}"),
                    SurfaceStatus::Unavailable { reason } => format!("unavailable: {reason}"),
                };
                state.statuses.push(Logged {
                    at_s: logged_at,
                    what: format!("{output}: {text}"),
                });
            }
            EngineEvent::Engine(Event::Capture(json)) => {
                if let Ok(value) = json.decode::<serde_json::Value>() {
                    state.capture.push((logged_at, value));
                }
            }
            EngineEvent::Engine(Event::PageSwapped { swapped: false, .. }) => {
                state.swaps_refused += 1;
            }
            _ => {}
        }
    }

    pub fn record_stt(&self, what: String) {
        let logged = self.log(what);
        self.state().stt.push(logged);
    }

    pub fn record_status(&self, status: &EngineStatus) {
        if let EngineStatus::Failed { reason } = status {
            let logged = self.log(reason.clone());
            let mut state = self.state();
            if state.engine_failed.is_none() {
                state.engine_failed = Some(logged);
            }
        }
    }

    pub fn totals(&self) -> SinkTotals {
        let state = self.state();
        let mut sum = SinkTotals::default();
        for sink in state.sinks.values() {
            sum.produced += sink.produced;
            sum.dropped += sink.dropped;
            sum.late += sink.late;
        }
        sum
    }

    pub fn produced(&self, output: &str) -> u64 {
        self.state().sinks.get(output).map_or(0, |s| s.produced)
    }
}

/// Follows the engine's events and status until the handle is dropped.
pub fn follow(engine: &EngineHandle, collector: Arc<Collector>) {
    let mut events = engine.subscribe();
    let sink = collector.clone();
    tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => sink.record(event),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    sink.record_stt(format!("collector lagged {n} engine events"));
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    });
    let mut status = engine.watch_status();
    tokio::spawn(async move {
        loop {
            let now = status.borrow_and_update().clone();
            collector.record_status(&now);
            if status.changed().await.is_err() {
                return;
            }
        }
    });
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Sample {
    pub t_s: f64,
    /// The soak process: it holds the supervisor, transcription and
    /// resilience, as the `kairo` UI process does (without GPUI).
    pub client_rss: Option<u64>,
    pub client_fds: Option<u64>,
    pub engine_pid: Option<u32>,
    pub engine_rss: Option<u64>,
    pub engine_fds: Option<u64>,
    pub helper_rss: u64,
    pub helpers: usize,
    pub web_content_rss: u64,
    pub web_contents: usize,
    /// System-wide GPU memory in use (Apple `IOAccelerator`), not per process.
    pub gpu_in_use: Option<u64>,
    pub produced: u64,
    pub dropped: u64,
    pub late: u64,
    pub stt_bytes: u64,
}

/// `"In use system memory"` of the first accelerator `ioreg` lists.
pub fn gpu_in_use() -> Option<u64> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let output = std::process::Command::new("ioreg")
        .args(["-r", "-d", "1", "-w", "0", "-c", "IOAccelerator"])
        .output()
        .ok()?;
    parse_gpu_in_use(&String::from_utf8_lossy(&output.stdout))
}

pub fn parse_gpu_in_use(text: &str) -> Option<u64> {
    let key = "\"In use system memory\"=";
    let start = text.find(key)? + key.len();
    let digits: String = text[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

pub fn sample(
    collector: &Collector,
    engine: &EngineHandle,
    helper_name: &str,
    stt_bytes: u64,
) -> Sample {
    let me = std::process::id();
    let engine_pid = engine.pid();
    let (helpers, web) = match engine_pid {
        Some(pid) => (
            procs::children_named(pid, helper_name),
            procs::own_web_content(),
        ),
        None => (Vec::new(), Vec::new()),
    };
    let totals = collector.totals();
    Sample {
        t_s: collector.at(),
        client_rss: procs::rss_bytes(me),
        client_fds: procs::fd_count(me),
        engine_pid,
        engine_rss: engine_pid.and_then(procs::rss_bytes),
        engine_fds: engine_pid.and_then(procs::fd_count),
        helper_rss: helpers.iter().filter_map(|p| procs::rss_bytes(p.pid)).sum(),
        helpers: helpers.len(),
        web_content_rss: web.iter().filter_map(|p| procs::rss_bytes(p.pid)).sum(),
        web_contents: web.len(),
        gpu_in_use: gpu_in_use(),
        produced: totals.produced,
        dropped: totals.dropped,
        late: totals.late,
        stt_bytes,
    }
}

/// Samples every `every` until `stop` resolves.
pub async fn sampler(
    collector: Arc<Collector>,
    engine: EngineHandle,
    helper_name: String,
    stt_bytes: impl Fn() -> u64 + Send + 'static,
    every: Duration,
    samples: Arc<Mutex<Vec<Sample>>>,
) {
    let mut tick = tokio::time::interval(every);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        let (c, e, h, bytes) = (
            collector.clone(),
            engine.clone(),
            helper_name.clone(),
            stt_bytes(),
        );
        // `ioreg` and the process walk block; keep them off the runtime.
        if let Ok(sample) = tokio::task::spawn_blocking(move || sample(&c, &e, &h, bytes)).await {
            samples
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push(sample);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn totals_survive_an_engine_relaunch() {
        let mut sink = SinkTotals::default();
        sink.update(100, 1, 2);
        sink.update(250, 1, 5);
        // The relaunched engine reports again from zero, before or after
        // the launch event arrives.
        sink.update(30, 0, 0);
        assert_eq!((sink.produced, sink.dropped, sink.late), (280, 1, 5));
        sink.engine_restarted();
        sink.update(60, 2, 1);
        assert_eq!((sink.produced, sink.dropped, sink.late), (340, 3, 6));
    }

    #[test]
    fn the_collector_follows_stats_errors_and_launches() {
        let collector = Collector::new(Instant::now());
        collector.record(EngineEvent::Engine(Event::Stats(vec![
            kairo_engine_proto::SinkStats {
                output: "a".into(),
                produced: 10,
                dropped: 1,
                late: 0,
            },
        ])));
        collector.record(EngineEvent::Launched {
            pid: 9,
            relaunch: true,
        });
        collector.record(EngineEvent::Engine(Event::Stats(vec![
            kairo_engine_proto::SinkStats {
                output: "a".into(),
                produced: 5,
                dropped: 0,
                late: 2,
            },
        ])));
        collector.record(EngineEvent::Engine(Event::Error {
            output: Some("a".into()),
            message: "boom".into(),
        }));
        collector.record(EngineEvent::Engine(Event::PageSwapped {
            output: "a".into(),
            seq: 3,
            swapped: false,
        }));
        collector.record_status(&EngineStatus::Failed {
            reason: "budget".into(),
        });
        let totals = collector.totals();
        assert_eq!((totals.produced, totals.dropped, totals.late), (15, 1, 2));
        assert_eq!(collector.produced("a"), 15);
        assert_eq!(collector.produced("missing"), 0);
        let state = collector.state();
        assert_eq!(state.engine_errors[0].what, "a: boom");
        assert_eq!(state.launches.len(), 1);
        assert_eq!(state.swaps_refused, 1);
        assert_eq!(state.engine_failed.as_ref().unwrap().what, "budget");
    }

    #[test]
    fn gpu_memory_is_read_from_ioreg_text() {
        let text = r#"  "PerformanceStatistics" = {"Alloc system memory"=123,"In use system memory"=987654,"Other"=1}"#;
        assert_eq!(parse_gpu_in_use(text), Some(987_654));
        assert_eq!(parse_gpu_in_use("nothing here"), None);
        assert_eq!(parse_gpu_in_use(r#""In use system memory"=x"#), None);
    }
}
