//! Per-profile onboarding and the one-time migration for existing installs.

use std::{
    num::NonZeroUsize,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use kairo_core::{AppCore, CoreError, Handle, Message, Publisher};
use kairo_domain::{
    cloud::{
        complete_step, finish_onboarding, normalize_onboarding_state, reset_onboarding,
        seed_onboarding_from_settings, set_current_step, skip_step,
    },
    overlay_defaults::normalize_overlay_settings,
    settings::{AppSettings, OnboardingState, OnboardingStepId},
};
use kairo_store::{
    json_store::{JsonStore, OpenState},
    settings::{MigrationFlag, SettingsStore, now_ms, open_store, store_spec},
};
use serde_json::Value;

const STATE_KEY: &str = "onboarding";

#[derive(Debug)]
pub enum OnboardingError {
    Runtime(CoreError),
    Persistence(String),
}

impl From<CoreError> for OnboardingError {
    fn from(error: CoreError) -> Self {
        Self::Runtime(error)
    }
}

impl std::fmt::Display for OnboardingError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Runtime(error) => error.fmt(formatter),
            Self::Persistence(error) => write!(formatter, "Could not save onboarding: {error}"),
        }
    }
}

impl std::error::Error for OnboardingError {}

/// The actor keeps the onboarding file separate from application preferences.
pub struct Onboarding {
    store: JsonStore,
    topics: Publisher,
}

impl Onboarding {
    /// Run before the cloud session clears legacy provider settings, as at
    /// Electron startup. Migration reads presence only and never saves keys.
    pub async fn register(
        core: &AppCore,
        user_data: PathBuf,
        settings: Arc<Mutex<SettingsStore>>,
    ) -> Result<(), OnboardingError> {
        let topics = core.publisher();
        let actor = core
            .blocking(move || Self::open(user_data, settings, topics))
            .await??;
        core.register(
            actor,
            NonZeroUsize::new(16).expect("nonzero mailbox capacity"),
        )?;
        core.call(PublishInitial).await?;
        Ok(())
    }

    fn open(
        user_data: PathBuf,
        settings: Arc<Mutex<SettingsStore>>,
        topics: Publisher,
    ) -> Result<Self, OnboardingError> {
        let mut settings = settings.lock().map_err(|_| CoreError::TaskFailed)?;
        let spec = store_spec("kairo-onboarding").expect("onboarding store is registered");
        let mut actor = Self {
            store: open_store(&user_data, spec),
            topics,
        };
        let flag = MigrationFlag::CloudOnboarding;
        if !settings.migration_done(flag) {
            let seeded = seed_onboarding_from_settings(&seed_input(&settings), now_ms());
            actor.save(&seeded)?;
            // A restart must see the wizard state before it sees the migration flag.
            actor.store.flush().map_err(OnboardingError::Persistence)?;
            settings.mark_migration_done(flag);
            settings.flush().map_err(OnboardingError::Persistence)?;
        }
        Ok(actor)
    }

    fn state(&self) -> OnboardingState {
        normalize_onboarding_state(self.store.get(STATE_KEY).unwrap_or(&Value::Null))
    }

    fn save(&mut self, state: &OnboardingState) -> Result<(), OnboardingError> {
        if let OpenState::Unreadable(message) = self.store.open_state() {
            return Err(OnboardingError::Persistence(message.clone()));
        }
        self.store
            .set_as(STATE_KEY, state)
            .map_err(|error| OnboardingError::Persistence(error.to_string()))
    }
}

fn seed_input(settings: &SettingsStore) -> AppSettings {
    // An unrelated malformed section must not suppress migration of valid
    // output, church and retired provider-presence information.
    let raw = settings.data();
    let mut input = AppSettings::defaults(normalize_overlay_settings(raw.get("overlay")));
    let field = |section: &str, key: &str| {
        raw.get(section)
            .and_then(|section| section.get(key))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    input.church.name = field("church", "name");
    input.stt.api_key = field("stt", "apiKey");
    input.stt.bible_api_key = field("stt", "bibleApiKey");
    if let Some(configured) = raw.get("secretsConfigured") {
        input
            .extra
            .insert("secretsConfigured".into(), configured.clone());
    }
    input
}

/// Local bootstrap and account views can query the current healed state.
pub struct GetOnboarding;
impl Message for GetOnboarding {
    type Actor = Onboarding;
}
impl Handle<GetOnboarding> for Onboarding {
    type Reply = OnboardingState;

    async fn handle(&mut self, _: GetOnboarding) -> Self::Reply {
        self.state()
    }
}

struct PublishInitial;
impl Message for PublishInitial {
    type Actor = Onboarding;
}
impl Handle<PublishInitial> for Onboarding {
    type Reply = ();

    async fn handle(&mut self, _: PublishInitial) {
        self.topics.publish(self.state());
    }
}

pub enum OnboardingCommand {
    Complete(OnboardingStepId),
    Skip(OnboardingStepId),
    SetCurrent(OnboardingStepId),
    Finish,
    Reset,
}

impl Message for OnboardingCommand {
    type Actor = Onboarding;
}
impl Handle<OnboardingCommand> for Onboarding {
    type Reply = Result<OnboardingState, OnboardingError>;

    async fn handle(&mut self, command: OnboardingCommand) -> Self::Reply {
        let current = self.state();
        let next = match command {
            OnboardingCommand::Complete(step) => complete_step(current, step),
            OnboardingCommand::Skip(step) => skip_step(current, step),
            OnboardingCommand::SetCurrent(step) => set_current_step(current, step),
            OnboardingCommand::Finish => finish_onboarding(current, now_ms()),
            OnboardingCommand::Reset => reset_onboarding(&current),
        };
        self.save(&next)?;
        self.topics.publish(next.clone());
        Ok(next)
    }
}
