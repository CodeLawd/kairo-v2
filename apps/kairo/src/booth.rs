//! The booth's backend over the application's services (prompt 077):
//! `window.api.orchestrator`, `scripture`, `services`, `resilience`,
//! `program` and `account.openWeb` in Electron, and the core topics the
//! renderer stores followed.

use std::sync::Arc;

use kairo_domain::cloud::LicenseSnapshot;
use kairo_domain::program::{PresentationSettings, ProgramState, normalize_presentation_settings};
use kairo_domain::scripture::trace::ScriptureTraceStage;
use kairo_domain::scripture::{
    LivePlanState, PendingAutoPresent, ScriptureRowTranslation, ScriptureSuggestion,
};
use kairo_domain::services::ServiceSnapshot;
use kairo_domain::settings::AppSettings;
use kairo_domain::status::{
    AudioLevel, OrchestratorConfig, OrchestratorStatus, ResilienceStatus, SttProvider,
};
use kairo_domain::transcript::{InterimResult, TranscriptResult};
use kairo_output::program::OnScreens;
use serde_json::Value;
use ui_operator::booth::BoothBackend;
use ui_operator::booth::backend::{
    BoothAnswer, BoothCall, BoothFeed, BoothSnapshot, DeviceLabels, FeedSink, InputLevel,
    LegacyNugget, Licence, Listening, OnScreensLayer, Reply,
};
use ui_operator::booth::text::{NUGGET_STORAGE_KEY, legacy_nuggets_value, parse_legacy_nuggets};
use ui_operator::layout::UiState;

use crate::services::{ApplicationServices, SettingsChanged};

pub struct ServicesBooth {
    services: Arc<ApplicationServices>,
    ui_state: Arc<dyn UiState>,
}

impl ServicesBooth {
    pub fn new(services: Arc<ApplicationServices>, ui_state: Arc<dyn UiState>) -> Self {
        Self { services, ui_state }
    }

    fn settings(&self) -> Option<AppSettings> {
        let value = self
            .services
            .settings_wiring
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_all();
        serde_json::from_value(value).ok()
    }
}

fn licence_of(snapshot: &LicenseSnapshot) -> Licence {
    Licence {
        activated: snapshot.activated,
        deepgram: snapshot.providers.deepgram,
    }
}

fn level_of(level: &AudioLevel) -> InputLevel {
    InputLevel {
        rms: level.rms,
        clipping: level.clipping,
    }
}

/// Labels for the audio tabs. Video inputs are listed only once access was
/// asked for (`RequestCameraAccess`), as Chromium hid them until then.
fn audio_devices() -> DeviceLabels {
    let mut labels = DeviceLabels::default();
    for device in kairo_stt::devices::list_audio_input_devices() {
        if !device.label.is_empty() && !labels.audio_inputs.contains(&device.label) {
            labels.audio_inputs.push(device.label);
        }
    }
    for device in kairo_audio::output::list_output_devices() {
        if !device.label.is_empty() && !labels.audio_outputs.contains(&device.label) {
            labels.audio_outputs.push(device.label);
        }
    }
    labels
}

/// The tasks feeding one listener; they stop when it drops.
struct Feeds(Vec<tokio::task::JoinHandle<()>>);
impl Drop for Feeds {
    fn drop(&mut self) {
        for task in &self.0 {
            task.abort();
        }
    }
}

impl BoothBackend for ServicesBooth {
    fn snapshot(&self) -> BoothSnapshot {
        let core = &self.services.core;
        let settings = self.settings();
        let licence = core
            .watch::<LicenseSnapshot>()
            .latest()
            .map(|l| licence_of(&l))
            .unwrap_or_default();
        let presentation = self
            .services
            .settings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .data()
            .get("presentation")
            .map(normalize_presentation_settings);
        BoothSnapshot {
            status: core
                .watch::<OrchestratorStatus>()
                .latest()
                .map(|s| (*s).clone()),
            resilience: core
                .watch::<ResilienceStatus>()
                .latest()
                .map(|s| (*s).clone()),
            services: core
                .watch::<ServiceSnapshot>()
                .latest()
                .map(|s| (*s).clone()),
            legacy_nuggets: parse_legacy_nuggets(self.ui_state.get(NUGGET_STORAGE_KEY).as_ref()),
            default_translation: settings
                .as_ref()
                .map(|s| s.scripture.default_translation.clone())
                .unwrap_or_else(|| kairo_domain::settings::DEFAULT_TRANSLATION_ID.into()),
            shortcuts: settings
                .as_ref()
                .and_then(|s| s.display.shortcuts.clone())
                .unwrap_or_default(),
            automation: settings
                .as_ref()
                .map(|s| kairo_domain::settings::ScriptureAutomation::of(&s.scripture))
                .unwrap_or_default(),
            translations: Vec::new(),
            live_plan: Some(self.services.plans.state()),
            plans: self.services.plans.list_plans(),
            licence,
            program: self.services.program.state(),
            presentation,
            // Listing devices starts CoreAudio, which blocked the UI thread
            // for about 80 ms at launch (prompt 107); `listen` sends them.
            devices: DeviceLabels::default(),
        }
    }

