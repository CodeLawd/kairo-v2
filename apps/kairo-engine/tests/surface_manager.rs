//! The surface manager on the real engine: reconcile, a push through the
//! orchestrator's routing, an engine crash with the slide replayed, and a
//! reconcile with no change sending nothing.
//!
//! Opens one rehearsal (half-size) window on the first display. Skips where
//! no window system is available unless `KAIRO_REQUIRE_ENGINE` is set.
#![cfg(unix)]

use kairo_core::engine::*;
use kairo_domain::media::{MediaItem, MediaPlayback};
use kairo_domain::overlay::{OverlayOutput, OverlayOutputKind, Size};
use kairo_domain::overlay_defaults::{DEFAULT_OVERLAY_THEME, make_overlay_output};
use kairo_domain::program::{PresentationSettings, ProgramState};
use kairo_engine_proto::*;
use kairo_orchestrator::clients::EngineClient;
use kairo_orchestrator::engine::EngineSurfaces;
use kairo_output::{LobbyMedia, OutputSettings, SurfaceManager};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

mod common;
use common::until;

const ENGINE: &str = env!("CARGO_BIN_EXE_kairo-engine");

struct Settings(Mutex<Vec<OverlayOutput>>);

impl OutputSettings for Settings {
    fn outputs(&self) -> Vec<OverlayOutput> {
        self.0.lock().unwrap().clone()
    }
    fn presentation(&self) -> PresentationSettings {
        PresentationSettings::default()
    }
    fn persist_display_rebinding(&self, _: &str, _: u32, _: u32, _: bool) -> bool {
        false
    }
}

struct NoMedia;

impl LobbyMedia for NoMedia {
    fn playlist_items(&self, _: &str) -> Vec<MediaItem> {
        Vec::new()
    }
    fn playback(&self, _: &str) -> MediaPlayback {
        MediaPlayback::default()
    }
}

/// The engine's command counter, read with a harmless command.
fn seq(engine: &EngineHandle) -> u64 {
    engine
        .command(Command::RetainSurfaces {
            outputs: engine.surfaces(),
        })
        .seq
}

#[tokio::test(flavor = "multi_thread")]
async fn the_manager_drives_the_real_engine_through_a_crash() {
    let mut options = EngineOptions::new(ENGINE);
    options.grace = Duration::from_secs(2);
    let engine = EngineHandle::spawn(options);
    let mut status = engine.watch_status();
    loop {
        match status.borrow_and_update().clone() {
            EngineStatus::Running { .. } => break,
            EngineStatus::Restarting { reason } | EngineStatus::Failed { reason }
                if reason.contains("exit status: 70")
                    && std::env::var_os("KAIRO_REQUIRE_ENGINE").is_none() =>
            {
                eprintln!("skipped: no window system ({reason})");
                return;
            }
            EngineStatus::Failed { reason } => panic!("{reason}"),
            _ => {}
        }
        tokio::time::timeout(Duration::from_secs(10), status.changed())
            .await
            .expect("engine start")
            .unwrap();
    }
    until("displays", || !engine.displays().is_empty()).await;
    let display = engine.displays()[0].clone();
    // Kairo's controls are on this display: the screen opens as a rehearsal window.
    engine.report_control_display(Some(display.native_id));
    let (width, height) = display.logical_size();
    let screen = OverlayOutput {
        enabled: true,
        display_id: Some(display.id as f64),
        display_label: display.label.clone(),
        display_size: Some(Size {
            width: width as f64,
            height: height as f64,
            extra: Default::default(),
        }),
        ..make_overlay_output("screen", OverlayOutputKind::Screen)
    };
    let settings = Arc::new(Settings(Mutex::new(vec![screen.clone()])));
    let client: Arc<dyn OutputEngine> = Arc::new(engine.clone());
    let surfaces = Arc::new(EngineSurfaces::new(client.clone()));
    let mut manager = SurfaceManager::new(
        client,
        settings,
        Arc::new(NoMedia),
        kairo_core::Publisher::default(),
    )
    .with_routing(surfaces.clone());
    manager.init();
    manager.reconcile(None).await;
    until("screen ready", || {
        engine.surface_status("screen") == Some(SurfaceStatus::Ready)
    })
    .await;
    assert!(manager.takes_push(&screen));
    assert_eq!(surfaces.all_surfaces(), ["screen"]);

    // A push through the orchestrator's client, and a program layer.
    assert!(
        surfaces
            .show_slide(
                "screen".into(),
                "John 3:16".into(),
                "For God so loved the world".into(),
                DEFAULT_OVERLAY_THEME.clone(),
                None,
            )
            .await
    );
    manager.set_program(ProgramState {
        message: Some("Welcome".into()),
        ..ProgramState::default()
    });

    // No change: nothing is sent.
    let before = seq(&engine);
    let started = Instant::now();
    manager.reconcile(None).await;
    let elapsed = started.elapsed();
    assert_eq!(
        seq(&engine),
        before + 1,
        "a no-change reconcile sent commands"
    );
    eprintln!("no-change reconcile on the real engine: {elapsed:?}");

    // Crash: the supervisor relaunches and replays the slide and the layer.
    let pid = engine.pid().expect("running");
    assert_eq!(unsafe { libc::kill(pid as i32, libc::SIGKILL) }, 0);
    until("relaunch", || engine.pid().is_some_and(|p| p != pid)).await;
    until("screen ready again", || {
        engine.surface_status("screen") == Some(SurfaceStatus::Ready)
    })
    .await;
    let state = match engine.query(Query::State).await {
        Some(Reply::State(states)) => states,
        other => panic!("{other:?}"),
    };
    let screen_state = state
        .iter()
        .find(|s| s.spec.output == "screen")
        .expect("replayed");
    assert_eq!(
        screen_state.last_render.as_ref().unwrap().text,
        "For God so loved the world"
    );
    assert!(screen_state.layers.message.is_some());
    assert!(manager.takes_push(&screen));

    if std::env::var_os("KAIRO_HOLD_OUTPUT").is_some() {
        // For a visual check of the replayed window.
        tokio::time::sleep(Duration::from_secs(8)).await;
    }
    manager.destroy_all();
    engine.shutdown();
}
