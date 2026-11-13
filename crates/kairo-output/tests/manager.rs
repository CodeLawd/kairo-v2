//! The surface manager against a fake engine. Ports
//! `video-devices/__tests__/surface-manager.test.ts`, the routing and
//! lifecycle cases of `surface.test.ts`, `overlay-dispatch.test.ts` and
//! `program-preview-sync.test.ts`, with crash, replug, lobby and stage cases.

use kairo_core::Publisher;
use kairo_core::engine::OutputEngine;
use kairo_domain::media::{MediaItem, MediaKind, MediaPlayback};
use kairo_domain::overlay::{OutputSource, OverlayOutput, OverlayOutputKind, Size};
use kairo_domain::overlay_defaults::make_overlay_output;
use kairo_domain::program::{PresentationSettings, ProgramState, ProgramWeb, StageDisplayConfig};
use kairo_domain::video_devices::{VideoDeviceSnapshot, VideoOutputState};
use kairo_engine_proto::*;
use kairo_output::fake::FakeEngine;
use kairo_output::manager::{SettingsRewritten, ShowKey, StageStatusChanged};
use kairo_output::{LobbyMedia, OutputSettings, SurfaceManager};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// ─── Doubles ────────────────────────────────────────────────────────────────

#[derive(Default)]
struct Settings {
    outputs: Mutex<Vec<OverlayOutput>>,
    presentation: Mutex<PresentationSettings>,
    presentation_reads: AtomicUsize,
    rebinds: Mutex<Vec<(String, u32, u32, bool)>>,
}

impl OutputSettings for Settings {
    fn outputs(&self) -> Vec<OverlayOutput> {
        self.outputs.lock().unwrap().clone()
    }
    fn presentation(&self) -> PresentationSettings {
        self.presentation_reads.fetch_add(1, Ordering::SeqCst);
        self.presentation.lock().unwrap().clone()
    }
    fn persist_display_rebinding(&self, output: &str, old: u32, new: u32, stage: bool) -> bool {
        let mut outputs = self.outputs.lock().unwrap();
        let Some(o) = outputs.iter_mut().find(|o| o.id == output) else {
            return false;
        };
        if o.display_id != Some(old as f64) || old == new {
            return false;
        }
        o.display_id = Some(new as f64);
        self.rebinds
            .lock()
            .unwrap()
            .push((output.into(), old, new, stage));
        true
    }
}

#[derive(Default)]
struct Media {
    playlists: Mutex<BTreeMap<String, Vec<MediaItem>>>,
}

impl LobbyMedia for Media {
    fn playlist_items(&self, id: &str) -> Vec<MediaItem> {
        self.playlists
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .unwrap_or_default()
    }
    fn playback(&self, _id: &str) -> MediaPlayback {
        MediaPlayback::default()
    }
}

fn item(id: &str, kind: MediaKind) -> MediaItem {
    MediaItem {
        id: id.into(),
        name: id.into(),
        path: format!("/media/{id}"),
        kind,
        ext: String::new(),
        folder: String::new(),
        size_bytes: 1.,
    }
}

struct Rig {
    engine: Arc<FakeEngine>,
    settings: Arc<Settings>,
    media: Arc<Media>,
    topics: Publisher,
    manager: SurfaceManager,
}

fn rig() -> Rig {
    let engine = Arc::new(FakeEngine::new());
    let settings = Arc::new(Settings::default());
    let media = Arc::new(Media::default());
    let topics = Publisher::default();
    let mut manager = SurfaceManager::new(
        engine.clone(),
        settings.clone(),
        media.clone(),
        topics.clone(),
    )
    .with_clock(|| 1_000_000.);
    manager.init();
    Rig {
        engine,
        settings,
        media,
        topics,
        manager,
    }
}

fn display(id: u32, label: &str, x: i32, control: bool) -> DisplayInfo {
    DisplayInfo {
        id,
        label: label.into(),
        bounds: Rect {
            x,
            y: 0,
            width: 1920,
            height: 1080,
        },
        scale_factor: 1.,
        native_id: id as u64,
        primary: control,
        hosts_main_window: control,
        refresh_millihertz: None,
    }
}

fn output(id: &str, kind: OverlayOutputKind) -> OverlayOutput {
    OverlayOutput {
        enabled: true,
        ..make_overlay_output(id, kind)
    }
}

fn screen(id: &str, display: u32, label: &str) -> OverlayOutput {
    OverlayOutput {
        display_id: Some(display as f64),
        display_label: label.into(),
        display_size: Some(Size {
            width: 1920.,
            height: 1080.,
            extra: Default::default(),
        }),
        ..output(id, OverlayOutputKind::Screen)
    }
}

fn card(id: &str) -> OverlayOutput {
    OverlayOutput {
        device_port_id: "decklink:0:0".into(),
        device_mode_id: "1080p25".into(),
        ..output(id, OverlayOutputKind::Device)
    }
}