    fn listen(&self, sink: FeedSink) -> Listening {
        let core = self.services.core.clone();
        // Called on the UI thread: the feeds run on the app's runtime.
        let runtime = self.services.runtime().clone();
        let mut tasks = Vec::new();
        macro_rules! stream {
            ($ty:ty, $wrap:expr) => {{
                let mut events = core.stream::<$ty>();
                let sink = sink.clone();
                let wrap = $wrap;
                tasks.push(runtime.spawn(async move {
                    while let Some(value) = events.next().await {
                        // A lagged reader skips what it missed and goes on.
                        let Ok(value) = value else { continue };
                        if let Some(feed) = wrap(&*value) {
                            sink(feed);
                        }
                    }
                }));
            }};
        }
        macro_rules! watch {
            ($ty:ty, $wrap:expr) => {{
                let mut values = core.watch::<$ty>();
                let sink = sink.clone();
                let wrap = $wrap;
                tasks.push(runtime.spawn(async move {
                    while let Some(value) = values.next().await {
                        if let Some(feed) = wrap(&*value) {
                            sink(feed);
                        }
                    }
                }));
            }};
        }
        stream!(TranscriptResult, |v: &TranscriptResult| Some(
            BoothFeed::Transcript(v.clone())
        ));
        stream!(InterimResult, |v: &InterimResult| Some(BoothFeed::Interim(
            v.text.clone()
        )));
        stream!(ScriptureSuggestion, |v: &ScriptureSuggestion| Some(
            BoothFeed::Suggestion(v.clone())
        ));
        stream!(ScriptureRowTranslation, |v: &ScriptureRowTranslation| Some(
            BoothFeed::RetranslateRow(v.clone())
        ));
        watch!(PendingAutoPresent, |v: &PendingAutoPresent| Some(
            BoothFeed::PendingAuto(v.clone())
        ));
        watch!(OrchestratorStatus, |v: &OrchestratorStatus| Some(
            BoothFeed::Status(v.clone())
        ));
        watch!(ResilienceStatus, |v: &ResilienceStatus| Some(
            BoothFeed::Resilience(v.clone())
        ));
        watch!(ServiceSnapshot, |v: &ServiceSnapshot| Some(
            BoothFeed::Services(v.clone())
        ));
        watch!(AudioLevel, |v: &AudioLevel| Some(BoothFeed::Level(
            level_of(v)
        )));
        watch!(ProgramState, |v: &ProgramState| Some(BoothFeed::Program(
            v.clone()
        )));
        watch!(LicenseSnapshot, |v: &LicenseSnapshot| Some(
            BoothFeed::Licence(licence_of(v))
        ));
        watch!(LivePlanState, |v: &LivePlanState| Some(
            BoothFeed::LivePlan(Some(v.clone()))
        ));
        let plans = self.services.plans.clone();
        watch!(
            SettingsChanged,
            move |SettingsChanged(value): &SettingsChanged| {
                let settings: AppSettings = serde_json::from_value(value.clone()).ok()?;
                Some(BoothFeed::Settings {
                    default_translation: settings.scripture.default_translation.clone(),
                    shortcuts: settings.display.shortcuts.clone().unwrap_or_default(),
                    automation: kairo_domain::settings::ScriptureAutomation::of(
                        &settings.scripture,
                    ),
                })
            }
        );
        // Settings changes also carry the presentation and the plan list.
        let mut changes = core.watch::<SettingsChanged>();
        let feed = sink.clone();
        tasks.push(runtime.spawn(async move {
            while let Some(value) = changes.next().await {
                let SettingsChanged(value) = &*value;
                if let Some(presentation) = value.get("presentation") {
                    feed(BoothFeed::Presentation(normalize_presentation_settings(
                        presentation,
                    )));
                }
                feed(BoothFeed::Plans(plans.list_plans()));
            }
        }));
        let feed = sink.clone();
        tasks.push(runtime.spawn(async move {
            if let Ok(devices) = tokio::task::spawn_blocking(audio_devices).await {
                feed(BoothFeed::Devices(devices));
            }
        }));
        Box::new(Feeds(tasks))
    }

