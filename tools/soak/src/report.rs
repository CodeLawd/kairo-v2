//! Leak-trend analysis and the written report.

use crate::chaos::{Outcome, Result as ChaosResult};
use crate::monitor::{Logged, Sample, SinkTotals};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// Growth is measured from the second service cycle, once every segment
/// (lyrics, verses, documents, messages) has run once (owner, DECISIONS
/// 2026-11-16 "108 — Leak baseline starts at the second cycle"; it replaces
/// the 2-minute warm-up).
pub const WARM_UP_S: f64 = crate::service::CYCLE.as_secs() as f64;
/// Budget: RSS growth under 5% after the warm-up.
pub const MAX_GROWTH: f64 = 0.05;
const WINDOW_S: f64 = 60.0;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Growth {
    /// Median over the minute after the warm-up.
    pub baseline: f64,
    /// Median over the last minute.
    pub end: f64,
    pub fraction: f64,
    /// Least-squares slope after the warm-up, per minute.
    pub slope_per_min: f64,
    pub samples: usize,
}

fn median(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    Some(if values.len().is_multiple_of(2) {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    })
}

/// `None` when the series does not cover a baseline minute and a separate
/// final minute after the warm-up.
pub fn growth(points: &[(f64, f64)]) -> Option<Growth> {
    let after: Vec<(f64, f64)> = points
        .iter()
        .copied()
        .filter(|(t, _)| *t >= WARM_UP_S)
        .collect();
    let last = after.last()?.0;
    if last - WARM_UP_S < 2.0 * WINDOW_S {
        return None;
    }
    let baseline = median(
        after
            .iter()
            .filter(|(t, _)| *t < WARM_UP_S + WINDOW_S)
            .map(|p| p.1)
            .collect(),
    )?;
    let end = median(
        after
            .iter()
            .filter(|(t, _)| *t >= last - WINDOW_S)
            .map(|p| p.1)
            .collect(),
    )?;
    let n = after.len() as f64;
    let (mt, mv) = (
        after.iter().map(|p| p.0).sum::<f64>() / n,
        after.iter().map(|p| p.1).sum::<f64>() / n,
    );
    let var: f64 = after.iter().map(|p| (p.0 - mt).powi(2)).sum();
    let cov: f64 = after.iter().map(|p| (p.0 - mt) * (p.1 - mv)).sum();
    let slope = if var > 0.0 { cov / var * 60.0 } else { 0.0 };
    Some(Growth {
        baseline,
        end,
        fraction: if baseline > 0.0 {
            (end - baseline) / baseline
        } else {
            0.0
        },
        slope_per_min: slope,
        samples: after.len(),
    })
}

pub fn client_rss(samples: &[Sample]) -> Vec<(f64, f64)> {
    samples
        .iter()
        .filter_map(|s| Some((s.t_s, s.client_rss? as f64)))
        .collect()
}

pub fn client_fds(samples: &[Sample]) -> Vec<(f64, f64)> {
    samples
        .iter()
        .filter_map(|s| Some((s.t_s, s.client_fds? as f64)))
        .collect()
}

/// The engine series of the process that ran from the warm-up to the end;
/// empty when it was relaunched inside that window.
pub fn engine_series(samples: &[Sample], pick: impl Fn(&Sample) -> Option<u64>) -> Vec<(f64, f64)> {
    let Some(last_pid) = samples.last().and_then(|s| s.engine_pid) else {
        return Vec::new();
    };
    let window_ok = samples
        .iter()
        .filter(|s| s.t_s >= WARM_UP_S)
        .all(|s| s.engine_pid == Some(last_pid));
    if !window_ok {
        return Vec::new();
    }
    samples
        .iter()
        .filter(|s| s.engine_pid == Some(last_pid))
        .filter_map(|s| Some((s.t_s, pick(s)? as f64)))
        .collect()
}

/// Handles leak when the final minute holds more than 10 (or 5%) above the
/// baseline minute.
pub fn handles_leak(g: &Growth) -> bool {
    g.end - g.baseline > (g.baseline * MAX_GROWTH).max(10.0)
}

#[derive(Clone, Debug, Serialize)]
pub struct Verdict {
    pub client_rss: Option<Growth>,
    pub engine_rss: Option<Growth>,
    pub client_fds: Option<Growth>,
    pub engine_fds: Option<Growth>,
    pub gpu_in_use: Option<Growth>,
    pub unrecovered: Vec<String>,
    pub not_verified: Vec<String>,
    pub leaks: Vec<String>,
    pub pass: bool,
}