fn stage(id: &str, display: Option<u32>, label: &str) -> StageDisplayConfig {
    serde_json::from_value(serde_json::json!({
        "id": id, "name": "Stage", "enabled": true,
        "displayId": display, "displayLabel": label,
        "displaySize": {"width": 1920, "height": 1080},
        "showNext": true, "showClock": true, "showTimer": true
    }))
    .unwrap()
}

fn devices(ready: &[(&str, bool, &str)]) -> VideoDeviceSnapshot {
    VideoDeviceSnapshot {
        ports: Vec::new(),
        providers: Vec::new(),
        outputs: ready
            .iter()
            .map(|(id, ready, reason)| {
                (
                    (*id).into(),
                    VideoOutputState {
                        ready: *ready,
                        reason: (*reason).into(),
                        test_pattern: None,
                        diagnostics: None,
                    },
                )
            })
            .collect(),
    }
}

fn show(engine: &FakeEngine, id: &str, text: &str) -> bool {
    engine
        .command(Command::ShowSlide {
            output: id.into(),
            source: id.into(),
            slide: SlideContent {
                reference: "John 3:16".into(),
                text: text.into(),
                theme: Json::encode(&kairo_domain::overlay_defaults::DEFAULT_OVERLAY_THEME.clone()),
                colored_lines: None,
            },
            instant: false,
        })
        .accepted
}

fn painted(engine: &FakeEngine, id: &str) -> Option<String> {
    engine.state(id).and_then(|s| s.last_render).map(|r| r.text)
}

// ─── surface-manager.test.ts ────────────────────────────────────────────────

#[tokio::test]
async fn manager_retains_current_content_across_disconnect_and_excludes_filtered_content() {
    let mut r = rig();
    let card = card("card");
    r.manager.reconcile(Some(vec![card.clone()])).await;
    r.manager
        .video_devices_changed(devices(&[("card", true, "")]));
    assert!(r.manager.takes_push(&card));
    assert!(show(&r.engine, "card", "Current content"));

    let mut no_scripture = card.clone();
    no_scripture.show.scripture = false;
    r.manager.reconcile(Some(vec![no_scripture])).await;
    assert!(r.manager.live_targets(Some(ShowKey::Scripture)).is_empty());

    // The port goes away: the engine parks the card and reports it.
    r.manager.reconcile(Some(vec![card.clone()])).await;
    r.engine.set_status(
        "card",
        SurfaceStatus::Parked {
            reason: "Port disconnected".into(),
        },
    );
    r.manager
        .video_devices_changed(devices(&[("card", false, "Port disconnected")]));
    assert!(!r.manager.takes_push(&card));
    assert_eq!(r.manager.output_state(&card).reason, "Port disconnected");

    // Replugged: the same surface comes back showing the same slide.
    r.engine.set_status("card", SurfaceStatus::Ready);
    r.manager
        .video_devices_changed(devices(&[("card", true, "")]));
    r.manager.reconcile(Some(vec![card.clone()])).await;
    assert!(r.manager.takes_push(&card));
    assert_eq!(
        painted(&r.engine, "card").as_deref(),
        Some("Current content")
    );
    r.manager.destroy_all();
    assert!(r.engine.surfaces().is_empty());
}

// ─── surface.test.ts (what the manager owns; capture paths are 061–064) ─────

#[tokio::test]
async fn device_mode_change_and_replug_keep_the_slide() {
    let mut r = rig();
    let mut out = card("card");
    r.manager.reconcile(Some(vec![out.clone()])).await;
    show(&r.engine, "card", "Keep this slide");
    out.device_mode_id = "1080p50".into();
    r.manager.reconcile(Some(vec![out.clone()])).await;
    let state = r.engine.state("card").unwrap();
    assert!(matches!(&state.spec.sink, SinkSpec::Device { mode_id, .. } if mode_id == "1080p50"));
    assert_eq!(
        painted(&r.engine, "card").as_deref(),
        Some("Keep this slide")
    );
}

#[tokio::test]
async fn test_patterns_need_a_ready_card_and_never_replace_the_slide() {
    use kairo_domain::video_devices::VideoTestPattern;
    let mut r = rig();
    r.manager.reconcile(Some(vec![card("card")])).await;
    show(&r.engine, "card", "Keep current verse");
    assert!(
        !r.manager
            .set_video_test_pattern("card", Some(VideoTestPattern::Motion))
    );
    r.manager
        .video_devices_changed(devices(&[("card", true, "")]));
    assert!(
        r.manager
            .set_video_test_pattern("card", Some(VideoTestPattern::Motion))
    );
    assert_eq!(
        r.manager.video_test_pattern("card"),
        Some(VideoTestPattern::Motion)
    );
    assert_eq!(
        r.manager.video_devices().outputs["card"].test_pattern,
        Some(Some(VideoTestPattern::Motion))
    );
    assert!(r.manager.set_video_test_pattern("card", None));
    assert_eq!(r.manager.video_test_pattern("card"), None);
    assert_eq!(
        painted(&r.engine, "card").as_deref(),
        Some("Keep current verse")
    );
    assert!(!r.manager.set_video_test_pattern("missing", None));
}

