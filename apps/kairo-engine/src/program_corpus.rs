//! The prompt 054 corpus against Chromium: every program layer and style
//! option, the info layer, the stage display and the test patterns, pushed
//! through the engine's own path (shell, workers, compositor) and compared
//! with what `overlay.html` and `stage.html` drew from the same data.
//!
//! References come from `golden-capture ... programs`; they are cached in
//! `.cache/golden-capture/054/programs` (or `KAIRO_PROGRAM_REFERENCE`), not
//! committed. Without them the comparison says so and returns, unless
//! `KAIRO_REQUIRE_ORACLE` is set. The cases themselves are committed in
//! `tests/golden/054-programs/cases.json`.

use crate::visual_corpus::{Rig, compare, fixtures, load_png, rig, save_png, workspace};
use kairo_compositor::Slot;
use kairo_engine_proto::{Command, Json, LayerKind, TestPattern};
use kairo_scene::program::Now;
use serde_json::Value;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn reference_dir() -> Option<PathBuf> {
    let dir = std::env::var_os("KAIRO_PROGRAM_REFERENCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace().join(".cache/golden-capture/054/programs"));
    dir.is_dir().then_some(dir)
}

fn cases() -> Vec<Value> {
    let path = workspace().join("tests/golden/054-programs/cases.json");
    let text = std::fs::read_to_string(&path).expect("cases.json");
    let fix = fixtures();
    let text = text.replace("@FIX@", fix.to_str().expect("utf-8 path"));
    serde_json::from_str::<Vec<Value>>(&text).expect("cases")
}

fn kind_of(name: &str) -> LayerKind {
    match name {
        "message" => LayerKind::Message,
        "props" => LayerKind::Props,
        "timer" => LayerKind::Timer,
        "poll" => LayerKind::Poll,
        "question" => LayerKind::Question,
        "logo" => LayerKind::Logo,
        other => panic!("unknown layer {other}"),
    }
}

/// Clears every layer the previous case may have left.
fn clear(rig: &mut Rig) -> u64 {
    let mut seq = 0;
    for kind in LayerKind::ALL {
        seq = rig.push(Command::SetLayer {
            output: "o".into(),
            kind,
            layer: None,
        });
    }
    rig.push(Command::SetInfo {
        output: "o".into(),
        info: None,
    });
    seq = seq.max(rig.push(Command::SetTestPattern {
        output: "o".into(),
        pattern: None,
    }));
    seq
}

fn show_case(rig: &mut Rig, case: &Value) {
    let now = Now {
        ms: case["now"].as_f64().expect("now"),
        clock: case["clock"].as_str().expect("clock").to_owned(),
    };
    rig.shell.pinned = Some(now.clone());
    let seq = clear(rig);
    rig.settle(seq);
    let mut last = 0;
    if let Some(layers) = case.get("layers").and_then(Value::as_object) {
        for (name, value) in layers {
            last = rig.push(Command::SetLayer {
                output: "o".into(),
                kind: kind_of(name),
                layer: Some(Json::from_value(value)),
            });
        }
    }
    if let Some(info) = case.get("info").filter(|v| !v.is_null()) {
        last = rig.push(Command::SetInfo {
            output: "o".into(),
            info: Some(Json::from_value(info)),
        });
    }
    if let Some(pattern) = case.get("pattern").filter(|v| !v.is_null()) {
        rig.state.spec.label = pattern["name"].as_str().expect("name").to_owned();
        let pattern = match pattern["pattern"].as_str() {
            Some("bars") => TestPattern::Bars,
            Some("text") => TestPattern::Text,
            _ => TestPattern::Motion,
        };
        last = rig.push(Command::SetTestPattern {
            output: "o".into(),
            pattern: Some(pattern),
        });
    }
    if let Some(stage) = case.get("stage").filter(|v| !v.is_null()) {
        let view = serde_json::from_value(stage.clone()).expect("stage view");
        rig.show_stage(view, now);
        return;
    }
    rig.settle(last);
}

/// Per case: SSIM at least `min`, at most `far` of the pixels more than 24
/// levels off. DECISIONS 054 says why each looser case is looser.
fn limits(id: &str) -> (f64, f64) {
    match id {
        // Many lines of large text overflowing their box: Chromium stacks
        // lines in 1/64 px units and snaps each baseline, so a run of lines
        // drifts by one pixel at different lines than here.
        "stage-overflow" => (0.96, 0.025),
        "question-overflow" => (0.975, 0.02),
        _ => (0.975, 0.01),
    }
}

#[test]
fn every_program_case_matches_chromium() {
    let Some(dir) = reference_dir() else {
        assert!(
            std::env::var_os("KAIRO_REQUIRE_ORACLE").is_none(),
            "program references missing: run golden-capture ... programs"
        );
        eprintln!("skipped: no program references (golden-capture ... programs)");
        return;
    };
    let Some(mut rig) = rig() else { return };
    let out = std::env::var_os("KAIRO_PROGRAM_OUT").map(PathBuf::from);
    let mut rows = Vec::new();
    let mut failures = Vec::new();
    for case in cases() {
        let id = case["id"].as_str().expect("id").to_owned();
        show_case(&mut rig, &case);
        let ours = rig.frame_over_grey();
        // The stage draws in its own slot; take it down again.
        if case.get("stage").is_some_and(|v| !v.is_null()) {
            rig.compositor.apply(
                "o",
                0,
                vec![kairo_compositor::thread::Op::Keyed {
                    slot: Slot::Pattern,
                    content: None,
                    ms: 0,
                }],
            );
        }
        let Some(reference) = load_png(&dir.join(format!("{id}.png"))) else {
            failures.push(format!("{id}: no reference"));
            continue;
        };
        if let Some(out) = &out {
            let _ = std::fs::create_dir_all(out);
            save_png(&out.join(format!("{id}.png")), &ours);
        }
        let (ssim, far) = compare(&ours, &reference, 24);
        let (min, max_far) = limits(&id);
        if ssim < min || far > max_far {
            failures.push(format!(
                "{id}: ssim {ssim:.4} (min {min}), far {far:.4} (max {max_far})"
            ));
        }
        rows.push((id, ssim, far));
    }
    for (id, ssim, far) in &rows {
        eprintln!("{id:<32} ssim {ssim:.4} far {far:.4}");
    }
    let mean = rows.iter().map(|r| r.1).sum::<f64>() / rows.len().max(1) as f64;
    let worst = rows.iter().map(|r| r.1).fold(1.0, f64::min);
    eprintln!("cases {} mean ssim {mean:.5} worst {worst:.5}", rows.len());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(mean >= 0.995, "mean ssim {mean}");
}

/// A running countdown on screen: ticks that change nothing draw nothing,
/// and a change redraws only the readout's few pixels.
#[test]
fn a_tick_redraws_only_the_readout() {
    let Some(mut rig) = rig() else { return };
    let start = 1_800_000_000_000.0;
    let layer = serde_json::json!({
        "kind": "timer",
        "prop": {"id": "timer-main", "name": "Countdown", "background": "#101820", "objects": [
            {"id": "band", "kind": "shape", "x": 0, "y": 0.7, "w": 1, "h": 0.3, "opacity": 1,
             "shape": "rectangle", "fill": "#1b4f72", "radiusPx": 0},
            {"id": "r", "kind": "timer", "x": 0.3, "y": 0.05, "w": 0.4, "h": 0.12, "opacity": 1,
             "fontSizePx": 96, "color": "#5eead4", "overrunColor": "#f87171", "bold": true, "align": "center"}
        ]},
        "timer": {"durationSec": 300, "endsAt": start + 300_000.0, "remainingSec": 300,
                  "rollover": true, "now": start}
    });
    rig.shell.pinned = Some(Now {
        ms: start,
        clock: "9:00 AM".into(),
    });
    let seq = rig.push(Command::SetLayer {
        output: "o".into(),
        kind: LayerKind::Timer,
        layer: Some(Json::from_value(&layer)),
    });
    rig.settle(seq);
    let before = rig.frame_over_grey();
    let at = |ms: f64| Now {
        ms: start + ms,
        clock: "9:00 AM".into(),
    };
    // 5:00 for the first second: nothing to draw.
    assert_eq!(rig.tick_at(&at(250.0)), 0);
    assert_eq!(rig.tick_at(&at(500.0)), 0);
    // 4:59: one readout redrawn.
    assert_eq!(rig.tick_at(&at(1_250.0)), 1);
    rig.drain();
    let layer = rig.shell.live.get(&Slot::Timer).expect("live timer");
    let part = &layer.parts[0];
    assert_eq!(part.state.text, "4:59");
    // The redrawn child is a small tile, not the frame.
    let child = &layer.children[part.index];
    let rect = child.rect.expect("readout rect");
    assert!(
        rect.width * rect.height < 1920.0 * 1080.0 * 0.05,
        "{rect:?}"
    );
    // Only pixels inside the readout's box changed on screen.
    let deadline = Instant::now() + Duration::from_secs(5);
    let after = loop {
        let frame = rig.frame_over_grey();
        if frame != before || Instant::now() > deadline {
            break frame;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut changed = 0usize;
    for (i, (a, b)) in before.chunks(4).zip(after.chunks(4)).enumerate() {
        if a != b {
            changed += 1;
            let (x, y) = ((i % 1920) as f32, (i / 1920) as f32);
            assert!(
                (576.0..=1344.0).contains(&x) && (54.0..=184.0).contains(&y),
                "pixel {x},{y} outside the readout changed"
            );
        }
    }
    assert!(changed > 0, "the readout did not change");
    // Past the end: overrun colour, still one readout.
    assert_eq!(rig.tick_at(&at(301_000.0)), 1);
    rig.drain();
    let part = &rig.shell.live[&Slot::Timer].parts[0];
    assert_eq!(part.state.text, "-0:01");
    assert_eq!(
        part.state.color,
        kairo_scene::color::Rgba::new(0xf8, 0x71, 0x71, 255)
    );
}

/// Clearing or replacing a layer stops its ticks; a stale redraw is dropped.
#[test]
fn a_replaced_layer_stops_ticking() {
    let Some(mut rig) = rig() else { return };
    rig.shell.pinned = Some(Now {
        ms: 0.0,
        clock: "9:00 AM".into(),
    });
    let info = serde_json::json!({
        "clock": true, "stageMessage": null,
        "layout": {"position": "top", "size": "medium"},
        "sizes": {"barPx": 72, "messagePx": 60},
        "style": {"rollover": true, "color": "#5eead4", "overrunColor": "#f87171", "backdrop": false}
    });
    let seq = rig.push(Command::SetInfo {
        output: "o".into(),
        info: Some(Json::from_value(&info)),
    });
    rig.settle(seq);
    assert!(rig.shell.live.contains_key(&Slot::Info));
    assert_eq!(
        rig.tick_at(&Now {
            ms: 60_000.0,
            clock: "9:01 AM".into()
        }),
        1
    );
    let seq = rig.push(Command::SetInfo {
        output: "o".into(),
        info: None,
    });
    rig.drain();
    rig.settle(seq);
    assert!(!rig.shell.live.contains_key(&Slot::Info));
    assert_eq!(
        rig.tick_at(&Now {
            ms: 120_000.0,
            clock: "9:02 AM".into()
        }),
        0
    );
}

/// Process CPU time (user + system) so far.
#[cfg(unix)]
fn cpu_seconds() -> f64 {
    // SAFETY: `getrusage` fills the struct it is given.
    let usage = unsafe {
        let mut usage: libc::rusage = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &mut usage);
        usage
    };
    let secs = |t: libc::timeval| t.tv_sec as f64 + t.tv_usec as f64 / 1e6;
    secs(usage.ru_utime) + secs(usage.ru_stime)
}

/// Prompt 054's budget: a running timer on four outputs under 2 % of one
/// core. One compositor and one worker pool, as in the engine; four
/// offscreen 1080p outputs, each with a running countdown in the timer
/// layer, ticked every 250 ms for 30 s on the real clock.
#[cfg(unix)]
#[test]
#[ignore = "measurement: cargo test --release -p kairo-engine --bin kairo-engine four_outputs -- --ignored --nocapture"]
fn four_outputs_with_a_running_timer_stay_under_two_percent() {
    use crate::program::Shell;
    use crate::visuals::{Job, Output, VisualWork};
    use kairo_compositor::thread::{Compositor, Op, Sink};
    use kairo_engine_proto::{SinkSpec, SurfaceSpec, SurfaceState};
    use std::sync::mpsc;

    let (tx, events) = mpsc::channel();
    let compositor = match Compositor::start(move |e| {
        let _ = tx.send(e);
    }) {
        Ok(c) => c,
        Err(error) => {
            eprintln!("skipped: {error}");
            return;
        }
    };
    let names = ["a", "b", "c", "d"];
    for name in names {
        compositor.open(name, 1080, Sink::Offscreen);
    }
    let visuals = VisualWork::new();
    type Back = (usize, Slot, String, u64, Output);
    let (back_tx, back_rx) = mpsc::channel::<Back>();
    let submit = |i: usize, slot: Slot, key: String, generation: u64, job: Job| {
        let tx = back_tx.clone();
        visuals.submit(
            job,
            None,
            Box::new(move |o| {
                let _ = tx.send((i, slot, key, generation, o));
            }),
        );
    };
    let start_ms = crate::layers::now().ms;
    let layer = serde_json::json!({
        "kind": "timer",
        "prop": {"id": "timer-main", "name": "Countdown", "background": null, "objects": [
            {"id": "r", "kind": "timer", "x": 0.3, "y": 0.05, "w": 0.4, "h": 0.12, "opacity": 1,
             "fontSizePx": 96, "color": "#5eead4", "overrunColor": "#f87171", "bold": true, "align": "center"}
        ]},
        "timer": {"durationSec": 600, "endsAt": start_ms + 600_000.0, "remainingSec": 600,
                  "rollover": true, "now": start_ms}
    });
    let mut shells: Vec<Shell> = names.iter().map(|n| Shell::new(n, 1080)).collect();
    for (i, shell) in shells.iter_mut().enumerate() {
        let state = SurfaceState::new(SurfaceSpec {
            output: names[i].into(),
            label: names[i].into(),
            sink: SinkSpec::Ndi {
                name: names[i].into(),
            },
        });
        let command = Command::SetLayer {
            output: names[i].into(),
            kind: LayerKind::Timer,
            layer: Some(Json::from_value(&layer)),
        };
        for request in shell.command(&command, &state, &state).visuals {
            submit(
                i,
                request.slot,
                request.key,
                request.generation,
                request.job,
            );
        }
        let (_, slot, key, generation, out) = back_rx.recv().expect("first draw");
        let Output::Layer(out) = out else {
            panic!("a layer")
        };
        let content = shell.layer_done(slot, &key, generation, out);
        compositor.apply(
            names[i],
            1,
            vec![Op::Keyed {
                slot,
                content: Some((content, key)),
                ms: 0,
            }],
        );
    }
    // Let start-up (font loading, first frames) finish before measuring.
    std::thread::sleep(Duration::from_secs(2));
    while events.try_recv().is_ok() {}
    let (cpu0, wall0) = (cpu_seconds(), Instant::now());
    let mut redraws = 0;
    while wall0.elapsed() < Duration::from_secs(30) {
        let now = crate::layers::now();
        for (i, shell) in shells.iter_mut().enumerate() {
            for (slot, key, generation, live) in shell.tick(&now) {
                redraws += 1;
                submit(i, slot, key, generation, Job::Live(live));
            }
        }
        let next = Instant::now() + Duration::from_millis(250);
        while let Ok((i, slot, _, generation, out)) =
            back_rx.recv_timeout(next.saturating_duration_since(Instant::now()))
        {
            if let Output::Live(out) = out
                && let Some(op) = shells[i].live_done(slot, generation, *out)
            {
                compositor.apply(names[i], 0, vec![op]);
            }
        }
        while events.try_recv().is_ok() {}
    }
    let wall = wall0.elapsed().as_secs_f64();
    let percent = (cpu_seconds() - cpu0) / wall * 100.0;
    eprintln!(
        "four outputs, running timer: {percent:.3}% of one core over {wall:.1} s; \
         {redraws} readouts redrawn"
    );
    // One redraw per output per second.
    assert!((100..=140).contains(&redraws), "{redraws}");
    assert!(percent < 2.0, "{percent}% of one core");
}
