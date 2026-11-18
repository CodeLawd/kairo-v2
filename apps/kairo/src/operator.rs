//! The Operator workspace's backend over the application's services
//! (`window.api.program`, `media`, `tracks`, `output` in Electron).

use std::sync::{Arc, Mutex};

use kairo_core::engine::OutputEngine;
use kairo_core::preview::PreviewHub;
use kairo_domain::overlay_defaults::normalize_overlay_settings;
use kairo_engine_proto::{Query, Reply};
use kairo_store::json_store::JsonStore;
use serde_json::Value;
use ui_operator::backend::{Done, LiveSnapshot, OperatorBackend, OperatorCommand, TimeReply};
use ui_operator::layers::Layer;
use ui_operator::layout::UiState;

use crate::services::ApplicationServices;

/// `ui-state.json`, shared by every view preference.
pub struct UiStateStore(Mutex<JsonStore>);

impl UiStateStore {
    pub fn open(user_data: &std::path::Path) -> Self {
        Self(Mutex::new(kairo_store::settings::open_ui_state(user_data)))
    }
}

impl UiState for UiStateStore {
    fn get(&self, key: &str) -> Option<Value> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(key)
            .cloned()
    }
    fn set(&self, key: &str, value: Value) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .set(key, value);
    }
}

pub struct ServicesBackend {
    services: Arc<ApplicationServices>,
    hub: Option<PreviewHub>,
    /// The stored `overlay` section and its outputs, normalized once per change.
    outputs: Mutex<Option<(Value, Vec<kairo_domain::overlay::OverlayOutput>)>>,
}

impl ServicesBackend {
    pub fn new(services: Arc<ApplicationServices>) -> Self {
        let engine: Arc<dyn OutputEngine> = Arc::new(services.engine.clone());
        let hub = match PreviewHub::new(engine) {
            Ok(hub) => Some(hub),
            Err(error) => {
                tracing::error!(%error, "[Operator] live preview could not start");
                None
            }
        };
        Self {
            services,
            hub,
            outputs: Mutex::new(None),
        }
    }
}

/// Takes one layer down (`LiveLayerStrip` `clear`).
async fn clear(services: &ApplicationServices, layer: Layer) -> Result<(), String> {
    match layer {
        Layer::Audio => {
            services
                .tracks
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .stop();
        }
        Layer::Messages => {
            services.program.clear_message();
        }
        Layer::Props => {
            let logo = services.program.state().logo;
            services.program.clear_props();
            if logo {
                services.program.set_logo(false);
            }
        }
        // `clearLiveText`: verse or lyric text off; the background stays.
        Layer::Slide => {
            services.orchestrator.clear_text().await;
        }
        Layer::Background => {
            services.actions().clear_media().await;
        }
        Layer::Camera => {
            services.program.set_camera(None, None);
        }
    }
    Ok(())
}

impl OperatorBackend for ServicesBackend {
    fn snapshot(&self) -> LiveSnapshot {
        let services = &self.services;
        let outputs = {
            let settings = services.settings.lock().unwrap_or_else(|e| e.into_inner());
            let raw = settings
                .data()
                .get("overlay")
                .cloned()
                .unwrap_or(Value::Null);
            let mut cache = self.outputs.lock().unwrap_or_else(|e| e.into_inner());
            match cache.as_ref() {
                Some((seen, outputs)) if *seen == raw => outputs.clone(),
                _ => {
                    let outputs = normalize_overlay_settings(Some(&raw)).outputs;
                    *cache = Some((raw, outputs.clone()));
                    outputs
                }
            }
        };
        let media = services
            .media
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .library();
        let track_live = services
            .tracks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .library()
            .live_id
            .is_some();
        let engine = &services.engine;
        let document_live = engine
            .surfaces()
            .iter()
            .any(|id| engine.surface_state(id).is_some_and(|s| s.document_mode));
        LiveSnapshot {
            outputs,
            program: services.program.state(),
            media,
            track_live,
            document_live,
        }
    }

    fn run(&self, command: OperatorCommand, done: Done) {
        let services = self.services.clone();
        self.services.spawn(async move {
            let result = match command {
                OperatorCommand::Clear(layer) => clear(&services, layer).await,
                OperatorCommand::ClearAll { others } => {
                    // `clearLiveAll`: text and background together, then the rest.
                    services.orchestrator.clear_overlay().await;
                    let mut result = Ok(());
                    for layer in others {
                        if let Err(error) = clear(&services, layer).await {
                            result = Err(error);
                        }
                    }
                    result
                }
                OperatorCommand::SetPaused(paused) => {
                    services.actions().set_media_paused(paused);
                    Ok(())
                }
                OperatorCommand::Seek(seconds) => {
                    services.actions().seek_media(seconds);
                    Ok(())
                }
                OperatorCommand::SetLoop { item_id, looped } => {
                    let mut patch = serde_json::Map::new();
                    patch.insert("loop".into(), Value::Bool(looped));
                    services
                        .actions()
                        .set_media_playback(&item_id, &patch, false)
                        .map(|_| ())
                }
            };
            done(result);
        });
    }

    fn video_time(&self, output: &str, reply: TimeReply) {
        let engine = self.services.engine.clone();
        let output = output.to_owned();
        self.services.spawn(async move {
            let time = match engine.query(Query::VideoTime { output }).await {
                Some(Reply::VideoTime(time)) => time,
                _ => None,
            };
            reply(time);
        });
    }

    fn preview_hub(&self) -> Option<PreviewHub> {
        self.hub.clone()
    }
}

struct TeleprompterBackend(Arc<ApplicationServices>);

impl ui_operator::teleprompter::Backend for TeleprompterBackend {
    fn request(
        &self,
        request: ui_operator::teleprompter::Request,
        done: Box<dyn FnOnce(Result<ui_operator::teleprompter::Reply, String>) + Send>,
    ) {
        let services = self.0.clone();
        self.0
            .spawn(async move { done(services.teleprompter_request(request).await) });
    }
}

pub fn install_teleprompter(
    services: Arc<ApplicationServices>,
    ui_state: Arc<dyn UiState>,
    window: &mut gpui_kit::Window,
    cx: &mut gpui_kit::App,
) -> gpui_kit::Entity<ui_operator::teleprompter::Teleprompter> {
    use gpui_kit::AppContext;
    let backend = Arc::new(TeleprompterBackend(services));
    cx.new(|cx| ui_operator::teleprompter::Teleprompter::new(backend, ui_state, window, cx))
}