#[tokio::test]
async fn a_live_web_page_reaches_the_screen_once_reloads_and_leaves() {
    let mut r = rig();
    r.manager
        .reconcile(Some(vec![output("feed", OverlayOutputKind::Ndi)]))
        .await;
    r.engine.take_commands();
    let web = |reload| ProgramState {
        web: Some(ProgramWeb {
            url: "https://example.com/".into(),
            reload,
        }),
        ..ProgramState::default()
    };
    let sent = |engine: &FakeEngine| {
        engine
            .take_commands()
            .into_iter()
            .filter(|c| matches!(c, Command::SetWeb { .. }))
            .collect::<Vec<_>>()
    };
    r.manager.set_program(web(0.));
    assert_eq!(sent(&r.engine).len(), 1);
    r.manager.set_program(web(0.));
    assert!(
        sent(&r.engine).is_empty(),
        "an unchanged page is not sent again"
    );
    r.manager.set_program(web(1.));
    assert!(
        matches!(&sent(&r.engine)[..], [Command::SetWeb { web: Some(w), .. }] if w.reload == 1.)
    );
    r.manager.set_program(ProgramState::default());
    assert!(matches!(
        &sent(&r.engine)[..],
        [Command::SetWeb { web: None, .. }]
    ));
}

#[tokio::test]
async fn only_the_audio_surface_may_sound() {
    let mut r = rig();
    r.engine
        .set_displays(vec![display(1, "Projector", 0, false)]);
    r.manager
        .reconcile(Some(vec![
            output("feed", OverlayOutputKind::Ndi),
            screen("main", 1, "Projector"),
        ]))
        .await;
    let audio = |id: &str| r.engine.state(id).unwrap().config.audio.enabled;
    assert!(audio("main"), "the first live screen plays sound");
    assert!(!audio("feed"));
    // Without a screen, the primary NDI feed does.
    r.manager
        .reconcile(Some(vec![output("feed", OverlayOutputKind::Ndi)]))
        .await;
    assert!(audio("feed"));
}

#[tokio::test]
async fn the_web_page_scrolls_with_the_preview_only_while_one_is_live() {
    let mut r = rig();
    r.manager
        .reconcile(Some(vec![output("feed", OverlayOutputKind::Ndi)]))
        .await;
    r.engine.take_commands();
    r.manager.scroll_web(0., 400.);
    assert!(
        r.engine.take_commands().is_empty(),
        "nothing to scroll without a page"
    );
    r.manager.set_program(ProgramState {
        web: Some(ProgramWeb {
            url: "https://example.com/".into(),
            reload: 0.,
        }),
        ..ProgramState::default()
    });
    r.engine.take_commands();
    r.manager.scroll_web(0., 400.6);
    assert!(matches!(&r.engine.take_commands()[..], [Command::WebScroll { y, .. }] if *y == 400.6));
}

// ─── overlay-dispatch.test.ts ───────────────────────────────────────────────

#[tokio::test]
async fn screens_cards_and_every_ndi_feed_take_independent_pushes() {
    let mut r = rig();
    r.engine
        .set_displays(vec![display(1, "Projector", 0, false)]);
    let outputs = vec![
        screen("projector", 1, "Projector"),
        card("card"),
        output("main", OverlayOutputKind::Ndi),
        output("stream", OverlayOutputKind::Ndi),
        OverlayOutput {
            enabled: false,
            ..screen("off", 1, "Projector")
        },
    ];
    r.manager.reconcile(Some(outputs)).await;
    r.manager
        .video_devices_changed(devices(&[("card", true, "")]));
    let ids: Vec<_> = r
        .manager
        .live_targets(None)
        .into_iter()
        .map(|o| o.id)
        .collect();
    assert_eq!(ids, ["projector", "card", "main", "stream"]);
    assert_eq!(
        r.manager.followers(),
        ["main", "stream", "projector", "card"]
    );
    assert!(r.manager.is_primary_ndi("main"));
    let SinkSpec::Ndi { name } = r.engine.state("stream").unwrap().spec.sink else {
        panic!()
    };
    assert_eq!(name, "Kairo stream");
    let SinkSpec::Ndi { name } = r.engine.state("main").unwrap().spec.sink else {
        panic!()
    };
    assert_eq!(name, "Kairo Scripture");
}

