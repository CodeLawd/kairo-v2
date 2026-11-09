//! Program recording settings and status, from `src/lib/capture.ts`.

use crate::serde_util::{Extra, number_enum, required, string_enum};
use serde::{Deserialize, Serialize};

string_enum!(
    pub enum CaptureCodec {
        H264 = "h264",
        Hevc = "hevc",
        ProRes422 = "prores422",
        ProRes4444 = "prores4444",
    }
);
string_enum!(
    pub enum CaptureResolution {
        P1080 = "1080p",
        P720 = "720p",
    }
);
number_enum!(
    pub enum CaptureFrameRate {
        Fps24 = 24,
        Fps25 = 25,
        Fps30 = 30,
        Fps50 = 50,
        Fps60 = 60,
    }
);

/// `AppSettings.capture`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureSettings {
    pub source_output_id: String,
    pub folder: String,
    pub codec: CaptureCodec,
    pub resolution: CaptureResolution,
    pub frame_rate: CaptureFrameRate,
    pub bitrate_mbps: f64,
    pub audio_device_id: String,
    pub audio_device_name: String,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `DEFAULT_CAPTURE_SETTINGS`.
impl Default for CaptureSettings {
    fn default() -> Self {
        Self {
            source_output_id: String::new(),
            folder: String::new(),
            codec: CaptureCodec::H264,
            resolution: CaptureResolution::P1080,
            frame_rate: CaptureFrameRate::Fps30,
            bitrate_mbps: 12.0,
            audio_device_id: String::new(),
            audio_device_name: String::new(),
            extra: Extra::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CaptureAudioInput {
    pub id: String,
    pub name: String,
}

/// `Default` is `IDLE_CAPTURE_STATUS`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureStatus {
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub recording: bool,
    #[serde(deserialize_with = "required")]
    pub started_at: Option<f64>,
    #[serde(deserialize_with = "required")]
    pub path: Option<String>,
    #[serde(deserialize_with = "required")]
    pub source_name: Option<String>,
    #[serde(deserialize_with = "required")]
    pub error: Option<String>,
    #[serde(deserialize_with = "required")]
    pub warning: Option<String>,
}
