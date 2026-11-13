//! The surface manager (`surface-manager.ts`).
//!
//! Owns every program surface: one per enabled `ndi` and `syphon` output,
//! one per enabled `device` output, one per enabled `screen` output whose
//! display is connected, and one stage surface per enabled stage display.
//! [`SurfaceManager::reconcile`] is the only place surfaces are opened or
//! closed; it runs on launch, after every overlay or presentation settings
//! write, and on display changes.
//!
//! Every command goes through the engine client, which keeps each
//! surface's intended state and replays it into a relaunched engine, so a
//! surface keeps what it showed across an engine crash, a parked display
//! and a replugged card. The manager remembers what it last sent to each
//! surface (`Sent`), so a reconcile or a program change with nothing new
//! sends nothing.

use crate::displays;
use crate::lobby::LobbyPlayer;
use crate::ports::{LobbyMedia, OutputSettings, RoutingSink};
use kairo_core::Publisher;
use kairo_core::engine::OutputEngine;
use kairo_domain::brand::{NDI_SENDER_NAME, PRODUCT_NAME};
use kairo_domain::displays::{Dimensions, DisplayBinding, frame_height_for, resolve_display};
use kairo_domain::overlay::{DisplayInfo, OverlayOutput, OverlayOutputKind, TestPatternInput};
use kairo_domain::overlay_outputs::{follows_program, is_rendered_kind, primary_ndi_output_id};
use kairo_domain::program::{
    OutputShowFilter, PresentationSettings, ProgramState, StageDisplayStatus, StageView,
    confidence_for, next_time_of_day, program_layers_for, screen_timer_style, transition_ms,
};
use kairo_domain::video_devices::{VideoDeviceSnapshot, VideoOutputState, VideoPort};
use kairo_engine_proto::surface::Layers;
use kairo_engine_proto::{
    self as proto, AudioConfig, CameraLayer, Command, Json, LayerKind, Query, Reply, SinkSpec,
    SurfaceConfig, SurfaceSpec, SurfaceStatus, TestPattern, WebLayer,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

/// Why an output can or cannot take a push right now; the Outputs UI shows `reason`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputState {
    pub ready: bool,
    pub reason: String,
}

impl OutputState {
    fn new(ready: bool, reason: &str) -> Self {
        Self {
            ready,
            reason: reason.into(),
        }
    }
}

/// Pushed whenever stage-display status changes (`program:stageStatusChanged`).
#[derive(Clone, Debug, PartialEq)]
pub struct StageStatusChanged(pub Vec<StageDisplayStatus>);

/// Pushed after display changes settle (`displays:changed`).
#[derive(Clone, Debug, PartialEq)]
pub struct DisplaysChanged(pub Vec<DisplayInfo>);

/// The manager itself rewrote settings (a re-matched display id). The
/// operator must re-read settings, or its next write puts the old id back.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingsRewritten;

/// The video card snapshot with each card's test pattern (`videoDevices:changed`).
#[derive(Clone, Debug, PartialEq)]
pub struct VideoDevicesChanged(pub VideoDeviceSnapshot);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Program(OverlayOutputKind),
    Stage,
}

/// What this manager last told one surface.
#[derive(Default)]
struct Sent {
    config: Option<SurfaceConfig>,
    layers: Layers,
    stage: Option<Json>,
}

struct Entry {
    role: Role,
    spec: SurfaceSpec,
    sent: Sent,
    lobby: Option<LobbyPlayer>,
    /// This NDI surface carries the established "Kairo Scripture" sender.
    primary_ndi: bool,
}

pub struct SurfaceManager {
    engine: Arc<dyn OutputEngine>,
    settings: Arc<dyn OutputSettings>,
    media: Arc<dyn LobbyMedia>,
    routing: Option<Arc<dyn RoutingSink>>,
    topics: Publisher,
    now_ms: fn() -> f64,
    initialized: bool,
    /// The outputs as of the last reconcile: filters and order come from here.
    outputs: Vec<OverlayOutput>,
    /// Presentation settings as of the last reconcile. Every settings write
    /// that touches them reconciles, so program changes (every push) use
    /// this copy instead of re-reading settings.
    presentation_cache: Option<PresentationSettings>,
    program: ProgramState,
    /// In opening order.
    entries: BTreeMap<String, Entry>,
    order: Vec<String>,
    screen_states: HashMap<String, OutputState>,
    stage_states: HashMap<String, StageDisplayStatus>,
    /// Stage status as last announced, so only real changes are pushed.
    sent_stage_status: Option<Vec<StageDisplayStatus>>,
    devices: VideoDeviceSnapshot,
    /// Screens and stages closed with the main window (macOS); the next
    /// reconcile reopens them.
    screens_closed: bool,
}

impl SurfaceManager {
    pub fn new(
        engine: Arc<dyn OutputEngine>,
        settings: Arc<dyn OutputSettings>,
        media: Arc<dyn LobbyMedia>,
        topics: Publisher,
    ) -> Self {
        Self {
            engine,
            settings,
            media,
            routing: None,
            topics,
            now_ms: kairo_domain::js::now_ms,
            initialized: false,
            outputs: Vec::new(),
            presentation_cache: None,
            program: ProgramState::default(),
            entries: BTreeMap::new(),
            order: Vec::new(),
            screen_states: HashMap::new(),
            stage_states: HashMap::new(),
            sent_stage_status: None,
            devices: VideoDeviceSnapshot {
                ports: Vec::new(),
                providers: Vec::new(),
                outputs: BTreeMap::new(),
            },
            screens_closed: false,
        }
    }