#[tokio::test]
async fn playlist_destinations_are_excluded_from_pushes_and_clear() {
    let mut r = rig();
    r.engine.set_displays(vec![display(1, "Foyer", 0, false)]);
    let lobby = OverlayOutput {
        source: OutputSource::Playlist,
        ..screen("lobby", 1, "Foyer")
    };
    let card_lobby = OverlayOutput {
        source: OutputSource::Playlist,
        ..card("card")
    };
    r.manager
        .reconcile(Some(vec![lobby.clone(), card_lobby]))
        .await;
    r.manager
        .video_devices_changed(devices(&[("card", true, "")]));
    assert!(r.manager.live_targets(None).is_empty());
    assert!(r.manager.followers().is_empty());
    assert_eq!(
        r.manager.output_state(&lobby).reason,
        "Choose a playlist to play"
    );
}

// ─── program-preview-sync.test.ts ───────────────────────────────────────────

#[tokio::test]
async fn a_text_clear_publishes_current_null_and_other_changes_keep_it() {
    use kairo_orchestrator::clients::ProgramClient;
    let settings = Arc::new(Settings::default());
    let topics = Publisher::default();
    let applied = Arc::new(Mutex::new(Vec::<ProgramState>::new()));
    let sink = applied.clone();
    let program = kairo_output::ProgramService::new(
        settings,
        topics.clone(),
        Box::new(move |state| sink.lock().unwrap().push(state)),
    );
    let mut watch = topics.watch::<ProgramState>();
    program.set_slide("John 1:3".into(), "He created everything.".into(), None);
    program.show_message("Welcome");
    assert_eq!(
        watch.latest().unwrap().current.as_ref().unwrap().reference,
        "John 1:3"
    );
    program.clear_current();
    assert_eq!(watch.latest().unwrap().current, None);
    let applied = applied.lock().unwrap();
    assert_eq!(applied.len(), 3);
    assert_eq!(applied[1].message.as_deref(), Some("Welcome"));
    // A second clear changes nothing and sends nothing.
    drop(applied);
    program.clear_current();
}

#[tokio::test]
async fn program_service_validates_its_inputs() {
    let settings = Arc::new(Settings::default());
    let program =
        kairo_output::ProgramService::new(settings, Publisher::default(), Box::new(|_| {}));
    assert_eq!(
        program.set_prop("gone", true).unwrap_err(),
        "That prop no longer exists."
    );
    assert!(program.set_prop("gone", false).is_ok());
    assert!(program.set_web(Some("file:///etc/passwd")).is_err());
    let state = program.set_web(Some("example.com")).unwrap();
    assert_eq!(state.web.as_ref().unwrap().reload, 0.);
    assert_eq!(program.reload_web().web.unwrap().reload, 1.);
    assert_eq!(program.show_message("  ").message, None);
    assert_eq!(
        program
            .show_message(&"x".repeat(1200))
            .message
            .unwrap()
            .len(),
        1000
    );
    let state = program.set_camera(Some(" Cam "), Some(" Mic "));
    assert_eq!(
        (state.camera.as_deref(), state.camera_audio.as_deref()),
        (Some("Cam"), Some("Mic"))
    );
    assert_eq!(program.set_camera(None, Some("Mic")).camera_audio, None);
    let started = program.start_timer(None);
    assert!(started.timer.ends_at.is_some());
}

// ─── Displays, stages, rebinding ────────────────────────────────────────────

#[tokio::test]
async fn screen_reasons_and_a_program_screen_outranks_a_stage() {
    let mut r = rig();
    r.engine.set_displays(vec![
        display(1, "Laptop", 0, true),
        display(2, "Projector", 1920, false),
    ]);
    {
        let mut p = r.settings.presentation.lock().unwrap();
        p.stage_displays = vec![
            stage("s1", Some(2), "Projector"),
            stage("s2", None, ""),
            stage("s3", Some(9), "Gone"),
            stage("s4", Some(1), "Laptop"),
        ];
    }
    let none = OverlayOutput {
        display_id: None,
        ..screen("none", 0, "")
    };
    let outputs = vec![
        screen("main", 2, "Projector"),
        screen("second", 2, "Projector"),
        screen("gone", 7, "Missing"),
        none,
    ];
    r.manager.reconcile(Some(outputs)).await;
    let reason = |m: &SurfaceManager, id: &str| m.screen_state(id).reason;
    assert_eq!(reason(&r.manager, "main"), "Live");
    assert_eq!(
        reason(&r.manager, "second"),
        "Display in use by another screen"
    );
    assert_eq!(reason(&r.manager, "gone"), "Display not connected");
    assert_eq!(reason(&r.manager, "none"), "No display chosen");
    let stages: Vec<_> = r
        .manager
        .stage_status()
        .into_iter()
        .map(|s| (s.ready, s.reason))
        .collect();
    assert_eq!(
        stages,
        [
            (false, "Display in use by a program screen".into()),
            (false, "No display chosen".into()),
            (false, "Display not connected".into()),
            (
                true,
                "On the control display — showing a rehearsal window".into()
            ),
        ]
    );
    // The stage surface is a stage from its first command.
    let s4 = r.engine.state("s4").unwrap();
    assert!(s4.stage.is_some());
    // A window that fails to open keeps Electron's words.
    r.engine.set_status(
        "main",
        SurfaceStatus::Unavailable {
            reason: "no GPU".into(),
        },
    );
    assert_eq!(reason(&r.manager, "main"), "Screen window failed to open");
    r.engine.set_status(
        "s4",
        SurfaceStatus::Unavailable {
            reason: "no GPU".into(),
        },
    );
    assert_eq!(
        r.manager.stage_status()[3].reason,
        "Stage window failed to open"
    );
}

