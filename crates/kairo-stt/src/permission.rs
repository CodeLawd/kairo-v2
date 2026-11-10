//! Microphone permission. From `microphoneAccess` in
//! `src/main/services/capture/index.ts`.
//!
//! On macOS the system asks once, with the Info.plist purpose string
//! [`MICROPHONE_PURPOSE`] (set by packaging, prompt 101). Denied and
//! restricted are final until the operator changes System Settings. Other
//! platforms have no app-level prompt and report granted.

/// `NSMicrophoneUsageDescription`, unchanged from the Electron build.
pub const MICROPHONE_PURPOSE: &str =
    "Kairo listens to the service audio so it can transcribe the sermon in real time.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MicrophoneStatus {
    Granted,
    Denied,
    Restricted,
    NotDetermined,
}

/// The current status, without prompting.
pub fn microphone_status() -> MicrophoneStatus {
    platform::status()
}

/// `true` when capture may start. Prompts only when the status is not yet
/// determined; a denial or restriction returns `false` without a prompt.
pub async fn ensure_microphone_access() -> bool {
    match microphone_status() {
        MicrophoneStatus::Granted => true,
        MicrophoneStatus::Denied | MicrophoneStatus::Restricted => false,
        MicrophoneStatus::NotDetermined => platform::request().await,
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::MicrophoneStatus;
    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};

    pub fn status() -> MicrophoneStatus {
        // SAFETY: AVMediaTypeAudio is a framework constant.
        let Some(audio) = (unsafe { AVMediaTypeAudio }) else {
            return MicrophoneStatus::Denied;
        };
        // SAFETY: a class method taking a valid media type.
        match unsafe { AVCaptureDevice::authorizationStatusForMediaType(audio) } {
            AVAuthorizationStatus::Authorized => MicrophoneStatus::Granted,
            AVAuthorizationStatus::Denied => MicrophoneStatus::Denied,
            AVAuthorizationStatus::Restricted => MicrophoneStatus::Restricted,
            _ => MicrophoneStatus::NotDetermined,
        }
    }

    pub async fn request() -> bool {
        // SAFETY: as above.
        let Some(audio) = (unsafe { AVMediaTypeAudio }) else {
            return false;
        };
        let (tx, rx) = tokio::sync::oneshot::channel();
        let tx = std::sync::Mutex::new(Some(tx));
        let block = RcBlock::new(move |granted: Bool| {
            if let Some(tx) = tx.lock().ok().and_then(|mut t| t.take()) {
                let _ = tx.send(granted.as_bool());
            }
        });
        // SAFETY: the block is retained by AVFoundation until it runs once.
        unsafe { AVCaptureDevice::requestAccessForMediaType_completionHandler(audio, &block) };
        rx.await.unwrap_or(false)
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::MicrophoneStatus;

    pub fn status() -> MicrophoneStatus {
        MicrophoneStatus::Granted
    }

    pub async fn request() -> bool {
        true
    }
}