    /// Tells the orchestrator's [`kairo_orchestrator::engine::EngineSurfaces`]
    /// which surfaces follow the service after every reconcile.
    pub fn with_routing(mut self, routing: Arc<dyn RoutingSink>) -> Self {
        self.routing = Some(routing);
        self
    }

    /// The clock for timer views; tests pin it.
    pub fn with_clock(mut self, now_ms: fn() -> f64) -> Self {
        self.now_ms = now_ms;
        self
    }

    pub fn init(&mut self) {
        self.initialized = true;
    }

    pub fn initialized(&self) -> bool {
        self.initialized
    }

    // ─── Displays ───────────────────────────────────────────────────────────

    /// `listDisplays`: empty until the manager is initialized.
    pub fn list_displays(&self) -> Vec<DisplayInfo> {
        if !self.initialized {
            return Vec::new();
        }
        displays::list(&self.engine.displays())
    }

    /// `identify`: each display's number and name for 3 s.
    pub fn identify(&self) -> bool {
        self.initialized && self.engine.identify_displays()
    }

    /// `showTestPattern`: the yellow frame an output of this shape covers.
    pub fn show_test_pattern(&self, input: &TestPatternInput) -> bool {
        self.initialized && self.engine.show_display_test_pattern(input)
    }

    /// The display list changed and settled (250 ms): reconcile, then tell
    /// the operator.
    pub async fn displays_changed(&mut self) {
        self.reconcile(None).await;
        self.topics.publish(DisplaysChanged(self.list_displays()));
    }

    // ─── Reconcile ──────────────────────────────────────────────────────────

    /// Makes the surfaces match the stored outputs, stage displays and
    /// connected displays. `None` reads the outputs from settings. Callers
    /// go through the actor, which runs one reconcile at a time.
    pub async fn reconcile(&mut self, outputs: Option<Vec<OverlayOutput>>) {
        if !self.initialized {
            return;
        }
        let outputs = outputs.unwrap_or_else(|| self.settings.outputs());
        self.outputs = outputs;
        self.presentation_cache = None;
        let presentation = self.presentation();
        let displays = displays::list(&self.engine.displays());
        let mut repersist: Vec<(String, u32, u32)> = Vec::new();
        self.screens_closed = false;

        self.reconcile_ndi();
        self.reconcile_syphon();
        // A card's acquisition runs on the engine's own device thread, so a
        // slow card driver never holds up the screens below.
        self.reconcile_devices();
        let used = self.reconcile_screens(&displays, &mut repersist);
        self.reconcile_stages(&presentation, &displays, used);
        self.emit_stage_status();

        if !repersist.is_empty() {
            self.repersist_display_ids(&repersist);
        }

        self.apply_config(&presentation);
        self.apply_program(&presentation);
        self.publish_routing();
    }

    fn presentation(&mut self) -> PresentationSettings {
        if self.presentation_cache.is_none() {
            self.presentation_cache = Some(self.settings.presentation());
        }
        self.presentation_cache.clone().expect("cached")
    }

    /// Opens `spec`, or moves an existing surface to it (the engine keeps
    /// what it showed). True when the surface is new.
    fn open(&mut self, spec: SurfaceSpec, role: Role) -> bool {
        let id = spec.output.clone();
        if let Some(entry) = self.entries.get_mut(&id) {
            if entry.spec != spec {
                self.engine.command(Command::OpenSurface(spec.clone()));
                entry.spec = spec;
            }
            return false;
        }
        self.engine.command(Command::OpenSurface(spec.clone()));
        self.entries.insert(
            id.clone(),
            Entry {
                role,
                spec,
                sent: Sent::default(),
                lobby: None,
                primary_ndi: false,
            },
        );
        self.order.push(id);
        true
    }

    /// Removes a surface and what it showed: turning an output back on starts blank.
    fn destroy(&mut self, id: &str) {
        if let Some(entry) = self.entries.remove(id) {
            if let Some(lobby) = &entry.lobby {
                lobby.stop();
            }
            self.order.retain(|o| o != id);
            self.engine.command(Command::CloseSurface {
                output: id.into(),
                destroy: true,
            });
        }
    }

    /// `close()`: a display or card surface parks and keeps what it showed.
    fn park(&mut self, id: &str) {
        if self.entries.contains_key(id) && self.engine.surface_state(id).is_some_and(|s| !s.parked)
        {
            self.engine.command(Command::CloseSurface {
                output: id.into(),
                destroy: false,
            });
        }
    }

    /// `restore()`: reopens a parked surface with what it last showed.
    fn restore(&mut self, id: &str) {
        if self.engine.surface_state(id).is_some_and(|s| s.parked) {
            self.engine
                .command(Command::RestoreSurface { output: id.into() });
        }
    }

    fn drop_where(&mut self, keep: impl Fn(&str, &Entry) -> bool) {
        let gone: Vec<String> = self
            .order
            .iter()
            .filter(|id| !keep(id, &self.entries[*id]))
            .cloned()
            .collect();
        for id in gone {
            self.destroy(&id);
        }
    }