#[tokio::test]
async fn stage_status_is_announced_only_when_it_changes() {
    let mut r = rig();
    r.engine
        .set_displays(vec![display(2, "Projector", 0, false)]);
    r.settings.presentation.lock().unwrap().stage_displays =
        vec![stage("s1", Some(2), "Projector")];
    let mut watch = r.topics.watch::<StageStatusChanged>();
    r.manager.reconcile(Some(Vec::new())).await;
    assert_eq!(watch.latest().unwrap().0[0].reason, "Live");
    r.manager.reconcile(Some(Vec::new())).await;
    r.manager.emit_stage_status();
    let repeat = tokio::time::timeout(Duration::from_millis(50), watch.next()).await;
    assert!(repeat.is_err(), "no repeat for the same status");
    r.engine.set_displays(Vec::new());
    r.manager.reconcile(Some(Vec::new())).await;
    assert_eq!(watch.latest().unwrap().0[0].reason, "Display not connected");
}

#[tokio::test]
async fn a_rematched_display_id_is_persisted_and_rebroadcast() {
    let mut r = rig();
    // Windows re-ids a replugged projector; label and size still match.
    r.engine
        .set_displays(vec![display(42, "Projector", 0, false)]);
    let main = screen("main", 7, "Projector");
    *r.settings.outputs.lock().unwrap() = vec![main.clone()];
    let mut rewrites = r.topics.stream::<SettingsRewritten>();
    r.manager.reconcile(None).await;
    assert_eq!(r.manager.screen_state("main").reason, "Live");
    assert_eq!(
        *r.settings.rebinds.lock().unwrap(),
        [("main".to_string(), 7, 42, false)]
    );
    assert!(matches!(rewrites.next().await, Some(Ok(_))));
    let SinkSpec::Display(sink) = r.engine.state("main").unwrap().spec.sink else {
        panic!()
    };
    assert_eq!(sink.binding.display_id, Some(42));
    // The engine re-matching between reconciles: persisted while still old.
    r.manager.display_rebound("main", 42, 43, false);
    assert_eq!(r.settings.outputs.lock().unwrap()[0].display_id, Some(43.));
    r.manager.display_rebound("main", 42, 44, false);
    assert_eq!(
        r.settings.outputs.lock().unwrap()[0].display_id,
        Some(43.),
        "a newer choice wins"
    );
}

#[tokio::test]
async fn an_unplugged_screen_parks_with_its_slide_and_a_replug_repaints_it() {
    let mut r = rig();
    r.engine
        .set_displays(vec![display(2, "Projector", 0, false)]);
    let main = screen("main", 2, "Projector");
    r.manager.reconcile(Some(vec![main.clone()])).await;
    show(&r.engine, "main", "Still on screen");
    r.engine.set_displays(Vec::new());
    r.manager.reconcile(Some(vec![main.clone()])).await;
    assert!(r.engine.state("main").unwrap().parked);
    assert!(!r.manager.takes_push(&main));
    r.engine
        .set_displays(vec![display(2, "Projector", 0, false)]);
    r.manager.reconcile(Some(vec![main.clone()])).await;
    assert!(!r.engine.state("main").unwrap().parked);
    assert_eq!(
        painted(&r.engine, "main").as_deref(),
        Some("Still on screen")
    );
    // Turned off: the surface and its slide go; back on starts blank.
    r.manager
        .reconcile(Some(vec![OverlayOutput {
            enabled: false,
            ..main.clone()
        }]))
        .await;
    assert!(r.engine.state("main").is_none());
    r.manager.reconcile(Some(vec![main])).await;
    assert_eq!(painted(&r.engine, "main"), None);
}

