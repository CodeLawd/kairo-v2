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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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

/// `IDLE_CAPTURE_STATUS`.
impl Default for CaptureStatus {
    fn default() -> Self {
        Self {
            available: false,
            reason: None,
            recording: false,
            started_at: None,
            path: None,
            source_name: None,
            error: None,
            warning: None,
        }
    }
}

pub const CAPTURE_CODECS: &[(CaptureCodec, &str, &str)] = &[
    (
        CaptureCodec::H264,
        "H.264",
        "Plays everywhere. Good for sharing and uploading.",
    ),
    (
        CaptureCodec::Hevc,
        "HEVC (H.265)",
        "Same quality as H.264 in about half the space.",
    ),
    (
        CaptureCodec::ProRes422,
        "Apple ProRes 422",
        "For editing. Large files.",
    ),
    (
        CaptureCodec::ProRes4444,
        "Apple ProRes 4444",
        "For editing, with transparency — lyrics and lower thirds to key over video.",
    ),
];

pub const CAPTURE_RESOLUTIONS: &[(CaptureResolution, &str, u32)] = &[
    (CaptureResolution::P1080, "1080p (1920 wide)", 1920),
    (CaptureResolution::P720, "720p (1280 wide)", 1280),
];

pub const CAPTURE_FRAME_RATES: &[CaptureFrameRate] = CaptureFrameRate::ALL;
pub const CAPTURE_BITRATES: &[(u32, &str)] = &[
    (6, "Small — 6 Mbps"),
    (12, "Standard — 12 Mbps"),
    (20, "High — 20 Mbps"),
    (40, "Very high — 40 Mbps"),
];

pub fn normalize_capture_settings(raw: &serde_json::Value) -> CaptureSettings {
    use crate::normalize::{safe_enum, safe_string};
    let default = CaptureSettings::default();
    let rate = crate::js::value_to_number(raw.get("frameRate"));
    let bitrate = crate::js::value_to_number(raw.get("bitrateMbps"));
    CaptureSettings {
        source_output_id: safe_string(raw.get("sourceOutputId"), ""),
        folder: safe_string(raw.get("folder"), ""),
        codec: safe_enum(raw.get("codec"), default.codec),
        resolution: safe_enum(raw.get("resolution"), default.resolution),
        frame_rate: CAPTURE_FRAME_RATES
            .iter()
            .copied()
            .find(|f| f64::from(f.value()) == rate)
            .unwrap_or(default.frame_rate),
        bitrate_mbps: if bitrate.is_finite() {
            crate::js::math_round(bitrate).clamp(2.0, 100.0)
        } else {
            default.bitrate_mbps
        },
        audio_device_id: safe_string(raw.get("audioDeviceId"), ""),
        audio_device_name: safe_string(raw.get("audioDeviceName"), ""),
        extra: Extra::new(),
    }
}

pub fn gigabytes_per_hour(settings: &CaptureSettings) -> f64 {
    let mbps = match settings.codec {
        CaptureCodec::H264 | CaptureCodec::Hevc => {
            return settings.bitrate_mbps * 3600.0 / 8.0 / 1000.0;
        }
        CaptureCodec::ProRes422 => 147.0,
        CaptureCodec::ProRes4444 => 330.0,
    };
    let pixels = if settings.resolution == CaptureResolution::P720 {
        (1280.0 * 720.0) / (1920.0 * 1080.0)
    } else {
        1.0
    };
    mbps * pixels * (f64::from(settings.frame_rate.value()) / 30.0) * 3600.0 / 8.0 / 1000.0
}

pub fn capture_sources(
    outputs: &[crate::overlay::OverlayOutput],
) -> impl Iterator<Item = &crate::overlay::OverlayOutput> {
    outputs
        .iter()
        .filter(|output| crate::overlay_outputs::is_rendered_kind(output.kind))
}

pub fn capture_source_for<'a>(
    settings: &CaptureSettings,
    outputs: &'a [crate::overlay::OverlayOutput],
) -> Option<&'a crate::overlay::OverlayOutput> {
    use crate::overlay::OverlayOutputKind;
    capture_sources(outputs).min_by_key(|output| {
        if output.id == settings.source_output_id {
            0
        } else if output.enabled && output.kind == OverlayOutputKind::Screen {
            1
        } else if output.enabled {
            2
        } else {
            3
        }
    })
}

pub fn capture_frame_size(resolution: CaptureResolution, aspect: f64) -> (u32, u32) {
    let width = match resolution {
        CaptureResolution::P1080 => 1920,
        CaptureResolution::P720 => 1280,
    };
    let shape = if aspect > 0.2 && aspect < 5.0 {
        aspect
    } else {
        16.0 / 9.0
    };
    let height = (f64::from(width) / shape / 2.0).round() as u32 * 2;
    (width, height.max(2))
}

/// The timestamp is supplied as local calendar components.
pub fn capture_file_name(
    product: &str,
    source: &str,
    date: (i32, u32, u32),
    time: (u32, u32, u32),
) -> String {
    let (year, month, day) = date;
    let (hour, minute, second) = time;
    let stamp = format!("{year}-{month:02}-{day:02} {hour:02}.{minute:02}.{second:02}");
    let source: String = source
        .chars()
        .filter(|&c| c >= ' ' && !"\\/:*?\"<>|".contains(c))
        .collect();
    let name = [product, crate::js::trim(&source), &stamp]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    format!("{name}.mov")
}

pub fn format_capture_elapsed(ms: f64) -> String {
    if ms.is_nan() {
        return "NaN:NaN".into();
    }
    if ms == f64::INFINITY {
        return "Infinity:NaN:NaN".into();
    }
    crate::media::format_media_clock(ms / 1000.0)
}
