//! Video output devices (DeckLink and similar), from `src/lib/video-devices.ts`.

use crate::serde_util::{nullable, required, string_enum};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoMode {
    pub id: String,
    pub label: String,
    pub width: f64,
    pub height: f64,
    pub frame_rate_n: f64,
    pub frame_rate_d: f64,
    pub interlaced: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoPort {
    pub id: String,
    pub provider: String,
    pub device_name: String,
    pub port_name: String,
    pub modes: Vec<VideoMode>,
    pub supports_key_fill: bool,
    #[serde(deserialize_with = "required")]
    pub unavailable_reason: Option<String>,
}

string_enum!(
    pub enum DriverState {
        Current = "current",
        Compatibility = "compatibility",
        Unsupported = "unsupported",
        Missing = "missing",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VideoDriverStatus {
    pub state: DriverState,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VideoProviderStatus {
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driver: Option<VideoDriverStatus>,
}

/// `VideoProviderStatus & { name }`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NamedVideoProvider {
    pub name: String,
    #[serde(flatten)]
    pub status: VideoProviderStatus,
}

string_enum!(
    pub enum VideoTestPattern {
        Bars = "bars",
        Text = "text",
        Motion = "motion",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoPlaybackDiagnostics {
    pub completed: f64,
    pub late: f64,
    pub dropped: f64,
    pub repeated: f64,
    pub received: f64,
    pub output_fps: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoOutputDiagnostics {
    pub captured_frames: f64,
    pub capture_fps: f64,
    #[serde(deserialize_with = "required")]
    pub hardware: Option<VideoPlaybackDiagnostics>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoOutputState {
    pub ready: bool,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "nullable")]
    pub test_pattern: Option<Option<VideoTestPattern>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<VideoOutputDiagnostics>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VideoDeviceSnapshot {
    pub ports: Vec<VideoPort>,
    pub providers: Vec<NamedVideoProvider>,
    pub outputs: BTreeMap<String, VideoOutputState>,
}