#[tokio::test]
async fn closing_the_main_window_closes_screens_and_stages_but_keeps_content() {
    let mut r = rig();
    r.engine.set_displays(vec![
        display(1, "Projector", 0, false),
        display(2, "Confidence", 1920, false),
    ]);
    r.settings.presentation.lock().unwrap().stage_displays =
        vec![stage("s1", Some(2), "Confidence")];
    *r.settings.outputs.lock().unwrap() = vec![
        screen("main", 1, "Projector"),
        output("feed", OverlayOutputKind::Ndi),
    ];
    r.manager.reconcile(None).await;
    show(&r.engine, "main", "Kept");
    r.manager.close_screens();
    assert!(r.engine.state("main").unwrap().parked);
    assert!(r.engine.state("s1").unwrap().parked);
    assert!(
        !r.engine.state("feed").unwrap().parked,
        "feeds keep running"
    );
    r.manager.reconcile(None).await;
    assert!(!r.engine.state("main").unwrap().parked);
    assert!(!r.engine.state("s1").unwrap().parked);
    assert_eq!(painted(&r.engine, "main").as_deref(), Some("Kept"));
}

// ─── No-change work, presentation cache, crash ──────────────────────────────

#[tokio::test]
async fn a_reconcile_with_no_change_does_no_engine_work() {
    let mut r = rig();
    r.engine.set_displays(vec![
        display(1, "Projector", 0, false),
        display(2, "Stage", 1920, false),
    ]);
    r.settings.presentation.lock().unwrap().stage_displays = vec![stage("s1", Some(2), "Stage")];
    let outputs = vec![
        screen("main", 1, "Projector"),
        card("card"),
        output("feed", OverlayOutputKind::Ndi),
        output("syphon", OverlayOutputKind::Syphon),
    ];
    r.manager.reconcile(Some(outputs.clone())).await;
    r.manager.set_program(ProgramState {
        message: Some("Welcome".into()),
        logo: true,
        ..ProgramState::default()
    });
    assert!(!r.engine.take_commands().is_empty());
    let started = std::time::Instant::now();
    r.manager.reconcile(Some(outputs)).await;
    let elapsed = started.elapsed();
    assert_eq!(r.engine.take_commands(), Vec::<Command>::new());
    eprintln!("no-change reconcile: {elapsed:?}, 0 commands");
    r.manager.set_program(r.manager.program().clone());
    assert!(r.engine.take_commands().is_empty());
}

#[tokio::test]
async fn program_changes_use_the_cached_presentation() {
    let mut r = rig();
    r.manager
        .reconcile(Some(vec![output("feed", OverlayOutputKind::Ndi)]))
        .await;
    let reads = r.settings.presentation_reads.load(Ordering::SeqCst);
    for n in 0..20 {
        r.manager.set_program(ProgramState {
            message: Some(format!("m{n}")),
            ..ProgramState::default()
        });
    }
    assert_eq!(r.settings.presentation_reads.load(Ordering::SeqCst), reads);
}

#[tokio::test]
async fn a_crashed_engine_gets_every_surface_back_and_the_manager_sends_nothing_new() {
    let mut r = rig();
    r.engine
        .set_displays(vec![display(1, "Projector", 0, false)]);
    let outputs = vec![
        screen("main", 1, "Projector"),
        output("feed", OverlayOutputKind::Ndi),
    ];
    r.manager.reconcile(Some(outputs.clone())).await;
    r.manager.set_program(ProgramState {
        logo: true,
        ..ProgramState::default()
    });
    show(&r.engine, "main", "Survives a crash");
    let replay = r.engine.crash();
    assert!(replay.iter().any(|c| matches!(c,
        Command::ShowSlide { output, slide, instant: true, .. } if output == "main" && slide.text == "Survives a crash")));
    assert!(replay.iter().any(
        |c| matches!(c, Command::SetLayer { output, kind: LayerKind::Logo, .. } if output == "feed")
    ));
    r.engine.take_commands();
    r.manager.reconcile(Some(outputs)).await;
    assert!(r.engine.take_commands().is_empty());
    // A stopped engine: nothing takes a push.
    r.engine.stop();
    assert!(r.manager.live_targets(None).is_empty());
}

#[tokio::test]
async fn a_slow_card_never_holds_up_screens() {
    let mut r = rig();
    r.engine
        .set_displays(vec![display(1, "Projector", 0, false)]);
    let main = screen("main", 1, "Projector");
    // The card has not answered at all.
    r.manager
        .reconcile(Some(vec![card("card"), main.clone()]))
        .await;
    assert!(r.manager.takes_push(&main));
    assert_eq!(
        r.manager.output_state(&card("card")).reason,
        "Output unavailable"
    );
}

// ─── Lobby ──────────────────────────────────────────────────────────────────

fn slides(engine: &FakeEngine) -> Vec<Command> {
    engine
        .take_commands()
        .into_iter()
        .filter(|c| {
            matches!(
                c,
                Command::ShowSlide { .. } | Command::VideoTransport { .. }
            )
        })
        .collect()
}

fn background(command: &Command) -> serde_json::Value {
    let Command::ShowSlide { slide, .. } = command else {
        panic!("{command:?}")
    };
    slide.theme.value().unwrap()["background"].clone()
}

