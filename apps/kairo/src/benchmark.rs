//! Opt-in, development-only measurements over the real application and services.
//! `KAIRO_BENCH_ROOT` isolates all profile/workspace writes. Never shipped.

use std::{path::PathBuf, sync::Arc, time::Duration};

use gpui_kit::{App, Entity};
use kairo_store::paths::{AppPaths, compute, detect_inputs};
use serde_json::json;

/// The isolated, empty profile has no credentials. Report the normal
/// unavailable-keychain error instead of prompting for access to real keys.
pub struct NoKeychain;
impl kairo_store::secrets::KeyProtector for NoKeychain {
    fn load(&self, _: &str) -> Result<Option<[u8; 32]>, kairo_store::secrets::SecretError> {
        Err(kairo_store::secrets::SecretError::Keychain)
    }
    fn save(&self, _: &str, _: &[u8; 32]) -> Result<(), kairo_store::secrets::SecretError> {
        Err(kairo_store::secrets::SecretError::Keychain)
    }
}

pub fn paths() -> Result<AppPaths, Box<dyn std::error::Error>> {
    let Some(root) = std::env::var_os("KAIRO_BENCH_ROOT") else {
        return Ok(kairo_store::paths::detect()?);
    };
    let root = PathBuf::from(root);
    if !root.is_absolute() {
        return Err("KAIRO_BENCH_ROOT must be absolute".into());
    }
    let marker = root.join("kairo-benchmark-profile");
    if root.exists() && !marker.is_file() {
        return Err("Refusing a benchmark directory without its profile marker".into());
    }
    std::fs::create_dir_all(&root)?;
    std::fs::write(marker, b"Development benchmark data only\n")?;
    let mut inputs = detect_inputs()?;
    inputs.home = root.to_string_lossy().into_owned();
    inputs.documents = Some(root.join("Documents").to_string_lossy().into_owned());
    inputs.videos = Some(root.join("Movies").to_string_lossy().into_owned());
    inputs.packaged = false;
    inputs.shared_profile = false;
    let paths = compute(&inputs);
    let workspace = paths.default_workspace_root();
    let songs = workspace.join("Songs");
    std::fs::create_dir_all(&songs)?;
    for i in 0..5_000 {
        let song: kairo_domain::lyrics::LyricsSong = serde_json::from_value(json!({
            "id": format!("bench-{i:05}"), "title": format!("Song {i:05}"), "artist": "Benchmark",
            "sections": [{"type":"verse", "label":"Verse 1", "lines":["Amazing grace", "How sweet the sound"]}],
            "createdAt": 1, "updatedAt": 1
        }))?;
        let path = songs.join(kairo_domain::workspace::song_file_name(&song));
        if !path.exists() {
            std::fs::write(path, kairo_domain::workspace::serialize_song_file(&song)?)?;
        }
    }
    let media = workspace.join("Media");
    std::fs::create_dir_all(&media)?;
    // A valid, opaque 1x1 PNG; the workload measures library UI, not image decoding.
    let png: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 4,
        0, 0, 0, 181, 28, 12, 2, 0, 0, 0, 11, 73, 68, 65, 84, 120, 218, 99, 100, 248, 15, 0, 1, 5,
        1, 1, 39, 24, 227, 102, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];
    for i in 0..2_000 {
        let path = media.join(format!("Image {i:05}.png"));
        if !path.exists() {
            std::fs::write(path, png)?;
        }
    }
    Ok(paths)
}

