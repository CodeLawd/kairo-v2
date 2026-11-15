//! Screens backend: serialized field edits over current settings; topics and
//! engine events drive snapshots while the Screens window is open.
use crate::services::{ApplicationServices, SettingsChanged};
use futures_channel::mpsc::unbounded;
use kairo_core::engine::EngineEvent;
use kairo_domain::settings::AppSettings;
use kairo_output::actor::*;
use std::sync::Arc;
use ui_screens::{
    Command, Reply, ScreensBackend, Snapshot,
    model::{self, Edit},
};

type WriteRequest = (
    Edit,
    tokio::sync::oneshot::Sender<Result<AppSettings, String>>,
);

pub struct ServicesScreens {
    services: Arc<ApplicationServices>,
    ui: Arc<dyn ui_operator::UiState>,
    writer: tokio::sync::mpsc::UnboundedSender<WriteRequest>,
}
impl ServicesScreens {
    pub fn new(services: Arc<ApplicationServices>, ui: Arc<dyn ui_operator::UiState>) -> Self {
        let (writer, mut requests) = tokio::sync::mpsc::unbounded_channel::<WriteRequest>();
        let worker = services.clone();
        services.spawn(async move {
            while let Some((edit, answer)) = requests.recv().await {
                let result = persist(&worker, edit).await;
                if let Err(error) = &result {
                    tracing::error!(%error,"[Screens] could not save changes");
                }
                let _ = answer.send(result);
            }
        });
        Self {
            services,
            ui,
            writer,
        }
    }
    fn work<T: Send + 'static>(
        &self,
        future: impl std::future::Future<Output = Result<T, String>> + Send + 'static,
    ) -> Reply<Result<T, String>> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.spawn(async move {
            let _ = tx.send(future.await);
        });
        Box::pin(async move {
            rx.await
                .unwrap_or_else(|_| Err("The Screens request was interrupted.".into()))
        })
    }
}
/// Each request reads the latest state and writes only its affected section.
/// The mailbox preserves order even after the owning window closes.
async fn persist(services: &Arc<ApplicationServices>, edit: Edit) -> Result<AppSettings, String> {
    let s = services.clone();
    services
        .core
        .blocking(move || {
            let mut wiring = s.settings_wiring.lock().unwrap_or_else(|e| e.into_inner());
            let mut settings: AppSettings =
                serde_json::from_value(wiring.get_all()).map_err(|e| e.to_string())?;
            let key = model::apply(&mut settings, &edit)?;
            let raw = serde_json::to_value(&settings).map_err(|e| e.to_string())?;
            wiring
                .set(key, raw[key].clone())
                .map_err(|e| e.to_string())?;
            serde_json::from_value(wiring.get_all()).map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())?
}
async fn snapshot(services: &Arc<ApplicationServices>) -> Result<Snapshot, String> {
    let services_read = services.clone();
    let settings = services
        .core
        .blocking(move || {
            serde_json::from_value::<AppSettings>(
                services_read
                    .settings_wiring
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get_all(),
            )
            .map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())??;
    let mut result = Snapshot::new(settings);
    result.displays = services
        .core
        .call(ListDisplays {})
        .await
        .map_err(|e| e.to_string())?;
    result.devices = services
        .core
        .call(VideoDevicesStatus {})
        .await
        .map_err(|e| e.to_string())?;
    result.stages = services
        .core
        .call(StageStatus {})
        .await
        .map_err(|e| e.to_string())?;
    for output in &result.settings.overlay.outputs {
        let state = services
            .core
            .call(GetOutputState {
                output: output.clone(),
            })
            .await
            .map_err(|e| e.to_string())?;
        result
            .outputs
            .insert(output.id.clone(), (state.ready, state.reason));
    }
    result.playlists = services
        .media
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .library()
        .playlists;
    result.ndi_audio = kairo_output::manager::ndi_audio_supported(&services.engine).await;
    Ok(result)
}
impl ScreensBackend for ServicesScreens {
    fn open_preview(&self) -> Option<ui_screens::backend::ThemePreview> {
        use kairo_core::engine::OutputEngine;
        use kairo_engine_proto::{
            Command as EngineCommand, Json, SinkSpec, SlideContent, SurfaceConfig, SurfaceSpec,
        };
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let output = format!(
            "screens-sample-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let engine: Arc<dyn OutputEngine> = Arc::new(self.services.engine.clone());
        let hub = match kairo_core::preview::PreviewHub::new(engine.clone()) {
            Ok(hub) => hub,
            Err(error) => {
                tracing::error!(%error, "[Screens] preview transport unavailable");
                return None;
            }
        };
        if !engine
            .command(EngineCommand::OpenSurface(SurfaceSpec {
                output: output.clone(),
                label: "Screens theme sample".into(),
                sink: SinkSpec::Preview,
            }))
            .accepted
        {
            return None;
        }
        let lifetime = SampleLifetime {
            engine: engine.clone(),
            output: output.clone(),
        };
        let config = SurfaceConfig {
            web_enabled: false,
            ..SurfaceConfig::default()
        };
        engine.command(EngineCommand::Configure {
            output: output.clone(),
            config,
        });
        let target = output.clone();
        Some(ui_screens::backend::ThemePreview::new(
            hub,
            output,
            move |theme| {
                engine.command(EngineCommand::ShowSlide {
                output: target.clone(), source: target.clone(), instant: true,
                slide: SlideContent {
                    reference: "John 3:16 (KJV)".into(),
                    text: "For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life.".into(),
                    theme: Json::encode(&isolated_sample_theme(&theme, &target)), colored_lines: None,
                },
            });
            },
            lifetime,
        ))
    }
    fn load(&self) -> Reply<Result<Snapshot, String>> {
        let services = self.services.clone();
        self.work(async move { snapshot(&services).await })
    }
    fn edit(&self, edit: Edit) -> Reply<Result<AppSettings, String>> {
        let (answer, result) = tokio::sync::oneshot::channel();
        let sent = self.writer.send((edit, answer)).is_ok();
        Box::pin(async move {
            if !sent {
                return Err("The Screens writer has stopped.".into());
            }
            result
                .await
                .unwrap_or_else(|_| Err("The Screens write was interrupted.".into()))
        })
    }
    fn run(&self, command: Command) -> Reply<Result<String, String>> {
        let services = self.services.clone();
        self.work(async move {
            let (ok, success, failure) = match command {
                Command::Identify => (
                    services
                        .core
                        .call(IdentifyDisplays {})
                        .await
                        .map_err(|e| e.to_string())?,
                    "Identifying screens",
                    "Could not identify displays. Try again.",
                ),
                Command::Frame(input) => (
                    services
                        .core
                        .call(ShowTestPattern { input })
                        .await
                        .map_err(|e| e.to_string())?,
                    "Frame shown",
                    "Could not show the frame. Check the display connection.",
                ),
                Command::TestVerse => (
                    services.orchestrator.test_overlay().await,
                    "Test verse sent",
                    "Nothing took the test verse",
                ),
                Command::Clear => (
                    services.orchestrator.clear_overlay().await,
                    "Cleared",
                    "Clear failed",
                ),
                Command::CardPattern { id, pattern } => (
                    services
                        .core
                        .call(SetVideoTestPattern {
                            output: id,
                            pattern,
                        })
                        .await
                        .map_err(|e| e.to_string())?,
                    "Test pattern changed",
                    "The output is unavailable. Check its connection and try again.",
                ),
                Command::RefreshCards => {
                    let _ = kairo_output::manager::list_ports(&services.engine).await;
                    let snapshot = services
                        .engine
                        .query(kairo_engine_proto::Query::VideoDevices)
                        .await;
                    let Some(kairo_engine_proto::Reply::VideoDevices(raw)) = snapshot else {
                        return Err("Could not read video device status".into());
                    };
                    let snapshot: kairo_domain::video_devices::VideoDeviceSnapshot =
                        raw.decode().map_err(|e| e.to_string())?;
                    if let Some(failure) = snapshot
                        .providers
                        .iter()
                        .find(|p| !p.status.available && p.status.reason.is_some())
                    {
                        return Err(failure
                            .status
                            .driver
                            .as_ref()
                            .map(|d| d.message.clone())
                            .or_else(|| failure.status.reason.clone())
                            .unwrap_or_default());
                    }
                    return Ok(if snapshot.ports.is_empty() {
                        "No video outputs found. Check Desktop Video Setup, then refresh again."
                            .into()
                    } else {
                        format!(
                            "Found {} video output{}.",
                            snapshot.ports.len(),
                            if snapshot.ports.len() == 1 { "" } else { "s" }
                        )
                    });
                }
            };
            if ok {
                Ok(success.into())
            } else {
                Err(failure.into())
            }
        })
    }
    fn events(&self) -> ui_screens::backend::Events {
        let (tx, rx) = unbounded();
        let services = self.services.clone();
        let mut settings = services.core.watch::<SettingsChanged>();
        let mut displays = services
            .core
            .watch::<kairo_output::manager::DisplaysChanged>();
        let mut devices = services
            .core
            .watch::<kairo_output::manager::VideoDevicesChanged>();
        let mut stages = services
            .core
            .watch::<kairo_output::manager::StageStatusChanged>();
        let mut engine = services.engine.subscribe();
        let media = services
            .media
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .subscribe();
        let task = self.services.runtime().spawn(async move {
            loop {
                tokio::select! {

                    value=settings.next()=>if value.is_none(){break;},
                    value=displays.next()=>if value.is_none(){break;},
                    value=devices.next()=>if value.is_none(){break;},
                    value=stages.next()=>if value.is_none(){break;},
                    value=media.next()=>if value.is_none(){break;},
                    value=engine.recv()=>match value {
                        Ok(EngineEvent::Engine(kairo_engine_proto::Event::SurfaceStatus{..}))=>{},
                        Err(tokio::sync::broadcast::error::RecvError::Closed)=>break,
                        _=>continue,
                    },
                }
                if tx.unbounded_send(snapshot(&services).await).is_err() {
                    break;
                }
            }
        });
        ui_screens::backend::Events {
            receiver: rx,
            lifetime: Box::new(AbortOnDrop(task.abort_handle())),
        }
    }
    fn read_ui(&self, key: &str) -> Option<String> {
        self.ui.get(key).and_then(|v| v.as_str().map(str::to_owned))
    }
    fn write_ui(&self, key: &str, value: String) {
        self.ui.set(key, serde_json::Value::String(value));
    }
}

struct AbortOnDrop(tokio::task::AbortHandle);
impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

struct SampleLifetime {
    engine: Arc<dyn kairo_core::engine::OutputEngine>,
    output: String,
}
impl Drop for SampleLifetime {
    fn drop(&mut self) {
        self.engine
            .command(kairo_engine_proto::Command::CloseSurface {
                output: self.output.clone(),
                destroy: true,
            });
    }
}

/// Private preview media must not join a live output's decoder: changing
/// a sample's loop/transport would otherwise affect the shared source.
fn isolated_sample_theme(
    theme: &kairo_domain::overlay::OverlayTheme,
    namespace: &str,
) -> kairo_domain::overlay::OverlayTheme {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    namespace.hash(&mut hash);
    let mut sample = theme.clone();
    // mediaCue's integer domain is JavaScript's exact 53-bit integer range.
    sample.background.media_cue = Some((hash.finish() & ((1u64 << 53) - 1)) as f64);
    for element in &mut sample.elements {
        element.id = format!("{namespace}:{}", element.id);
    }
    sample
}

#[cfg(test)]
mod tests {
    use super::isolated_sample_theme;
    #[test]
    fn private_sample_media_does_not_share_live_source_ids() {
        let source =
            kairo_domain::overlay_defaults::normalize_overlay_theme(Some(&serde_json::json!({
                "background":{"type":"video","mediaPath":"/a.mp4","mediaCue":7},
                "elements":[{"id":"video","kind":"video","mediaPath":"/a.mp4"}]
            })));
        let a = isolated_sample_theme(&source, "sample-a");
        let b = isolated_sample_theme(&source, "sample-b");
        assert_ne!(a.background.media_cue, b.background.media_cue);
        assert_eq!(a.background.media_path, source.background.media_path);
        assert_eq!(source.background.media_cue, Some(7.));
        assert!(!source.elements.is_empty());
        assert_ne!(a.elements[0].id, b.elements[0].id);
        assert_eq!(a.elements[0].media_loop, source.elements[0].media_loop);
    }
}