async fn settle() {
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
}

#[tokio::test(start_paused = true)]
async fn lobby_images_hold_videos_play_once_and_one_video_rewinds() {
    let mut r = rig();
    r.engine.set_displays(vec![display(1, "Foyer", 0, false)]);
    let lobby = OverlayOutput {
        source: OutputSource::Playlist,
        playlist_id: "walk-in".into(),
        slide_sec: 3.,
        ..screen("lobby", 1, "Foyer")
    };
    r.manager.reconcile(Some(vec![lobby.clone()])).await;
    // Empty: retried every 5 s.
    settle().await;
    assert!(slides(&r.engine).is_empty());
    r.media.playlists.lock().unwrap().insert(
        "walk-in".into(),
        vec![
            item("a.jpg", MediaKind::Image),
            item("b.mp4", MediaKind::Video),
        ],
    );
    tokio::time::advance(Duration::from_millis(4900)).await;
    settle().await;
    assert!(slides(&r.engine).is_empty());
    tokio::time::advance(Duration::from_millis(200)).await;
    settle().await;
    let shown = slides(&r.engine);
    assert_eq!(background(&shown[0])["mediaPath"], "/media/a.jpg");
    // The image holds for slideSec.
    tokio::time::advance(Duration::from_millis(2900)).await;
    settle().await;
    assert!(slides(&r.engine).is_empty());
    tokio::time::advance(Duration::from_millis(200)).await;
    settle().await;
    let shown = slides(&r.engine);
    assert_eq!(background(&shown[0])["mediaPath"], "/media/b.mp4");
    assert_eq!(
        background(&shown[0])["mediaLoop"],
        false,
        "a lobby video never loops"
    );
    // Polled after a 1 s grace, then every 500 ms, until it ends.
    let queries = r.engine.queries();
    tokio::time::advance(Duration::from_millis(900)).await;
    settle().await;
    assert_eq!(r.engine.queries(), queries);
    tokio::time::advance(Duration::from_millis(200)).await;
    settle().await;
    assert_eq!(r.engine.queries(), queries + 1);
    tokio::time::advance(Duration::from_millis(500)).await;
    settle().await;
    assert_eq!(r.engine.queries(), queries + 2);
    r.engine.set_video_ended("lobby", true);
    tokio::time::advance(Duration::from_millis(500)).await;
    settle().await;
    assert_eq!(
        background(&slides(&r.engine)[0])["mediaPath"],
        "/media/a.jpg"
    );

    // A one-video playlist rewinds instead of repainting.
    r.media
        .playlists
        .lock()
        .unwrap()
        .insert("solo".into(), vec![item("loop.mp4", MediaKind::Video)]);
    r.manager
        .reconcile(Some(vec![OverlayOutput {
            playlist_id: "solo".into(),
            ..lobby.clone()
        }]))
        .await;
    settle().await;
    assert!(matches!(
        &slides(&r.engine)[..],
        [Command::ShowSlide { .. }]
    ));
    tokio::time::advance(Duration::from_millis(1100)).await;
    settle().await;
    let again = slides(&r.engine);
    assert!(
        matches!(&again[..], [
            Command::VideoTransport { transport: VideoTransport::Seek(s), .. },
            Command::VideoTransport { transport: VideoTransport::Pause(false), .. },
        ] if *s == 0.),
        "{again:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn a_video_that_never_ends_moves_on_after_thirty_minutes() {
    let mut r = rig();
    r.engine.set_displays(vec![display(1, "Foyer", 0, false)]);
    r.media.playlists.lock().unwrap().insert(
        "p".into(),
        vec![
            item("stream.mp4", MediaKind::Video),
            item("b.jpg", MediaKind::Image),
        ],
    );
    let lobby = OverlayOutput {
        source: OutputSource::Playlist,
        playlist_id: "p".into(),
        ..screen("lobby", 1, "Foyer")
    };
    r.manager.reconcile(Some(vec![lobby])).await;
    settle().await;
    slides(&r.engine);
    tokio::time::advance(Duration::from_secs(29 * 60)).await;
    settle().await;
    assert!(slides(&r.engine).is_empty());
    for _ in 0..(2 * 61 * 2) {
        tokio::time::advance(Duration::from_millis(500)).await;
        settle().await;
    }
    let shown = slides(&r.engine);
    assert!(
        shown.iter().any(|c| matches!(c, Command::ShowSlide { .. })
            && background(c)["mediaPath"] == "/media/b.jpg")
    );
}

#[tokio::test(start_paused = true)]
async fn lobbies_never_get_service_pushes_camera_web_or_sound() {
    let mut r = rig();
    r.engine.set_displays(vec![display(1, "Foyer", 0, false)]);
    let lobby = OverlayOutput {
        source: OutputSource::Playlist,
        playlist_id: "p".into(),
        ..screen("lobby", 1, "Foyer")
    };
    r.manager
        .reconcile(Some(vec![
            lobby.clone(),
            output("feed", OverlayOutputKind::Ndi),
        ]))
        .await;
    r.manager.set_program(ProgramState {
        camera: Some("Cam".into()),
        camera_audio: Some("Mic".into()),
        web: Some(ProgramWeb {
            url: "https://example.com/".into(),
            reload: 0.,
        }),
        ..ProgramState::default()
    });
    let lobby_state = r.engine.state("lobby").unwrap();
    assert!(lobby_state.layers.camera.is_none());
    assert!(lobby_state.layers.web.is_none());
    assert!(!lobby_state.config.audio.enabled);
    assert!(!lobby_state.config.camera_audible);
    assert!(r.engine.state("feed").unwrap().layers.camera.is_some());
    assert!(r.engine.state("feed").unwrap().config.camera_audible);
    assert!(!r.manager.takes_push(&lobby));
    assert_eq!(
        r.manager.screen_state("lobby").reason,
        "Playing its own playlist — service pushes do not land here"
    );
    // Back to the service: the player stops and its item is cleared.
    r.engine.take_commands();
    r.manager
        .reconcile(Some(vec![OverlayOutput {
            source: OutputSource::Program,
            ..lobby.clone()
        }]))
        .await;
    assert!(
        r.engine
            .take_commands()
            .iter()
            .any(|c| matches!(c, Command::Clear { output } if output == "lobby"))
    );
    assert!(r.manager.followers().contains(&"lobby".to_string()));
}

#[tokio::test(start_paused = true)]
async fn a_card_playlist_starts_once_the_card_is_ready() {
    let mut r = rig();
    r.media
        .playlists
        .lock()
        .unwrap()
        .insert("p".into(), vec![item("a.jpg", MediaKind::Image)]);
    let lobby = OverlayOutput {
        source: OutputSource::Playlist,
        playlist_id: "p".into(),
        ..card("card")
    };
    r.manager.reconcile(Some(vec![lobby])).await;
    settle().await;
    assert!(slides(&r.engine).is_empty());
    r.manager
        .video_devices_changed(devices(&[("card", true, "")]));
    settle().await;
    assert_eq!(slides(&r.engine).len(), 1);
}

// ─── The actor ──────────────────────────────────────────────────────────────

#[test]
fn the_actor_serializes_reconciles_and_debounces_display_changes() {
    use kairo_output::actor::*;
    let core = kairo_core::AppCore::new().unwrap();
    let engine = Arc::new(FakeEngine::new());
    engine.set_displays(vec![display(1, "Projector", 0, false)]);
    let settings = Arc::new(Settings::default());
    *settings.outputs.lock().unwrap() = vec![screen("main", 1, "Projector")];
    let surfaces = Arc::new(kairo_orchestrator::engine::EngineSurfaces::new(
        engine.clone(),
    ));
    let (e, s, routing) = (engine.clone(), settings.clone(), surfaces.clone());
    let c = core.clone();
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async move {
            let inner = c.clone();
            c.run(async move {
                let c = inner;
                spawn_surface_manager(&c, e, s, Arc::new(Media::default()), Some(routing))
                    .await
                    .unwrap();
                // Two reconciles racing a settings write: one surface each.
                let (a, b) = tokio::join!(
                    c.call(Reconcile { outputs: None }),
                    c.call(Reconcile { outputs: None })
                );
                a.unwrap();
                b.unwrap();
            })
            .await
            .unwrap();
        });
    let opens = engine
        .commands()
        .iter()
        .filter(|c| matches!(c, Command::OpenSurface(s) if s.output == "main"))
        .count();
    assert_eq!(opens, 1);
    use kairo_orchestrator::clients::EngineClient;
    assert_eq!(surfaces.all_surfaces(), ["main"]);
    // A burst of display changes reconciles once, after it settles.
    let c = core.clone();
    let e = engine.clone();
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async move {
            let inner = c.clone();
            c.run(async move {
                let c = inner;
                let mut changed = c.watch::<kairo_output::manager::DisplaysChanged>();
                for _ in 0..5 {
                    e.set_displays(Vec::new());
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                let state = c
                    .call(GetOutputState {
                        output: screen("main", 1, "Projector"),
                    })
                    .await
                    .unwrap();
                assert!(state.ready, "not yet reconciled while changes keep coming");
                tokio::time::sleep(Duration::from_millis(400)).await;
                assert!(changed.latest().is_some());
                let state = c
                    .call(GetOutputState {
                        output: screen("main", 1, "Projector"),
                    })
                    .await
                    .unwrap();
                assert_eq!(state.reason, "Display not connected");
            })
            .await
            .unwrap();
        });
}
