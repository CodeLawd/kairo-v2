//! Drives a real `kairo-engine` through `kairo-core`'s supervisor, for
//! checking output by eye.
//!
//! ```text
//! cargo run -p kairo-engine --example drive -- [--display N] [--full] [--secs S]
//!     [--kill-after S] [--fade MS] [--frame-height H] [--theme ID] [--case ID]
//!     [--text TEXT] [--reference REF] [--program ID] [--live]
//! ```
//!
//! `--program ID` also puts up the layers of a prompt 054 corpus case
//! (`tests/golden/054-programs/cases.json`). `--live` puts up a running
//! countdown in a prop, the timer design, a ticker message and the info
//! clock, so ticking and scrolling can be watched.
//!
//! Opens one output on display N (default 0): a rehearsal window, or the
//! whole display with `--full`. Shows a red slide carrying a verse in the
//! built-in theme ID (default `broadcast`; also `warm-paper`, `midnight`,
//! `lyrics-stage`, `lyrics-lower`); with `--fade` it then crossfades to a
//! blue slide with different words over MS milliseconds. Logs every engine
//! event, optionally SIGKILLs the engine once to show the relaunch, then
//! shuts down after S seconds. Only colour backgrounds draw so far (053), so
//! each theme's background is replaced by a flat colour.

use kairo_core::engine::*;
use kairo_engine_proto::*;
use std::time::{Duration, Instant};

