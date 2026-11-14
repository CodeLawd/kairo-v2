//! The account and setup views' backend over the services (`account:*`,
//! `onboarding:*` and `settings:set` in Electron), and the feed that keeps
//! the views' [`Account`] current.

use std::sync::Arc;

use kairo_cloud::onboarding::OnboardingCommand;
use kairo_domain::cloud::{
    CloudResult, DevicePairingState, LicenseSnapshot, SessionSnapshot, SignInInput, SignUpInput,
};
use kairo_domain::program::PresentationSettings;
use kairo_domain::settings::{
    ChurchProfile, OnboardingState, OverlaySettings, SettingsWithSecretsStatus,
};
use serde_json::Value;
use ui_account::{AccountBackend, OnboardingWrite, Reply};

use crate::services::{ApplicationServices, SettingsChanged, UiEvent};

pub struct ServicesAccount(pub Arc<ApplicationServices>);

/// Runs `work` on the service runtime and answers on any executor.
fn on_runtime<T, F>(services: &Arc<ApplicationServices>, work: F) -> Reply<Result<T, String>>
where
    T: Send + 'static,
    F: std::future::Future<Output = Result<T, String>> + Send + 'static,
{
    let (tx, rx) = tokio::sync::oneshot::channel();
    services.spawn(async move {
        let _ = tx.send(work.await);
    });
    Box::pin(async move {
        rx.await
            .unwrap_or_else(|_| Err("The request was interrupted.".into()))
    })
}

impl ServicesAccount {
    fn set_section(&self, key: &'static str, value: Value) -> Reply<Result<(), String>> {
        let services = self.0.clone();
        on_runtime(&self.0, async move {
            let core = services.core.clone();
            core.blocking(move || {
                services
                    .settings_wiring
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .set(key, value)
                    .map_err(|e| e.to_string())
            })
            .await
            .map_err(|e| e.to_string())?
        })
    }
}

impl AccountBackend for ServicesAccount {
    fn sign_in(&self, input: SignInInput) -> Reply<Result<CloudResult<SessionSnapshot>, String>> {
        let cloud = self.0.cloud.clone();
        on_runtime(&self.0, async move { Ok(cloud.sign_in(input).await) })
    }

    fn sign_up(&self, input: SignUpInput) -> Reply<Result<CloudResult<SessionSnapshot>, String>> {
        let cloud = self.0.cloud.clone();
        on_runtime(&self.0, async move { Ok(cloud.sign_up(input).await) })
    }

    fn sign_out(&self) -> Reply<Result<SessionSnapshot, String>> {
        let cloud = self.0.cloud.clone();
        on_runtime(&self.0, async move { Ok(cloud.sign_out().await) })
    }

    fn request_password_reset(&self, email: String) -> Reply<Result<(), String>> {
        let cloud = self.0.cloud.clone();
        on_runtime(&self.0, async move {
            let result = cloud.request_password_reset(email).await;
            match result.error {
                Some(error) if !result.ok => Err(error),
                _ => Ok(()),
            }
        })
    }

    fn resend_verification(&self) -> Reply<Result<CloudResult<()>, String>> {
        let cloud = self.0.cloud.clone();
        on_runtime(
            &self.0,
            async move { Ok(cloud.resend_verification().await) },
        )
    }

    fn verify_email_code(
        &self,
        code: String,
    ) -> Reply<Result<CloudResult<SessionSnapshot>, String>> {
        let cloud = self.0.cloud.clone();
        on_runtime(
            &self.0,
            async move { Ok(cloud.verify_email_code(code).await) },
        )
    }

    fn start_pairing(&self) -> Reply<Result<DevicePairingState, String>> {
        let cloud = self.0.cloud.clone();
        on_runtime(&self.0, async move { Ok(cloud.start_pairing().await) })
    }

    fn cancel_pairing(&self) -> Reply<Result<DevicePairingState, String>> {
        let cloud = self.0.cloud.clone();
        on_runtime(&self.0, async move { Ok(cloud.cancel_pairing().await) })
    }

    fn open_web(&self, path: String) -> Reply<Result<(), String>> {
        let cloud = self.0.cloud.clone();
        on_runtime(&self.0, async move {
            cloud.open_web(Some(&path)).await.map_err(|e| e.to_string())
        })
    }

    fn activate_license(&self, key: String) -> Reply<Result<CloudResult<LicenseSnapshot>, String>> {
        let licensing = self.0.licensing.clone();
        on_runtime(&self.0, async move {
            Ok(licensing.activate_form(&key, None).await)
        })
    }