pub fn verdict(
    samples: &[Sample],
    chaos: &[ChaosResult],
    engine_failed: Option<&Logged>,
    chaos_run: bool,
) -> Verdict {
    let client_rss = growth(&client_rss(samples));
    let engine_rss = growth(&engine_series(samples, |s| s.engine_rss));
    let client_fds = growth(&client_fds(samples));
    let engine_fds = growth(&engine_series(samples, |s| s.engine_fds));
    let gpu = growth(
        &samples
            .iter()
            .filter_map(|s| Some((s.t_s, s.gpu_in_use? as f64)))
            .collect::<Vec<_>>(),
    );
    let mut leaks = Vec::new();
    for (name, g) in [("client RSS", &client_rss), ("engine RSS", &engine_rss)] {
        match g {
            Some(g) if g.fraction >= MAX_GROWTH => leaks.push(format!(
                "{name} grew {:.1}% after the warm-up",
                g.fraction * 100.0
            )),
            None if !(chaos_run && name == "engine RSS") => {
                leaks.push(format!("{name}: not enough samples after the warm-up"))
            }
            _ => {}
        }
    }
    for (name, g) in [
        ("client handles", &client_fds),
        ("engine handles", &engine_fds),
    ] {
        if let Some(g) = g
            && handles_leak(g)
        {
            leaks.push(format!(
                "{name} rose from {:.0} to {:.0}",
                g.baseline, g.end
            ));
        }
    }
    let mut unrecovered: Vec<String> = chaos
        .iter()
        .filter(|r| r.outcome == Outcome::Failed)
        .map(|r| format!("{:?} at {:.0} s: {}", r.kind, r.at_s, r.detail))
        .collect();
    if let Some(failed) = engine_failed {
        unrecovered.push(format!(
            "engine supervisor gave up at {:.0} s: {}",
            failed.at_s, failed.what
        ));
    }
    let not_verified = chaos
        .iter()
        .filter(|r| r.outcome == Outcome::NotVerified)
        .map(|r| format!("{:?}: {}", r.kind, r.detail))
        .collect();
    Verdict {
        pass: unrecovered.is_empty() && leaks.is_empty(),
        client_rss,
        engine_rss,
        client_fds,
        engine_fds,
        gpu_in_use: gpu,
        unrecovered,
        not_verified,
        leaks,
    }
}

#[derive(Serialize)]
pub struct Report<'a> {
    pub label: &'a str,
    pub started: String,
    pub minutes: f64,
    pub chaos: bool,
    pub engine: String,
    pub outputs_ready: Vec<String>,
    pub outputs_unavailable: Vec<(String, String)>,
    pub web_output: Option<String>,
    pub verdict: &'a Verdict,
    pub chaos_results: &'a [ChaosResult],
    pub sinks: BTreeMap<String, SinkTotals>,
    pub script: BTreeMap<&'static str, u64>,
    pub engine_errors: Vec<Logged>,
    pub launches: Vec<Logged>,
    pub losses: Vec<Logged>,
    pub statuses: Vec<Logged>,
    pub stt: Vec<Logged>,
    pub swaps_refused: u64,
    pub samples: &'a [Sample],
}

fn mib(bytes: f64) -> String {
    format!("{:.1} MiB", bytes / (1024.0 * 1024.0))
}

fn growth_row(out: &mut String, name: &str, g: &Option<Growth>, bytes: bool, note: &str) {
    let fmt = |v: f64| if bytes { mib(v) } else { format!("{v:.0}") };
    match g {
        Some(g) => {
            let _ = writeln!(
                out,
                "| {name} | {} | {} | {:+.2}% | {}/min | {} |",
                fmt(g.baseline),
                fmt(g.end),
                g.fraction * 100.0,
                if bytes {
                    format!("{:+.2} MiB", g.slope_per_min / (1024.0 * 1024.0))
                } else {
                    format!("{:+.2}", g.slope_per_min)
                },
                g.samples
            );
        }
        None => {
            let _ = writeln!(out, "| {name} | — | — | not assessed | {note} | — |");
        }
    }
}

