//! Application composition. Local bootstrap is ordered independently of cloud
//! hydration; GPUI operations cross the main-thread bridge explicitly.

use kairo_cloud::licensing::LicensingService;
use kairo_cloud::service_records::{RecordServices, ServiceRecords};
use kairo_cloud::telemetry::Telemetry;
use kairo_cloud::{CloudConfig, CloudSession, OrgProfilePatch};
use kairo_core::application::{
    ApplicationLifecycle, BackgroundTask, LifecycleServices, LocalResult, LocalStep,
};
use kairo_core::engine::EngineHandle;
use kairo_core::settings::{SettingsServices, SettingsWiring};
use kairo_core::{AppCore, Publisher};
use kairo_domain::audience::PollView;
use kairo_domain::cloud::{LicenseSnapshot, LicensedFeature, SecretsConfigured};
use kairo_domain::media::{MediaItem, MediaPlayback};
use kairo_domain::overlay::OverlayOutput;
use kairo_domain::overlay_defaults::normalize_overlay_settings;
use kairo_domain::program::{ProgramQuestion, ProgramState};
use kairo_domain::scripture::{ScriptureResult, ScriptureSuggestion, SermonPlan};
use kairo_domain::services::ServiceSnapshot;
use kairo_domain::settings::{OverlaySettings, ScriptureSettings};
use kairo_domain::status::{LlmProvider, SttProvider, UpdateStatus};
use kairo_domain::transcript::TranscriptResult;
use kairo_domain::usage::UsageFeature;
use kairo_media::{library::MediaService, tracks::TracksService};
use kairo_orchestrator::clients::{self, TranscriptEvent};
use kairo_orchestrator::detector::{LiveDetector, LocalEvidenceSource};
use kairo_orchestrator::engine::EngineSurfaces;
use kairo_orchestrator::live_plan::{LivePlanService, SermonPlanStore};
use kairo_orchestrator::orchestrator::{Orchestrator, Services};
use kairo_orchestrator::voice::BoxFuture;
use kairo_output::ProgramService;
use kairo_scripture::api_bible::{ApiBibleCache, ApiBibleSource, OfflineBibles};
use kairo_scripture::model_detector::{ModelConfig, ModelProvider};
use kairo_scripture::{ScriptureService, Subscription};
use kairo_store::paths::AppPaths;
use kairo_store::secrets::{ApiCacheCrypto, KeyProtector, KeyringProtector};
use kairo_store::settings::{PostOpenServices, SettingsStore};
use kairo_store::workspace::Workspace;
use kairo_stt::capture::{CaptureEvent, CaptureHandle};
use kairo_stt::{AudioRelay, DeepgramStt, SttService};
use kairo_transfer::{
    LibrariesService, PassagesService, SetlistService, SongUsageService, TransferService,
};
use kairo_updater::{UpdaterConfig, UpdaterService};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock, Weak, mpsc};
use tokio::sync::{broadcast, oneshot};
use ui_shell::state::{AccountView, HouseTrack, OutputReadiness, ShellState, UpdateState};

pub type Settings = Arc<Mutex<SettingsStore>>;
pub type Media = Arc<Mutex<MediaService<Settings>>>;
pub type Tracks = Arc<Mutex<TracksService<Settings>>>;
pub type MainThreadJob = Box<dyn FnOnce() + Send>;

pub enum UiEvent {
    OpenWindow {
        services: Arc<ApplicationServices>,
        ready: oneshot::Sender<Result<(), String>>,
    },
    MainThread(MainThreadJob),
    State(Box<ShellState>),
    Feedback(Result<(), String>),
    /// A report for the launch screen's load `generation`.
    Bootstrap {
        generation: u64,
        report: BootstrapReport,
    },
    ConfirmInstall(Arc<ApplicationServices>),
    /// A change for the account and setup views.
    Account(Box<crate::account::AccountUpdate>),
    Quit,
}

#[derive(Clone)]
pub struct UiBridge {
    events: mpsc::Sender<UiEvent>,
    slido: Option<Arc<ui_web::SlidoWebHost>>,
}
impl UiBridge {
    pub fn channel() -> (Self, mpsc::Receiver<UiEvent>) {
        let (tx, rx) = mpsc::channel();
        (
            Self {
                events: tx,
                slido: None,
            },
            rx,
        )
    }
    pub fn with_slido(mut self, host: ui_web::SlidoWebHost) -> Self {
        self.slido = Some(Arc::new(host));
        self
    }
    pub fn send(&self, event: UiEvent) -> Result<(), String> {
        self.events
            .send(event)
            .map_err(|_| "The operator window is unavailable.".into())
    }
    pub async fn dialog(
        &self,
        request: kairo_core::dialogs::DialogRequest,
    ) -> Result<Vec<PathBuf>, String> {
        let (tx, rx) = oneshot::channel();
        self.send(UiEvent::MainThread(Box::new(move || {
            let _ = tx.send(crate::dialogs::show(&request));
        })))?;
        rx.await
            .map_err(|_| "The file dialog closed unexpectedly.".to_owned())?
    }
}

impl crate::dialogs::NativeDialogs for UiBridge {
    fn pick(
        &self,
        request: kairo_core::dialogs::DialogRequest,
    ) -> BoxFuture<'_, Result<Vec<PathBuf>, String>> {
        Box::pin(self.dialog(request))
    }
}

/// All runtime services have real implementations. The API is deliberately
/// public for subsequent workspace views to issue typed calls directly.
#[non_exhaustive]
pub struct ApplicationServices {
    pub core: AppCore,
    paths: AppPaths,
    pub settings: Settings,
    pub settings_wiring: Mutex<SettingsWiring>,
    pub workspace: Arc<Mutex<Workspace>>,
    pub lyrics: Arc<kairo_lyrics::LyricsService>,
    pub scripture: ScriptureService,
    pub offline: Arc<OfflineBibles>,
    pub plans: LivePlanService,
    pub media: Media,
    pub tracks: Tracks,
    pub documents: Arc<kairo_documents::service::DocumentsService>,
    pub converter: Arc<kairo_documents::slide_converter::SlideConverter>,
    pub program: Arc<ProgramService>,
    pub orchestrator: Arc<Orchestrator>,
    pub surfaces: Arc<EngineSurfaces>,
    pub engine: EngineHandle,
    pub stt: Arc<SttService>,
    pub cloud: CloudSession,
    pub licensing: LicensingService,
    pub telemetry: Arc<Telemetry>,
    pub records: ServiceRecords<RecordAdapter>,
    pub audience: Arc<kairo_audience::AudienceService>,
    pub updater: Arc<UpdaterService>,
    pub engage: kairo_audience::EngageFlows,
    pub setlist: SetlistService,
    pub libraries: LibrariesService,
    pub passages: PassagesService,
    pub song_usage: SongUsageService,
    pub transfer: Arc<TransferService>,
    pub ui: UiBridge,
    pub picked: Arc<kairo_media::allowlist::PickedMediaAllowlist>,
    pub capture_status: Arc<RwLock<kairo_domain::capture::CaptureStatus>>,
    pub quote: kairo_scripture::quote::corpus::QuoteCorpus,
    /// Recent scripture latency traces (`scripture:recentTraces`,
    /// `scripture:markRendered`).
    pub traces: Arc<kairo_core::scripture_traces::ScriptureTraces>,
    runtime: tokio::runtime::Handle,
    _subscriptions: Mutex<Vec<Subscription>>,
    _pack_subscription: kairo_scripture::installer::Unsubscribe,
}

