//! The Scripture page's backend over the application's services (prompt 078):
//! `window.api.scripture`, `settings`, `passages`, `libraries` and
//! `transfer` in Electron, and the renderer stores the page read.

use std::sync::Arc;

use kairo_domain::kairo_bundle::TransferExportRequest;
use kairo_domain::scripture::{ApiBibleAuthorizationState, LivePlanState};
use kairo_domain::settings::AppSettings;
use serde_json::Value;
use ui_operator::layout::UiState;
use ui_scripture::backend::{
    FeedSink, Listening, OptionChange, Reply, ScriptureAnswer, ScriptureBackend, ScriptureCall,
    ScriptureFeed, ScriptureSnapshot, SlideOptions,
};
use ui_scripture::rows::RECENT_LOOKUPS_KEY;

use crate::services::{ApplicationServices, SettingsChanged};

pub struct ServicesScripture {
    services: Arc<ApplicationServices>,
    ui_state: Arc<dyn UiState>,
}

impl ServicesScripture {
    pub fn new(services: Arc<ApplicationServices>, ui_state: Arc<dyn UiState>) -> Self {
        Self { services, ui_state }
    }
}

fn all_settings(services: &ApplicationServices) -> Value {
    services
        .settings_wiring
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_all()
}

fn options_of(value: &Value) -> SlideOptions {
    match serde_json::from_value::<AppSettings>(value.clone()) {
        Ok(s) => SlideOptions {
            show_verse_numbers: s.overlay.show_verse_numbers,
            show_translation: s.overlay.show_translation,
            max_verses: s.overlay.max_verses,
            default_translation: s.scripture.default_translation,
            background: card_background(&s.overlay.outputs),
        },
        Err(_) => SlideOptions::default(),
    }
}

/// The main output's scripture-theme background, for the verse cards.
fn card_background(
    outputs: &[kairo_domain::overlay::OverlayOutput],
) -> ui_scripture::backend::CardBackground {
    use kairo_domain::overlay::{OverlayBackgroundType, OverlayContentKind};
    let Some(output) = kairo_domain::overlay_outputs::primary_rendered_output(outputs)
        .or_else(|| outputs.iter().find(|o| o.enabled))
        .or(outputs.first())
    else {
        return Default::default();
    };
    let theme =
        kairo_domain::overlay_outputs::output_theme_for(output, OverlayContentKind::Scripture);
    let bg = &theme.background;
    let color = || {
        let hex = bg.color.trim().trim_start_matches('#');
        (hex.len() == 6)
            .then(|| u32::from_str_radix(hex, 16).ok())
            .flatten()
    };
    let opacity = bg.opacity.clamp(0.0, 1.0) as f32;
    let mut card = match bg.kind {
        OverlayBackgroundType::Image => ui_scripture::backend::CardBackground {
            color: color(),
            image: bg.media_path.clone().filter(|p| !p.is_empty()),
            opacity,
            theme: None,
        },
        OverlayBackgroundType::Transparent => ui_scripture::backend::CardBackground::default(),
        // A video or gradient shows its base colour.
        _ => ui_scripture::backend::CardBackground {
            color: color(),
            image: None,
            opacity,
            theme: None,
        },
    };
    card.theme = Some(Box::new(theme.clone()));
    card
}

/// Feed tasks; they stop when the guard drops.
struct Feeds(Vec<tokio::task::JoinHandle<()>>);
impl Drop for Feeds {
    fn drop(&mut self) {
        for task in &self.0 {
            task.abort();
        }
    }
}

impl ScriptureBackend for ServicesScripture {
    fn snapshot(&self) -> ScriptureSnapshot {
        let services = &self.services;
        ScriptureSnapshot {
            options: options_of(&all_settings(services)),
            // Read off the UI thread; arrives as a feed.
            translations: Vec::new(),
            plans: services.plans.list_plans(),
            live_plan: Some(services.plans.state()),
            passages: services.passages.snapshot(),
            libraries: services.libraries.snapshot().scripture,
            recent_lookups: self.ui_state.get(RECENT_LOOKUPS_KEY),
            api_bible_warning: false,
        }
    }