pub fn install(
    shell: &Entity<ui_shell::Shell>,
    dock: &Entity<ui_media::MediaDockView>,
    lyrics: &Entity<ui_lyrics::LyricsWorkspace>,
    services: Arc<crate::services::ApplicationServices>,
    window: &mut gpui_kit::Window,
    cx: &mut App,
) {
    // Require the isolated profile even for the account-screen bypass.
    if std::env::var_os("KAIRO_BENCH_ROOT").is_none() {
        return;
    }
    let shell = shell.downgrade();
    let dock = dock.downgrade();
    let lyrics = lyrics.downgrade();
    let route = std::env::var("KAIRO_BENCH_ROUTE").unwrap_or_else(|_| "operator".into());
    let handle = window.window_handle();
    cx.spawn(async move |cx| {
        cx.background_executor().timer(Duration::from_secs(5)).await;
        cx.update(|cx| {
            cx.activate(true);
            let _ = handle.update(cx, |_, window, _| window.activate_window());
            let _ = shell.update(cx, |s, cx| {
                s.set_front(None, cx);
                s.navigate(match route.as_str() {
                    "lyrics" => ui_shell::Route::Lyrics,
                    "scripture" => ui_shell::Route::Scripture,
                    "theme" => ui_shell::Route::Theme,
                    "web" => ui_shell::Route::Web,
                    _ => ui_shell::Route::Operator,
                }, cx);
            });
            if route == "media" {
                let _ = dock.update(cx, |d, cx| {
                    d.model().clone().update(cx, |m, cx| m.set_open(true, cx));
                });
            }
        });
        if std::env::var_os("KAIRO_BENCH_STREAM").is_some() {
            let publisher = services.core.publisher();
            services.runtime().spawn(async move {
                use kairo_stt::recording::{RecordedEvent, read_recording};
                let recording = std::env::var("KAIRO_STT_REPLAY_PATH").ok()
                    .map(std::fs::read_to_string).transpose()
                    .and_then(|s| s.map(|s| read_recording(&s).map_err(std::io::Error::other)).transpose());
                let events = match recording {
                    Ok(Some(events)) if !events.is_empty() => events,
                    Ok(_) => {
                        let mut synthetic: Vec<_> = (0..10).map(|i| RecordedEvent::Interim { result: kairo_domain::transcript::InterimResult {
                        audio_epoch: Some(1.), audio_start: Some(i as f64), audio_duration: Some(1.),
                        text: format!("For God so loved the world that he gave his only Son {i}"), stability: 0.9, timestamp: i as f64,
                        }}).collect();
                        let result = serde_json::from_value(json!({"id":"benchmark", "text":"For God so loved the world that he gave his only Son.", "words":[], "isFinal":true, "timestamp":0, "duration":1}));
                        match result {
                            Ok(result) => synthetic.push(RecordedEvent::Final { result }),
                            Err(e) => { tracing::error!(%e, "benchmark transcript invalid"); return; }
                        }
                        synthetic
                    },
                    Err(e) => { tracing::error!(%e, "benchmark replay refused"); return; }
                };
                for (index, event) in events.into_iter().cycle().enumerate() {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    match event {
                        RecordedEvent::Interim { result } => publisher.emit(result),
                        RecordedEvent::Final { mut result } => {
                            result.id = format!("{}-{index}", result.id);
                            result.timestamp = kairo_domain::js::now_ms();
                            publisher.emit(result);
                        },
                        RecordedEvent::Gap { .. } => unreachable!("read_recording refuses gaps"),
                    }
                }
            });
        }
        let Some(report) = std::env::var_os("KAIRO_BENCH_REPORT") else { return };
        cx.background_executor().timer(Duration::from_secs(5)).await;
        gpui_kit::profiler::set_trace_enabled(true);
        let mut collector = gpui_kit::profiler::FrameTimingCollector::new();
        let counts = cx.update(|cx| (
            lyrics.update(cx, |p, cx| p.model().read(cx).shown().len()).unwrap_or(0),
            dock.update(cx, |d, cx| d.model().read(cx).library().items.len()).unwrap_or(0),
        ));
        let report = PathBuf::from(report);
        if let Err(e) = std::fs::write(report.with_extension("started"), b"measuring\n") {
            tracing::error!(%e, "benchmark start marker failed");
            return;
        }
        if matches!(route.as_str(), "lyrics" | "media") {
            cx.spawn(async move |cx| {
                use gpui_kit::{PlatformInput, ScrollDelta, ScrollEvent, point, px};
                for frame in 0..1_800 {
                    cx.background_executor().timer(Duration::from_nanos(16_666_667)).await;
                    if handle.update(cx, |_, window, cx| {
                        let y = if route == "media" { f32::from(window.viewport_size().height) - 100. } else { 250. };
                        window.dispatch_event(PlatformInput::Scroll(ScrollEvent {
                            position: point(px(if route == "lyrics" { 200. } else { 520. }), px(if route == "lyrics" { 400. } else { y })),
                            delta: ScrollDelta::Pixels(point(px(0.), px(if (frame / 300) % 2 == 0 { -32. } else { 32. }))),
                            ..Default::default()
                        }), cx);
                    }).is_err() { break; }
                }
            }).detach();
        }
        cx.background_executor().timer(Duration::from_secs(30)).await;
        let mut draw = Vec::new();
        let mut intervals = Vec::new();
        let mut last = None;
        for event in collector.collect_unseen() {
            match event {
                gpui_kit::profiler::FrameEvent::Draw(t) => draw.push(t.draw_duration().as_secs_f64() * 1000.),
                gpui_kit::profiler::FrameEvent::Present(t) => {
                    if let Some(previous) = last { intervals.push(t.present_start.duration_since(previous).as_secs_f64() * 1000.); }
                    last = Some(t.present_start);
                }
            }
        }
        fn summary(mut values: Vec<f64>) -> serde_json::Value {
            values.sort_by(f64::total_cmp);
            if values.is_empty() { return serde_json::Value::Null; }
            json!({"n":values.len(), "mean_ms":values.iter().sum::<f64>() / values.len() as f64,
                "p50_ms":values[values.len()/2], "p95_ms":values[(values.len()-1)*95/100]})
        }
        let value = json!({"pid":std::process::id(), "seconds":30, "songs":counts.0, "media_items":counts.1, "draw":summary(draw), "present_intervals":summary(intervals)});
        if let Err(e) = std::fs::write(report, value.to_string()) { tracing::error!(%e, "benchmark report failed"); }
        gpui_kit::profiler::set_trace_enabled(false);
    }).detach();
}