struct StartupState {
    core: AppCore,
    paths: AppPaths,
    settings: Settings,
    workspace: Arc<Mutex<Workspace>>,
    engine: EngineHandle,
    ui: UiBridge,
    lyrics: OnceLock<Arc<kairo_lyrics::LyricsService>>,
    scripture: ScriptureService,
    bible: OnceLock<ApiBibleSource>,
    offline: OnceLock<Arc<OfflineBibles>>,
    plans: LivePlanService,
    services: OnceLock<Arc<ApplicationServices>>,
    runtime: tokio::runtime::Handle,
    protector: Arc<dyn KeyProtector>,
}

pub struct Startup(Arc<StartupState>);
impl Startup {
    pub fn new(
        core: AppCore,
        paths: AppPaths,
        settings: Settings,
        engine: EngineHandle,
        ui: UiBridge,
    ) -> Result<Arc<Self>, String> {
        Self::with_protector(
            core,
            paths,
            settings,
            engine,
            ui,
            Arc::new(KeyringProtector),
        )
    }

    fn with_protector(
        core: AppCore,
        paths: AppPaths,
        settings: Settings,
        engine: EngineHandle,
        ui: UiBridge,
        protector: Arc<dyn KeyProtector>,
    ) -> Result<Arc<Self>, String> {
        let scripture = ScriptureService::new(Arc::new(
            kairo_scripture::pack::HttpPackFetcher::new().map_err(|e| e.to_string())?,
        ));
        let state = StartupState {
            workspace: Arc::new(Mutex::new(Workspace::new(paths.default_workspace_root()))),
            plans: LivePlanService::new(Box::new(SermonPlanStore::open(&paths.user_data))),
            core,
            paths,
            settings,
            engine,
            ui,
            scripture,
            lyrics: OnceLock::new(),
            bible: OnceLock::new(),
            offline: OnceLock::new(),
            services: OnceLock::new(),
            runtime: tokio::runtime::Handle::current(),
            protector,
        };
        Ok(Arc::new(Self(Arc::new(state))))
    }
    pub async fn start(self: &Arc<Self>) -> Result<(), String> {
        ApplicationLifecycle::new(self.clone())
            .start()
            .await
            .map_err(|e| e.to_string())
    }
}

fn missing(service: &str) -> String {
    format!("Startup has not opened {service}.")
}
fn get<T: Clone>(value: &OnceLock<T>, name: &str) -> Result<T, String> {
    value.get().cloned().ok_or_else(|| missing(name))
}
fn set<T>(target: &OnceLock<T>, value: T, name: &str) -> Result<(), String> {
    target
        .set(value)
        .map_err(|_| format!("Startup opened {name} twice."))
}
fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value.lock().unwrap_or_else(|e| e.into_inner())
}

impl LifecycleServices for Startup {
    fn local_step(&self, step: LocalStep) -> LocalResult {
        let state = self.0.clone();
        Box::pin(async move {
            use LocalStep::*;
            match step {
                Settings => lock(&state.settings).flush()?,
                Workspace => {
                    lock(&state.workspace).ensure(&lock(&state.settings));
                }
                DefaultMediaFolder => {
                    lock(&state.workspace).apply_default_media_folder(&mut lock(&state.settings))
                }
                OpenLyrics => {
                    let lyrics = kairo_lyrics::LyricsService::open(
                        &state.paths.user_data,
                        state.paths.default_workspace_root(),
                        state.settings.clone(),
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                    set(&state.lyrics, Arc::new(lyrics), "lyrics")?;
                }
                OpenScripture => state
                    .scripture
                    .open(&state.paths.user_data, &bundled_bible(&state.paths)?)
                    .await
                    .map_err(|e| e.to_string())?,
                MigrateNkjvDefault => {
                    struct Installed(Result<bool, String>);
                    impl PostOpenServices for Installed {
                        fn bible_pack_installed(&self, _: &str) -> Result<bool, String> {
                            self.0.clone()
                        }
                    }
                    let installed = state
                        .scripture
                        .get_local_bible_pack_status("NKJV")
                        .await
                        .map(|s| s.installed)
                        .map_err(|e| e.to_string());
                    lock(&state.settings).migrate_unreadable_nkjv_default(&Installed(installed));
                }
                DefaultTranslation => {
                    let settings: ScriptureSettings = lock(&state.settings)
                        .section("scripture")
                        .map_err(|e| e.to_string())?;
                    state
                        .scripture
                        .set_default_translation(&settings.default_translation);
                }
                OfflineBibles => {
                    let profile = state.paths.user_data.clone();
                    let protector = state.protector.clone();
                    let crypto = tokio::task::spawn_blocking(move || {
                        ApiCacheCrypto::open(&profile, protector.as_ref(), "Kairo")
                    })
                    .await
                    .map_err(|e| e.to_string())?;
                    let cache = match crypto {
                        Ok((crypto, _)) => match ApiBibleCache::open(
                            &state.paths.user_data.join("api-bible-cache.db"),
                            crypto,
                        )
                        .await
                        {
                            Ok(cache) => Some(cache),
                            Err(error) => {
                                tracing::warn!(%error, "[Scripture] Offline Bible cache disabled");
                                None
                            }
                        },
                        Err(error) => {
                            tracing::warn!(%error, "[Scripture] Offline Bible cache disabled");
                            None
                        }
                    };
                    let source = ApiBibleSource::new(
                        cache,
                        Arc::new(|| kairo_store::settings::now_ms() as i64),
                    );
                    state
                        .scripture
                        .attach_api_bible(Some(Arc::new(source.clone())));
                    let offline = kairo_scripture::api_bible::OfflineBibles::new(
                        source.clone(),
                        Arc::new(Vec::new),
                        {
                            let topics = state.core.publisher();
                            Arc::new(move || {
                                topics.emit(BibleCacheChanged);
                            })
                        },
                    );
                    set(&state.bible, source, "API.Bible")?;
                    set(&state.offline, Arc::new(offline), "offline Bibles")?;
                }
                LivePlan => state.plans.init(),
                OnboardingMigration => kairo_cloud::onboarding::Onboarding::register(
                    &state.core,
                    state.paths.user_data.clone(),
                    state.settings.clone(),
                )
                .await
                .map_err(|e| e.to_string())?,
                Reporting => {
                    let services = compose(&state).await?;
                    set(&state.services, services, "application services")?;
                }
                Updater => {
                    let services = get(&state.services, "application services")?;
                    // `init` schedules its background check, rather than awaiting it.
                    let updater = services.updater.clone();
                    tokio::spawn(async move {
                        updater.init().await;
                    });
                }
                Window => {
                    let (ready, reply) = oneshot::channel();
                    state.ui.send(UiEvent::OpenWindow {
                        services: get(&state.services, "application services")?,
                        ready,
                    })?;
                    reply
                        .await
                        .map_err(|_| "The operator window did not open.".to_owned())??;
                }
                SurfaceInit => {
                    let services = get(&state.services, "application services")?;
                    kairo_output::actor::init_surface_manager(
                        &state.core,
                        Arc::new(state.engine.clone()),
                        state.settings.clone(),
                        services.media.clone(),
                        Some(services.surfaces.clone()),
                    )
                    .map_err(|e| e.to_string())?;
                }
                Reconcile => {
                    let core = state.core.clone();
                    tokio::spawn(async move {
                        if let Err(error) = core
                            .call(kairo_output::actor::Reconcile { outputs: None })
                            .await
                        {
                            tracing::warn!(%error, "[Output] initial reconciliation failed");
                        }
                    });
                }
            }
            Ok(())
        })
    }
    fn start_background(&self, task: BackgroundTask) {
        if task == BackgroundTask::Ndi {
            self.0.engine.start();
            return;
        }
        let Some(services) = self.0.services.get().cloned() else {
            tracing::error!(?task, "background service started before composition");
            return;
        };
        self.0.runtime.spawn(async move {
            match task {
                BackgroundTask::License => services.licensing.start(),
                BackgroundTask::Session => services.cloud.start().await,
                BackgroundTask::ScanMedia => {
                    let media = services.media.clone();
                    let _ = tokio::task::spawn_blocking(move || lock(&media).scan()).await;
                }
                BackgroundTask::ScanTracks => {
                    let tracks = services.tracks.clone();
                    let _ = tokio::task::spawn_blocking(move || lock(&tracks).scan()).await;
                }
                BackgroundTask::ReconnectSession => services.cloud.reconnect().await,
                BackgroundTask::PullOrgSecretsThrottled => {
                    services.cloud.pull_org_secrets(true).await
                }
                BackgroundTask::WakeSermonUploader => services.records.wake_uploads(),
                BackgroundTask::WakeUsage => services.telemetry.wake().await,
                BackgroundTask::Ndi => unreachable!("handled before scheduling"),
            }
        });
    }
}

pub struct BibleCacheChanged;

fn bundled_bible(paths: &AppPaths) -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("KAIRO_BIBLE_DB") {
        return Ok(path.into());
    }
    let mut candidates = Vec::new();
    if let Some(exe) = &paths.exe
        && let Some(parent) = exe.parent()
    {
        candidates.extend([
            parent.join("resources/bible.db"),
            parent.join("../Resources/bible.db"),
            parent.join("bible.db"),
        ]);
    }
    if !paths.packaged {
        candidates.push(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../kairo-presenter/resources/bible.db"),
        );
    }
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| {
            "The bundled Bible database is missing. Set KAIRO_BIBLE_DB for a development build."
                .into()
        })
}