    fn call(&self, call: BoothCall, reply: Reply) {
        let services = self.services.clone();
        let settings = self.settings();
        self.services.spawn(async move {
            reply(run(&services, settings, call).await);
        });
    }

    fn mark_rendered(&self, correlation_id: &str) {
        self.services.traces.mark(
            Some(correlation_id),
            ScriptureTraceStage::SuggestionRenderedAt,
        );
    }

    fn save_presentation(&self, presentation: PresentationSettings) {
        let services = self.services.clone();
        self.services.spawn(async move {
            let core = services.core.clone();
            let value = serde_json::to_value(presentation).unwrap_or(Value::Null);
            let saved = core
                .blocking(move || {
                    services
                        .settings_wiring
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .set("presentation", value)
                        .map_err(|e| e.to_string())
                })
                .await;
            if let Err(error) = saved.map_err(|e| e.to_string()).and_then(|r| r) {
                tracing::error!(%error, "[Operator] presentation save failed");
            }
        });
    }

    fn save_legacy_nuggets(&self, nuggets: &[LegacyNugget]) {
        self.ui_state
            .set(NUGGET_STORAGE_KEY, legacy_nuggets_value(nuggets));
    }
}

async fn run(
    services: &Arc<ApplicationServices>,
    settings: Option<AppSettings>,
    call: BoothCall,
) -> Result<BoothAnswer, String> {
    let program = &services.program;
    let done = Ok(BoothAnswer::Done);
    match call {
        BoothCall::Approve(id) => {
            services.orchestrator.approve_suggestion(&id).await;
            done
        }
        BoothCall::Dismiss(id) => {
            services.orchestrator.dismiss_suggestion(&id).await;
            done
        }
        BoothCall::Register(suggestion) => {
            services.scripture.receive_suggestion(suggestion);
            done
        }
        BoothCall::PresentDirectly(suggestion) => {
            services
                .orchestrator
                .present_scripture_directly(suggestion)
                .await;
            done
        }
        BoothCall::Search { query, translation } => services
            .scripture
            .search(&query, Some(&translation))
            .await
            .map(BoothAnswer::Results)
            .map_err(|e| e.to_string()),
        BoothCall::Retranslate {
            suggestion,
            translation,
        } => services
            .orchestrator
            .retranslate_suggestion(suggestion, &translation)
            .await
            .map(BoothAnswer::Suggestion),
        BoothCall::Translations => services
            .scripture
            .get_translation_options(false, false)
            .await
            .map(BoothAnswer::Translations)
            .map_err(|e| e.to_string()),
        BoothCall::SetAutoMode(enabled) => {
            services.actions().set_auto_mode(enabled).await;
            done
        }
        BoothCall::StartListening { auto_mode } => {
            let settings = settings.ok_or("Settings could not be read.")?;
            let devices = kairo_stt::devices::list_audio_input_devices();
            let config = OrchestratorConfig {
                audio_device_id: kairo_stt::devices::resolve_capture_device_id(
                    &devices,
                    &settings.audio.device_id,
                ),
                stt_provider: SttProvider::Deepgram,
                stt_api_key: String::new(),
                stt_language: if settings.stt.language.is_empty() {
                    "en".into()
                } else {
                    settings.stt.language.clone()
                },
                llm_provider: settings.stt.llm_provider,
                llm_api_key: String::new(),
                scripture_model: None,
                scripture_translation: settings.scripture.default_translation.clone(),
                auto_mode,
                confidence_threshold: settings.scripture.confidence_threshold,
                auto_present_delay_sec: f64::from(
                    settings.scripture.auto_present_delay_sec.value(),
                ),
                auto_present_passages: settings.scripture.auto_present_passages,
            };
            services
                .orchestrator
                .start_from_ui(config)
                .await
                .map(|_| BoothAnswer::Done)
        }
        BoothCall::StopListening => {
            services.orchestrator.stop().await;
            done
        }
        BoothCall::Service(command) => services
            .records
            .command(command)
            .await
            .map(BoothAnswer::Services),
        BoothCall::RestoreSession => {
            let active = services
                .core
                .watch::<ServiceSnapshot>()
                .latest()
                .is_some_and(|s| s.active_id.is_some());
            let snapshot = services
                .core
                .call(kairo_core::resilience::LoadRecovery {
                    active_service: active,
                })
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())?;
            if let Some(snapshot) = snapshot {
                services.orchestrator.restore_session(&snapshot).await?;
            }
            done
        }
        BoothCall::DiscardSession => {
            services
                .core
                .call(kairo_core::resilience::DiscardRecovery)
                .await
                .map_err(|e| e.to_string())?;
            done
        }
        BoothCall::FollowStepped => {
            services.orchestrator.note_follow_step();
            done
        }
        BoothCall::SetAutomation(automation) => {
            services.actions().set_automation(automation).await;
            done
        }
        BoothCall::ClearScriptureIfLive {
            book,
            chapter,
            verse,
        } => Ok(BoothAnswer::Cleared(
            services
                .orchestrator
                .clear_scripture_if_live(&book, chapter, verse)
                .await,
        )),
        BoothCall::ClearText => {
            services.orchestrator.clear_text().await;
            done
        }
        BoothCall::TimerSet { seconds, id } => {
            program.set_timer(seconds, Some(&id));
            done
        }
        BoothCall::TimerStart(id) => {
            program.start_timer(Some(&id));
            done
        }
        BoothCall::TimerPause(id) => {
            program.pause_timer(Some(&id));
            done
        }
        BoothCall::TimerReset(id) => {
            program.reset_timer(Some(&id));
            done
        }
        BoothCall::TimerShow(id) => {
            program.show_timer(&id);
            done
        }
        BoothCall::SetOnScreens { layer, on } => {
            let layer = match layer {
                OnScreensLayer::Countdown => OnScreens::Countdown,
                OnScreensLayer::Clock => OnScreens::Clock,
            };
            program.set_on_screens(layer, on);
            done
        }
        BoothCall::SetStageMessage(text) => {
            program.set_stage_message(text.as_deref());
            done
        }
        BoothCall::ShowMessage(text) => {
            program.show_message(&text);
            done
        }
        BoothCall::ClearMessage => {
            program.clear_message();
            done
        }
        BoothCall::SetProp { id, on } => program.set_prop(&id, on).map(|_| BoothAnswer::Done),
        BoothCall::ClearProps => {
            program.clear_props();
            done
        }
        BoothCall::PickImage => services
            .actions()
            .pick_program_image()
            .await
            .map(|p| BoothAnswer::Picked(p.map(|p| p.to_string_lossy().into_owned()))),
        BoothCall::SetCamera { label, audio } => {
            program.set_camera(label.as_deref(), audio.as_deref());
            done
        }
        BoothCall::RequestCameraAccess => {
            // Asks for camera access once (macOS shows its prompt), then
            // lists the inputs by the labels the engine opens them by.
            let listed = tokio::task::spawn_blocking(|| kairo_camera::system().video_devices())
                .await
                .map_err(|e| e.to_string())?;
            match listed {
                Ok(devices) => Ok(BoothAnswer::VideoInputs(kairo_camera::label::labels(&devices))),
                Err(kairo_camera::CameraError::Denied) => Err(
                    "Kairo needs camera access to list video inputs. Allow it in System Settings → Privacy & Security → Camera, then try again."
                        .into(),
                ),
                Err(kairo_camera::CameraError::Unsupported) => {
                    Err("Video inputs are not supported on this computer.".into())
                }
                Err(error) => Err(format!("Could not list video inputs: {error}")),
            }
        }
        BoothCall::Export { file_name, body } => {
            let folder = std::env::var_os("HOME")
                .map(|home| std::path::PathBuf::from(home).join("Downloads"))
                .ok_or("No Downloads folder.")?;
            let path = unique_path(&folder, &file_name);
            tokio::fs::write(&path, body)
                .await
                .map_err(|e| e.to_string())?;
            crate::handlers::open_path(&path, true)?;
            done
        }
        BoothCall::OpenWeb(path) => services
            .cloud
            .open_web(Some(&path))
            .await
            .map(|_| BoothAnswer::Done)
            .map_err(|e| e.to_string()),
    }
}

