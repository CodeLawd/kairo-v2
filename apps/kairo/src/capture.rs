//! Capture and the Live popover over the application's services (prompt
//! 091): `capture.start`, `stop`, `status`, `onStatus`, `chooseFolder`,
//! `reveal`, `audioInputs`, `settings.set('capture')`, the Syphon switches
//! and output readiness (`ndi.getStatus`).

use std::sync::Arc;
use std::time::Duration;

use kairo_domain::capture::{CaptureSettings, CaptureStatus, normalize_capture_settings};
use kairo_domain::overlay::OverlayOutput;
use kairo_domain::overlay_defaults::normalize_overlay_settings;
use serde_json::Value;
use ui_capture::{CaptureFeed, CaptureSnapshot, FeedSink, Listening, OutputReady, Reply};

use crate::handlers::Handlers;
use crate::services::{ApplicationServices, SettingsChanged};

/// How often readiness is read (`useScreensStatus` polls every 3 s).
const READY_POLL: Duration = Duration::from_millis(3_000);

pub struct ServicesCapture {
    services: Arc<ApplicationServices>,
}

impl ServicesCapture {
    pub fn new(services: Arc<ApplicationServices>) -> Self {
        Self { services }
    }
}

struct Tasks(Vec<tokio::task::JoinHandle<()>>);
impl Drop for Tasks {
    fn drop(&mut self) {
        for t in &self.0 {
            t.abort();
        }
    }
}

fn sections(all: &Value) -> (CaptureSettings, Vec<OverlayOutput>) {
    (
        normalize_capture_settings(all.get("capture").unwrap_or(&Value::Null)),
        normalize_overlay_settings(all.get("overlay")).outputs,
    )
}

fn readiness(services: &ApplicationServices, outputs: &[OverlayOutput]) -> Vec<OutputReady> {
    outputs
        .iter()
        .filter(|o| o.enabled)
        .map(|o| {
            let reason = services.engine.unavailable_reason(&o.id);
            OutputReady {
                id: o.id.clone(),
                ready: Some(reason.is_none()),
                reason,
            }
        })
        .collect()
}

impl ServicesCapture {
    fn all(&self) -> Value {
        self.services
            .settings_wiring
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_all()
    }
}

impl ui_capture::CaptureBackend for ServicesCapture {
    fn snapshot(&self) -> CaptureSnapshot {
        let (settings, outputs) = sections(&self.all());
        CaptureSnapshot {
            status: Handlers(self.services.clone()).capture_status(),
            ready: readiness(&self.services, &outputs),
            settings,
            outputs,
        }
    }

    fn listen(&self, sink: FeedSink) -> Listening {
        let services = self.services.clone();
        let runtime = services.runtime();
        let mut tasks = Vec::new();
        tasks.push(runtime.spawn({
            let mut status = services.core.watch::<CaptureStatus>();
            let sink = sink.clone();
            async move {
                while let Some(value) = status.next().await {
                    sink(CaptureFeed::Status((*value).clone()));
                }
            }
        }));
        tasks.push(runtime.spawn({
            let mut values = services.core.watch::<SettingsChanged>();
            let sink = sink.clone();
            async move {
                while let Some(value) = values.next().await {
                    let (settings, outputs) = sections(&value.0);
                    sink(CaptureFeed::Settings(settings, outputs));
                }
            }
        }));
        let weak = Arc::downgrade(&services);
        tasks.push(runtime.spawn(async move {
            let mut last = None;
            loop {
                tokio::time::sleep(READY_POLL).await;
                let Some(services) = weak.upgrade() else {
                    break;
                };
                let all = services
                    .settings_wiring
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get_all();
                let ready = readiness(&services, &sections(&all).1);
                if last.as_ref() != Some(&ready) {
                    last = Some(ready.clone());
                    sink(CaptureFeed::Ready(ready));
                }
            }
        }));
        Box::new(Tasks(tasks))
    }

    fn start(&self, reply: Reply<CaptureStatus>) {
        let services = self.services.clone();
        self.services.spawn(async move {
            reply(Handlers(services).start_capture().await);
        });
    }

    fn stop(&self, reply: Reply<CaptureStatus>) {
        let services = self.services.clone();
        self.services.spawn(async move {
            reply(Handlers(services).stop_capture().await);
        });
    }