struct ScriptureAdapter(ScriptureService);
impl clients::ScriptureClient for ScriptureAdapter {
    fn search(
        &self,
        query: String,
        translation: String,
    ) -> BoxFuture<'_, Result<Vec<ScriptureResult>, String>> {
        Box::pin(async move {
            self.0
                .search(&query, Some(&translation))
                .await
                .map_err(|e| e.to_string())
        })
    }
    fn receive_suggestion(&self, suggestion: ScriptureSuggestion) {
        self.0.receive_suggestion(suggestion);
    }
    fn pending_suggestion(&self, id: &str) -> Option<ScriptureSuggestion> {
        self.0.get_pending_suggestion(id)
    }
    fn dismiss_suggestion(&self, id: &str) {
        self.0.dismiss_suggestion(id);
    }
    fn request_row_translation(&self, passage_id: &str, translation: &str) {
        self.0.request_row_translation(passage_id, translation);
    }
}
struct MediaAdapter(Media);
impl clients::MediaClient for MediaAdapter {
    fn live_item(&self) -> Option<MediaItem> {
        lock(&self.0).live_item().cloned()
    }
    fn live_playback(&self) -> Option<MediaPlayback> {
        lock(&self.0).live_playback()
    }
    fn live_cue(&self) -> f64 {
        lock(&self.0).live_cue()
    }
    fn clear_live_item(&self) {
        lock(&self.0).set_live_item(None);
    }
}
struct OverlayAdapter(Settings);
impl clients::SettingsClient for OverlayAdapter {
    fn overlay(&self) -> OverlaySettings {
        normalize_overlay_settings(lock(&self.0).data().get("overlay"))
    }
}
struct UsageAdapter(Arc<Telemetry>);
impl clients::UsageClient for UsageAdapter {
    fn track(&self, event: &str) {
        if let Ok(feature) = serde_json::from_value::<UsageFeature>(Value::String(event.into())) {
            let telemetry = self.0.clone();
            tokio::spawn(async move {
                telemetry.track(feature, 1.).await;
            });
        }
    }
}
struct Gate {
    licensing: LicensingService,
    active: Arc<AtomicBool>,
}
impl clients::SessionGate for Gate {
    fn assert_transcription(&self) -> BoxFuture<'_, Result<(), String>> {
        Box::pin(async move {
            self.licensing
                .assert_feature(LicensedFeature::Transcription)
                .await
                .map_err(|e| e.to_string())
        })
    }
    fn has_active_service(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }
}

pub struct RecordAdapter {
    orchestrator: Arc<Mutex<Weak<Orchestrator>>>,
    stt: Arc<SttService>,
    plans: LivePlanService,
    settings: Settings,
}
impl RecordServices for RecordAdapter {
    async fn clear_history(&self) {
        self.stt.clear_history();
    }
    async fn stop_orchestrator(&self) -> Result<(), String> {
        let orchestrator = lock(&self.orchestrator)
            .upgrade()
            .ok_or_else(|| missing("orchestrator"))?;
        orchestrator.stop().await;
        Ok(())
    }
    async fn sermon_plan(&self, id: &str) -> Option<SermonPlan> {
        self.plans.plan(id)
    }
    async fn llm_provider(&self) -> LlmProvider {
        if lock(&self.settings)
            .data()
            .get("stt")
            .and_then(|v| v.get("llmProvider"))
            .and_then(Value::as_str)
            == Some("deepseek")
        {
            LlmProvider::Deepseek
        } else {
            LlmProvider::Anthropic
        }
    }
}

struct SttAdapter {
    stt: Arc<SttService>,
    relay: AudioRelay,
    capture: Mutex<Option<CaptureHandle>>,
    settings: Settings,
    topics: Publisher,
}
impl clients::SttClient for SttAdapter {
    fn configure(&self, provider: SttProvider, language: &str, keyterms: Vec<String>) {
        self.stt.configure(provider, language, keyterms);
    }
    fn start(&self) -> BoxFuture<'_, Result<(), String>> {
        Box::pin(async move {
            if !kairo_stt::permission::ensure_microphone_access().await {
                return Err("Microphone access is required to transcribe the service.".into());
            }
            self.stt.start().await.map_err(|e| e.to_string())
        })
    }
    fn stop(&self) {
        self.stt.stop();
    }
    fn history(&self) -> Vec<TranscriptResult> {
        self.stt.history()
    }
    fn set_history(&self, history: Vec<TranscriptResult>) {
        self.stt.set_history(history);
    }
    fn attach_audio(&self) -> Result<(), String> {
        self.stt
            .deepgram()
            .attach_stream(self.relay.create_stream());
        let id = lock(&self.settings)
            .data()
            .get("audio")
            .and_then(|v| v.get("deviceId"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let relay = self.relay.clone();
        let topics = self.topics.clone();
        match kairo_stt::capture::start_capture(&id, move |event| match event {
            CaptureEvent::Pcm(pcm) => relay.feed(pcm),
            CaptureEvent::Level(level) => topics.publish(level),
            CaptureEvent::Error(error) => {
                topics.emit(error);
            }
            CaptureEvent::FellBackToDefault { requested } => {
                tracing::warn!(%requested, "[Audio] using the default input")
            }
        }) {
            Ok(handle) => {
                *lock(&self.capture) = Some(handle);
                Ok(())
            }
            Err(error) => {
                self.relay.destroy_stream();
                self.stt.deepgram().detach_stream();
                Err(error.to_string())
            }
        }
    }
    fn detach_audio(&self) {
        lock(&self.capture).take();
        self.stt.deepgram().detach_stream();
        self.relay.destroy_stream();
    }
}

struct Evidence(ScriptureService);
impl LocalEvidenceSource for Evidence {
    fn chapter_verses(
        &self,
        book: &str,
        chapter: f64,
    ) -> Vec<kairo_scripture::service::QuoteCandidate> {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(self.0.get_local_chapter_verses(book, chapter))
        })
        .unwrap_or_default()
    }
    fn quote_candidates(&self, phrase: &str) -> Vec<kairo_scripture::service::QuoteCandidate> {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(self.0.search_local_quote_candidates(phrase))
        })
        .unwrap_or_default()
    }
}