    /// One surface per enabled `ndi` output. The first keeps the established
    /// sender ("Kairo Scripture") so existing receivers still find it; the
    /// others get their own senders named after the output. A new name is
    /// a new sender, and a new surface.
    fn reconcile_ndi(&mut self) {
        let primary_id = primary_ndi_output_id(&self.outputs).map(str::to_owned);
        let wanted: Vec<OverlayOutput> = self
            .outputs
            .iter()
            .filter(|o| o.kind == OverlayOutputKind::Ndi && o.enabled)
            .cloned()
            .collect();
        let mut keep = Vec::new();
        for (index, output) in wanted.iter().enumerate() {
            let primary = primary_id.as_deref() == Some(output.id.as_str());
            let name = if primary {
                NDI_SENDER_NAME.to_owned()
            } else if output.name.is_empty() {
                format!("{PRODUCT_NAME} NDI {}", index + 1)
            } else {
                format!("{PRODUCT_NAME} {}", output.name)
            };
            let spec = SurfaceSpec {
                output: output.id.clone(),
                label: if primary {
                    "NDI".into()
                } else {
                    format!("NDI ({})", output.name)
                },
                sink: SinkSpec::Ndi { name },
            };
            if let Some(existing) = self.entries.get(&output.id)
                && (existing.spec.sink != spec.sink || existing.primary_ndi != primary)
            {
                self.destroy(&output.id);
            }
            if self.open(spec, Role::Program(OverlayOutputKind::Ndi)) && !primary {
                tracing::info!(output = %output.id, "[Output] Extra NDI sender added");
            }
            if let Some(entry) = self.entries.get_mut(&output.id) {
                entry.primary_ndi = primary;
            }
            keep.push(output.id.clone());
        }
        self.drop_where(|id, entry| {
            entry.role != Role::Program(OverlayOutputKind::Ndi) || keep.iter().any(|k| k == id)
        });
    }

    /// One Syphon server per enabled `syphon` output, named after the
    /// output. Renaming an output restarts its server under the new name.
    fn reconcile_syphon(&mut self) {
        let wanted: Vec<OverlayOutput> = self
            .outputs
            .iter()
            .filter(|o| o.kind == OverlayOutputKind::Syphon && o.enabled)
            .cloned()
            .collect();
        let mut keep = Vec::new();
        for (index, output) in wanted.iter().enumerate() {
            let spec = SurfaceSpec {
                output: output.id.clone(),
                label: format!("Syphon ({})", output.name),
                sink: SinkSpec::syphon(&output.name, index),
            };
            if self
                .entries
                .get(&output.id)
                .is_some_and(|e| e.spec.sink != spec.sink)
            {
                self.destroy(&output.id);
            }
            if self.open(spec, Role::Program(OverlayOutputKind::Syphon)) {
                tracing::info!(output = %output.id, "[Output] Syphon output added");
            }
            keep.push(output.id.clone());
        }
        self.drop_where(|id, entry| {
            entry.role != Role::Program(OverlayOutputKind::Syphon) || keep.iter().any(|k| k == id)
        });
    }

    /// One surface per enabled `device` output. The engine acquires the
    /// port; a lost port parks the surface there and a replug restores it
    /// with what it showed. A card that switches source is rebuilt.
    fn reconcile_devices(&mut self) {
        let wanted: Vec<OverlayOutput> = self
            .outputs
            .iter()
            .filter(|o| o.kind == OverlayOutputKind::Device && o.enabled)
            .cloned()
            .collect();
        let mut keep = Vec::new();
        for output in &wanted {
            let lobby = !follows_program(output.kind, output.source);
            if self
                .entries
                .get(&output.id)
                .is_some_and(|e| e.lobby.is_some() != lobby)
            {
                self.destroy(&output.id);
            }
            let spec = SurfaceSpec {
                output: output.id.clone(),
                label: format!("Video card ({})", output.name),
                sink: SinkSpec::Device {
                    port_id: output.device_port_id.clone(),
                    mode_id: output.device_mode_id.clone(),
                    key_fill: output.device_key_fill,
                },
            };
            let new = self.open(spec, Role::Program(OverlayOutputKind::Device));
            let entry = self.entries.get_mut(&output.id).expect("opened");
            if new && lobby {
                // Starts once the card is ready (`apply_config`).
                entry.lobby = Some(LobbyPlayer::new(
                    self.engine.clone(),
                    self.media.clone(),
                    output.clone(),
                ));
            } else if let Some(player) = &entry.lobby {
                player.update(output.clone());
            }
            keep.push(output.id.clone());
        }
        self.drop_where(|id, entry| {
            entry.role != Role::Program(OverlayOutputKind::Device) || keep.iter().any(|k| k == id)
        });
    }

