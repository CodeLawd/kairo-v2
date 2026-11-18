//! The surface manager as a core actor.
//!
//! Every call is a message to one [`SurfaceActor`]; the mailbox runs them
//! one at a time, which is what serializes reconciles (`this.reconciling`
//! in Electron). An event pump feeds it the engine's events: display
//! changes after a 250 ms quiet period, card snapshots, re-matched
//! displays and surface status.

use crate::manager::{OutputState, ShowKey, SurfaceManager};
use crate::ports::{LobbyMedia, OutputSettings, RoutingSink};
use kairo_core::engine::{EngineEvent, OutputEngine};
use kairo_core::{AppCore, CoreError, Handle, Message};
use kairo_domain::overlay::{DisplayInfo, OverlayOutput, TestPatternInput};
use kairo_domain::program::{ProgramState, StageDisplayStatus};
use kairo_domain::video_devices::{VideoDeviceSnapshot, VideoTestPattern};
use kairo_engine_proto::Event;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;

/// Quiet period before display changes are acted on. A window drag or a
/// replug arrives as a burst; reconciling each step would rebuild windows
/// mid-gesture.
pub const DISPLAY_CHANGE_DEBOUNCE: Duration = Duration::from_millis(250);

pub struct SurfaceActor(pub SurfaceManager);