struct AudienceOutput(Arc<ProgramService>);
impl kairo_audience::QuestionOutput for AudienceOutput {
    fn show_question(&self, question: ProgramQuestion) {
        self.0.show_question(question);
    }
    fn clear_question(&self) {
        self.0.clear_question();
    }
    fn show_poll(&self, poll: &PollView) {
        self.0.show_poll(poll.clone());
    }
    fn clear_poll(&self) {
        self.0.clear_poll();
    }
}

struct SettingsAdapter {
    core: AppCore,
    program: Arc<ProgramService>,
    scripture: ScriptureService,
    cloud: CloudSession,
    providers: Arc<RwLock<SecretsConfigured>>,
    orchestrator: tokio::sync::mpsc::UnboundedSender<ScriptureChange>,
}
enum ScriptureChange {
    Translation(String),
    Auto(bool),
    Confidence(f64),
}
impl SettingsServices for SettingsAdapter {
    fn secrets_configured(&self) -> SecretsConfigured {
        self.providers
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    fn reconcile_outputs(&self, outputs: Option<Value>) {
        let outputs =
            outputs.and_then(|value| serde_json::from_value::<Vec<OverlayOutput>>(value).ok());
        let core = self.core.clone();
        tokio::spawn(async move {
            if let Err(error) = core.call(kairo_output::actor::Reconcile { outputs }).await {
                tracing::error!(%error, "[Settings] output reconciliation failed");
            }
        });
    }
    fn prune_props(&self) {
        self.program.prune_props();
    }
    fn sync_church_profile(&self, profile: Value) {
        let patch = OrgProfilePatch {
            name: profile
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_owned),
            timezone: profile
                .get("timezone")
                .and_then(Value::as_str)
                .map(str::to_owned),
            service_times: profile
                .get("serviceTimes")
                .cloned()
                .and_then(|v| serde_json::from_value(v).ok()),
        };
        let cloud = self.cloud.clone();
        tokio::spawn(async move {
            cloud.sync_org_profile(patch).await;
        });
    }

    fn broadcast_settings_changed(&self, settings: Value) {
        self.core.publisher().publish(SettingsChanged(settings));
    }
    fn scripture_default_translation(&self, translation: Value) {
        if let Some(value) = translation.as_str() {
            self.scripture.set_default_translation(value);
        }
    }
    fn scripture_auto_mode(&self, enabled: Value) {
        self.scripture
            .set_auto_mode(kairo_domain::js::truthy(Some(&enabled)));
    }
    fn scripture_confidence(&self, threshold: Value) {
        self.scripture
            .set_confidence_threshold(kairo_domain::js::value_to_number(Some(&threshold)));
    }
    fn orchestrator_translation(&self, translation: Value) {
        if let Some(value) = translation.as_str() {
            let _ = self
                .orchestrator
                .send(ScriptureChange::Translation(value.into()));
        }
    }
    fn orchestrator_auto_mode(&self, enabled: Value) {
        let _ = self
            .orchestrator
            .send(ScriptureChange::Auto(kairo_domain::js::truthy(Some(
                &enabled,
            ))));
    }
    fn orchestrator_confidence(&self, threshold: Value) {
        let _ = self.orchestrator.send(ScriptureChange::Confidence(
            kairo_domain::js::value_to_number(Some(&threshold)),
        ));
    }
}

pub struct SettingsChanged(pub Value);

/// What the launch screen hears about its load.
pub enum BootstrapReport {
    Progress(kairo_domain::status::BootstrapProgress),
    Finished(Result<Box<kairo_domain::status::AppBootstrapSnapshot>, String>),
}

/// Answers a load that ends without a snapshot (its task dropped or
/// panicked), so the launch screen opens with a warning instead of waiting
/// for ever, as a failed `app:bootstrap` invoke does in Electron.
struct BootstrapAnswer {
    ui: UiBridge,
    generation: u64,
    answered: bool,
}

impl Drop for BootstrapAnswer {
    fn drop(&mut self) {
        if !self.answered {
            let _ = self.ui.send(UiEvent::Bootstrap {
                generation: self.generation,
                report: BootstrapReport::Finished(
                    Err("The startup request did not finish.".into()),
                ),
            });
        }
    }
}