    /// Returns the ids of displays now showing a screen output.
    fn reconcile_screens(
        &mut self,
        displays: &[DisplayInfo],
        repersist: &mut Vec<(String, u32, u32)>,
    ) -> Vec<f64> {
        let screens: Vec<OverlayOutput> = self
            .outputs
            .iter()
            .filter(|o| o.kind == OverlayOutputKind::Screen)
            .cloned()
            .collect();
        let mut used: Vec<f64> = Vec::new();
        self.screen_states.clear();

        for output in screens.iter().filter(|o| o.enabled) {
            let Some(display_id) = output.display_id else {
                self.screen_states.insert(
                    output.id.clone(),
                    OutputState::new(false, "No display chosen"),
                );
                self.park(&output.id);
                continue;
            };
            let binding = DisplayBinding {
                display_id: Some(display_id),
                display_label: output.display_label.clone(),
                display_size: output.display_size.clone(),
            };
            let Some(display) = resolve_display(&binding, displays, &used).cloned() else {
                let connected = resolve_display(&binding, displays, &[]).is_some();
                self.screen_states.insert(
                    output.id.clone(),
                    OutputState::new(
                        false,
                        if connected {
                            "Display in use by another screen"
                        } else {
                            "Display not connected"
                        },
                    ),
                );
                self.park(&output.id);
                continue;
            };
            if display.id != display_id {
                repersist.push((output.id.clone(), display_id as u32, display.id as u32));
            }
            used.push(display.id);

            let frame_height = frame_height_for(
                Dimensions {
                    width: display.size.width,
                    height: display.size.height,
                },
                output.aspect,
                Some(Dimensions {
                    width: output.custom_aspect.width,
                    height: output.custom_aspect.height,
                }),
            );
            let spec = SurfaceSpec {
                output: output.id.clone(),
                label: if output.name.is_empty() {
                    "Screen".into()
                } else {
                    output.name.clone()
                },
                sink: SinkSpec::Display(proto::DisplaySink {
                    binding: proto_binding(Some(display.id), &binding),
                    rehearsal: display.hosts_main_window,
                    frame_height: frame_height as u32,
                }),
            };
            if self.open(spec, Role::Program(OverlayOutputKind::Screen)) {
                let label = &display.label;
                tracing::info!(output = %output.id, display = %label, "[Output] Screen output opened");
            }
            self.file_screen(output);
            self.screen_states.insert(
                output.id.clone(),
                screen_ready_state(output, display.hosts_main_window),
            );
            self.restore(&output.id);
        }

        // Outputs that were removed or turned off lose their surface, and
        // with it the slide they last showed, so turning one back on starts blank.
        let live: Vec<String> = screens
            .iter()
            .filter(|o| o.enabled)
            .map(|o| o.id.clone())
            .collect();
        self.drop_where(|id, entry| {
            entry.role != Role::Program(OverlayOutputKind::Screen) || live.iter().any(|l| l == id)
        });
        used
    }

    /// Files a screen's surface by its source. Switching source keeps the
    /// same surface: into a lobby it gains a playlist player; back to the
    /// service it drops the player and the last playlist item.
    fn file_screen(&mut self, output: &OverlayOutput) {
        let engine = self.engine.clone();
        let media = self.media.clone();
        let entry = self.entries.get_mut(&output.id).expect("opened");
        if follows_program(output.kind, output.source) {
            if let Some(player) = entry.lobby.take() {
                player.stop();
                engine.command(Command::Clear {
                    output: output.id.clone(),
                });
            }
            return;
        }
        match &entry.lobby {
            Some(player) => player.update(output.clone()),
            None => {
                let player = LobbyPlayer::new(engine, media, output.clone());
                player.start();
                entry.lobby = Some(player);
                tracing::info!(output = %output.id, "[Output] Screen output is running its own playlist");
            }
        }
    }

    fn reconcile_stages(
        &mut self,
        presentation: &PresentationSettings,
        displays: &[DisplayInfo],
        mut used: Vec<f64>,
    ) {
        self.stage_states.clear();
        let mut keep = Vec::new();
        for config in presentation.stage_displays.iter().filter(|c| c.enabled) {
            let status = |ready: bool, reason: &str| StageDisplayStatus {
                id: config.id.clone(),
                ready,
                reason: reason.into(),
            };
            keep.push(config.id.clone());
            let binding = DisplayBinding {
                display_id: config.display_id,
                display_label: config.display_label.clone(),
                display_size: config.display_size.clone(),
            };
            let display = config
                .display_id
                .and_then(|_| resolve_display(&binding, displays, &[]).cloned());
            let Some(display) = display.filter(|d| !used.contains(&d.id)) else {
                let reason = if config.display_id.is_none() {
                    "No display chosen"
                } else if resolve_display(&binding, displays, &[]).is_some() {
                    "Display in use by a program screen"
                } else {
                    "Display not connected"
                };
                self.stage_states
                    .insert(config.id.clone(), status(false, reason));
                if self
                    .entries
                    .get(&config.id)
                    .is_some_and(|e| e.role == Role::Stage)
                {
                    self.park(&config.id);
                }
                continue;
            };
            used.push(display.id);
            let spec = SurfaceSpec {
                output: config.id.clone(),
                label: config.name.clone(),
                sink: SinkSpec::Display(proto::DisplaySink {
                    binding: proto_binding(Some(display.id), &binding),
                    rehearsal: display.hosts_main_window,
                    frame_height: 1080,
                }),
            };
            self.stage_states.insert(
                config.id.clone(),
                status(
                    true,
                    if display.hosts_main_window {
                        "On the control display — showing a rehearsal window"
                    } else {
                        "Live"
                    },
                ),
            );
            if self.open(spec, Role::Stage) {
                // The engine places a surface as a stage only once it has a
                // stage view; send it with the open.
                let view = self.stage_view(presentation, &config.id);
                if let Some(view) = view {
                    self.send_stage(&config.id, view);
                }
            }
            self.restore(&config.id);
        }
        self.drop_where(|id, entry| entry.role != Role::Stage || keep.iter().any(|k| k == id));
    }

