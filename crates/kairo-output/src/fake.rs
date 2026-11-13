//! A fake engine for tests (feature `fake`). It keeps surface state with
//! the same `SurfaceState::apply` the real client uses, records every
//! command, and lets a test set displays, sink status, card snapshots and
//! video ends, crash the engine (state replayed, as the supervisor does)
//! or stop it.

use kairo_core::engine::{EngineEvent, EngineFuture, OutputEngine, Sent};
use kairo_domain::overlay::TestPatternInput;
use kairo_domain::video_devices::VideoDeviceSnapshot;
use kairo_engine_proto::*;
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;
use tokio::sync::broadcast;

#[derive(Default)]
struct State {
    surfaces: Vec<(SurfaceState, Option<SurfaceStatus>)>,
    displays: Vec<DisplayInfo>,
    commands: Vec<Command>,
    seq: u64,
    video_ended: HashMap<String, bool>,
    devices: Option<VideoDeviceSnapshot>,
    stopped: bool,
    ndi_audio: bool,
    replays: usize,
    queries: usize,
}

pub struct FakeEngine {
    state: Mutex<State>,
    events: broadcast::Sender<EngineEvent>,
}

impl Default for FakeEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeEngine {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(State::default()),
            events: broadcast::channel(256).0,
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Every command sent since the last [`FakeEngine::take_commands`].
    pub fn take_commands(&self) -> Vec<Command> {
        std::mem::take(&mut self.lock().commands)
    }

    pub fn commands(&self) -> Vec<Command> {
        self.lock().commands.clone()
    }

    pub fn set_displays(&self, displays: Vec<DisplayInfo>) {
        self.lock().displays = displays.clone();
        let _ = self
            .events
            .send(EngineEvent::Engine(Event::Displays(displays)));
    }

    /// What the engine says about one sink.
    pub fn set_status(&self, output: &str, status: SurfaceStatus) {
        if let Some(s) = self
            .lock()
            .surfaces
            .iter_mut()
            .find(|s| s.0.spec.output == output)
        {
            s.1 = Some(status.clone());
        }
        let _ = self.events.send(EngineEvent::Engine(Event::SurfaceStatus {
            output: output.into(),
            status,
        }));
    }

    /// Whether the engine says its NDI feeds carry sound.
    pub fn set_ndi_audio(&self, supported: bool) {
        self.lock().ndi_audio = supported;
    }

    pub fn set_video_ended(&self, output: &str, ended: bool) {
        self.lock().video_ended.insert(output.into(), ended);
    }

    pub fn set_devices(&self, snapshot: VideoDeviceSnapshot) {
        self.lock().devices = Some(snapshot.clone());
        let _ = self
            .events
            .send(EngineEvent::Engine(Event::VideoDevices(Json::encode(
                &snapshot,
            ))));
    }

    pub fn send_event(&self, event: Event) {
        let _ = self.events.send(EngineEvent::Engine(event));
    }

    /// A crash and relaunch: the supervisor replays every surface's state
    /// into the new engine. Returns the replayed commands.
    pub fn crash(&self) -> Vec<Command> {
        let replay: Vec<Command> = {
            let mut state = self.lock();
            state.replays += 1;
            for surface in &mut state.surfaces {
                surface.1 = None;
            }
            state.surfaces.iter().flat_map(|s| s.0.replay()).collect()
        };
        let _ = self.events.send(EngineEvent::Lost {
            reason: "killed".into(),
            planned: false,
        });
        let _ = self.events.send(EngineEvent::Launched {
            pid: 2,
            relaunch: true,
        });
        replay
    }

    /// The failure budget is spent: nothing takes a push.
    pub fn stop(&self) {
        self.lock().stopped = true;
    }

    pub fn state(&self, output: &str) -> Option<SurfaceState> {
        self.surface_state(output)
    }

    pub fn queries(&self) -> usize {
        self.lock().queries
    }
}

fn ready(status: Option<&SurfaceStatus>) -> bool {
    matches!(status, None | Some(SurfaceStatus::Ready))
}

