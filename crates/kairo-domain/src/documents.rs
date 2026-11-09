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
