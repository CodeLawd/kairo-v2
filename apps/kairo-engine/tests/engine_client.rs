//! The orchestrator's `EngineClient` against the real engine.

use kairo_core::engine::*;
use kairo_domain::documents::DocumentSlide;
use kairo_domain::overlay::OverlayOutputKind;
use kairo_domain::overlay_defaults::{DEFAULT_OVERLAY_THEME, make_overlay_output};
use kairo_engine_proto::*;
use kairo_orchestrator::clients::{EngineClient, ShowKind};
use kairo_orchestrator::engine::EngineSurfaces;
use std::time::{Duration, Instant};

const ENGINE: &str = env!("CARGO_BIN_EXE_kairo-engine");

fn display_surface(output: &str, binding: DisplayBinding) -> Command {
    Command::OpenSurface(SurfaceSpec {
        output: output.into(),
        label: "Kairo test output".into(),
        sink: SinkSpec::Display(DisplaySink {
            binding,
            rehearsal: true,
            frame_height: 1080,
        }),
    })
}

async fn until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn engine_client_follows_program_surface_rules() {
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
    let surfaces = EngineSurfaces::new(engine.clone());
    let mut screen = make_overlay_output("screen", OverlayOutputKind::Screen);
    screen.enabled = true;
    let mut ndi = make_overlay_output("ndi", OverlayOutputKind::Ndi);
    ndi.enabled = true;
    let mut gone = make_overlay_output("gone", OverlayOutputKind::Screen);
    gone.enabled = true;
    surfaces.set_outputs(vec![screen.clone(), ndi.clone(), gone.clone()]);

    until("displays", || !engine.displays().is_empty()).await;
    let display = engine.displays()[0].clone();
    engine.command(display_surface(
        "screen",
        DisplayBinding {
            display_id: Some(display.id),
            label: display.label.clone(),
            size: Some((display.bounds.width, display.bounds.height)),
        },
    ));
    engine.command(display_surface(
        "gone",
        DisplayBinding {
            display_id: Some(u32::MAX),
            label: "Not attached".into(),
            size: Some((1, 1)),
        },
    ));
    engine.command(Command::OpenSurface(SurfaceSpec {
        output: "ndi".into(),
        label: "NDI".into(),
        sink: SinkSpec::Ndi {
            name: "Kairo".into(),
        },
    }));
    until("statuses", || {
        ["screen", "gone", "ndi"]
            .iter()
            .all(|o| engine.surface_status(o).is_some())
    })
    .await;

    // A display that is not attached parks; NDI is not in this build yet.
    assert_eq!(
        surfaces.unavailable_reason(&gone).as_deref(),
        Some("Display not connected")
    );
    assert_eq!(
        surfaces.unavailable_reason(&ndi).as_deref(),
        Some("NDI output is not available in this engine build")
    );
    assert_eq!(surfaces.unavailable_reason(&screen), None);
    let live: Vec<String> = surfaces
        .live_targets(None)
        .into_iter()
        .map(|o| o.id)
        .collect();
    assert_eq!(live, ["screen"]);
    let mut no_docs = screen.clone();
    no_docs.show.documents = false;
    surfaces.set_outputs(vec![no_docs, ndi.clone(), gone.clone()]);
    assert!(surfaces.live_targets(Some(ShowKind::Documents)).is_empty());
    assert_eq!(surfaces.live_targets(Some(ShowKind::Backgrounds)).len(), 1);
    assert_eq!(surfaces.all_surfaces(), ["screen", "gone", "ndi"]);

    let theme = DEFAULT_OVERLAY_THEME.clone();
    assert!(
        !surfaces
            .show_slide(
                "gone".into(),
                "Ps 1".into(),
                "x".into(),
                theme.clone(),
                None
            )
            .await
    );
    assert!(
        !surfaces
            .set_background("screen".into(), theme.background.clone())
            .await,
        "nothing painted yet"
    );
    assert!(
        surfaces
            .show_slide(
                "screen".into(),
                "John 3:16".into(),
                "For God".into(),
                theme.clone(),
                None
            )
            .await
    );
    assert!(
        surfaces
            .set_background("screen".into(), theme.background.clone())
            .await
    );

    let page = |n: f64| DocumentSlide {
        id: "deck".into(),
        page: n,
        path: format!("/tmp/page-{n}.png"),
        width: 1920.0,
        height: 1080.0,
        videos: vec![],
    };
    assert!(
        !surfaces
            .swap_document_page("screen".into(), "/tmp/p2.png".into(), Some(page(2.0)))
            .await
    );
    let started = Instant::now();
    assert!(
        surfaces
            .show_document("screen".into(), theme.clone(), Some(page(1.0)))
            .await
    );
    assert!(started.elapsed() < Duration::from_secs(1));
    let state = engine.surface_state("screen").unwrap();
    assert!(state.document_mode);
    let shown: serde_json::Value = state.last_render.unwrap().theme.value().unwrap();
    assert_eq!(
        shown["background"]["blurPx"], 0.0,
        "documents drop the blur"
    );
    assert!(
        surfaces
            .swap_document_page("screen".into(), "/tmp/p2.png".into(), Some(page(2.0)))
            .await
    );

    assert!(surfaces.clear_text("screen".into(), None).await);
    surfaces.clear("screen".into()).await;
    assert!(
        !surfaces.clear_text("screen".into(), None).await,
        "nothing painted after clear"
    );

    // The engine applied exactly what the client decided.
    let expected: Vec<SurfaceState> = engine
        .surfaces()
        .iter()
        .filter_map(|o| engine.surface_state(o))
        .collect();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(Reply::State(state)) = engine.query(Query::State).await
            && state == expected
        {
            break;
        }
        assert!(Instant::now() < deadline, "engine state differs");
    }
    assert_eq!(
        engine
            .query(Query::VideoEnded {
                output: "screen".into()
            })
            .await,
        Some(Reply::VideoEnded(true))
    );
    assert_eq!(
        engine
            .query(Query::VideoEnded {
                output: "missing".into()
            })
            .await,
        Some(Reply::UnknownSurface)
    );
    engine.shutdown();
}
