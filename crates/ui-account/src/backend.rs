//! What the account views ask of the app: the `account:*`, `onboarding:*`
//! and `settings:set` calls the Electron renderer makes over IPC.
//!
//! The app implements it over the cloud session, licensing and onboarding
//! actors; tests use a fake. Each call returns a future that does not need
//! any particular runtime, so views await it on GPUI's executor. A rejected
//! IPC call becomes `Err(message)`.

use kairo_domain::cloud::{
    CloudResult, DevicePairingState, LicenseSnapshot, SessionSnapshot, SignInInput, SignUpInput,
};
use kairo_domain::program::PresentationSettings;
use kairo_domain::settings::{ChurchProfile, OnboardingState, OnboardingStepId, OverlaySettings};
use std::future::Future;
use std::pin::Pin;

/// A pending answer.
pub type Reply<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// An `onboarding:*` write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnboardingWrite {
    /// `completeStep`.
    Complete(OnboardingStepId),
    /// `skipStep`.
    Skip(OnboardingStepId),
    /// `setCurrentStep`.
    SetCurrent(OnboardingStepId),
    /// `finish`.
    Finish,
}

/// The app's side of the account and setup views.
pub trait AccountBackend: Send + Sync + 'static {
    /// `account.signIn`.
    fn sign_in(&self, input: SignInInput) -> Reply<Result<CloudResult<SessionSnapshot>, String>>;
    /// `account.signUp`.
    fn sign_up(&self, input: SignUpInput) -> Reply<Result<CloudResult<SessionSnapshot>, String>>;
    /// `account.signOut`.
    fn sign_out(&self) -> Reply<Result<SessionSnapshot, String>>;
    /// `account.requestPasswordReset`.
    fn request_password_reset(&self, email: String) -> Reply<Result<(), String>>;
    /// `account.resendVerification`.
    fn resend_verification(&self) -> Reply<Result<CloudResult<()>, String>>;
    /// `account.verifyEmailCode`.
    fn verify_email_code(
        &self,
        code: String,
    ) -> Reply<Result<CloudResult<SessionSnapshot>, String>>;
    /// `account.startDevicePairing`.
    fn start_pairing(&self) -> Reply<Result<DevicePairingState, String>>;
    /// `account.cancelDevicePairing`.
    fn cancel_pairing(&self) -> Reply<Result<DevicePairingState, String>>;
    /// `account.openWeb`: a path on the website.
    fn open_web(&self, path: String) -> Reply<Result<(), String>>;
    /// `account.activateLicense`.
    fn activate_license(&self, key: String) -> Reply<Result<CloudResult<LicenseSnapshot>, String>>;
    /// `onboarding.*`.
    fn onboarding(&self, write: OnboardingWrite) -> Reply<Result<OnboardingState, String>>;
    /// `settings.set('church', …)`.
    fn save_church(&self, church: ChurchProfile) -> Reply<Result<(), String>>;
    /// `settings.set('overlay', …)`.
    fn save_overlay(&self, overlay: OverlaySettings) -> Reply<Result<(), String>>;
    /// `settings.set('presentation', …)`.
    fn save_presentation(&self, presentation: PresentationSettings) -> Reply<Result<(), String>>;
}