impl OutputEngine for FakeEngine {
    fn command(&self, command: Command) -> Sent {
        let mut state = self.lock();
        state.seq += 1;
        let seq = state.seq;
        let stopped = state.stopped;
        let accepted = match &command {
            Command::OpenSurface(spec) => {
                match state
                    .surfaces
                    .iter_mut()
                    .find(|s| s.0.spec.output == spec.output)
                {
                    Some(s) => {
                        s.0.apply(&command, true);
                    }
                    None => state.surfaces.push((SurfaceState::new(spec.clone()), None)),
                }
                true
            }
            Command::CloseSurface {
                output,
                destroy: true,
            } => {
                state.surfaces.retain(|s| &s.0.spec.output != output);
                true
            }
            Command::RetainSurfaces { outputs } => {
                state
                    .surfaces
                    .retain(|s| outputs.contains(&s.0.spec.output));
                true
            }
            Command::Shutdown
            | Command::ControlDisplay { .. }
            | Command::IdentifyDisplays
            | Command::RefreshVideoDevices
            | Command::DisplayTestPattern { .. } => true,
            _ => {
                let output = command.output().expect("surface command").to_owned();
                match state
                    .surfaces
                    .iter_mut()
                    .find(|s| s.0.spec.output == output)
                {
                    Some(s) => {
                        let available = !stopped && ready(s.1.as_ref());
                        s.0.apply(&command, available)
                    }
                    None => false,
                }
            }
        };
        state.commands.push(command);
        Sent { accepted, seq }
    }

    fn is_available(&self, output: &str) -> bool {
        let state = self.lock();
        !state.stopped
            && state
                .surfaces
                .iter()
                .find(|s| s.0.spec.output == output)
                .is_some_and(|s| !s.0.parked && ready(s.1.as_ref()))
    }

    fn unavailable_reason(&self, output: &str) -> Option<String> {
        let state = self.lock();
        if state.stopped {
            return Some("Output engine stopped".into());
        }
        let Some(s) = state.surfaces.iter().find(|s| s.0.spec.output == output) else {
            return Some("Display not connected".into());
        };
        if s.0.parked {
            return Some("Display not connected".into());
        }
        match &s.1 {
            Some(SurfaceStatus::Parked { reason } | SurfaceStatus::Unavailable { reason }) => {
                Some(reason.clone())
            }
            _ => None,
        }
    }

    fn surface_state(&self, output: &str) -> Option<SurfaceState> {
        self.lock()
            .surfaces
            .iter()
            .find(|s| s.0.spec.output == output)
            .map(|s| s.0.clone())
    }

    fn surface_status(&self, output: &str) -> Option<SurfaceStatus> {
        self.lock()
            .surfaces
            .iter()
            .find(|s| s.0.spec.output == output)
            .and_then(|s| s.1.clone())
    }

    fn surfaces(&self) -> Vec<String> {
        self.lock()
            .surfaces
            .iter()
            .map(|s| s.0.spec.output.clone())
            .collect()
    }

    fn displays(&self) -> Vec<DisplayInfo> {
        self.lock().displays.clone()
    }

    fn running(&self) -> bool {
        !self.lock().stopped
    }

    fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.events.subscribe()
    }

    fn query(&self, query: Query) -> EngineFuture<'_, Option<Reply>> {
        let mut state = self.lock();
        state.queries += 1;
        let reply = match query {
            Query::VideoEnded { output } => Some(Reply::VideoEnded(
                state.video_ended.get(&output).copied().unwrap_or(false),
            )),
            Query::VideoDevices => state
                .devices
                .as_ref()
                .map(|d| Reply::VideoDevices(Json::encode(d))),
            Query::NdiAudioSupported => Some(Reply::NdiAudioSupported(state.ndi_audio)),
            Query::State => Some(Reply::State(
                state.surfaces.iter().map(|s| s.0.clone()).collect(),
            )),
            _ => None,
        };
        Box::pin(async move { reply })
    }

    fn wait_frame<'a>(
        &'a self,
        _output: &'a str,
        _seq: u64,
        _timeout: Duration,
    ) -> EngineFuture<'a, bool> {
        Box::pin(async { true })
    }

    fn identify_displays(&self) -> bool {
        !self.lock().displays.is_empty() && self.command(Command::IdentifyDisplays).accepted
    }

    fn show_display_test_pattern(&self, input: &TestPatternInput) -> bool {
        self.lock()
            .displays
            .iter()
            .any(|d| d.id as f64 == input.display_id)
    }
}