    /// The engine rebinds a replugged display by label and size. Remember
    /// the new id so the next lookup is direct, and tell the operator.
    fn repersist_display_ids(&mut self, ids: &[(String, u32, u32)]) {
        let mut written = false;
        for (output, old_id, new_id) in ids {
            written |= self
                .settings
                .persist_display_rebinding(output, *old_id, *new_id, false);
        }
        if written {
            tracing::info!(
                ?ids,
                "[Output] Screen output re-matched to a display with a new id"
            );
            // The stored outputs now carry the new ids.
            for (output, _, new_id) in ids {
                if let Some(o) = self.outputs.iter_mut().find(|o| &o.id == output) {
                    o.display_id = Some(*new_id as f64);
                }
            }
            self.topics.emit(SettingsRewritten);
        }
    }

    /// `Event::DisplayRebound`: the engine re-matched a display between
    /// reconciles. Persisted only while the stored id is still the old one.
    pub fn display_rebound(&mut self, output: &str, old_id: u32, new_id: u32, stage: bool) {
        if self
            .settings
            .persist_display_rebinding(output, old_id, new_id, stage)
        {
            tracing::info!(
                output,
                old_id,
                new_id,
                "[Output] Display re-matched with a new id"
            );
            if !stage && let Some(o) = self.outputs.iter_mut().find(|o| o.id == output) {
                o.display_id = Some(new_id as f64);
            }
            self.topics.emit(SettingsRewritten);
        }
    }

    // ─── Program: transition, audio, layers, stage ──────────────────────────

    /// The surface that plays video sound: the first live screen, else the
    /// primary NDI surface. Exactly one, so two windows never echo each other.
    fn audio_surface(&self) -> Option<String> {
        let targets = self.live_targets(Some(ShowKey::Backgrounds));
        targets
            .iter()
            .find(|o| o.kind == OverlayOutputKind::Screen)
            .or_else(|| targets.iter().find(|o| self.is_primary_ndi(&o.id)))
            .map(|o| o.id.clone())
    }

    fn apply_config(&mut self, presentation: &PresentationSettings) {
        // Lobby screens are never the audio surface and have no NDI sender,
        // so the same rule leaves them silent: the room's sound is the service's.
        let audio_surface = self.audio_surface();
        let ms = transition_ms(&presentation.transition);
        let audio = &presentation.audio;
        let camera_on = self.program.camera.is_some() && self.program.camera_audio.is_some();
        let video_layout = Some(Json::encode(&presentation.video));
        let outputs = self.outputs.clone();
        for id in self.order.clone() {
            let entry = self.entries.get_mut(&id).expect("ordered");
            let Role::Program(kind) = entry.role else {
                continue;
            };
            let output = outputs.iter().find(|o| o.id == id);
            let follows = output.is_some_and(|o| follows_program(o.kind, o.source));
            let audible = audio_surface.as_deref() == Some(id.as_str());
            let config = SurfaceConfig {
                web_enabled: follows && output.is_some_and(|o| o.show.backgrounds),
                transition_ms: ms,
                audio: AudioConfig {
                    enabled: audio.enabled && audible,
                    volume: audio.volume,
                    output_label: audio.output_label.clone(),
                },
                camera_audible: audible && camera_on,
                ndi_audio: audio.ndi && kind == OverlayOutputKind::Ndi,
                video_layout: video_layout.clone(),
            };
            if entry.sent.config.as_ref() != Some(&config) {
                self.engine.command(Command::Configure {
                    output: id.clone(),
                    config: config.clone(),
                });
                entry.sent.config = Some(config);
            }
        }
        self.start_ready_card_players();
    }

    /// A card's playlist begins once the card is acquired.
    fn start_ready_card_players(&self) {
        for (id, entry) in &self.entries {
            if entry.role == Role::Program(OverlayOutputKind::Device)
                && let Some(player) = &entry.lobby
                && self.device_status(id).ready
            {
                player.start();
            }
        }
    }

    /// Program state changed (message, props, logo, camera, web page,
    /// current and next, timer).
    pub fn set_program(&mut self, state: ProgramState) {
        let camera_sound_changed =
            state.camera != self.program.camera || state.camera_audio != self.program.camera_audio;
        self.program = state;
        let presentation = self.presentation();
        // Which surface plays the camera's sound depends on the camera.
        if camera_sound_changed {
            self.apply_config(&presentation);
        }
        self.apply_program(&presentation);
    }

    pub fn program(&self) -> &ProgramState {
        &self.program
    }

