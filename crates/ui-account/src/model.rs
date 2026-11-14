//! Account and setup state (\`useAccountStore\`, \`useLicenseStore\` and the
//! parts of \`useBootstrapStore\` setup reads): one entity per window, fed by
//! the app from the startup snapshot and the push subscriptions, and written
//! back through [\`AccountBackend\`].

use crate::backend::AccountBackend;
use gpui_kit::{App, Context, Entity, EventEmitter, Global};
use kairo_domain::cloud::{
    CloudSessionState, DevicePairingState, DevicePairingStatus, LicenseSnapshot, SessionSnapshot,
};
use kairo_domain::overlay::DisplayInfo;
use kairo_domain::settings::{AppSettings, OnboardingState};
use std::sync::Arc;

/// \`EMPTY_SESSION\`.
pub fn signed_out() -> SessionSnapshot {
    SessionSnapshot {
        state: CloudSessionState::SignedOut,
        user: None,
        org: None,
        orgs: Vec::new(),
        last_synced_at: None,
        credentials_persisted: None,
    }
}

/// \`IDLE_PAIRING\`.
pub fn idle_pairing() -> DevicePairingState {
    DevicePairingState {
        status: DevicePairingStatus::Idle,
        user_code: None,
        verification_uri: None,
        expires_at: None,
        message: None,
    }
}

/// Something in the account state changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountChanged {
    /// The session.
    Session,
    /// Browser pairing.
    Pairing,
    /// The licence.
    License,
    /// Setup progress.
    Onboarding,
    /// Settings setup reads.
    Settings,
    /// Connected displays.
    Displays,
}

/// The account state for a window.
pub struct Account {
    backend: Arc<dyn AccountBackend>,
    session: SessionSnapshot,
    pairing: DevicePairingState,
    license: Option<LicenseSnapshot>,
    onboarding: OnboardingState,
    settings: Option<AppSettings>,
    displays: Vec<DisplayInfo>,
    local_zone: String,
}

impl EventEmitter<AccountChanged> for Account {}

impl Account {
    /// Signed out, with nothing loaded. \`local_zone\` is this computer's IANA
    /// zone (\`LOCAL_ZONE\`).
    pub fn new(backend: Arc<dyn AccountBackend>, local_zone: impl Into<String>) -> Self {
        Self {
            backend,
            session: signed_out(),
            pairing: idle_pairing(),
            license: None,
            onboarding: OnboardingState::default(),
            settings: None,
            displays: Vec::new(),
            local_zone: local_zone.into(),
        }
    }

    /// The app's calls.
    pub fn backend(&self) -> Arc<dyn AccountBackend> {
        self.backend.clone()
    }
    /// The session.
    pub fn session(&self) -> &SessionSnapshot {
        &self.session
    }
    /// Browser pairing.
    pub fn pairing(&self) -> &DevicePairingState {
        &self.pairing
    }
    /// The licence, once known.
    pub fn license(&self) -> Option<&LicenseSnapshot> {
        self.license.as_ref()
    }
    /// Setup progress.
    pub fn onboarding(&self) -> &OnboardingState {
        &self.onboarding
    }
    /// Settings, once startup delivered them.
    pub fn settings(&self) -> Option<&AppSettings> {
        self.settings.as_ref()
    }
    /// Connected displays.
    pub fn displays(&self) -> &[DisplayInfo] {
        &self.displays
    }
    /// This computer's zone.
    pub fn local_zone(&self) -> &str {
        &self.local_zone
    }

    /// \`setSession\`.
    pub fn set_session(&mut self, session: SessionSnapshot, cx: &mut Context<Self>) {
        if self.session != session {
            self.session = session;
            cx.emit(AccountChanged::Session);
            cx.notify();
        }
    }
    /// \`setPairing\`.
    pub fn set_pairing(&mut self, pairing: DevicePairingState, cx: &mut Context<Self>) {
        if self.pairing != pairing {
            self.pairing = pairing;
            cx.emit(AccountChanged::Pairing);
            cx.notify();
        }
    }
    /// \`useLicenseStore.setSnapshot\`.
    pub fn set_license(&mut self, license: LicenseSnapshot, cx: &mut Context<Self>) {
        if self.license.as_ref() != Some(&license) {
            self.license = Some(license);
            cx.emit(AccountChanged::License);
            cx.notify();
        }
    }
    /// \`setOnboarding\`.
    pub fn set_onboarding(&mut self, state: OnboardingState, cx: &mut Context<Self>) {
        if self.onboarding != state {
            self.onboarding = state;
            cx.emit(AccountChanged::Onboarding);
            cx.notify();
        }
    }
    /// \`setSettings\`.
    pub fn set_settings(&mut self, settings: AppSettings, cx: &mut Context<Self>) {
        if self.settings.as_ref() != Some(&settings) {
            self.settings = Some(settings);
            cx.emit(AccountChanged::Settings);
            cx.notify();
        }
    }
    /// \`patchSettings\`, after a successful save.
    pub fn update_settings(&mut self, update: impl FnOnce(&mut AppSettings), cx: &mut Context<Self>) {
        if let Some(settings) = self.settings.as_mut() {
            update(settings);
            cx.emit(AccountChanged::Settings);
            cx.notify();
        }
    }
    /// \`displays.onChanged\`.
    pub fn set_displays(&mut self, displays: Vec<DisplayInfo>, cx: &mut Context<Self>) {
        if self.displays != displays {
            self.displays = displays;
            cx.emit(AccountChanged::Displays);
            cx.notify();
        }
    }
}

/// The window's account entity, for views that open later.
pub struct AccountHandle(pub Entity<Account>);
impl Global for AccountHandle {}

/// The account entity installed with [\`install\`].
pub fn account(cx: &App) -> Option<Entity<Account>> {
    cx.try_global::<AccountHandle>().map(|h| h.0.clone())
}

/// Makes \`account\` the app's account state.
pub fn install(account: Entity<Account>, cx: &mut App) {
    cx.set_global(AccountHandle(account));
}