macro_rules! message {
    ($(#[$doc:meta])* $name:ident { $($field:ident : $ty:ty),* $(,)? } => $reply:ty, |$m:ident, $msg:ident| $body:expr) => {
        $(#[$doc])*
        #[derive(Debug)]
        pub struct $name { $(pub $field: $ty),* }
        impl Message for $name {
            type Actor = SurfaceActor;
        }
        impl Handle<$name> for SurfaceActor {
            type Reply = $reply;
            #[allow(unused_variables)]
            async fn handle(&mut self, $msg: $name) -> $reply {
                let $m = &mut self.0;
                $body
            }
        }
    };
}

message!(
    /// Reconcile with these outputs, or with the stored ones.
    Reconcile { outputs: Option<Vec<OverlayOutput>> } => (), |m, msg| m.reconcile(msg.outputs).await
);
message!(SetProgram { state: ProgramState } => (), |m, msg| m.set_program(msg.state));
message!(
    /// The main window closed on macOS: screens and stages close, content kept.
    CloseScreens {} => (), |m, msg| m.close_screens()
);
message!(
    /// The main window reopened: reconcile brings the screens back.
    ReopenScreens {} => (), |m, msg| m.reconcile(None).await
);
message!(DestroyAll {} => (), |m, msg| m.destroy_all());
message!(ScrollWeb { x: f64, y: f64 } => (), |m, msg| m.scroll_web(msg.x, msg.y));
message!(WebPointer { point: Option<(f64, f64)> } => (), |m, msg| m.web_pointer(msg.point));
message!(ListDisplays {} => Vec<DisplayInfo>, |m, msg| m.list_displays());
message!(IdentifyDisplays {} => bool, |m, msg| m.identify());
message!(ShowTestPattern { input: TestPatternInput } => bool, |m, msg| m.show_test_pattern(&msg.input));
message!(StageStatus {} => Vec<StageDisplayStatus>, |m, msg| m.stage_status());
message!(GetOutputState { output: OverlayOutput } => OutputState, |m, msg| m.output_state(&msg.output));
message!(LiveTargets { show: Option<ShowKey> } => Vec<OverlayOutput>, |m, msg| m.live_targets(msg.show));
message!(VideoDevicesStatus {} => VideoDeviceSnapshot, |m, msg| m.video_devices());
message!(
    SetVideoTestPattern { output: String, pattern: Option<VideoTestPattern> } => bool,
    |m, msg| m.set_video_test_pattern(&msg.output, msg.pattern)
);
message!(GetProgram {} => ProgramState, |m, msg| m.program().clone());
message!(Followers {} => Vec<String>, |m, msg| m.followers());
message!(DisplaysSettled {} => (), |m, msg| m.displays_changed().await);
message!(LoadTeleprompter { id: String, text: String, font_px: f32, speed: f64, mirror: bool } => (), |m, msg| m.load_teleprompter(msg.id, msg.text, msg.font_px, msg.speed, msg.mirror));
message!(TeleprompterTransport { operation: kairo_engine_proto::StageOperation } => bool, |m, msg| m.teleprompter_transport(msg.operation));
message!(SetTeleprompterLayout { font_px: f32, mirror: bool } => bool, |m, msg| m.teleprompter_layout(msg.font_px, msg.mirror));
message!(GetTeleprompterPosition {} => Option<kairo_engine_proto::StagePosition>, |m, msg| m.teleprompter_position());
message!(GetTeleprompterScript {} => Option<String>, |m, msg| m.teleprompter_script());
message!(GetPrompterTargets {} => (Vec<crate::manager::PrompterTarget>, Option<String>, bool), |m, msg| {
    let (target, live) = m.prompter_live();
    (m.prompter_targets(), target, live)
});
message!(PrompterGoLive { target: String } => bool, |m, msg| m.prompter_go_live(msg.target));
message!(PrompterTakeOff {} => (), |m, msg| m.prompter_take_off());
message!(ResetStageSession {} => (), |m, _msg| m.reset_stage_session());
message!(EngineNotice { event: Event } => (), |m, msg| match msg.event {
    Event::VideoDevices(json) => {
        if let Ok(snapshot) = json.decode() {
            m.video_devices_changed(snapshot);
        }
    }
    Event::DisplayRebound { output, old_id, new_id, stage } => {
        m.display_rebound(&output, old_id, new_id, stage)
    }
    Event::SurfaceStatus { .. } => m.emit_stage_status(),
    Event::StagePosition(position) => m.stage_position(position),
    _ => {}
});

/// Registers the manager as an actor, starts the event pump and runs the
/// first reconcile. `routing` is the orchestrator's `EngineSurfaces`. Await
/// it on the core runtime (`core.run`), where lobby players and the pump run.
pub async fn spawn_surface_manager(
    core: &AppCore,
    engine: Arc<dyn OutputEngine>,
    settings: Arc<dyn OutputSettings>,
    media: Arc<dyn LobbyMedia>,
    routing: Option<Arc<dyn RoutingSink>>,
) -> Result<(), CoreError> {
    init_surface_manager(core, engine, settings, media, routing)?;
    core.call(Reconcile { outputs: None }).await
}

/// Register output services after the operator window exists. Reconciliation
/// is separate so application startup preserves the main-entry ordering.
pub fn init_surface_manager(
    core: &AppCore,
    engine: Arc<dyn OutputEngine>,
    settings: Arc<dyn OutputSettings>,
    media: Arc<dyn LobbyMedia>,
    routing: Option<Arc<dyn RoutingSink>>,
) -> Result<(), CoreError> {
    let mut manager = SurfaceManager::new(engine.clone(), settings, media, core.publisher());
    if let Some(routing) = routing {
        manager = manager.with_routing(routing);
    }
    manager.init();
    let events = engine.subscribe();
    core.register(
        SurfaceActor(manager),
        NonZeroUsize::new(256).expect("non-zero"),
    )?;
    tokio::spawn(pump(core.clone(), events));
    Ok(())
}

/// Engine events into actor messages. Display changes are debounced.
async fn pump(core: AppCore, mut events: tokio::sync::broadcast::Receiver<EngineEvent>) {
    let mut settle: Option<tokio::time::Instant> = None;
    loop {
        let deadline = settle;
        let event = tokio::select! {
            event = events.recv() => event,
            _ = async {
                match deadline {
                    Some(at) => tokio::time::sleep_until(at).await,
                    None => std::future::pending().await,
                }
            } => {
                settle = None;
                if core.call(DisplaysSettled {}).await.is_err() {
                    return;
                }
                continue;
            }
        };
        match event {
            Ok(EngineEvent::Engine(Event::Displays(_))) => {
                settle = Some(tokio::time::Instant::now() + DISPLAY_CHANGE_DEBOUNCE);
            }
            Ok(EngineEvent::Engine(
                event @ (Event::VideoDevices(_)
                | Event::DisplayRebound { .. }
                | Event::SurfaceStatus { .. }
                | Event::StagePosition(_)),
            )) => {
                if core.call(EngineNotice { event }).await.is_err() {
                    return;
                }
            }
            Ok(EngineEvent::Lost { .. }) => {
                if core.call(ResetStageSession {}).await.is_err() {
                    return;
                }
            }
            Ok(_) => {}
            Err(RecvError::Lagged(skipped)) => {
                tracing::warn!(skipped, "[Output] engine events skipped; reconciling");
                settle = Some(tokio::time::Instant::now() + DISPLAY_CHANGE_DEBOUNCE);
            }
            Err(RecvError::Closed) => return,
        }
    }
}