    fn layers_for(&self, output: &OverlayOutput, presentation: &PresentationSettings) -> Layers {
        let state = &self.program;
        let program = program_layers_for(
            &output.show,
            state,
            presentation,
            (self.now_ms)(),
            |target, now| next_time_of_day(target, now, &chrono::Local),
        );
        // A lobby shows its playlist, never the service camera or web page.
        let service_backgrounds =
            output.show.backgrounds && follows_program(output.kind, output.source);
        let encode =
            |layer: Option<kairo_domain::program::ProgramLayer>| layer.as_ref().map(Json::encode);
        Layers {
            poll: encode(program.poll),
            question: encode(program.question),
            message: encode(program.message),
            props: encode(program.props),
            timer: encode(program.timer),
            logo: encode(program.logo),
            camera: service_backgrounds
                .then(|| {
                    state.camera.as_ref().map(|label| CameraLayer {
                        label: label.clone(),
                        audio_label: state.camera_audio.clone(),
                    })
                })
                .flatten(),
            web: service_backgrounds
                .then(|| {
                    state.web.as_ref().map(|web| WebLayer {
                        url: web.url.clone(),
                        reload: web.reload,
                    })
                })
                .flatten(),
            info: confidence_for(output, state, presentation).map(|info| Json::encode(&info)),
        }
    }

    fn stage_view(&self, presentation: &PresentationSettings, id: &str) -> Option<Json> {
        let config = presentation.stage_displays.iter().find(|c| c.id == id)?;
        let state = &self.program;
        Some(Json::encode(&StageView {
            current: state.current.clone(),
            next: state.next.clone(),
            stage_message: state.stage_message.clone(),
            timer: state.timer.clone(),
            show_next: config.show_next,
            show_clock: config.show_clock,
            show_timer: config.show_timer,
            timer_style: screen_timer_style(presentation, state),
        }))
    }

    fn send_stage(&mut self, id: &str, view: Json) {
        let Some(entry) = self.entries.get_mut(id) else {
            return;
        };
        if entry.sent.stage.as_ref() == Some(&view) {
            return;
        }
        self.engine.command(Command::SetStage {
            output: id.into(),
            view: view.clone(),
        });
        entry.sent.stage = Some(view);
    }

    /// Sends each surface only the layers that changed, so an unrelated
    /// update never re-fades a logo.
    fn apply_program(&mut self, presentation: &PresentationSettings) {
        let outputs: Vec<OverlayOutput> = self
            .outputs
            .iter()
            .filter(|o| o.enabled && is_rendered_kind(o.kind))
            .cloned()
            .collect();
        for output in &outputs {
            if !self
                .entries
                .get(&output.id)
                .is_some_and(|e| matches!(e.role, Role::Program(_)))
            {
                continue;
            }
            let next = self.layers_for(output, presentation);
            let entry = self.entries.get_mut(&output.id).expect("checked");
            let previous = std::mem::replace(&mut entry.sent.layers, next.clone());
            let id = &output.id;
            for kind in LayerKind::ALL {
                if next.layer(kind) != previous.layer(kind) {
                    self.engine.command(Command::SetLayer {
                        output: id.clone(),
                        kind,
                        layer: next.layer(kind).cloned(),
                    });
                }
            }
            if next.camera != previous.camera {
                self.engine.command(Command::SetCamera {
                    output: id.clone(),
                    camera: next.camera.clone(),
                });
            }
            if next.web != previous.web {
                self.engine.command(Command::SetWeb {
                    output: id.clone(),
                    web: next.web.clone(),
                });
            }
            if next.info != previous.info {
                self.engine.command(Command::SetInfo {
                    output: id.clone(),
                    info: next.info.clone(),
                });
            }
        }
        let stages: Vec<String> = self
            .order
            .iter()
            .filter(|id| self.entries[*id].role == Role::Stage)
            .cloned()
            .collect();
        for id in stages {
            if let Some(view) = self.stage_view(presentation, &id) {
                self.send_stage(&id, view);
            }
        }
    }

    // ─── Stage status ───────────────────────────────────────────────────────

    /// One entry per configured stage display, in settings order.
    pub fn stage_status(&mut self) -> Vec<StageDisplayStatus> {
        let presentation = self.presentation();
        presentation
            .stage_displays
            .iter()
            .map(|config| {
                if !config.enabled {
                    return StageDisplayStatus {
                        id: config.id.clone(),
                        ready: false,
                        reason: "Off".into(),
                    };
                }
                let status = self
                    .stage_states
                    .get(&config.id)
                    .cloned()
                    .unwrap_or_else(|| StageDisplayStatus {
                        id: config.id.clone(),
                        ready: false,
                        reason: "Display not connected".into(),
                    });
                if status.ready
                    && matches!(
                        self.engine.surface_status(&config.id),
                        Some(SurfaceStatus::Unavailable { .. })
                    )
                {
                    return StageDisplayStatus {
                        id: config.id.clone(),
                        ready: false,
                        reason: "Stage window failed to open".into(),
                    };
                }
                status
            })
            .collect()
    }

    /// Pushes stage status only when it changed.
    pub fn emit_stage_status(&mut self) {
        let status = self.stage_status();
        if self.sent_stage_status.as_ref() == Some(&status) {
            return;
        }
        self.sent_stage_status = Some(status.clone());
        self.topics.publish(StageStatusChanged(status));
    }

    // ─── Web ────────────────────────────────────────────────────────────────

