//! Runtime status pushed to the UI: audio, bootstrap, NDI, orchestrator,
//! resilience and updates, from `src/lib/ipc.ts`.

use crate::serde_util::{number_enum, required, string_enum};
use serde::{Deserialize, Serialize};

string_enum!(
    pub enum SttProvider {
        Deepgram = "deepgram",
        Whisper = "whisper",
        None = "none",
    }
);
string_enum!(
    pub enum LlmProvider {
        Anthropic = "anthropic",
        Deepseek = "deepseek",
    }
);
number_enum!(
    /// Seconds between a detection and the verse going live.
    pub enum AutoPresentDelaySec {
        Zero = 0,
        One = 1,
        Two = 2,
        Three = 3,
    }
);

string_enum!(
    pub enum AudioDeviceKind {
        AudioInput = "audioinput",
        AudioOutput = "audiooutput",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevice {
    pub id: String,
    pub label: String,
    pub kind: AudioDeviceKind,
    pub is_default: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AudioLevel {
    pub rms: f64,
    pub peak: f64,
    pub clipping: bool,
    pub timestamp: f64,
}

string_enum!(
    pub enum AudioErrorCode {
        DeviceNotFound = "DEVICE_NOT_FOUND",
        PermissionDenied = "PERMISSION_DENIED",
        CaptureFailed = "CAPTURE_FAILED",
        Unknown = "UNKNOWN",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AudioError {
    pub code: AudioErrorCode,
    pub message: String,
}

string_enum!(
    pub enum BootstrapResource {
        Settings = "settings",
        Orchestrator = "orchestrator",
        Transcription = "transcription",
        Translations = "translations",
        SermonPlans = "sermonPlans",
        LivePlan = "livePlan",
        Lyrics = "lyrics",
        Onboarding = "onboarding",
        Account = "account",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BootstrapResourceError {
    pub resource: BootstrapResource,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BootstrapProgress {
    pub completed: f64,
    pub total: f64,
    pub step: String,
}

/// Everything the UI needs for its first paint.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBootstrapSnapshot {
    #[serde(deserialize_with = "required")]
    pub settings: Option<crate::settings::SettingsWithSecretsStatus>,
    #[serde(deserialize_with = "required")]
    pub orchestrator: Option<OrchestratorStatus>,
    pub transcription: Vec<crate::transcript::TranscriptResult>,
    pub translations: Vec<crate::scripture::ScriptureTranslationOption>,
    pub sermon_plans: Vec<crate::scripture::SermonPlan>,
    #[serde(deserialize_with = "required")]
    pub live_plan: Option<crate::scripture::LivePlanState>,
    pub lyrics: Vec<crate::lyrics::LyricsSong>,
    pub onboarding: crate::settings::OnboardingState,
    pub account: crate::cloud::SessionSnapshot,
    pub errors: Vec<BootstrapResourceError>,
    pub completed_at: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NdiOutputStatus {
    pub id: String,
    pub ready: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NdiStatus {
    pub available: bool,
    pub sending: bool,
    pub outputs: Vec<NdiOutputStatus>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchestratorConfig {
    pub audio_device_id: String,
    pub stt_provider: SttProvider,
    pub stt_api_key: String,
    pub stt_language: String,
    pub llm_provider: LlmProvider,
    pub llm_api_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scripture_model: Option<String>,
    pub scripture_translation: String,
    pub auto_mode: bool,
    pub confidence_threshold: f64,
    pub auto_present_delay_sec: f64,
    /// Detected passages (ranges) auto-present their first verse too.
    #[serde(default)]
    pub auto_present_passages: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStats {
    pub session_id: String,
    pub started_at: f64,
    pub duration_ms: f64,
    pub total_words: f64,
    pub total_detections: f64,
    pub total_presentations: f64,
    pub detector_calls: f64,
    pub avg_detector_latency_ms: f64,
    pub estimated_cost_usd: f64,
}

string_enum!(
    pub enum ServiceName {
        Audio = "audio",
        Stt = "stt",
        Detector = "detector",
        Output = "output",
    }
);
string_enum!(
    pub enum HealthStatus {
        Ok = "ok",
        Error = "error",
        Degraded = "degraded",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceHealth {
    pub service: ServiceName,
    pub status: HealthStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    pub last_updated: f64,
}

string_enum!(
    pub enum OverallHealth {
        AllGood = "ALL_GOOD",
        Degraded = "DEGRADED",
        Critical = "CRITICAL",
    }
);
string_enum!(
    pub enum DetectorFallbackKind {
        Offline = "offline",
        Provider = "provider",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DetectorFallbackReason {
    pub kind: DetectorFallbackKind,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResilienceStatus {
    pub overall_health: OverallHealth,
    pub internet_connected: bool,
    pub claude_fallback_active: bool,
    #[serde(deserialize_with = "required")]
    pub detector_fallback_reason: Option<DetectorFallbackReason>,
    pub recovery_session_available: bool,
    pub health: Vec<ServiceHealth>,
}

/// The speech-recognition health message while the upload is struggling.
/// AI detection pauses and the transcript warns while it stands.
pub const WEAK_CONNECTION: &str = "Weak connection — captions may lag";

/// Whether `status` reports a weak connection for speech recognition.
pub fn stt_connection_weak(health: &[ServiceHealth]) -> bool {
    health
        .iter()
        .any(|h| h.service == ServiceName::Stt && h.last_error.as_deref() == Some(WEAK_CONNECTION))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchestratorStatus {
    pub running: bool,
    pub auto_mode: bool,
    pub health: Vec<ServiceHealth>,
    pub total_presentations: f64,
}

/// `UpdateStatus`, tagged by `state`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum UpdateStatus {
    #[serde(rename_all = "camelCase")]
    Idle {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        current_version: Option<String>,
    },
    Checking,
    Available {
        version: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        notes: Option<String>,
    },
    Downloading {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        notes: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        percent: Option<f64>,
    },
    Downloaded {
        version: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        notes: Option<String>,
    },
    Error {
        message: String,
    },
}