    fn listen(&self, sink: FeedSink) -> Listening {
        let services = self.services.clone();
        let core = services.core.clone();
        let runtime = services.runtime().clone();
        let mut tasks = Vec::new();
        {
            let sink = sink.clone();
            let services = services.clone();
            tasks.push(runtime.spawn(async move {
                if let Ok(t) = services
                    .scripture
                    .get_translation_options(false, false)
                    .await
                {
                    sink(ScriptureFeed::Translations(t));
                }
                // Electron's authorization check is not run natively (072), so
                // the state is "unchecked": warn only when there is no key.
                let has_key = services.licensing.secrets_configured().await.bible;
                sink(ScriptureFeed::ApiBibleWarning(
                    ui_shell::bootstrap::should_show_api_bible_warning(
                        ApiBibleAuthorizationState::Unchecked,
                        has_key,
                        true,
                    ),
                ));
            }));
        }
        {
            let mut values = core.watch::<SettingsChanged>();
            let sink = sink.clone();
            let services = services.clone();
            tasks.push(runtime.spawn(async move {
                while let Some(value) = values.next().await {
                    let SettingsChanged(value) = &*value;
                    sink(ScriptureFeed::Options(options_of(value)));
                    sink(ScriptureFeed::Plans(services.plans.list_plans()));
                    if let Ok(t) = services
                        .scripture
                        .get_translation_options(false, false)
                        .await
                    {
                        sink(ScriptureFeed::Translations(t));
                    }
                }
            }));
        }
        {
            let mut values = core.watch::<LivePlanState>();
            let sink = sink.clone();
            tasks.push(runtime.spawn(async move {
                while let Some(value) = values.next().await {
                    sink(ScriptureFeed::LivePlan(Some((*value).clone())));
                }
            }));
        }
        {
            let mut done = core.watch::<crate::transfer::TransferCommitted>();
            let sink = sink.clone();
            let services = services.clone();
            tasks.push(runtime.spawn(async move {
                while done.next().await.is_some() {
                    sink(ScriptureFeed::Plans(services.plans.list_plans()));
                }
            }));
        }
        let passages = sink.clone();
        services
            .passages
            .on_changed(move |list| passages(ScriptureFeed::Passages(list.clone())));
        let libraries = sink.clone();
        services
            .libraries
            .on_changed(move |state| libraries(ScriptureFeed::Libraries(state.scripture.clone())));
        Box::new(Feeds(tasks))
    }

    fn call(&self, call: ScriptureCall, reply: Reply) {
        let services = self.services.clone();
        self.services.spawn(async move {
            reply(run(&services, call).await);
        });
    }

    fn save_recent_lookups(&self, value: Value) {
        self.ui_state.set(RECENT_LOOKUPS_KEY, value);
    }
}

async fn save_setting(
    services: &Arc<ApplicationServices>,
    change: OptionChange,
) -> Result<(), String> {
    let services = services.clone();
    services
        .core
        .clone()
        .blocking(move || {
            let mut wiring = services
                .settings_wiring
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let all = wiring.get_all();
            let mut patch = |key: &str, field: &str, value: Value| {
                let mut section = all
                    .get(key)
                    .cloned()
                    .unwrap_or(Value::Object(Default::default()));
                if let Some(map) = section.as_object_mut() {
                    map.insert(field.into(), value);
                }
                wiring.set(key, section).map_err(|e| e.to_string())
            };
            match change {
                OptionChange::VerseNumbers(on) => {
                    patch("overlay", "showVerseNumbers", Value::Bool(on))?;
                    patch("scripture", "showVerseNumbers", Value::Bool(on))
                }
                OptionChange::ShowTranslation(on) => {
                    patch("overlay", "showTranslation", Value::Bool(on))
                }
                OptionChange::MaxVerses(n) => patch("overlay", "maxVerses", serde_json::json!(n)),
                OptionChange::DefaultTranslation(t) => {
                    patch("scripture", "defaultTranslation", Value::String(t))
                }
            }
        })
        .await
        .map_err(|e| e.to_string())?
}

