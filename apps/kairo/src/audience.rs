//! The Engage page's backend over the application's services (prompt 090):
//! `AudienceService` and `EngageFlows` (046, 047), the Kairo account,
//! `presentation.question` and per-viewer storage.

use std::sync::Arc;

use kairo_domain::cloud::{CloudSessionState, SessionSnapshot};
use kairo_domain::program::{PresentationSettings, normalize_presentation_settings};
use serde_json::Value;
use ui_audience::backend::{
    AccountView, AudienceAnswer, AudienceCall, AudienceFeed, AudienceSnapshot, FeedSink, Listening,
    Reply,
};

use crate::services::{ApplicationServices, SettingsChanged};

pub struct ServicesAudience {
    services: Arc<ApplicationServices>,
    ui_state: Arc<dyn ui_operator::UiState>,
}

impl ServicesAudience {
    pub fn new(
        services: Arc<ApplicationServices>,
        ui_state: Arc<dyn ui_operator::UiState>,
    ) -> Self {
        Self { services, ui_state }
    }
}

fn account_of(s: &SessionSnapshot) -> AccountView {
    AccountView {
        signed_in: s.state != CloudSessionState::SignedOut,
        online: s.state == CloudSessionState::Active,
    }
}

fn presentation_of(all: &Value) -> PresentationSettings {
    all.get("presentation")
        .map(normalize_presentation_settings)
        .unwrap_or_default()
}

struct Guard {
    tasks: Vec<tokio::task::JoinHandle<()>>,
    listener: (Arc<kairo_audience::AudienceService>, u64),
}
impl Drop for Guard {
    fn drop(&mut self) {
        for t in &self.tasks {
            t.abort();
        }
        self.listener.0.remove_listener(self.listener.1);
    }
}

fn state(
    r: kairo_audience::Result<kairo_domain::audience::AudienceState>,
) -> Result<AudienceAnswer, String> {
    r.map(|s| AudienceAnswer::State(Box::new(s)))
        .map_err(|e| e.0)
}

impl ui_audience::AudienceBackend for ServicesAudience {
    fn snapshot(&self) -> AudienceSnapshot {
        let all = self
            .services
            .settings_wiring
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_all();
        AudienceSnapshot {
            state: self.services.audience.state(),
            // The account arrives on the feed; signed out until it does.
            account: AccountView::default(),
            style: presentation_of(&all).question,
        }
    }

    fn listen(&self, sink: FeedSink) -> Listening {
        let services = self.services.clone();
        let changed = sink.clone();
        let id = services
            .audience
            .on_changed(move |s| changed(AudienceFeed::State(Box::new(s.clone()))));
        let mut tasks = Vec::new();
        tasks.push(services.runtime().spawn({
            let (services, sink) = (services.clone(), sink.clone());
            async move {
                sink(AudienceFeed::Account(account_of(
                    &services.cloud.session().await,
                )));
                let mut sessions = services.core.watch::<SessionSnapshot>();
                while let Some(s) = sessions.next().await {
                    sink(AudienceFeed::Account(account_of(&s)));
                }
            }
        }));
        tasks.push(services.runtime().spawn({
            let mut values = services.core.watch::<SettingsChanged>();
            async move {
                while let Some(value) = values.next().await {
                    let SettingsChanged(all) = &*value;
                    sink(AudienceFeed::Style(Box::new(presentation_of(all).question)));
                }
            }
        }));
        Box::new(Guard {
            tasks,
            listener: (services.audience.clone(), id),
        })
    }

