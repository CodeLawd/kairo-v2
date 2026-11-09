//! Transcription results, from `src/lib/ipc.ts`.

use crate::serde_util::{Extra, True, present};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TranscriptWord {
    pub word: String,
    pub start: f64,
    pub end: f64,
    pub confidence: f64,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A final transcript segment. Persisted inside service records.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptResult {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub audio_epoch: Option<f64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub audio_start: Option<f64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub audio_duration: Option<f64>,
    pub id: String,
    pub text: String,
    pub words: Vec<TranscriptWord>,
    pub is_final: True,
    pub timestamp: f64,
    pub duration: f64,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterimResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_epoch: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_start: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_duration: Option<f64>,
    pub text: String,
    pub stability: f64,
    pub timestamp: f64,
}
