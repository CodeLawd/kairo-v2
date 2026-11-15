//! The Web workspace's backend over the application's services (prompt
//! 089): `program.setWeb`, `reloadWeb`, `scrollWeb`, `webPointer` and
//! `settings.set('web')`.

use std::sync::Arc;

use kairo_domain::program::ProgramState;
use kairo_domain::web::normalize_web_settings;
use serde_json::Value;
use ui_web::page::{FeedSink, Listening, Reply, WebFeed, WebSnapshot};

use crate::services::{ApplicationServices, SettingsChanged};

pub struct ServicesWeb {
    services: Arc<ApplicationServices>,
}

impl ServicesWeb {
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

fn web_of(all: &Value) -> kairo_domain::web::WebSettings {
    normalize_web_settings(&all["web"])
}

impl ui_web::WebBackend for ServicesWeb {
    fn snapshot(&self) -> WebSnapshot {
        let all = self
            .services
            .settings_wiring
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_all();
        WebSnapshot {
            live: self.services.program.state().web.map(|w| w.url),
            settings: web_of(&all),
        }
    }

    fn listen(&self, sink: FeedSink) -> Listening {
        let services = self.services.clone();
        let mut tasks = Vec::new();
        tasks.push(services.runtime().spawn({
            let mut values = services.core.watch::<SettingsChanged>();
            let sink = sink.clone();
            async move {
                while let Some(value) = values.next().await {
                    let SettingsChanged(all) = &*value;
                    sink(WebFeed::Settings(web_of(all)));
                }
            }
        }));
        tasks.push(services.runtime().spawn({
            let mut program = services.core.watch::<ProgramState>();
            async move {
                let mut last: Option<Option<String>> = None;
                while let Some(state) = program.next().await {
                    let url = state.web.as_ref().map(|w| w.url.clone());
                    if last.as_ref() != Some(&url) {
                        last = Some(url.clone());
                        sink(WebFeed::Live(url));
                    }
                }
            }
        }));
        Box::new(Tasks(tasks))
    }

    fn set_web(&self, url: Option<String>, reply: Reply) {
        let program = self.services.program.clone();
        self.services.spawn(async move {
            reply(program.set_web(url.as_deref()).map(|_| ()));
        });
    }

    fn reload_web(&self) {
        self.services.program.reload_web();
    }

    fn scroll_web(&self, x: f64, y: f64) {
        let core = self.services.core.clone();
        self.services.spawn(async move {
            if let Err(error) = core.call(kairo_output::actor::ScrollWeb { x, y }).await {
                tracing::warn!(%error, "[Web] scroll did not reach screens");
            }
        });
    }

    fn web_pointer(&self, point: Option<(f64, f64)>) {
        let core = self.services.core.clone();
        self.services.spawn(async move {
            if let Err(error) = core.call(kairo_output::actor::WebPointer { point }).await {
                tracing::warn!(%error, "[Web] pointer did not reach screens");
            }
        });
    }

    fn save_settings(&self, settings: kairo_domain::web::WebSettings) {
        let services = self.services.clone();
        self.services.spawn(async move {
            let value = serde_json::to_value(settings).unwrap_or(Value::Null);
            let core = services.core.clone();
            let saved = core
                .blocking(move || {
                    services
                        .settings_wiring
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .set("web", value)
                        .map_err(|e| e.to_string())
                })
                .await;
            if let Err(error) = saved.map_err(|e| e.to_string()).and_then(|r| r) {
                tracing::error!(%error, "[Web] settings save failed");
            }
        });
    }
}

/// Puts the Web page on its route with the live rail. The preview shows only
/// while the Web tab is showing and no window covers it: without GPUI Fast
/// the native page draws over GPUI content in its bounds.
pub fn install(
    backend: Arc<dyn ui_web::WebBackend>,
    operator: &gpui_kit::Entity<ui_operator::OperatorWorkspace>,
    shell: &gpui_kit::Entity<ui_shell::Shell>,
    window: &mut gpui_kit::Window,
    cx: &mut gpui_kit::App,
) -> gpui_kit::Entity<ui_web::WebPage> {
    use gpui_kit::AppContext;
    let page =
        cx.new(|cx| ui_web::WebPage::new(backend, ui_web::NativePreview::factory(), window, cx));
    let rail = operator.read(cx).rail().clone();
    page.update(cx, |p, cx| p.set_rail(Some(rail.into()), cx));
    let target = page.downgrade();
    cx.observe(shell, move |shell, cx| {
        let s = shell.read(cx);
        let active = s.route() == ui_shell::Route::Web && !s.covered();
        let _ = target.update(cx, |p, cx| p.set_active(active, cx));
    })
    .detach();
    page
}