    fn call(&self, call: AudienceCall, reply: Reply<AudienceAnswer>) {
        let services = self.services.clone();
        self.services.spawn(async move {
            let a = &services.audience;
            let e = &services.engage;
            let done =
                |r: kairo_audience::Result<()>| r.map(|_| AudienceAnswer::Done).map_err(|e| e.0);
            let result = match call {
                AudienceCall::GetState => Ok(AudienceAnswer::State(Box::new(a.state()))),
                AudienceCall::Start { kind, options } => state(e.start(&kind, &options).await),
                AudienceCall::Stop(id) => state(a.stop(&id).await),
                AudienceCall::Open(id) => Ok(AudienceAnswer::State(Box::new(a.open(&id)))),
                AudienceCall::Moderate { id, state: s } => state(a.moderate(&id, s.as_str()).await),
                AudienceCall::Show(id) => state(a.show(&id)),
                AudienceCall::Clear => Ok(AudienceAnswer::State(Box::new(a.clear()))),
                AudienceCall::ShowJoinCode => state(a.show_join_code()),
                AudienceCall::ShowPoll(id) => state(a.show_poll(&id)),
                AudienceCall::ClearPoll => Ok(AudienceAnswer::State(Box::new(a.clear_poll()))),
                AudienceCall::Command(c) => state(a.command(c).await),
                AudienceCall::SetRoomSettings(s) => state(a.set_room_settings(s).await),
                AudienceCall::Invite { email, name } => state(e.invite(&email, &name).await),
                AudienceCall::Revoke(id) => state(e.revoke(&id).await),
                AudienceCall::ListEngagements => e
                    .list()
                    .await
                    .map(AudienceAnswer::Engagements)
                    .map_err(|e| e.0),
                AudienceCall::CreateEngagement(input) => done(e.create(&input).await),
                AudienceCall::DeleteEngagement(code) => done(e.delete(&code).await),
                AudienceCall::Resume(code) => state(e.resume(&code, false).await),
                AudienceCall::Reopen => state(e.reopen().await),
                AudienceCall::OpenAudiencePage => a
                    .audience_page_url()
                    .map(AudienceAnswer::Url)
                    .map_err(|e| e.0),
            };
            reply(result);
        });
    }

    fn save_style(&self, style: kairo_domain::program::ProgramQuestionStyle, reply: Reply<()>) {
        let services = self.services.clone();
        self.services.spawn(async move {
            let core = services.core.clone();
            let saved = core
                .blocking(move || {
                    let mut wiring = services
                        .settings_wiring
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    let mut presentation = presentation_of(&wiring.get_all());
                    presentation.question = style;
                    let value = serde_json::to_value(presentation).map_err(|e| e.to_string())?;
                    wiring.set("presentation", value).map_err(|e| e.to_string())
                })
                .await;
            reply(saved.map_err(|e| e.to_string()).and_then(|r| r));
        });
    }

    fn save_qr(&self, png: Vec<u8>, file_name: String, reply: Reply<()>) {
        self.services.spawn(async move {
            let result = async {
                let folder = std::env::var_os("HOME")
                    .map(|home| std::path::PathBuf::from(home).join("Downloads"))
                    .ok_or("No Downloads folder.")?;
                let path = crate::booth::unique_path(&folder, &file_name);
                tokio::fs::write(&path, png)
                    .await
                    .map_err(|e| e.to_string())?;
                crate::handlers::open_path(&path, true)
            }
            .await;
            reply(result.map_err(|e: String| e));
        });
    }

    fn read_ui(&self, key: &str) -> Option<String> {
        // Electron's `localStorage` text, or a value this app wrote.
        match self.ui_state.get(key)? {
            Value::Null => None,
            Value::String(text) => Some(text),
            other => Some(other.to_string()),
        }
    }

    fn write_ui(&self, key: &str, value: Option<String>) {
        let v = value
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .unwrap_or(Value::Null);
        self.ui_state.set(key, v);
    }
}

/// Puts the Engage page on its route with the live rail.
pub fn install(
    backend: Arc<dyn ui_audience::AudienceBackend>,
    operator: &gpui_kit::Entity<ui_operator::OperatorWorkspace>,
    window: &mut gpui_kit::Window,
    cx: &mut gpui_kit::App,
) -> gpui_kit::Entity<ui_audience::AudiencePage> {
    use gpui_kit::AppContext;
    let page = cx.new(|cx| ui_audience::AudiencePage::new(backend, window, cx));
    let rail = operator.read(cx).rail().clone();
    page.update(cx, |p, cx| p.set_rail(Some(rail.into()), cx));
    page
}