    fn save(&self, settings: CaptureSettings) {
        let services = self.services.clone();
        self.services.spawn(async move {
            let value = serde_json::to_value(settings).unwrap_or(Value::Null);
            let core = services.core.clone();
            let saved = core
                .blocking(move || {
                    let mut wiring = services
                        .settings_wiring
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    wiring.set("capture", value).map_err(|e| e.to_string())
                })
                .await;
            if let Err(error) = saved.map_err(|e| e.to_string()).and_then(|r| r) {
                tracing::error!(%error, "[Capture] settings save failed");
            }
        });
    }

    fn choose_folder(&self, reply: Reply<Option<String>>) {
        let services = self.services.clone();
        self.services.spawn(async move {
            reply(
                Handlers(services)
                    .choose_capture_folder()
                    .await
                    .map(|f| f.map(|p| p.to_string_lossy().into_owned())),
            );
        });
    }

    fn reveal(&self) {
        let services = self.services.clone();
        self.services.spawn(async move {
            if let Err(error) = Handlers(services).reveal_capture().await {
                tracing::warn!(%error, "[Capture] reveal failed");
            }
        });
    }

    fn audio_inputs(&self, reply: Reply<Vec<kairo_domain::capture::CaptureAudioInput>>) {
        let services = self.services.clone();
        self.services.spawn(async move {
            reply(Handlers(services).capture_audio_inputs().await);
        });
    }

    fn set_output_enabled(&self, id: String, enabled: bool) {
        let services = self.services.clone();
        self.services.spawn(async move {
            let edit = ui_screens::model::Edit::Output {
                id,
                patch: serde_json::json!({ "enabled": enabled }),
            };
            if let Err(error) = crate::screens::persist(&services, edit).await {
                tracing::error!(%error, "[Capture] Syphon switch save failed");
            }
        });
    }
}

/// The Live popover and the Capture Settings window.
pub fn install(
    services: &Arc<ApplicationServices>,
    shell: &gpui_kit::Entity<ui_shell::Shell>,
    cx: &mut gpui_kit::App,
) {
    use gpui_kit::AppContext;
    let backend: Arc<dyn ui_capture::CaptureBackend> =
        Arc::new(ServicesCapture::new(services.clone()));
    let capture = cx.new(|cx| ui_capture::Capture::new(backend, cx));
    let menu =
        cx.new(|cx| ui_capture::LiveMenu::new(capture.clone(), cfg!(target_os = "macos"), cx));
    let target = shell.downgrade();
    cx.subscribe(&menu, move |_, event: &ui_capture::LiveMenuEvent, cx| {
        let event = *event;
        let _ = target.update(cx, |shell, cx| {
            shell.set_live_open(false, cx);
            match event {
                ui_capture::LiveMenuEvent::OpenSettings => {
                    shell.open_overlay(ui_shell::Overlay::Capture, cx)
                }
                ui_capture::LiveMenuEvent::OpenScreens(at) => shell.open_screens(at, cx),
            }
        });
    })
    .detach();
    // Off the Mac the popover shows the Live button's detail line.
    let detail = menu.downgrade();
    cx.observe(shell, move |shell, cx| {
        let text = shell.read(cx).live_detail();
        let _ = detail.update(cx, |m, cx| m.set_detail(text, cx));
    })
    .detach();
    let close = shell.downgrade();
    shell.update(cx, |shell, cx| {
        shell.set_live_menu(menu.into(), cx);
        shell.set_overlay_builder(
            ui_shell::Overlay::Capture,
            Box::new(move |_, window, cx| {
                let view = cx
                    .new(|cx| ui_capture::CaptureSettingsWindow::new(capture.clone(), window, cx));
                let close = close.clone();
                let handle = window.window_handle();
                cx.subscribe(
                    &view,
                    move |view, _: &ui_capture::CaptureWindowEvent, cx| {
                        let _ = handle.update(cx, |_, window, cx| {
                            view.update(cx, |v, cx| v.restore_focus(window, cx))
                        });
                        let _ = close.update(cx, |s, cx| s.close_overlay(cx));
                    },
                )
                .detach();
                view.into()
            }),
        );
    });
}
