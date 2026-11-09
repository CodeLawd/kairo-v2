//! Projected documents, from `src/lib/documents.ts`.

use crate::serde_util::{Extra, True, required, string_enum};
use serde::{Deserialize, Serialize};

string_enum!(
    pub enum DocumentFormat {
        Pdf = "pdf",
        Ppt = "ppt",
        Pptx = "pptx",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocumentBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocumentVideo {
    pub file: String,
    #[serde(rename = "box")]
    pub frame: DocumentBox,
    pub start: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionDocument {
    pub id: String,
    pub name: String,
    pub format: DocumentFormat,
    pub pages: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slide_size: Option<crate::overlay::Size>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub videos: Option<Vec<Vec<DocumentVideo>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocumentSlide {
    pub id: String,
    pub page: f64,
    pub path: String,
    pub width: f64,
    pub height: f64,
    pub videos: Vec<DocumentVideo>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentVideoStatus {
    pub current_time: f64,
    pub duration: f64,
    pub paused: bool,
    pub ended: bool,
    pub visible: bool,
    #[serde(deserialize_with = "required")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocumentPlaybackStatus {
    pub id: String,
    pub page: f64,
    pub videos: Vec<DocumentVideoStatus>,
}

/// `DocumentVideoCommand`, tagged by `action`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum DocumentVideoCommand {
    Pause,
    Play,
    Restart,
    Seek { seconds: f64 },
}

string_enum!(
    pub enum PowerpointConverterId {
        PowerPoint = "powerpoint",
        Wps = "wps",
        Keynote = "keynote",
        LibreOffice = "libreoffice",
        Builtin = "builtin",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentsCapabilities {
    pub converters: Vec<PowerpointConverterId>,
    pub can_convert_power_point: bool,
    pub libre_office: bool,
}

/// `AppSettings.documents`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentsSettings {
    pub slideshow_sec: f64,
    pub slideshow_loop: bool,
    pub slideshow_auto_start: bool,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `DEFAULT_DOCUMENTS_SETTINGS`.
impl Default for DocumentsSettings {
    fn default() -> Self {
        Self {
            slideshow_sec: 10.0,
            slideshow_loop: true,
            slideshow_auto_start: false,
            extra: Extra::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentImportInfo {
    pub name: String,
    pub format: DocumentFormat,
    pub size_bytes: f64,
    pub max_size_bytes: f64,
}

string_enum!(
    pub enum SlideConverterState {
        Unsupported = "unsupported",
        Missing = "missing",
        Downloading = "downloading",
        Installing = "installing",
        Ready = "ready",
        Error = "error",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlideConverterStatus {
    pub state: SlideConverterState,
    pub version: String,
    pub size_bytes: f64,
    pub received_bytes: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(deserialize_with = "required")]
    pub other_converter: Option<PowerpointConverterId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentNeedsConverter {
    pub needs_converter: True,
    pub token: String,
    pub name: String,
    pub format: DocumentFormat,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DocumentPrepareOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub simplified: Option<bool>,
}

/// `DocumentPrepared`. `data` is a `Uint8Array` over IPC; as JSON it is an
/// array of bytes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocumentPrepared {
    pub id: String,
    pub data: Vec<u8>,
    pub name: String,
    pub format: DocumentFormat,
}

pub const SLIDESHOW_MIN_SEC: f64 = 2.0;
pub const SLIDESHOW_MAX_SEC: f64 = 3600.0;
pub const SIMPLIFIED_IMPORT_WARNING: &str = "Imported with Kairo’s simplified renderer, so some pictures, fonts and effects may be missing. Download the slide converter in Settings → Documents, then import this file again for an exact copy.";
pub const POWERPOINT_NEEDS_CONVERTER: &str = "This older .ppt file needs PowerPoint, WPS, Keynote, or LibreOffice. Save it as .pptx or export a PDF and import that instead.";
pub const POWERPOINT_NEEDS_LIBREOFFICE: &str = POWERPOINT_NEEDS_CONVERTER;

pub fn normalize_documents_settings(raw: &serde_json::Value) -> DocumentsSettings {
    let default = DocumentsSettings::default();
    let seconds = crate::js::value_to_number(raw.get("slideshowSec"));
    DocumentsSettings {
        slideshow_sec: if seconds.is_finite() {
            crate::js::math_round(seconds).clamp(SLIDESHOW_MIN_SEC, SLIDESHOW_MAX_SEC)
        } else {
            default.slideshow_sec
        },
        slideshow_loop: raw["slideshowLoop"]
            .as_bool()
            .unwrap_or(default.slideshow_loop),
        slideshow_auto_start: raw["slideshowAutoStart"]
            .as_bool()
            .unwrap_or(default.slideshow_auto_start),
        extra: Extra::new(),
    }
}

pub fn format_document_size(bytes: f64) -> String {
    let mb = bytes / (1024.0 * 1024.0);
    if !mb.is_finite() || mb.abs() >= 1e21 {
        return format!("{} MB", crate::js::number_to_string(mb));
    }
    if mb == 0.0 {
        return "0.0 MB".into();
    }
    // toFixed rounds exact halfway cases away from zero. At one decimal
    // place, only odd multiples of a quarter can be exact binary halfway values.
    let quarters = mb.abs() * 4.0;
    if quarters % 2.0 == 1.0 {
        let tenths = (quarters * 2.5).round() as u64;
        let sign = if mb < 0.0 { "-" } else { "" };
        return format!("{sign}{}.{} MB", tenths / 10, tenths % 10);
    }
    format!("{mb:.1} MB")
}

pub fn valid_document_page(page: f64, count: f64) -> bool {
    page.fract() == 0.0 && page >= 0.0 && page < count
}

pub fn normalize_document_name(name: &str) -> Result<String, &'static str> {
    let clean: String = name.chars().filter(|&c| c >= ' ').collect();
    let name = clean
        .split(crate::js::is_whitespace)
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if name.is_empty() {
        return Err("Enter a name for this document.");
    }
    if name.encode_utf16().count() > 120 {
        return Err("Document names can be up to 120 characters.");
    }
    if name.contains(['/', '\\']) {
        return Err("Document names cannot include slashes.");
    }
    Ok(name)
}

pub fn can_convert_power_point(capabilities: Option<&DocumentsCapabilities>) -> bool {
    capabilities.is_some_and(|value| {
        value.can_convert_power_point || !value.converters.is_empty() || value.libre_office
    })
}

pub fn documents_error_message(raw: &str) -> &str {
    static WRAPPER: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)^Error invoking remote method '[^']+':\s*(?:Error:\s*)?").unwrap()
    });
    WRAPPER
        .find(raw)
        .map_or(raw, |wrapper| &raw[wrapper.end()..])
}