async fn compose(state: &StartupState) -> Result<Arc<ApplicationServices>, String> {
    let core = state.core.clone();
    kairo_core::resilience::ResilienceActor::register(
        &core,
        &state.paths.user_data,
        std::num::NonZeroUsize::new(64).expect("nonzero mailbox"),
    )
    .await
    .map_err(|e| e.to_string())?;
    let settings = state.settings.clone();
    let cloud = CloudSession::open_local(
        CloudConfig::production().map_err(|e| e.to_string())?,
        state.paths.user_data.clone(),
        state.protector.clone(),
        Some(settings.clone()),
        core.publisher(),
    )
    .await
    .map_err(|e| e.to_string())?;
    let licensing = LicensingService::open_local(
        cloud.clone(),
        state.paths.user_data.clone(),
        state.protector.clone(),
        core.publisher(),
    )
    .await;
    let telemetry = Arc::new(
        Telemetry::open(
            cloud.clone(),
            state.paths.user_data.clone(),
            settings.clone(),
        )
        .await
        .map_err(|e| e.to_string())?,
    );
    let lyrics = get(&state.lyrics, "lyrics")?;
    let media = Arc::new(Mutex::new(MediaService::new(settings.clone())));
    let tracks = Arc::new(Mutex::new(TracksService::new(settings.clone())));
    let surfaces = Arc::new(EngineSurfaces::new(state.engine.clone()));
    let program = Arc::new(ProgramService::new(
        settings.clone(),
        core.publisher(),
        Box::new({
            let core = core.clone();
            move |state| {
                if let Err(error) = core.send(kairo_output::actor::SetProgram { state }) {
                    tracing::warn!(%error, "[Program] surface actor unavailable");
                }
            }
        }),
    ));
    let stt = Arc::new(SttService::new(
        Arc::new(licensing.clone()),
        DeepgramStt::new(Arc::new(licensing.clone())),
    ));
    let weak_orchestrator = Arc::new(Mutex::new(Weak::new()));
    let records = ServiceRecords::open(
        &state.paths.user_data,
        RecordAdapter {
            orchestrator: weak_orchestrator.clone(),
            stt: stt.clone(),
            plans: state.plans.clone(),
            settings: settings.clone(),
        },
        cloud.clone(),
        core.publisher(),
    )
    .await?;
    let active = Arc::new(AtomicBool::new(records.active().await.is_some()));
    let (transcripts, _) = broadcast::channel(512);
    let quote = kairo_scripture::quote::corpus::QuoteCorpus::new();
    quote.open(state.paths.user_data.join("bible.db"));
    quote.attach_api_cache(get(&state.bible, "API.Bible")?.cache().cloned());
    let orchestrator = Arc::new(Orchestrator::new(Services {
        engine: surfaces.clone(),
        program: program.clone(),
        media: Arc::new(MediaAdapter(media.clone())),
        scripture: Arc::new(ScriptureAdapter(state.scripture.clone())),
        stt: Arc::new(SttAdapter {
            stt: stt.clone(),
            relay: AudioRelay::default(),
            capture: Mutex::new(None),
            settings: settings.clone(),
            topics: core.publisher(),
        }),
        transcripts: {
            let tx = transcripts.clone();
            Arc::new(move || tx.subscribe())
        },
        gate: Arc::new(Gate {
            licensing: licensing.clone(),
            active: active.clone(),
        }),
        settings: Arc::new(OverlayAdapter(settings.clone())),
        usage: Arc::new(UsageAdapter(telemetry.clone())),
        live_plan: state.plans.clone(),
        detector: {
            let quote = quote.clone();
            let licensing = licensing.clone();
            let scripture = state.scripture.clone();
            let plans = state.plans.clone();
            let settings_for_model = settings.clone();
            Arc::new(move |config| {
                crate::quote_detector::QuoteDetector::new(
                    LiveDetector::new(
                        Some(Arc::new(licensing.clone())),
                        ModelConfig {
                            provider: if config.llm_provider == LlmProvider::Deepseek {
                                ModelProvider::Deepseek
                            } else {
                                ModelProvider::Anthropic
                            },
                            model: lock(&settings_for_model)
                                .data()
                                .get("stt")
                                .and_then(|v| v.get("llmModel"))
                                .and_then(Value::as_str)
                                .map(str::to_owned),
                            ..Default::default()
                        },
                        Some(Arc::new(Evidence(scripture.clone()))),
                        {
                            let plans = plans.clone();
                            Arc::new(move || plans.index())
                        },
                    ),
                    quote.clone(),
                )
            })
        },
        resilience: Some(core.clone()),
    }));
    *lock(&weak_orchestrator) = Arc::downgrade(&orchestrator);
    let updater = Arc::new(UpdaterService::new(UpdaterConfig::for_this_process(
        env!("CARGO_PKG_VERSION"),
        state.paths.packaged,
        Some(kairo_store::build_config::WEB_URL.into()),
    )));
    let audience = Arc::new(kairo_audience::AudienceService::new(
        Arc::new(AudienceOutput(program.clone())),
        !state.paths.packaged && std::env::var("KAIRO_AUDIENCE_SAMPLE").as_deref() == Ok("1"),
    ));
    let mut engage = kairo_audience::EngageFlows::new(
        (*audience).clone(),
        Arc::new(cloud.clone()),
        kairo_store::build_config::audience_api_base_url(),
        kairo_store::build_config::audience_api_fallback_url().map(str::to_owned),
        kairo_audience::sample_allowed(state.paths.packaged),
    );
    if let Some(host) = &state.ui.slido {
        engage = engage.with_slido(host.clone());
    }
    let converter = kairo_documents::slide_converter::SlideConverter::new(
        &state.paths.user_data,
        kairo_net::transport::client_builder()
            .build()
            .map_err(|e| e.to_string())?,
    );
    let printer: Arc<dyn kairo_documents::pptx_builtin::SlidePrinter> = {
        #[cfg(target_os = "macos")]
        {
            let ui = state.ui.clone();
            Arc::new(kairo_documents::webkit::WebKitPrinter::new(move |job| {
                if let Err(error) = ui.send(UiEvent::MainThread(job)) {
                    tracing::warn!(%error, "[Documents] UI printer unavailable");
                }
            }))
        }
        #[cfg(target_os = "linux")]
        {
            let ui = state.ui.clone();
            Arc::new(kairo_documents::webkitgtk::WebKitGtkPrinter::new(
                move |job| {
                    if let Err(error) = ui.send(UiEvent::MainThread(job)) {
                        tracing::warn!(%error, "[Documents] UI printer unavailable");
                    }
                },
            ))
        }
        #[cfg(windows)]
        {
            Arc::new(kairo_documents::webview2::WebView2Printer::new())
        }
    };
    let documents = kairo_documents::service::DocumentsService::new(
        &state.paths.user_data,
        Arc::new(kairo_documents::office_apps::SystemProbes),
        converter.clone(),
        printer,
    );
    let setlist = SetlistService::open(&state.paths.user_data, kairo_transfer::system_clock());
    let libraries = LibrariesService::open(&state.paths.user_data, kairo_transfer::system_clock());
    let passages = PassagesService::open(
        &state.paths.user_data,
        libraries.clone(),
        kairo_transfer::system_clock(),
    );
    let song_usage = SongUsageService::new(&state.paths.user_data, kairo_transfer::system_clock());
    let transfer = Arc::new(TransferService::new(
        lyrics.clone(),
        setlist.clone(),
        state.plans.clone(),
        env!("CARGO_PKG_VERSION"),
        kairo_transfer::system_clock(),
    ));
    let providers = Arc::new(RwLock::new(licensing.secrets_configured().await));
    let (changes, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    {
        let orchestrator = orchestrator.clone();
        tokio::spawn(async move {
            while let Some(change) = receiver.recv().await {
                match change {
                    ScriptureChange::Translation(value) => {
                        orchestrator.set_scripture_translation(&value).await
                    }
                    ScriptureChange::Auto(value) => orchestrator.set_auto_mode(value).await,
                    ScriptureChange::Confidence(value) => {
                        orchestrator.set_confidence_threshold(value).await
                    }
                }
            }
        });
    }
    let settings_wiring = Mutex::new(SettingsWiring::new(
        settings.clone(),
        Arc::new(SettingsAdapter {
            core: core.clone(),
            program: program.clone(),
            scripture: state.scripture.clone(),
            cloud: cloud.clone(),
            providers: providers.clone(),
            orchestrator: changes,
        }),
    ));
    let mut subscriptions = Vec::new();
    subscriptions.push(state.scripture.on_suggestion({
        let records = records.clone();
        let topics = core.publisher();
        move |value| {
            let value = value.clone();
            let records = records.clone();
            let topics = topics.clone();
            tokio::spawn(async move {
                if let Err(error) = records.scripture(value.clone()).await {
                    tracing::warn!(%error, "[Records] scripture event failed");
                }
                topics.emit(value);
            });
        }
    }));
    subscriptions.push(state.scripture.on_row_translation({
        let topics = core.publisher();
        move |value| {
            topics.emit(value.clone());
        }
    }));
    let pack_subscription = state.scripture.on_local_bible_install_progress({
        let topics = core.publisher();
        move |value| {
            topics.publish(value.clone());
        }
    });
    state.plans.on_change({
        let topics = core.publisher();
        let records = records.clone();
        move |value| {
            let records = records.clone();
            let value = value.clone();
            let topics = topics.clone();
            tokio::spawn(async move {
                if let Some(id) = &value.plan_id {
                    let _ = records.live_plan(id).await;
                }
                topics.publish(value);
            });
        }
    });
    subscriptions.push(state.scripture.on_corpus_change({
        let quote = quote.clone();
        move || quote.invalidate_quote_corpus()
    }));
    let services = Arc::new(ApplicationServices {
        core,
        paths: state.paths.clone(),
        settings,
        settings_wiring,
        workspace: state.workspace.clone(),
        lyrics,
        scripture: state.scripture.clone(),
        offline: get(&state.offline, "offline Bibles")?,
        plans: state.plans.clone(),
        media,
        tracks,
        documents,
        converter,
        program,
        orchestrator,
        surfaces,
        engine: state.engine.clone(),
        stt,
        cloud,
        licensing,
        telemetry,
        records,
        audience,
        updater,
        engage,
        setlist,
        libraries,
        passages,
        song_usage,
        transfer,
        ui: state.ui.clone(),
        picked: Arc::default(),
        capture_status: Arc::default(),
        quote,
        traces: Arc::new(kairo_core::scripture_traces::ScriptureTraces::default()),
        runtime: state.runtime.clone(),
        _subscriptions: Mutex::new(subscriptions),
        _pack_subscription: pack_subscription,
    });
    wire_events(
        &services,
        transcripts,
        active,
        providers,
        get(&state.bible, "API.Bible")?,
    );
    services.updater.set_live_output({
        let program = services.program.clone();
        let stt = services.stt.clone();
        move || {
            let program = program.state();
            program.current.is_some()
                || program.camera.is_some()
                || program.web.is_some()
                || stt.is_active()
        }
    });
    services.updater.set_before_install({
        let engine = services.engine.clone();
        let core = services.core.clone();
        move || {
            let engine = engine.clone();
            let core = core.clone();
            async move {
                // Stop capture while surfaces still paint, then release them within the
                // same bounded deadline used by normal quit.
                let _ = engine.command(kairo_engine_proto::Command::StopCapture);
                let _ = tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    core.call(kairo_output::actor::DestroyAll {}),
                )
                .await;
            }
        }
    });
    services.updater.set_quit({
        let ui = state.ui.clone();
        move || {
            let _ = ui.send(UiEvent::Quit);
        }
    });
    Ok(services)
}