fn value<T: std::str::FromStr>(name: &str) -> Option<T> {
    let mut args = std::env::args();
    args.find(|a| a == name)?;
    args.next()?.parse().ok()
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    tracing_subscriber::fmt().with_target(true).init();
    let index: usize = value("--display").unwrap_or(0);
    let secs: u64 = value("--secs").unwrap_or(10);
    let kill_after: Option<u64> = value("--kill-after");
    let full = std::env::args().any(|a| a == "--full");
    let fade: Option<f64> = value("--fade");
    let frame_height: u32 = value("--frame-height").unwrap_or(1080);

    let binary = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("kairo-engine");
    let engine = EngineHandle::spawn(EngineOptions::new(binary));
    let mut events = engine.subscribe();
    let start = Instant::now();
    let display = loop {
        if let Some(display) = engine.displays().get(index).cloned() {
            break display;
        }
        if let EngineStatus::Failed { reason } = engine.status() {
            panic!("engine failed: {reason}");
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "no display {index}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    println!("display {index}: {display:?}");
    engine.command(Command::OpenSurface(SurfaceSpec {
        output: "main".into(),
        label: "Kairo output".into(),
        sink: SinkSpec::Display(DisplaySink {
            binding: DisplayBinding {
                display_id: Some(display.id),
                label: display.label.clone(),
                size: Some((display.bounds.width, display.bounds.height)),
            },
            rehearsal: !full,
            frame_height,
        }),
    }));
    let theme_id: String = value("--theme").unwrap_or_else(|| "broadcast".into());
    let built_in = kairo_domain::theme_library::BUILT_IN_THEMES
        .iter()
        .find(|t| t.id == theme_id)
        .unwrap_or_else(|| panic!("no built-in theme {theme_id}"));
    // `--case ID` draws a prompt 053 corpus theme (background, elements)
    // instead of a flat colour; its pictures come from tests/fixtures.
    let case: Option<serde_json::Value> = value::<String>("--case").map(|id| {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let cases: Vec<serde_json::Value> = serde_json::from_slice(
            &std::fs::read(root.join("tests/golden/053-visuals/cases.json")).expect("cases.json"),
        )
        .expect("cases json");
        let found = cases
            .into_iter()
            .find(|c| c["id"] == id.as_str())
            .unwrap_or_else(|| panic!("no visual case {id}"));
        let fixtures = root
            .join("tests/fixtures/visuals")
            .canonicalize()
            .expect("fixtures");
        let text = found["theme"]
            .to_string()
            .replace("@FIX@", fixtures.to_str().unwrap());
        serde_json::from_str(&text).expect("theme json")
    });
    let slide = |color: &str, reference: &str, text: &str| {
        if let Some(theme) = &case {
            return SlideContent {
                reference: reference.into(),
                text: text.into(),
                theme: Json::from_value(theme),
                colored_lines: None,
            };
        }
        let mut theme = built_in.theme.clone();
        theme.background.kind = kairo_domain::overlay::OverlayBackgroundType::Color;
        theme.background.color = color.into();
        theme.background.opacity = 1.0;
        SlideContent {
            reference: reference.into(),
            text: text.into(),
            theme: Json::encode(&theme),
            colored_lines: None,
        }
    };
    let reference: String = value("--reference").unwrap_or_else(|| "John 3:16 (KJV)".into());
    let first_text: String = value("--text").unwrap_or_else(|| {
        "For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life.".into()
    });
    if let Some(ms) = fade {
        engine.command(Command::Configure {
            output: "main".into(),
            config: SurfaceConfig {
                transition_ms: ms,
                ..SurfaceConfig::default()
            },
        });
    }
    let sent = engine.command(Command::ShowSlide {
        output: "main".into(),
        source: "main".into(),
        slide: slide("#C81E1E", &reference, &first_text),
        instant: true,
    });
    println!(
        "slide accepted={} frame within 2 s: {}",
        sent.accepted,
        engine
            .wait_frame("main", sent.seq, Duration::from_secs(2))
            .await
    );

    if let Some(id) = value::<String>("--program") {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let fixtures = root
            .join("tests/fixtures/visuals")
            .canonicalize()
            .expect("fixtures");
        let text = std::fs::read_to_string(root.join("tests/golden/054-programs/cases.json"))
            .expect("cases.json")
            .replace("@FIX@", fixtures.to_str().unwrap());
        let cases: Vec<serde_json::Value> = serde_json::from_str(&text).expect("cases json");
        let case = cases
            .into_iter()
            .find(|c| c["id"] == id.as_str())
            .unwrap_or_else(|| panic!("no program case {id}"));
        for (name, layer) in case["layers"].as_object().into_iter().flatten() {
            let kind = match name.as_str() {
                "message" => LayerKind::Message,
                "props" => LayerKind::Props,
                "timer" => LayerKind::Timer,
                "poll" => LayerKind::Poll,
                "question" => LayerKind::Question,
                _ => LayerKind::Logo,
            };
            engine.command(Command::SetLayer {
                output: "main".into(),
                kind,
                layer: Some(Json::from_value(layer)),
            });
        }
        if !case["info"].is_null() {
            engine.command(Command::SetInfo {
                output: "main".into(),
                info: Some(Json::from_value(&case["info"])),
            });
        }
    }
    if std::env::args().any(|a| a == "--live") {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as f64;
        let run = serde_json::json!({"durationSec": 75, "endsAt": now + 65_000.0,
            "remainingSec": 75, "rollover": true, "now": now});
        let readout = |x: f64, y: f64| {
            serde_json::json!({"id": "r", "kind": "timer", "x": x, "y": y,
            "w": 0.3, "h": 0.1, "opacity": 1, "fontSizePx": 80, "color": "#5eead4",
            "overrunColor": "#f87171", "bold": true, "align": "center"})
        };
        engine.command(Command::SetLayer {
            output: "main".into(),
            kind: LayerKind::Props,
            layer: Some(Json::from_value(
                &serde_json::json!({"kind": "props", "timer": run,
                "props": [{"id": "p", "name": "Bug", "background": null, "objects": [
                    {"id": "s", "kind": "shape", "x": 0.68, "y": 0.04, "w": 0.3, "h": 0.12,
                     "opacity": 0.8, "shape": "rectangle", "fill": "#101820", "radiusPx": 24},
                    readout(0.68, 0.05)]}]}),
            )),
        });
        engine.command(Command::SetLayer {
            output: "main".into(),
            kind: LayerKind::Message,
            layer: Some(Json::from_value(&serde_json::json!({"kind": "message",
                "text": "Parents of child 42, please come to the nursery   ·   Cars in the red zone, please move them",
                "style": {"position": "bottom", "fontSizePx": 44, "color": "#ffffff",
                          "background": "rgba(0,0,0,0.72)", "scroll": true}}))),
        });
        engine.command(Command::SetInfo {
            output: "main".into(),
            info: Some(Json::from_value(&serde_json::json!({"clock": true,
                "stageMessage": "Two minutes left",
                "layout": {"position": "top", "size": "medium"},
                "sizes": {"barPx": 72, "messagePx": 60},
                "style": {"rollover": true, "color": "#5eead4", "overrunColor": "#f87171", "backdrop": false}}))),
        });
    }

    let mut faded = fade.is_none();
    #[cfg_attr(not(unix), allow(unused_mut, unused_variables))]
    let mut killed = false;
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        #[cfg(unix)]
        if let Some(after) = kill_after
            && !killed
            && start.elapsed() >= Duration::from_secs(after)
            && let Some(pid) = engine.pid()
        {
            killed = true;
            println!("SIGKILL engine {pid}");
            // SAFETY: the engine this example launched.
            unsafe { libc::kill(pid as i32, libc::SIGKILL) };
        }
        if !faded && start.elapsed() >= Duration::from_secs(3) {
            faded = true;
            println!("{:>8.3}s crossfade to blue", start.elapsed().as_secs_f64());
            engine.command(Command::ShowSlide {
                output: "main".into(),
                source: "main".into(),
                slide: slide(
                    "#1E3CC8",
                    "Psalm 23:1–3 (KJV)",
                    "1 The LORD is my shepherd; I shall not want.\n2 He maketh me to lie down in green pastures: he leadeth me beside the still waters.\n3 He restoreth my soul: he leadeth me in the paths of righteousness for his name’s sake.",
                ),
                instant: false,
            });
        }
        match tokio::time::timeout(Duration::from_millis(50), events.recv()).await {
            Ok(Ok(event)) => println!("{:>8.3}s {event:?}", start.elapsed().as_secs_f64()),
            Ok(Err(error)) => println!("events: {error}"),
            Err(_) => {}
        }
    }
    engine.shutdown();
    println!("status {:?}", engine.status());
}