    fn onboarding(&self, write: OnboardingWrite) -> Reply<Result<OnboardingState, String>> {
        let core = self.0.core.clone();
        on_runtime(&self.0, async move {
            let command = match write {
                OnboardingWrite::Complete(step) => OnboardingCommand::Complete(step),
                OnboardingWrite::Skip(step) => OnboardingCommand::Skip(step),
                OnboardingWrite::SetCurrent(step) => OnboardingCommand::SetCurrent(step),
                OnboardingWrite::Finish => OnboardingCommand::Finish,
            };
            core.call(command)
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())
        })
    }

    fn save_church(&self, church: ChurchProfile) -> Reply<Result<(), String>> {
        self.set_section(
            "church",
            serde_json::to_value(church).unwrap_or(Value::Null),
        )
    }

    fn save_overlay(&self, overlay: OverlaySettings) -> Reply<Result<(), String>> {
        self.set_section(
            "overlay",
            serde_json::to_value(overlay).unwrap_or(Value::Null),
        )
    }

    fn save_presentation(&self, presentation: PresentationSettings) -> Reply<Result<(), String>> {
        self.set_section(
            "presentation",
            serde_json::to_value(presentation).unwrap_or(Value::Null),
        )
    }
}

/// A change for the views' account model.
pub enum AccountUpdate {
    Session(SessionSnapshot),
    Pairing(DevicePairingState),
    License(Box<LicenseSnapshot>),
    Onboarding(OnboardingState),
    Settings(Box<SettingsWithSecretsStatus>),
    Displays(Vec<kairo_domain::overlay::DisplayInfo>),
}

/// Forwards every published snapshot the views read, current value first
/// (`account.onSessionChange`, `onPairingChange`, `onLicenseChange`,
/// `onboarding.onStateChange`, `settings.onChanged`).
pub fn feed(services: &Arc<ApplicationServices>) {
    fn forward<T, F>(services: &Arc<ApplicationServices>, wrap: F)
    where
        T: Send + Sync + 'static,
        F: Fn(&T) -> Option<AccountUpdate> + Send + 'static,
    {
        let mut watch = services.core.watch::<T>();
        let ui = services.ui.clone();
        services.spawn(async move {
            while let Some(value) = watch.next().await {
                if let Some(update) = wrap(&value)
                    && ui.send(UiEvent::Account(Box::new(update))).is_err()
                {
                    break;
                }
            }
        });
    }
    forward::<SessionSnapshot, _>(services, |v| Some(AccountUpdate::Session(v.clone())));
    forward::<DevicePairingState, _>(services, |v| Some(AccountUpdate::Pairing(v.clone())));
    forward::<LicenseSnapshot, _>(services, |v| {
        Some(AccountUpdate::License(Box::new(v.clone())))
    });
    forward::<OnboardingState, _>(services, |v| Some(AccountUpdate::Onboarding(v.clone())));
    forward::<SettingsChanged, _>(
        services,
        |SettingsChanged(value)| match serde_json::from_value::<SettingsWithSecretsStatus>(
            value.clone(),
        ) {
            Ok(settings) => Some(AccountUpdate::Settings(Box::new(settings))),
            Err(error) => {
                tracing::warn!(%error, "[Account] settings snapshot could not be read");
                None
            }
        },
    );
    // Seed what is not published until it changes.
    let services_for_seed = services.clone();
    services.spawn(async move {
        let services = services_for_seed;
        let license = services.licensing.get_status().await;
        let settings = services
            .settings_wiring
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_all();
        let displays = kairo_output::displays::list(&services.engine.displays());
        let pairing = services.cloud.pairing().await;
        let session = services.cloud.session().await;
        let mut updates = vec![
            AccountUpdate::Session(session),
            AccountUpdate::Pairing(pairing),
            AccountUpdate::License(Box::new(license)),
            AccountUpdate::Displays(displays),
        ];
        if let Ok(settings) = serde_json::from_value::<SettingsWithSecretsStatus>(settings) {
            updates.push(AccountUpdate::Settings(Box::new(settings)));
        }
        for update in updates {
            let _ = services.ui.send(UiEvent::Account(Box::new(update)));
        }
    });
}

/// This computer's IANA zone (`LOCAL_ZONE`), `UTC` when it cannot be read.
pub fn local_zone() -> String {
    iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".into())
}