async fn run(
    services: &Arc<ApplicationServices>,
    call: ScriptureCall,
) -> Result<ScriptureAnswer, String> {
    let done = Ok(ScriptureAnswer::Done);
    match call {
        ScriptureCall::Search { query, translation } => services
            .scripture
            .search(&query, Some(&translation))
            .await
            .map(ScriptureAnswer::Results)
            .map_err(|e| e.to_string()),
        ScriptureCall::SavePlan(plan) => {
            let plans = services.plans.clone();
            services
                .core
                .blocking(move || plans.save_plan(plan))
                .await
                .map(ScriptureAnswer::Plan)
                .map_err(|e| e.to_string())
        }
        ScriptureCall::DeletePlan(id) => {
            let plans = services.plans.clone();
            services
                .core
                .blocking(move || plans.delete_plan(&id))
                .await
                .map_err(|e| e.to_string())?;
            done
        }
        ScriptureCall::SetLivePlan(id) => {
            let plans = services.plans.clone();
            services
                .core
                .blocking(move || plans.set_plan(Some(&id)))
                .await
                .map(ScriptureAnswer::LivePlan)
                .map_err(|e| e.to_string())
        }
        ScriptureCall::Present(suggestion) => {
            services
                .orchestrator
                .present_scripture_directly(suggestion)
                .await;
            done
        }
        ScriptureCall::Passages(command) => services
            .passages
            .apply(command)
            .map(ScriptureAnswer::Passages)
            .map_err(|e| e.to_string()),
        ScriptureCall::Libraries(command) => services
            .libraries
            .apply(command)
            .map(|state| ScriptureAnswer::Libraries(state.scripture))
            .map_err(|e| e.to_string()),
        ScriptureCall::SaveOption(change) => {
            save_setting(services, change).await?;
            done
        }
        ScriptureCall::ImportSermonNotes => crate::handlers::Handlers(services.clone())
            .import_sermon_notes()
            .await
            .map(ScriptureAnswer::Draft),
        ScriptureCall::ScanSermonNotes(text) => {
            let translation = options_of(&all_settings(services)).default_translation;
            tokio::task::spawn_blocking(move || {
                kairo_scripture::sermon_notes::scan_sermon_notes(&text, &translation)
            })
            .await
            .map(ScriptureAnswer::Analysis)
            .map_err(|e| e.to_string())
        }
        ScriptureCall::ExportPlaylist(plan_id) => {
            crate::transfer::export(services, TransferExportRequest::Playlist { plan_id }).await;
            done
        }
        ScriptureCall::ImportKairo => {
            crate::transfer::pick(services).await;
            done
        }
    }
}

/// Creates the page, shares the operator's live rail and answers what the
/// page asks of the application.
pub fn install(
    backend: Arc<dyn ScriptureBackend>,
    operator: &gpui_kit::Entity<ui_operator::OperatorWorkspace>,
    shell: &gpui_kit::Entity<ui_shell::Shell>,
    window: &mut gpui_kit::Window,
    cx: &mut gpui_kit::App,
) -> gpui_kit::Entity<ui_scripture::ScriptureWorkspace> {
    use gpui_kit::AppContext;
    use gpui_kit::component::WindowExt;
    use ui_scripture::ScriptureEvent;
    let page = cx.new(|cx| ui_scripture::ScriptureWorkspace::new(backend, window, cx));
    // Only one route draws at a time, so the rail is shared, as Electron's
    // `LiveOutputRail` was the same component on both pages.
    let rail = operator.read(cx).rail().clone();
    page.update(cx, |p, cx| p.set_rail(Some(rail.into()), cx));
    let model = page.read(cx).model().clone();
    let target = shell.downgrade();
    let handle = window.window_handle();
    let focus = page.downgrade();
    let cleared = model.clone();
    cx.subscribe(shell, move |_, request: &ui_shell::ShellRequest, cx| {
        if matches!(request, ui_shell::ShellRequest::ClearLive) {
            cleared.update(cx, |m, cx| m.header_cleared(cx));
        }
    })
    .detach();
    cx.subscribe(&model, move |_, event: &ScriptureEvent, cx| {
        let event = event.clone();
        let (target, focus) = (target.clone(), focus.clone());
        let _ = handle.update(cx, move |_, window, cx| match event {
            ScriptureEvent::OpenSettings => {
                let _ = target.update(cx, |s, cx| s.open_settings("scripture", cx));
            }
            ScriptureEvent::Notify(text) => window.push_notification(
                gpui_kit::component::notification::Notification::success(text),
                cx,
            ),
            ScriptureEvent::FocusQuery => {
                let _ = focus.update(cx, |p, cx| p.focus_query(window, cx));
            }
            // 079: the page itself opens the review and creates the playlist.
            ScriptureEvent::ReviewSermonNotes(_) | ScriptureEvent::ImportFinished(_) => {}
        });
    })
    .detach();
    page
}