    /// The live web page follows the preview's scrolling on every surface showing it.
    pub fn scroll_web(&self, x: f64, y: f64) {
        for output in self.rendered_with_surface() {
            if self
                .engine
                .surface_state(&output.id)
                .is_some_and(|s| s.layers.web.is_some())
            {
                self.engine.command(Command::WebScroll {
                    output: output.id.clone(),
                    x,
                    y,
                });
            }
        }
    }

    /// The operator's pointer over the live page on every service surface
    /// showing it; `None` hides it.
    pub fn web_pointer(&self, point: Option<(f64, f64)>) {
        for output in self.rendered_with_surface() {
            if !follows_program(output.kind, output.source) {
                continue;
            }
            if self
                .engine
                .surface_state(&output.id)
                .is_some_and(|s| s.layers.web.is_some())
            {
                self.engine.command(Command::WebPointer {
                    output: output.id.clone(),
                    point,
                });
            }
        }
    }

    fn rendered_with_surface(&self) -> impl Iterator<Item = &OverlayOutput> {
        self.outputs.iter().filter(|o| {
            o.enabled
                && is_rendered_kind(o.kind)
                && self
                    .entries
                    .get(&o.id)
                    .is_some_and(|e| matches!(e.role, Role::Program(_)))
        })
    }

    // ─── Lookup ─────────────────────────────────────────────────────────────

    /// Whether a service push to this output can land now. Never a lobby.
    pub fn takes_push(&self, output: &OverlayOutput) -> bool {
        if !output.enabled || !follows_program(output.kind, output.source) {
            return false;
        }
        if !self
            .entries
            .get(&output.id)
            .is_some_and(|e| matches!(e.role, Role::Program(_)))
        {
            return false;
        }
        match output.kind {
            OverlayOutputKind::Device => {
                self.device_status(&output.id).ready && self.engine.is_available(&output.id)
            }
            OverlayOutputKind::Screen => {
                self.screen_states.get(&output.id).is_some_and(|s| s.ready)
                    && self.engine.is_available(&output.id)
            }
            _ => self.engine.is_available(&output.id),
        }
    }

    /// `liveTargets`: rendered outputs (in list order) that can take a push
    /// now; with `show`, only those whose content filter allows it.
    pub fn live_targets(&self, show: Option<ShowKey>) -> Vec<OverlayOutput> {
        self.outputs
            .iter()
            .filter(|o| is_rendered_kind(o.kind) && o.enabled)
            .filter(|o| show.is_none_or(|key| key.allows(&o.show)))
            .filter(|o| self.takes_push(o))
            .cloned()
            .collect()
    }

    /// Whether `output_id` owns the primary NDI sender.
    pub fn is_primary_ndi(&self, output_id: &str) -> bool {
        self.entries.get(output_id).is_some_and(|e| e.primary_ndi)
    }

    /// `screenState`.
    pub fn screen_state(&self, output_id: &str) -> OutputState {
        let Some(state) = self.screen_states.get(output_id) else {
            return OutputState::new(false, "Display not connected");
        };
        if state.ready
            && matches!(
                self.engine.surface_status(output_id),
                Some(SurfaceStatus::Unavailable { .. })
            )
        {
            return OutputState::new(false, "Screen window failed to open");
        }
        state.clone()
    }

    fn device_status(&self, id: &str) -> VideoOutputState {
        self.devices
            .outputs
            .get(id)
            .cloned()
            .unwrap_or(VideoOutputState {
                ready: false,
                reason: "Output unavailable".into(),
                test_pattern: None,
                diagnostics: None,
            })
    }

    /// Ready and reason for any rendered output: the one place that knows
    /// where each kind's state lives.
    pub fn output_state(&self, output: &OverlayOutput) -> OutputState {
        match output.kind {
            OverlayOutputKind::Screen => self.screen_state(&output.id),
            OverlayOutputKind::Device => {
                let status = self.device_status(&output.id);
                OutputState {
                    ready: status.ready,
                    reason: status.reason,
                }
            }
            OverlayOutputKind::Syphon => {
                if self.takes_push(output) {
                    return OutputState::new(true, "");
                }
                let reason = match self.engine.surface_status(&output.id) {
                    Some(
                        SurfaceStatus::Unavailable { reason } | SurfaceStatus::Parked { reason },
                    ) => reason,
                    _ => "Syphon unavailable".into(),
                };
                OutputState {
                    ready: false,
                    reason,
                }
            }
            _ => {
                if self.takes_push(output) {
                    OutputState::new(true, "")
                } else {
                    OutputState::new(false, "NDI sender unavailable")
                }
            }
        }
    }

    /// Every surface that follows the service, in Clear order: NDI, Syphon,
    /// program screens, then cards without a playlist.
    pub fn followers(&self) -> Vec<String> {
        let mut followers = Vec::new();
        for kind in [
            OverlayOutputKind::Ndi,
            OverlayOutputKind::Syphon,
            OverlayOutputKind::Screen,
            OverlayOutputKind::Device,
        ] {
            followers.extend(
                self.order
                    .iter()
                    .filter(|id| {
                        let entry = &self.entries[*id];
                        entry.role == Role::Program(kind) && entry.lobby.is_none()
                    })
                    .cloned(),
            );
        }
        followers
    }

    fn publish_routing(&self) {
        if let Some(routing) = &self.routing {
            routing.set_routing(self.outputs.clone(), self.followers());
        }
    }

