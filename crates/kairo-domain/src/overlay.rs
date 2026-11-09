//! Overlay themes and outputs, from `src/lib/ipc.ts` and `src/lib/displays.ts`.
//! Defaults and normalisation are prompt 013 (`overlay-defaults.ts`).

use crate::program::{ConfidenceLayout, OutputShowFilter};
use crate::serde_util::{Extra, nullable, present, required, string_enum};
use serde::{Deserialize, Serialize};

/// A width and height, as in `{ width: number; height: number }`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Size {
    pub width: f64,
    pub height: f64,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A box in % of the 1920×1080 frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayBox {
    pub x_pct: f64,
    pub y_pct: f64,
    pub width_pct: f64,
    pub height_pct: f64,
    #[serde(flatten)]
    pub extra: Extra,
}

string_enum!(
    pub enum OverlayTextAlign {
        Left = "left",
        Center = "center",
        Right = "right",
        Justify = "justify",
    }
);
string_enum!(
    pub enum OverlayVerticalAlign {
        Top = "top",
        Middle = "middle",
        Bottom = "bottom",
    }
);
string_enum!(
    pub enum OverlayTextDecoration {
        None = "none",
        Underline = "underline",
        LineThrough = "line-through",
    }
);
string_enum!(
    pub enum OverlayTextTransform {
        None = "none",
        Uppercase = "uppercase",
        Capitalize = "capitalize",
        Lowercase = "lowercase",
    }
);
string_enum!(
    pub enum OverlayOutlinePosition {
        Outside = "outside",
        Inside = "inside",
        Center = "center",
    }
);
string_enum!(
    pub enum OverlayOutlineStyle {
        Solid = "solid",
        Dashed = "dashed",
        Dotted = "dotted",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayTextShadow {
    pub enabled: bool,
    pub x_px: f64,
    pub y_px: f64,
    pub blur_px: f64,
    pub color: String,
    pub opacity: f64,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayTextOutline {
    pub enabled: bool,
    pub position: OverlayOutlinePosition,
    pub width_px: f64,
    pub style: OverlayOutlineStyle,
    pub color: String,
    pub opacity: f64,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Typography and effects for verse or reference text.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayTextStyle {
    pub font_family: String,
    pub font_size_px: f64,
    pub font_weight: f64,
    pub color: String,
    pub color_opacity: f64,
    pub line_height: f64,
    pub letter_spacing_px: f64,
    pub align: OverlayTextAlign,
    pub vertical_align: OverlayVerticalAlign,
    pub text_decoration: OverlayTextDecoration,
    pub text_transform: OverlayTextTransform,
    pub shadow: OverlayTextShadow,
    pub outline: OverlayTextOutline,
    #[serde(rename = "box")]
    pub frame: OverlayBox,
    #[serde(flatten)]
    pub extra: Extra,
}

string_enum!(
    pub enum ReferencePosition {
        Above = "above",
        Below = "below",
    }
);

/// `OverlayTextStyle & { show; position }`. Unknown keys land in `text.extra`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OverlayReferenceStyle {
    pub show: bool,
    pub position: ReferencePosition,
    #[serde(flatten)]
    pub text: OverlayTextStyle,
}

string_enum!(
    /// `shape` draws a preset from the shape catalog, named by `OverlayElement::shape`.
    pub enum OverlayElementKind {
        Rectangle = "rectangle",
        Ellipse = "ellipse",
        Shape = "shape",
        Image = "image",
        Video = "video",
    }
);
string_enum!(
    pub enum MediaFit {
        Cover = "cover",
        Contain = "contain",
        Fill = "fill",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayElementStroke {
    pub enabled: bool,
    pub color: String,
    pub opacity: f64,
    pub width_px: f64,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A shape, image or video layer on the slide.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayElement {
    pub id: String,
    pub kind: OverlayElementKind,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub shape: Option<String>,
    #[serde(rename = "box")]
    pub frame: OverlayBox,
    pub above_text: bool,
    pub opacity: f64,
    pub rotation_deg: f64,
    pub flip_x: bool,
    pub flip_y: bool,
    pub fill: String,
    pub fill_opacity: f64,
    pub stroke: OverlayElementStroke,
    pub radius_px: f64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub media_path: Option<String>,
    pub media_fit: MediaFit,
    pub media_loop: bool,
    pub hue: f64,
    pub saturation: f64,
    pub brightness: f64,
    pub contrast: f64,
    pub blur_px: f64,
    #[serde(flatten)]
    pub extra: Extra,
}

string_enum!(
    pub enum OverlayBackgroundType {
        Color = "color",
        Gradient = "gradient",
        Transparent = "transparent",
        Image = "image",
        Video = "video",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayBackground {
    #[serde(rename = "type")]
    pub kind: OverlayBackgroundType,
    pub color: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub color2: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub angle_deg: Option<f64>,
    pub opacity: f64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub media_path: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub media_fit: Option<MediaFit>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub media_loop: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub media_start_seconds: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "nullable")]
    pub media_end_seconds: Option<Option<f64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub media_rate: Option<f64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub media_cue: Option<f64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub hue: Option<f64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub saturation: Option<f64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub brightness: Option<f64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub contrast: Option<f64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub blur_px: Option<f64>,
    #[serde(flatten)]
    pub extra: Extra,
}

string_enum!(
    pub enum OverlayLayoutPosition {
        LowerThird = "lower-third",
        Center = "center",
        Top = "top",
        Full = "full",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayLayout {
    pub position: OverlayLayoutPosition,
    pub max_width_pct: f64,
    pub padding_px: f64,
    pub backdrop_box: bool,
    pub backdrop_color: String,
    /// Retired: always 0.
    pub backdrop_radius_px: f64,
    pub auto_fit_text: bool,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OverlayTheme {
    pub elements: Vec<OverlayElement>,
    pub background: OverlayBackground,
    pub verse: OverlayTextStyle,
    pub reference: OverlayReferenceStyle,
    pub layout: OverlayLayout,
    #[serde(flatten)]
    pub extra: Extra,
}

string_enum!(
    /// `syphon` is macOS only.
    pub enum OverlayOutputKind {
        Ndi = "ndi",
        Screen = "screen",
        Device = "device",
        Syphon = "syphon",
    }
);
string_enum!(
    pub enum OverlayContentKind {
        Scripture = "scripture",
        Lyrics = "lyrics",
    }
);

/// A per-content-kind override on one output.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayOutputVariant {
    #[serde(deserialize_with = "required")]
    pub theme_id: Option<String>,
    pub theme: OverlayTheme,
    pub template: String,
    #[serde(flatten)]
    pub extra: Extra,
}

string_enum!(
    /// `displays.ts` `OutputAspect`.
    pub enum OutputAspect {
        Letterbox = "letterbox",
        Ratio16x10 = "16:10",
        Ratio4x3 = "4:3",
        Ratio21x9 = "21:9",
        Ratio32x9 = "32:9",
        Ratio9x16 = "9:16",
        Ratio1x1 = "1:1",
        Fill = "fill",
        Custom = "custom",
    }
);
string_enum!(
    pub enum OutputSource {
        Program = "program",
        Playlist = "playlist",
    }
);

/// One destination of a push.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayOutput {
    pub id: String,
    pub name: String,
    pub kind: OverlayOutputKind,
    pub enabled: bool,
    pub order: f64,
    pub fallback_only: bool,
    #[serde(deserialize_with = "required")]
    pub theme_id: Option<String>,
    pub theme: OverlayTheme,
    pub template: String,
    #[serde(deserialize_with = "required")]
    pub lyrics: Option<OverlayOutputVariant>,
    #[serde(deserialize_with = "required")]
    pub display_id: Option<f64>,
    pub display_label: String,
    pub device_port_id: String,
    pub device_port_label: String,
    pub device_mode_id: String,
    pub device_key_fill: bool,
    #[serde(deserialize_with = "required")]
    pub display_size: Option<Size>,
    pub aspect: OutputAspect,
    pub custom_aspect: Size,
    pub show: OutputShowFilter,
    pub source: OutputSource,
    pub playlist_id: String,
    pub slide_sec: f64,
    pub confidence: ConfidenceLayout,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A saved theme in `themeLibrary`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomOverlayTheme {
    pub id: String,
    pub name: String,
    pub kind: OverlayContentKind,
    pub created_at: f64,
    pub updated_at: f64,
    pub theme: OverlayTheme,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub baseline: Option<OverlayTheme>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Per-output result of one fan-out push.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayDispatchResult {
    pub output_id: String,
    pub name: String,
    pub kind: OverlayOutputKind,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// `ipc.ts` `DisplayInfo`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayInfo {
    pub id: f64,
    pub label: String,
    pub bounds: DisplayBounds,
    pub size: Size,
    pub scale_factor: f64,
    pub primary: bool,
    pub hosts_main_window: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DisplayBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// `ipc.ts` `TestPatternInput`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestPatternInput {
    pub display_id: f64,
    pub name: String,
    pub aspect: OutputAspect,
    pub custom_aspect: Size,
}