fn wire_events(
    services: &Arc<ApplicationServices>,
    transcripts: broadcast::Sender<TranscriptEvent>,
    active: Arc<AtomicBool>,
    providers: Arc<RwLock<SecretsConfigured>>,
    bible: ApiBibleSource,
) {
    let media = lock(&services.media).subscribe();
    let topics = services.core.publisher();
    tokio::spawn(async move {
        while let Some(value) = media.next().await {
            topics.publish(value);
        }
    });
    let tracks = lock(&services.tracks).subscribe();
    let topics = services.core.publisher();
    tokio::spawn(async move {
        let mut player = kairo_audio::tracks::TracksPlayer::new();
        while let Some(value) = tracks.next().await {
            player.follow(&value);
            topics.publish(value);
        }
    });
    let mut events = services.engine.subscribe();
    let status = services.capture_status.clone();
    let topics = services.core.publisher();
    tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(kairo_core::engine::EngineEvent::Engine(
                    kairo_engine_proto::Event::Capture(value),
                )) => {
                    if let Ok(value) = value.decode::<kairo_domain::capture::CaptureStatus>() {
                        *status.write().unwrap_or_else(|e| e.into_inner()) = value.clone();
                        topics.publish(value);
                    }
                }
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    let topics = services.core.publisher();
    services
        .setlist
        .on_changed(move |value| topics.publish(value.clone()));
    let topics = services.core.publisher();
    services
        .libraries
        .on_changed(move |value| topics.publish(value.clone()));
    let topics = services.core.publisher();
    services
        .passages
        .on_changed(move |value| topics.publish(value.clone()));
    let mut surfaces = services
        .core
        .stream::<kairo_output::manager::SettingsRewritten>();
    let weak = Arc::downgrade(services);
    tokio::spawn(async move {
        while surfaces.next().await.is_some() {
            if let Some(services) = weak.upgrade() {
                lock(&services.settings_wiring).broadcast_changed();
            } else {
                break;
            }
        }
    });
    let weak = Arc::downgrade(&services.orchestrator);
    let core = services.core.clone();
    tokio::spawn(async move {
        let _ = kairo_core::resilience::monitor(&core, move || {
            let weak = weak.clone();
            async move {
                match weak.upgrade() {
                    Some(orchestrator) => Ok(orchestrator.recovery_snapshot().await),
                    None => Ok(None),
                }
            }
        })
        .await;
    });
    let mut stt = services.stt.subscribe();
    let records = services.records.clone();
    let topics = services.core.publisher();
    tokio::spawn(async move {
        loop {
            match stt.recv().await {
                Ok(kairo_stt::service::ServiceEvent::Stt(kairo_stt::SttEvent::Final(value))) => {
                    if let Err(error) = records.transcript(value.clone()).await {
                        tracing::warn!(%error, "[Records] transcript event failed");
                    }
                    topics.emit(value.clone());
                    let _ = transcripts.send(TranscriptEvent::Final(value));
                }
                Ok(kairo_stt::service::ServiceEvent::Stt(kairo_stt::SttEvent::Interim(value))) => {
                    topics.emit(value.clone());
                    let _ = transcripts.send(TranscriptEvent::Interim(value));
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(count)) => {
                    tracing::warn!(count, "[STT] event fan-out lagged")
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    let mut status = services.orchestrator.subscribe_status();
    let records = services.records.clone();
    let topics = services.core.publisher();
    tokio::spawn(async move {
        while let Ok(value) = status.recv().await {
            let _ = records.set_running(value.running).await;
            topics.publish(value);
        }
    });
    let mut pending = services.orchestrator.subscribe_pending_auto();
    let topics = services.core.publisher();
    tokio::spawn(async move {
        while let Ok(value) = pending.recv().await {
            topics.publish(value);
        }
    });
    let mut snapshots = services.core.watch::<ServiceSnapshot>();
    tokio::spawn(async move {
        while let Some(value) = snapshots.next().await {
            active.store(value.active_id.is_some(), Ordering::Release);
        }
    });
    let mut licences = services.core.watch::<LicenseSnapshot>();
    let weak = Arc::downgrade(services);
    let gateway = kairo_cloud::bible_gateway::BibleGateway::new(services.licensing.clone());
    let services_licensing = services.licensing.clone();
    tokio::spawn(async move {
        while licences.next().await.is_some() {
            *providers.write().unwrap_or_else(|e| e.into_inner()) =
                services_licensing.secrets_configured().await;
            gateway.sync(&bible).await;
            if let Some(services) = weak.upgrade() {
                lock(&services.settings_wiring).broadcast_changed();
            } else {
                break;
            }
        }
    });
    services.updater.on_change({
        let topics = services.core.publisher();
        move |value| topics.publish(value.clone())
    });
    services.audience.on_changed({
        let topics = services.core.publisher();
        move |value| topics.publish(value.clone())
    });
    let mut program = services.core.watch::<ProgramState>();
    let audience = services.audience.clone();
    tokio::spawn(async move {
        while program.next().await.is_some() {
            audience.sync_cleared();
        }
    });
    let weak = Arc::downgrade(services);
    tokio::spawn(async move {
        let mut previous = None;
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let Some(services) = weak.upgrade() else {
                break;
            };
            let state = services.shell_state().await;
            if previous.as_ref() != Some(&state) {
                let _ = services.ui.send(UiEvent::State(Box::new(state.clone())));
                previous = Some(state);
            }
        }
    });
    {
        let telemetry = services.telemetry.clone();
        tokio::spawn(async move {
            telemetry.run().await;
        });
    }
    {
        let records = services.records.clone();
        tokio::spawn(async move {
            records.run().await;
        });
    }
}

impl ApplicationServices {
    pub fn spawn(&self, future: impl std::future::Future<Output = ()> + Send + 'static) {
        self.runtime.spawn(future);
    }
    /// The services' runtime, for work started off it (the UI thread).
    pub fn runtime(&self) -> &tokio::runtime::Handle {
        &self.runtime
    }
    pub fn actions(&self) -> crate::actions::Actions {
        crate::actions::Actions {
            settings: self.settings.clone(),
            lyrics: self.lyrics.clone(),
            media: self.media.clone(),
            documents: self.documents.clone(),
            libraries: self.libraries.clone(),
            song_usage: self.song_usage.clone(),
            program: self.program.clone(),
            orchestrator: self.orchestrator.clone(),
            scripture: self.scripture.clone(),
            surfaces: self.surfaces.clone(),
            core: self.core.clone(),
            usage: Arc::new(UsageAdapter(self.telemetry.clone())),
            dialogs: Arc::new(self.ui.clone()),
            tracks: self.tracks.clone(),
            picked: self.picked.clone(),
        }
    }
    /// The local reads behind `app:bootstrap`. None touches the network:
    /// API.Bible authorization, devices and NDI settle afterwards.
    pub fn bootstrap_loaders(self: &Arc<Self>) -> kairo_core::bootstrap::BootstrapLoaders {
        let settings = self.clone();
        let orchestrator = self.orchestrator.clone();
        let stt = self.stt.clone();
        let scripture = self.scripture.clone();
        let sermon_plans = self.plans.clone();
        let live_plan = self.plans.clone();
        let lyrics = self.lyrics.clone();
        let core = self.core.clone();
        let cloud = self.cloud.clone();
        kairo_core::bootstrap::BootstrapLoaders {
            settings: Box::pin(async move {
                let value =
                    tokio::task::spawn_blocking(move || lock(&settings.settings_wiring).get_all())
                        .await
                        .map_err(|e| e.to_string())?;
                serde_json::from_value(value).map_err(|e| e.to_string())
            }),
            orchestrator: Box::pin(async move { Ok(orchestrator.status().await) }),
            transcription: Box::pin(async move { Ok(stt.history()) }),
            // Offline: launch never waits on API.Bible.
            translations: Box::pin(async move {
                scripture
                    .get_translation_options(false, false)
                    .await
                    .map_err(|e| e.to_string())
            }),
            sermon_plans: Box::pin(async move { Ok(sermon_plans.list_plans()) }),
            live_plan: Box::pin(async move { Ok(live_plan.state()) }),
            lyrics: Box::pin(async move { lyrics.get_library().await.map_err(|e| e.to_string()) }),
            // A disk read, like everything here.
            onboarding: Box::pin(async move {
                core.call(kairo_cloud::onboarding::GetOnboarding)
                    .await
                    .map_err(|e| e.to_string())
            }),
            // The token vault on disk; validating it happens later.
            account: Box::pin(async move { Ok(cloud.session().await) }),
        }
    }

    /// Runs the launch screen's load `generation`, reporting progress and
    /// then the snapshot to the window.
    pub async fn bootstrap(self: &Arc<Self>, generation: u64) {
        let mut answer = BootstrapAnswer {
            ui: self.ui.clone(),
            generation,
            answered: false,
        };
        let progress = self.ui.clone();
        let started = std::time::Instant::now();
        let snapshot = kairo_core::bootstrap::run_bootstrap(self.bootstrap_loaders(), move |p| {
            let _ = progress.send(UiEvent::Bootstrap {
                generation,
                report: BootstrapReport::Progress(p),
            });
        })
        .await;
        let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
        if snapshot.errors.is_empty() {
            tracing::info!(elapsed_ms, "[Bootstrap] Completed");
        } else {
            tracing::warn!(elapsed_ms, errors = ?snapshot.errors, "[Bootstrap] Completed with failures");
        }
        answer.answered = true;
        let _ = self.ui.send(UiEvent::Bootstrap {
            generation,
            report: BootstrapReport::Finished(Ok(Box::new(snapshot))),
        });
    }

    pub async fn focus(&self) {
        let cloud = self.cloud.clone();
        tokio::spawn(async move {
            cloud.pull_org_secrets(true).await;
        });
        self.records.wake_uploads();
        self.telemetry.wake().await;
    }
    pub async fn resume(&self) {
        self.cloud.reconnect().await;
    }
    pub async fn shell_state(&self) -> ShellState {
        let program = self.program.state();
        let account = self.cloud.session().await;
        let licence = self.licensing.get_status().await;
        let mut state = ShellState {
            live_label: program.current.as_ref().map(|s| s.reference.clone().into()),
            transcribing: self.orchestrator.status().await.running,
            account: AccountView {
                signed_in: account.state != kairo_domain::cloud::CloudSessionState::SignedOut,
                name: account.user.as_ref().map(|u| u.name.clone().into()),
                email: account.user.as_ref().map(|u| u.email.clone().into()),
                credentials_persisted: account.credentials_persisted,
            },
            activated: Some(licence.activated),
            recording: self
                .capture_status
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .recording,
            ..Default::default()
        };
        state.update = match self.updater.status() {
            UpdateStatus::Checking => UpdateState::Checking,
            UpdateStatus::Available { version, .. } => UpdateState::Available {
                version: version.into(),
            },
            UpdateStatus::Downloading { percent, .. } => UpdateState::Downloading {
                percent: percent.map(|p| p.clamp(0., 100.) as u8),
            },
            UpdateStatus::Downloaded { version, .. } => UpdateState::Downloaded {
                version: version.into(),
            },
            UpdateStatus::Error { .. } => UpdateState::Error,
            _ => UpdateState::Idle,
        };
        let overlay = normalize_overlay_settings(lock(&self.settings).data().get("overlay"));
        state.outputs = Some(
            overlay
                .outputs
                .into_iter()
                .filter(|o| o.enabled)
                .map(|o| {
                    let reason = self.engine.unavailable_reason(&o.id);
                    OutputReadiness {
                        name: o.name.into(),
                        ready: Some(reason.is_none()),
                        reason: reason.map(Into::into),
                    }
                })
                .collect(),
        );
        let tracks = lock(&self.tracks).library();
        state.house_track = tracks
            .live_id
            .as_deref()
            .and_then(|id| tracks.items.iter().find(|i| i.id == id))
            .map(|item| HouseTrack {
                name: item.name.clone().into(),
                paused: tracks.live_paused,
            });
        let settings = lock(&self.settings);
        if let Ok(display) = settings.section::<kairo_domain::settings::DisplaySettings>("display")
        {
            state.shortcuts = display.shortcuts.unwrap_or_default();
        }
        if let Ok(whats_new) =
            settings.section::<kairo_domain::settings::WhatsNewSettings>("whatsNew")
        {
            state.whats_new_last_seen = Some(whats_new.last_seen_version.into());
        }
        state
    }
}

#[derive(Clone, Debug)]
pub struct ImportRequested(pub kairo_domain::import_menu::ImportKind);

impl ApplicationServices {
    pub fn paths(&self) -> &AppPaths {
        &self.paths
    }
    /// Shell requests are dispatched onto the runtime, never executed in a
    /// render or GPUI update callback.
    pub fn request(self: &Arc<Self>, request: ui_shell::ShellRequest) {
        let services = self.clone();
        self.spawn(async move {
            use ui_shell::ShellRequest::*;
            match request {
                Bootstrap { generation } => services.bootstrap(generation).await,
                WindowFocused => {
                    services
                        .core
                        .publisher()
                        .emit(kairo_core::PowerEvent::Focus);
                    services.focus().await;
                }
                ClearLive => {
                    services
                        .telemetry
                        .track(UsageFeature::OutputClear, 1.)
                        .await;
                    services.orchestrator.clear_overlay().await;
                }
                CheckForUpdates => {
                    services.updater.check().await;
                }
                DownloadUpdate => {
                    services.updater.download().await;
                }
                InstallUpdate => {
                    if services.updater.install(false).await
                        == kairo_updater::InstallOutcome::NeedsConfirmation
                    {
                        let _ = services.ui.send(UiEvent::ConfirmInstall(services.clone()));
                    }
                }
                SetHouseTrackPaused(paused) => {
                    let tracks = services.tracks.clone();
                    let _ =
                        tokio::task::spawn_blocking(move || lock(&tracks).set_paused(paused)).await;
                }
                SendFeedback {
                    kind,
                    message,
                    context,
                } => {
                    let kind = match kind {
                        ui_shell::state::FeedbackKind::Idea => {
                            kairo_domain::cloud::FeedbackKind::Idea
                        }
                        ui_shell::state::FeedbackKind::Problem => {
                            kairo_domain::cloud::FeedbackKind::Problem
                        }
                        ui_shell::state::FeedbackKind::Praise => {
                            kairo_domain::cloud::FeedbackKind::Praise
                        }
                    };
                    let result = services
                        .cloud
                        .send_feedback(kind, message, Some(context.as_str().into()))
                        .await;
                    let _ = services.ui.send(UiEvent::Feedback(if result.ok {
                        Ok(())
                    } else {
                        Err(result
                            .error
                            .unwrap_or_else(|| "Something went wrong.".into()))
                    }));
                }
                OpenKairoFiles(paths) => {
                    for path in paths {
                        let opened = match services.transfer.preview_file(&path).await {
                            Ok(preview) => kairo_transfer::KairoOpened::Preview(Box::new(preview)),
                            Err(error) => kairo_transfer::KairoOpened::Error {
                                error: error.to_string(),
                                file_name: path
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .into_owned(),
                            },
                        };
                        services.core.publisher().emit(opened);
                    }
                }
                Import(kind) => {
                    services.core.publisher().emit(ImportRequested(kind));
                }
                // Electron's `render-process-gone`: a renderer crash for the
                // main window, which also counts `renderer_gone` in usage.
                // The panic text stays in the local log, not the report.
                FeaturePanicked { .. } => {
                    services
                        .telemetry
                        .record_crash(
                            kairo_cloud::telemetry::NativeCrash::Window,
                            kairo_cloud::telemetry::CrashReason::Panic,
                            0,
                        )
                        .await;
                }
                OpenExternal(_) | MarkWhatsNewSeen(_) => {}
            }
        });
    }
}

impl kairo_core::shutdown::ShutdownServices for ApplicationServices {
    async fn finish_capture(&self) -> Result<(), String> {
        if !self
            .capture_status
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .recording
        {
            return Ok(());
        }
        let mut status = self.core.watch::<kairo_domain::capture::CaptureStatus>();
        self.engine
            .command(kairo_engine_proto::Command::StopCapture);
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            while let Some(value) = status.next().await {
                if !value.recording {
                    return Ok(());
                }
            }
            Err("The recording engine stopped before finalizing the movie.".into())
        })
        .await
        .map_err(|_| "The recording did not finish before the shutdown deadline.".to_owned())?
    }
    async fn shutdown_audience(&self) -> Result<(), String> {
        self.audience.shutdown();
        Ok(())
    }
    async fn stop_quote_worker(&self) -> Result<(), String> {
        self.quote.close();
        Ok(())
    }
    async fn stop_ndi(&self) -> Result<(), String> {
        self.stt.stop();
        self.cloud.stop().await;
        Ok(())
    }
    async fn release_outputs(&self) -> Result<(), String> {
        self.core
            .call(kairo_output::actor::DestroyAll {})
            .await
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct NoKeychain;
    impl KeyProtector for NoKeychain {
        fn load(&self, _: &str) -> Result<Option<[u8; 32]>, kairo_store::secrets::SecretError> {
            Err(kairo_store::secrets::SecretError::Keychain)
        }
        fn save(&self, _: &str, _: &[u8; 32]) -> Result<(), kairo_store::secrets::SecretError> {
            Err(kairo_store::secrets::SecretError::Keychain)
        }
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires a desktop, the real kairo-engine binary and the reference bundled Bible"]
    async fn real_engine_startup() {
        let profile = tempfile::tempdir().unwrap();
        let paths = kairo_store::paths::compute(&kairo_store::paths::PathInputs {
            os: if cfg!(target_os = "macos") {
                kairo_store::paths::Os::MacOs
            } else if cfg!(windows) {
                kairo_store::paths::Os::Windows
            } else {
                kairo_store::paths::Os::Linux
            },
            home: profile.path().to_string_lossy().into_owned(),
            roaming_app_data: None,
            xdg_config_home: None,
            documents: Some(
                profile
                    .path()
                    .join("Documents")
                    .to_string_lossy()
                    .into_owned(),
            ),
            videos: None,
            temp: profile.path().join("tmp").to_string_lossy().into_owned(),
            exe: None,
            packaged: false,
            shared_profile: false,
        });
        let core = AppCore::new().unwrap();
        let engine = EngineHandle::new(kairo_core::engine::EngineOptions::new(
            std::env::var_os("KAIRO_ENGINE_BINARY")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/kairo-engine")
                }),
        ));
        let settings = Arc::new(Mutex::new(SettingsStore::open(
            &paths.user_data,
            &mut kairo_store::settings::LaunchContext {
                now_ms: kairo_store::settings::now_ms(),
                new_id: &mut kairo_store::settings::random_uuid,
            },
        )));
        let (bridge, events) = UiBridge::channel();
        let (opened, ready) = oneshot::channel();
        std::thread::spawn(move || {
            let mut opened = Some(opened);
            while let Ok(event) = events.recv() {
                match event {
                    UiEvent::OpenWindow { services, ready } => {
                        if let Some(opened) = opened.take() {
                            let _ = opened.send(services);
                        }
                        let _ = ready.send(Ok(()));
                    }
                    UiEvent::MainThread(_) => panic!("test did not request a native dialog"),
                    _ => {}
                }
            }
        });
        let started = std::time::Instant::now();
        let boot_core = core.clone();
        let boot_engine = engine.clone();
        core.run(async move {
            Startup::with_protector(
                boot_core,
                paths,
                settings,
                boot_engine,
                bridge,
                Arc::new(NoKeychain),
            )
            .unwrap()
            .start()
            .await
        })
        .await
        .unwrap()
        .unwrap();
        eprintln!(
            "067 local startup {} ms (no network awaited)",
            started.elapsed().as_millis()
        );
        let services = ready.await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(15), async {
            while engine.displays().is_empty() {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        let display = engine.displays()[0].clone();
        let configured = services.clone();
        core.run(async move {
            lock(&configured.settings_wiring).set("overlay",serde_json::json!({"outputs":[{"id":"wiring-check","name":"Application wiring","kind":"screen","enabled":true,"displayId":display.id,"displayLabel":display.label}]})).unwrap();
            configured.core.call(kairo_output::actor::Reconcile { outputs:None }).await.unwrap();
            let song=serde_json::from_value(serde_json::json!({"id":"wiring-check","title":"Application wiring verified","artist":"","sections":[{"type":"verse","label":"Verse 1","lines":["Application wiring verified"]}],"source":"text","isFavorite":false,"createdAt":1,"updatedAt":1})).unwrap();
            configured.lyrics.add_song(song).await.unwrap();
            configured.actions().push_lyrics("wiring-check",0.).await.unwrap();
        }).await.unwrap();
        eprintln!("067 real engine output ready");
        let hold = std::env::var("KAIRO_HOLD_OUTPUT")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(2);
        tokio::time::sleep(std::time::Duration::from_secs(hold)).await;
        core.shutdown(services).await.unwrap();
        engine.shutdown();
    }
}