pub fn markdown(report: &Report) -> String {
    let v = report.verdict;
    let mut out = String::new();
    let _ = writeln!(out, "# Soak run `{}`\n", report.label);
    let _ = writeln!(
        out,
        "Started {} · {:.1} min · chaos {} · engine `{}`\n",
        report.started,
        report.minutes,
        if report.chaos { "on" } else { "off" },
        report.engine
    );
    let _ = writeln!(
        out,
        "**Result: {}** — {} unrecovered failure(s), {} leak finding(s).\n",
        if v.pass { "PASS" } else { "FAIL" },
        v.unrecovered.len(),
        v.leaks.len()
    );
    let _ = writeln!(out, "## Outputs\n");
    let _ = writeln!(out, "Ready: {}", report.outputs_ready.join(", "));
    if let Some(web) = &report.web_output {
        let _ = writeln!(out, "\nShared web page on: {web}");
    }
    for (output, reason) in &report.outputs_unavailable {
        let _ = writeln!(out, "\nNot available: {output} — {reason}");
    }
    let _ = writeln!(
        out,
        "\n## Memory and handles (from the second service cycle)\n"
    );
    let _ = writeln!(
        out,
        "| Series | Baseline (first minute of cycle 2) | Last minute | Growth | Slope | Samples |\n|---|---|---|---|---|---|"
    );
    let relaunched = "engine relaunched inside the window";
    growth_row(&mut out, "Client RSS", &v.client_rss, true, "");
    growth_row(&mut out, "Engine RSS", &v.engine_rss, true, relaunched);
    growth_row(&mut out, "Client handles", &v.client_fds, false, "");
    growth_row(&mut out, "Engine handles", &v.engine_fds, false, relaunched);
    growth_row(
        &mut out,
        "GPU memory in use (system-wide)",
        &v.gpu_in_use,
        true,
        "not readable",
    );
    if let (Some(first), Some(last)) = (report.samples.first(), report.samples.last()) {
        let _ = writeln!(
            out,
            "\nHelper RSS {} → {}; web content processes {} → {} ({} → {}).",
            mib(first.helper_rss as f64),
            mib(last.helper_rss as f64),
            first.web_contents,
            last.web_contents,
            mib(first.web_content_rss as f64),
            mib(last.web_content_rss as f64)
        );
    }
    for leak in &v.leaks {
        let _ = writeln!(out, "\n- Leak finding: {leak}");
    }
    let _ = writeln!(out, "\n## Frames per sink\n");
    let _ = writeln!(
        out,
        "| Sink | Produced | Dropped | Late |\n|---|---|---|---|"
    );
    for (sink, t) in &report.sinks {
        let _ = writeln!(
            out,
            "| {sink} | {} | {} | {} |",
            t.produced, t.dropped, t.late
        );
    }
    let _ = writeln!(
        out,
        "\nScript: {}. Refused page swaps (fell back to a full document): {}.",
        report
            .script
            .iter()
            .map(|(k, v)| format!("{k} {v}"))
            .collect::<Vec<_>>()
            .join(", "),
        report.swaps_refused
    );
    if report.chaos {
        let _ = writeln!(out, "\n## Chaos events\n");
        let _ = writeln!(
            out,
            "| At | Event | Outcome | Recovery | Detail |\n|---|---|---|---|---|"
        );
        for r in report.chaos_results {
            let _ = writeln!(
                out,
                "| {:.0} s | {:?} | {:?} | {} | {} |",
                r.at_s,
                r.kind,
                r.outcome,
                r.recovery_ms
                    .map_or("—".into(), |ms| format!("{:.0} ms", ms)),
                r.detail.replace('|', "/")
            );
        }
    }
    if !v.unrecovered.is_empty() {
        let _ = writeln!(out, "\n## Unrecovered failures\n");
        for u in &v.unrecovered {
            let _ = writeln!(out, "- {u}");
        }
    }
    if !v.not_verified.is_empty() {
        let _ = writeln!(out, "\n## Not verified\n");
        for n in &v.not_verified {
            let _ = writeln!(out, "- {n}");
        }
    }
    let _ = writeln!(out, "\n## Engine log\n");
    for (title, items) in [
        ("Launches", &report.launches),
        ("Losses", &report.losses),
        ("Errors", &report.engine_errors),
        ("Transcription", &report.stt),
    ] {
        let _ = writeln!(out, "**{title}** ({})\n", items.len());
        for item in items.iter().take(40) {
            let _ = writeln!(out, "- {:.1} s: {}", item.at_s, item.what);
        }
        if items.len() > 40 {
            let _ = writeln!(out, "- … {} more in report.json", items.len() - 40);
        }
        let _ = writeln!(out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chaos::Kind;

    fn flat(value: f64, until: f64) -> Vec<(f64, f64)> {
        (0..=(until as u64 / 5))
            .map(|i| (i as f64 * 5.0, value))
            .collect()
    }

    #[test]
    fn steady_memory_has_no_growth() {
        let g = growth(&flat(100.0, 600.0)).unwrap();
        assert_eq!(g.fraction, 0.0);
        assert_eq!(g.slope_per_min, 0.0);
        assert_eq!(g.baseline, 100.0);
    }

    #[test]
    fn warm_up_growth_is_ignored_but_later_growth_is_not() {
        // Climbs during the first two minutes, then flat.
        let warm: Vec<(f64, f64)> = flat(0.0, 600.0)
            .into_iter()
            .map(|(t, _)| (t, if t < 120.0 { 50.0 + t } else { 200.0 }))
            .collect();
        assert_eq!(growth(&warm).unwrap().fraction, 0.0);
        // 10% linear growth after the warm-up.
        let leak: Vec<(f64, f64)> = flat(0.0, 600.0)
            .into_iter()
            .map(|(t, _)| (t, 1000.0 + (t - 120.0).max(0.0) * 0.25))
            .collect();
        let g = growth(&leak).unwrap();
        assert!(g.fraction > MAX_GROWTH, "{g:?}");
        assert!((g.slope_per_min - 15.0).abs() < 0.5, "{g:?}");
    }

    #[test]
    fn short_series_are_not_assessed() {
        assert_eq!(growth(&flat(1.0, 200.0)), None);
        assert_eq!(growth(&[]), None);
        assert_eq!(median(vec![]), None);
        assert_eq!(median(vec![3.0, 1.0, 2.0, 10.0]), Some(2.5));
    }

    fn sample(t: f64, pid: u32, rss: u64) -> Sample {
        Sample {
            t_s: t,
            client_rss: Some(rss),
            client_fds: Some(40),
            engine_pid: Some(pid),
            engine_rss: Some(rss),
            engine_fds: Some(60),
            ..Sample::default()
        }
    }

    #[test]
    fn a_relaunch_inside_the_window_drops_the_engine_series() {
        let steady: Vec<Sample> = (0..121).map(|i| sample(i as f64 * 5.0, 1, 1000)).collect();
        assert_eq!(engine_series(&steady, |s| s.engine_rss).len(), 121);
        let mut relaunched = steady.clone();
        for s in relaunched.iter_mut().skip(100) {
            s.engine_pid = Some(2);
        }
        assert!(engine_series(&relaunched, |s| s.engine_rss).is_empty());
        // A relaunch during the warm-up still leaves the window assessable.
        let mut early = steady;
        for s in early.iter_mut().take(10) {
            s.engine_pid = Some(9);
        }
        assert_eq!(engine_series(&early, |s| s.engine_rss).len(), 111);
    }

    #[test]
    fn the_verdict_fails_on_any_unrecovered_event_or_leak() {
        let samples: Vec<Sample> = (0..121).map(|i| sample(i as f64 * 5.0, 1, 1000)).collect();
        let ok = ChaosResult {
            kind: Kind::EngineKill,
            at_s: 95.0,
            outcome: Outcome::Recovered,
            recovery_ms: Some(400.0),
            detail: String::new(),
        };
        assert!(verdict(&samples, std::slice::from_ref(&ok), None, true).pass);
        let bad = ChaosResult {
            outcome: Outcome::Failed,
            ..ok.clone()
        };
        let v = verdict(&samples, &[ok.clone(), bad], None, true);
        assert!(!v.pass);
        assert_eq!(v.unrecovered.len(), 1);
        let gave_up = Logged {
            at_s: 3.0,
            what: "budget".into(),
        };
        assert!(!verdict(&samples, &[], Some(&gave_up), false).pass);
        let skipped = ChaosResult {
            outcome: Outcome::NotVerified,
            ..ok
        };
        let v = verdict(&samples, &[skipped], None, true);
        assert!(v.pass);
        assert_eq!(v.not_verified.len(), 1);
        // 20% growth fails.
        let leaking: Vec<Sample> = (0..121)
            .map(|i| sample(i as f64 * 5.0, 1, 1000 + i * 4))
            .collect();
        let v = verdict(&leaking, &[], None, false);
        assert!(!v.pass);
        assert_eq!(v.leaks.len(), 2);
        // A clean run without engine samples cannot pass.
        let no_engine: Vec<Sample> = samples
            .iter()
            .cloned()
            .map(|mut s| {
                s.engine_rss = None;
                s
            })
            .collect();
        assert!(!verdict(&no_engine, &[], None, false).pass);
        assert!(verdict(&no_engine, &[], None, true).pass);
    }

    #[test]
    fn handle_growth_needs_more_than_noise() {
        let g = |baseline, end| Growth {
            baseline,
            end,
            fraction: 0.0,
            slope_per_min: 0.0,
            samples: 10,
        };
        assert!(!handles_leak(&g(40.0, 48.0)));
        assert!(handles_leak(&g(40.0, 60.0)));
        assert!(!handles_leak(&g(1000.0, 1040.0)));
        assert!(handles_leak(&g(1000.0, 1060.0)));
    }
}
