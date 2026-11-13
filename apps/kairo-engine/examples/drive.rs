//! Drives a real `kairo-engine` through `kairo-core`'s supervisor, for
//! checking output by eye.
//!
//! ```text
//! cargo run -p kairo-engine --example drive -- [--display N] [--full] [--secs S]
//!     [--kill-after S] [--fade MS] [--frame-height H] [--theme ID] [--case ID]
//!     [--text TEXT] [--reference REF] [--program ID] [--live] [--video PATH]
//!     [--sound VOLUME] [--sound-device LABEL] [--camera LABEL]
//!     [--camera-audio LABEL] [--ndi NAME] [--identify] [--shape NAME]
//! ```
//!
//! `--ndi NAME` sends the output as an NDI source named NAME instead of
//! opening a window (`Kairo Scripture` is the primary feed).
//!
//! `--camera LABEL` puts a live camera or capture card up, matched by the
//! label the operator's list showed (`cargo run -p kairo-camera --example
//! list` prints them); `--camera-audio LABEL` plays an audio input with it
//! (heard with `--sound`).
//!
//! `--video PATH` plays a looping background video under the slides.
//! `--sound VOLUME` makes this output the audio surface at that volume
//! (0 to 1), on `--sound-device` or the system default.
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

    let default_binary = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("kairo-engine");
    let binary = value::<String>("--engine")
        .map(std::path::PathBuf::from)
        .unwrap_or(default_binary);
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
    // `--identify`: the 3 s display labels; `--shape NAME`: the 5 s shape
    // card on this display (prompt 060).
    if std::env::args().any(|a| a == "--identify") {
        engine.command(Command::IdentifyDisplays);
    }
    if let Some(name) = value::<String>("--shape") {
        engine.command(Command::DisplayTestPattern {
            display: display.id,
            name,
            shape: "16:9".into(),
            frame_height: 1080,
        });
    }
    // `--ndi NAME`: the output is an NDI feed instead of a window.
    let sink = match value::<String>("--ndi") {
        Some(name) => SinkSpec::Ndi { name },
        None => SinkSpec::Display(DisplaySink {
            binding: DisplayBinding {
                display_id: Some(display.id),
                label: display.label.clone(),
                size: Some((display.bounds.width, display.bounds.height)),
            },
            rehearsal: !full,
            frame_height,
        }),
    };
    engine.command(Command::OpenSurface(SurfaceSpec {
        output: "main".into(),
        label: "Kairo output".into(),
        sink,
    }));
    let video: Option<String> = value("--video");
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
        if let Some(path) = &video {
            // `--video PATH`: a looping background video at cue 1, kept
            // across the slides (the same file and cue).
            theme.background.kind = kairo_domain::overlay::OverlayBackgroundType::Video;
            theme.background.media_path = Some(path.clone());
            theme.background.media_loop = Some(true);
            theme.background.media_cue = Some(1.0);
        }
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
    let sound: Option<f64> = value("--sound");
    let camera: Option<String> = value("--camera");
    let camera_audio: Option<String> = value("--camera-audio");
    if fade.is_some() || sound.is_some() || camera.is_some() {
        engine.command(Command::Configure {
            output: "main".into(),
            config: SurfaceConfig {
                transition_ms: fade.unwrap_or(0.0),
                audio: AudioConfig {
                    enabled: sound.is_some(),
                    volume: sound.unwrap_or(1.0),
                    output_label: value("--sound-device").unwrap_or_default(),
                },
                camera_audible: sound.is_some() && camera_audio.is_some(),
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

    if let Some(label) = camera {
        engine.command(Command::SetCamera {
            output: "main".into(),
            camera: Some(CameraLayer {
                label,
                audio_label: camera_audio,
            }),
        });
    }

    // `--pages a.png,b.png,…` shows a document deck (prompt 058): page 0
    // with `--doc-video CLIP` embedded at a quarter of the slide, played,
    // then a page turn every second with the next page decoded ahead.
    if let Some(pages) = value::<String>("--pages") {
        let pages: Vec<String> = pages.split(',').map(str::to_owned).collect();
        let doc_slide = |path: &str| {
            let mut theme = kairo_domain::overlay_defaults::DEFAULT_OVERLAY_THEME.clone();
            theme.background.kind = kairo_domain::overlay::OverlayBackgroundType::Image;
            theme.background.media_path = Some(path.to_owned());
            theme.background.media_fit = Some(kairo_domain::overlay::MediaFit::Contain);
            theme.background.opacity = 1.0;
            SlideContent {
                reference: String::new(),
                text: String::new(),
                theme: Json::encode(&theme),
                colored_lines: None,
            }
        };
        let videos = value::<String>("--doc-video").map_or_else(Vec::new, |file| {
            vec![serde_json::json!({ "file": file, "start": 0,
                "box": { "x": 0.55, "y": 0.1, "width": 0.4, "height": 0.4 } })]
        });
        let deck = |page: usize, videos: &[serde_json::Value]| {
            Json::from_value(&serde_json::json!({ "id": "drive-deck", "page": page,
                "path": pages[page], "width": 1440, "height": 1080, "videos": videos }))
        };
        let shown = engine.command(Command::ShowDocument {
            output: "main".into(),
            source: "main".into(),
            slide: doc_slide(&pages[0]),
            document: Some(deck(0, &videos)),
        });
        println!(
            "document accepted={} frame: {}",
            shown.accepted,
            engine
                .wait_frame("main", shown.seq, Duration::from_secs(3))
                .await
        );
        if !videos.is_empty() {
            tokio::time::sleep(Duration::from_millis(500)).await;
            engine.command(Command::ControlDocumentVideo {
                output: "main".into(),
                id: "drive-deck".into(),
                page: 0.0,
                video: 0,
                command: Json::from_value(&serde_json::json!({ "action": "play" })),
            });
        }
        tokio::time::sleep(Duration::from_secs(4)).await;
        for (turn, page) in pages.iter().enumerate().skip(1) {
            engine.command(Command::PreloadPage {
                output: "main".into(),
                media_path: page.clone(),
            });
            tokio::time::sleep(Duration::from_millis(1000)).await;
            let sent = engine.command(Command::SwapPage {
                output: "main".into(),
                source: "main".into(),
                media_path: page.clone(),
                document: Some(deck(turn, &[])),
            });
            println!(
                "turn {turn} accepted={} frame: {}",
                sent.accepted,
                engine
                    .wait_frame("main", sent.seq, Duration::from_secs(2))
                    .await
            );
        }
    }

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

    if let Some(url) = value::<String>("--web") {
        engine.command(Command::SetWeb {
            output: "main".into(),
            web: Some(WebLayer { url, reload: 0. }),
        });
        engine.command(Command::WebPointer {
            output: "main".into(),
            point: Some((960., 540.)),
        });
    }
    let mut web_step = 0;
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
        if std::env::args().any(|a| a == "--web-exercise")
            && start.elapsed().as_secs() >= (3 + web_step * 2)
            && web_step < 8
        {
            match web_step {
                0 => {
                    engine.command(Command::WebInput {
                        output: "main".into(),
                        input: WebInput::Click { x: 100., y: 150. },
                    });
                }
                1 => {
                    engine.command(Command::WebScroll {
                        output: "main".into(),
                        x: 0.,
                        y: 700.,
                    });
                }
                2 => {
                    engine.command(Command::WebInput {
                        output: "main".into(),
                        input: WebInput::Click { x: 100., y: 90. },
                    });
                    engine.command(Command::WebInput {
                        output: "main".into(),
                        input: WebInput::Text {
                            text: "shared page".into(),
                        },
                    });
                }
                3 => {
                    engine.command(Command::WebInput {
                        output: "main".into(),
                        input: WebInput::Click { x: 370., y: 150. },
                    });
                }
                4 => {
                    engine.command(Command::WebInput {
                        output: "main".into(),
                        input: WebInput::Click { x: 610., y: 150. },
                    });
                }
                5 => {
                    engine.command(Command::WebInput {
                        output: "main".into(),
                        input: WebInput::Click { x: 850., y: 150. },
                    });
                }
                6 => {
                    engine.command(Command::WebInput {
                        output: "main".into(),
                        input: WebInput::Click { x: 1300., y: 150. },
                    });
                }
                _ => {
                    engine.command(Command::WebInput {
                        output: "main".into(),
                        input: WebInput::Click { x: 1100., y: 150. },
                    });
                }
            }
            web_step += 1;
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
