//! Service records (one per church service), from `src/lib/service-records.ts`.

use crate::scripture::{ScriptureSuggestion, SermonPlan};
use crate::serde_util::{Extra, nullable, present, required, string_enum};
use crate::transcript::TranscriptResult;
use serde::{Deserialize, Serialize};

string_enum!(
    pub enum NuggetOrigin {
        Automatic = "automatic",
        Manual = "manual",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceNugget {
    pub id: String,
    pub text: String,
    pub captured_at: f64,
    pub source_ids: Vec<String>,
    pub origin: NuggetOrigin,
    #[serde(flatten)]
    pub extra: Extra,
}

string_enum!(
    pub enum UploadStatus {
        Idle = "idle",
        Queued = "queued",
        Uploading = "uploading",
        Uploaded = "uploaded",
        Failed = "failed",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceUploadState {
    pub status: UploadStatus,
    #[serde(deserialize_with = "required")]
    pub sermon_id: Option<String>,
    pub attempts: f64,
    #[serde(deserialize_with = "required")]
    pub last_attempt_at: Option<f64>,
    #[serde(deserialize_with = "required")]
    pub error: Option<String>,
    pub retryable: bool,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `idleUpload()`.
impl Default for ServiceUploadState {
    fn default() -> Self {
        Self {
            status: UploadStatus::Idle,
            sermon_id: None,
            attempts: 0.0,
            last_attempt_at: None,
            error: None,
            retryable: true,
            extra: Extra::new(),
        }
    }
}

string_enum!(
    pub enum ServiceStatus {
        Live = "live",
        Ended = "ended",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceRecord {
    pub id: String,
    pub title: String,
    pub speaker: String,
    pub created_at: f64,
    #[serde(deserialize_with = "required")]
    pub ended_at: Option<f64>,
    pub status: ServiceStatus,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub paused_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "nullable")]
    pub paused_at: Option<Option<f64>>,
    pub transcript: Vec<TranscriptResult>,
    pub scriptures: Vec<ScriptureSuggestion>,
    pub notes: Vec<SermonPlan>,
    pub nuggets: Vec<ServiceNugget>,
    #[serde(deserialize_with = "required")]
    pub analysis_error: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub analyzed_source_ids: Option<Vec<String>>,
    pub upload: ServiceUploadState,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Persisted as `kairo-services.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceSnapshot {
    #[serde(deserialize_with = "required")]
    pub active_id: Option<String>,
    pub services: Vec<ServiceRecord>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `ServiceCommand`, tagged by `action`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum ServiceCommand {
    List,
    Start,
    #[serde(rename_all = "camelCase")]
    End {
        title: String,
        speaker: String,
        #[serde(deserialize_with = "required")]
        plan_id: Option<String>,
    },
    Discard,
    #[serde(rename_all = "camelCase")]
    Analyze {
        service_id: String,
    },
    #[serde(rename_all = "camelCase")]
    Nugget {
        text: String,
        source_ids: Vec<String>,
    },
    #[serde(rename_all = "camelCase")]
    RemoveNugget {
        service_id: String,
        nugget_id: String,
    },
    #[serde(rename_all = "camelCase")]
    Upload {
        service_id: String,
    },
}