/// `name.txt`, else `name (1).txt`, as a browser download names a repeat.
pub(crate) fn unique_path(folder: &std::path::Path, file_name: &str) -> std::path::PathBuf {
    let first = folder.join(file_name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = file_name.rsplit_once('.').unwrap_or((file_name, ""));
    (1..)
        .map(|n| {
            folder.join(if ext.is_empty() {
                format!("{stem} ({n})")
            } else {
                format!("{stem} ({n}).{ext}")
            })
        })
        .find(|p| !p.exists())
        .unwrap_or(first)
}

/// Creates the booth, puts it in the workspace and answers what it asks the
/// application for: the activation dialog, the props editor and notices.
pub fn install(
    backend: Arc<dyn BoothBackend>,
    workspace: &gpui_kit::Entity<ui_operator::OperatorWorkspace>,
    shell: &gpui_kit::Entity<ui_shell::Shell>,
    account: gpui_kit::Entity<ui_account::Account>,
    window: &mut gpui_kit::Window,
    cx: &mut gpui_kit::App,
) -> ui_operator::BoothViews {
    use gpui_kit::AppContext;
    use gpui_kit::component::WindowExt;
    use ui_operator::BoothEvent;
    let views = ui_operator::BoothViews::new(backend, window, cx);
    views.install(workspace, cx);
    let booth = views.booth.clone();
    let props = install_props(&booth, shell, cx);
    let host = workspace.downgrade();
    let shell = shell.downgrade();
    let handle = window.window_handle();
    cx.subscribe(&views.booth, move |_, event: &BoothEvent, cx| {
        let event = event.clone();
        let (booth, host, shell, account, props) = (
            booth.clone(),
            host.clone(),
            shell.clone(),
            account.clone(),
            props.clone(),
        );
        let _ = handle.update(cx, move |_, window, cx| match &event {
            BoothEvent::OpenActivation { title, description } => {
                let dialog = cx.new(|cx| {
                    ui_account::activation::ActivationDialog::new(
                        account.clone(),
                        Some(title.clone().into()),
                        Some(description.clone().into()),
                        window,
                        cx,
                    )
                });
                let booth = booth.clone();
                let closing = host.clone();
                cx.subscribe(
                    &dialog,
                    move |_, _: &ui_account::activation::DismissActivation, cx| {
                        booth.update(cx, |b, cx| b.close_key_prompt(cx));
                        let _ = closing.update(cx, |w, cx| w.set_dialog(None, cx));
                    },
                )
                .detach();
                let _ = host.update(cx, |w, cx| w.set_dialog(Some(dialog.into()), cx));
            }
            BoothEvent::CloseActivation => {
                let _ = host.update(cx, |w, cx| w.set_dialog(None, cx));
            }
            BoothEvent::OpenProps(select) => {
                // An open editor moves to the request; a new one starts there.
                let open = props.borrow().editor.as_ref().and_then(|e| e.upgrade());
                match open {
                    Some(editor) => editor.update(cx, |e, cx| e.select(select.clone(), cx)),
                    None => props.borrow_mut().select = select.clone(),
                }
                let _ = shell.update(cx, |s, cx| s.open_overlay(ui_shell::Overlay::Props, cx));
            }
            BoothEvent::Toast(text) => {
                window.push_notification(
                    gpui_kit::component::notification::Notification::error(text.clone()),
                    cx,
                );
            }
            BoothEvent::FocusSearch | BoothEvent::Alert(_) => {}
        });
    })
    .detach();
    views
}

/// Where the Props window opens next, and the one that is open.
#[derive(Default)]
struct PropsSlot {
    select: Option<String>,
    editor: Option<gpui_kit::WeakEntity<ui_operator::PropsEditor>>,
}

/// Registers the Props editor (prompt 088) as the shell's Props overlay.
fn install_props(
    booth: &gpui_kit::Entity<ui_operator::Booth>,
    shell: &gpui_kit::Entity<ui_shell::Shell>,
    cx: &mut gpui_kit::App,
) -> std::rc::Rc<std::cell::RefCell<PropsSlot>> {
    use gpui_kit::AppContext;
    let slot: std::rc::Rc<std::cell::RefCell<PropsSlot>> = Default::default();
    let (booth, target, build_slot) = (booth.clone(), shell.downgrade(), slot.clone());
    shell.update(cx, |shell, _| {
        shell.set_overlay_builder(
            ui_shell::Overlay::Props,
            Box::new(move |_, window, cx| {
                let select = build_slot.borrow_mut().select.take();
                let editor =
                    cx.new(|cx| ui_operator::PropsEditor::new(booth.clone(), select, window, cx));
                build_slot.borrow_mut().editor = Some(editor.downgrade());
                let (target, handle) = (target.clone(), window.window_handle());
                cx.subscribe(&editor, move |editor, _: &ui_operator::PropsEvent, cx| {
                    let _ = handle.update(cx, |_, window, cx| {
                        editor.update(cx, |e, cx| e.restore_focus(window, cx))
                    });
                    let _ = target.update(cx, |shell, cx| shell.close_overlay(cx));
                })
                .detach();
                gpui_kit::AnyView::from(editor)
            }),
        )
    });
    slot
}