    // ─── Video cards ────────────────────────────────────────────────────────

    /// The engine's card snapshot changed (`Event::VideoDevices`).
    pub fn video_devices_changed(&mut self, snapshot: VideoDeviceSnapshot) {
        self.devices = snapshot;
        self.start_ready_card_players();
        self.topics
            .publish(VideoDevicesChanged(self.video_devices()));
    }

    /// `videoDevices.status`: the snapshot with each card's test pattern.
    pub fn video_devices(&self) -> VideoDeviceSnapshot {
        let mut snapshot = self.devices.clone();
        for (id, output) in snapshot.outputs.iter_mut() {
            output.test_pattern = Some(self.video_test_pattern(id));
        }
        snapshot
    }

    pub fn video_test_pattern(
        &self,
        id: &str,
    ) -> Option<kairo_domain::video_devices::VideoTestPattern> {
        use kairo_domain::video_devices::VideoTestPattern as P;
        if self.entries.get(id)?.role != Role::Program(OverlayOutputKind::Device) {
            return None;
        }
        self.engine
            .surface_state(id)?
            .test_pattern
            .map(|p| match p {
                TestPattern::Bars => P::Bars,
                TestPattern::Text => P::Text,
                TestPattern::Motion => P::Motion,
            })
    }

    /// `setVideoTestPattern`: only on a ready card.
    pub fn set_video_test_pattern(
        &mut self,
        id: &str,
        pattern: Option<kairo_domain::video_devices::VideoTestPattern>,
    ) -> bool {
        use kairo_domain::video_devices::VideoTestPattern as P;
        if !self.device_status(id).ready
            || self.entries.get(id).map(|e| e.role)
                != Some(Role::Program(OverlayOutputKind::Device))
        {
            return false;
        }
        let pattern = pattern.map(|p| match p {
            P::Bars => TestPattern::Bars,
            P::Text => TestPattern::Text,
            P::Motion => TestPattern::Motion,
        });
        let shown = self
            .engine
            .command(Command::SetTestPattern {
                output: id.into(),
                pattern,
            })
            .accepted;
        self.topics
            .publish(VideoDevicesChanged(self.video_devices()));
        shown
    }

    // ─── Teardown ───────────────────────────────────────────────────────────

    /// Closes the screen and stage windows (the main window closed on
    /// macOS) but keeps what they showed. Reopening reconciles.
    pub fn close_screens(&mut self) {
        let ids: Vec<String> = self
            .order
            .iter()
            .filter(|id| {
                matches!(
                    self.entries[*id].role,
                    Role::Program(OverlayOutputKind::Screen) | Role::Stage
                )
            })
            .cloned()
            .collect();
        for id in ids {
            self.park(&id);
        }
        self.screens_closed = true;
    }

    pub fn screens_closed(&self) -> bool {
        self.screens_closed
    }

    /// Quit: every surface goes, players stop.
    pub fn destroy_all(&mut self) {
        self.initialized = false;
        for id in self.order.clone() {
            self.destroy(&id);
        }
        self.screen_states.clear();
        self.stage_states.clear();
    }
}

/// Content filter keys `liveTargets(show)` accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShowKey {
    Scripture,
    Lyrics,
    Documents,
    Backgrounds,
}

impl ShowKey {
    fn allows(self, show: &OutputShowFilter) -> bool {
        match self {
            Self::Scripture => show.scripture,
            Self::Lyrics => show.lyrics,
            Self::Documents => show.documents,
            Self::Backgrounds => show.backgrounds,
        }
    }
}

/// Status for a screen whose display is connected.
fn screen_ready_state(output: &OverlayOutput, on_control_display: bool) -> OutputState {
    if on_control_display {
        return OutputState::new(true, "On the control display — showing a rehearsal window");
    }
    if !follows_program(output.kind, output.source) {
        return OutputState::new(
            true,
            if output.playlist_id.is_empty() {
                "Choose a playlist to play"
            } else {
                "Playing its own playlist — service pushes do not land here"
            },
        );
    }
    OutputState::new(true, "Live")
}

fn proto_binding(resolved: Option<f64>, binding: &DisplayBinding) -> proto::DisplayBinding {
    proto::DisplayBinding {
        display_id: resolved.or(binding.display_id).map(|id| id as u32),
        label: binding.display_label.clone(),
        size: binding
            .display_size
            .as_ref()
            .map(|s| (s.width as u32, s.height as u32)),
    }
}

/// `videoDevices.listPorts`: refreshes the card list, then answers once the
/// engine reports it (or after the query timeout).
pub async fn list_ports(engine: &dyn OutputEngine) -> Vec<VideoPort> {
    let mut events = engine.subscribe();
    if engine.command(Command::RefreshVideoDevices).accepted {
        let wait = async {
            loop {
                match events.recv().await {
                    Ok(kairo_core::engine::EngineEvent::Engine(proto::Event::VideoDevices(_))) => {
                        break;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    _ => {}
                }
            }
        };
        let _ = tokio::time::timeout(kairo_core::engine::QUERY_TIMEOUT, wait).await;
    }
    match engine.query(Query::VideoDevices).await {
        Some(Reply::VideoDevices(json)) => json
            .decode::<VideoDeviceSnapshot>()
            .map(|s| s.ports)
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}
