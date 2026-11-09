//! Overlay defaults and the overlay settings normaliser, from
//! `src/lib/overlay-defaults.ts`.
//!
//! `normalize_overlay_settings` is the single place that heals any stored
//! `overlay` shape (missing keys, wrong types, out-of-range numbers, bad
//! colours, retired fields) into something safe to render and persist.
//! Normalisers take the raw JSON (`None` for an absent key) and return the
//! canonical type with no unknown keys, as the TypeScript builds fresh
//! objects. Running one twice gives the same result as running it once.

use crate::js::{self, math_round};
use crate::normalize::{
    as_object, clamp_num, non_neg_number, safe_bool, safe_color, safe_enum, safe_enum_in,
    safe_string,
};
use crate::overlay::{
    MediaFit, OutputAspect, OutputSource, OverlayBackground, OverlayBackgroundType, OverlayBox,
    OverlayLayout, OverlayLayoutPosition, OverlayOutlinePosition, OverlayOutlineStyle,
    OverlayOutput, OverlayOutputKind, OverlayOutputVariant, OverlayReferenceStyle,
    OverlayTextAlign, OverlayTextDecoration, OverlayTextOutline, OverlayTextShadow,
    OverlayTextStyle, OverlayTextTransform, OverlayTheme, OverlayVerticalAlign, ReferencePosition,
    Size,
};
use crate::overlay_boxes::{TEXT_BOX_MIN, boxes_for_layout_preset, clamp_overlay_box, overlay_box};
use crate::overlay_elements::normalize_overlay_elements;
use crate::overlay_outputs::{MAX_NDI_OUTPUTS, MAX_SYPHON_OUTPUTS, is_local_kind};
use crate::program::{
    ConfidenceLayout, OutputShowFilter, normalize_confidence_layout, normalize_show_filter,
};
use crate::serde_util::{Extra, string_enum};
use crate::settings::OverlaySettings;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::LazyLock;

// ─── Theme defaults ────────────────────────────────────────────────────────────

/// `DEFAULT_TEXT_SHADOW`.
pub fn default_text_shadow() -> OverlayTextShadow {
    OverlayTextShadow {
        enabled: true,
        x_px: 0.0,
        y_px: 4.0,
        blur_px: 12.0,
        color: "#000000".into(),
        opacity: 0.7,
        extra: Extra::new(),
    }
}

/// `DEFAULT_TEXT_OUTLINE`.
pub fn default_text_outline() -> OverlayTextOutline {
    OverlayTextOutline {
        enabled: false,
        position: OverlayOutlinePosition::Outside,
        width_px: 0.0,
        style: OverlayOutlineStyle::Solid,
        color: "#000000".into(),
        opacity: 1.0,
        extra: Extra::new(),
    }
}

const HELVETICA: &str = "'Helvetica Neue', Arial, sans-serif";

/// `DEFAULT_OVERLAY_THEME`.
pub static DEFAULT_OVERLAY_THEME: LazyLock<OverlayTheme> = LazyLock::new(|| {
    let boxes = boxes_for_layout_preset(
        OverlayLayoutPosition::LowerThird,
        ReferencePosition::Below,
        82.0,
        true,
    );
    OverlayTheme {
        elements: Vec::new(),
        background: OverlayBackground {
            kind: OverlayBackgroundType::Transparent,
            color: "#0b1220".into(),
            color2: None,
            angle_deg: None,
            opacity: 1.0,
            media_path: None,
            media_fit: Some(MediaFit::Cover),
            media_loop: None,
            media_start_seconds: None,
            media_end_seconds: None,
            media_rate: None,
            media_cue: None,
            hue: None,
            saturation: None,
            brightness: None,
            contrast: None,
            blur_px: None,
            extra: Extra::new(),
        },
        verse: OverlayTextStyle {
            font_family: HELVETICA.into(),
            font_size_px: 54.0,
            font_weight: 600.0,
            color: "#ffffff".into(),
            color_opacity: 1.0,
            line_height: 1.35,
            letter_spacing_px: 0.0,
            align: OverlayTextAlign::Center,
            vertical_align: OverlayVerticalAlign::Middle,
            text_decoration: OverlayTextDecoration::None,
            text_transform: OverlayTextTransform::None,
            shadow: default_text_shadow(),
            outline: default_text_outline(),
            frame: boxes.verse,
            extra: Extra::new(),
        },
        reference: OverlayReferenceStyle {
            show: true,
            position: ReferencePosition::Below,
            text: OverlayTextStyle {
                font_family: HELVETICA.into(),
                font_size_px: 32.0,
                font_weight: 700.0,
                color: "#5eead4".into(),
                color_opacity: 1.0,
                line_height: 1.25,
                letter_spacing_px: 0.32,
                align: OverlayTextAlign::Center,
                vertical_align: OverlayVerticalAlign::Middle,
                text_decoration: OverlayTextDecoration::None,
                text_transform: OverlayTextTransform::None,
                shadow: OverlayTextShadow {
                    enabled: false,
                    y_px: 2.0,
                    blur_px: 8.0,
                    ..default_text_shadow()
                },
                outline: default_text_outline(),
                frame: boxes.reference,
                extra: Extra::new(),
            },
        },
        layout: OverlayLayout {
            position: OverlayLayoutPosition::LowerThird,
            max_width_pct: 82.0,
            padding_px: 48.0,
            // Off until the operator turns it on.
            backdrop_box: false,
            backdrop_color: "rgba(3, 10, 20, 0.75)".into(),
            backdrop_radius_px: 0.0,
            auto_fit_text: false,
            extra: Extra::new(),
        },
        extra: Extra::new(),
    }
});

// ─── Output defaults ───────────────────────────────────────────────────────────

/// `DEFAULT_TEMPLATE`.
pub const DEFAULT_TEMPLATE: &str = "{Reference}\n{Text}";

/// `makeOverlayOutput(id, kind)` with no patch: every field populated.
/// Callers change what differs with struct update syntax.
pub fn make_overlay_output(id: &str, kind: OverlayOutputKind) -> OverlayOutput {
    OverlayOutput {
        id: id.to_owned(),
        name: id.to_owned(),
        kind,
        enabled: false,
        order: 0.0,
        fallback_only: false,
        theme_id: None,
        theme: DEFAULT_OVERLAY_THEME.clone(),
        template: DEFAULT_TEMPLATE.into(),
        lyrics: None,
        display_id: None,
        display_label: String::new(),
        device_port_id: String::new(),
        device_port_label: String::new(),
        device_mode_id: String::new(),
        device_key_fill: false,
        display_size: None,
        aspect: OutputAspect::Letterbox,
        custom_aspect: size(16.0, 9.0),
        show: OutputShowFilter::default(),
        source: OutputSource::Program,
        playlist_id: String::new(),
        slide_sec: 8.0,
        confidence: ConfidenceLayout::default(),
        extra: Extra::new(),
    }
}

fn size(width: f64, height: f64) -> Size {
    Size {
        width,
        height,
        extra: Extra::new(),
    }
}

/// `PROJECTOR_OUTPUT_ID`: disabled until the operator assigns a screen.
pub const PROJECTOR_OUTPUT_ID: &str = "projector";

/// `makeProjectorOutput`.
pub fn make_projector_output(order: f64) -> OverlayOutput {
    OverlayOutput {
        name: "Main Screen".into(),
        order,
        ..make_overlay_output(PROJECTOR_OUTPUT_ID, OverlayOutputKind::Screen)
    }
}

/// `DEFAULT_OVERLAY_OUTPUTS`: one Main Screen, switched on.
fn default_overlay_outputs() -> Vec<OverlayOutput> {
    vec![OverlayOutput {
        enabled: true,
        ..make_projector_output(0.0)
    }]
}

/// `DEFAULT_OVERLAY_SETTINGS`.
pub static DEFAULT_OVERLAY_SETTINGS: LazyLock<OverlaySettings> =
    LazyLock::new(|| OverlaySettings {
        template: DEFAULT_TEMPLATE.into(),
        show_translation: true,
        show_verse_numbers: true,
        max_verses: 0.0,
        outputs: default_overlay_outputs(),
        theme: DEFAULT_OVERLAY_THEME.clone(),
        extra: Extra::new(),
    });

// ─── Validation primitives ─────────────────────────────────────────────────────

/// `safeFontFamily`: strips characters that could break out of a quoted
/// CSS font-family string.
fn safe_font_family(v: Option<&Value>, fallback: &str) -> String {
    let s: String = safe_string(v, fallback)
        .chars()
        .filter(|c| !matches!(c, '"' | '\\' | ';' | '{' | '}'))
        .collect();
    let s = js::trim(&s);
    if s.is_empty() {
        fallback.to_owned()
    } else {
        s.to_owned()
    }
}

fn normalize_box(raw: Option<&Value>, fallback: &OverlayBox) -> OverlayBox {
    let r = as_object(raw);
    clamp_overlay_box(
        &overlay_box(
            clamp_num(r.get("xPct"), 0.0, 100.0, fallback.x_pct),
            clamp_num(r.get("yPct"), 0.0, 100.0, fallback.y_pct),
            clamp_num(r.get("widthPct"), 8.0, 100.0, fallback.width_pct),
            clamp_num(r.get("heightPct"), 6.0, 100.0, fallback.height_pct),
        ),
        TEXT_BOX_MIN,
    )
}

fn normalize_shadow(raw: Option<&Value>, fallback: &OverlayTextShadow) -> OverlayTextShadow {
    // Legacy: `shadow: true | false`.
    if let Some(Value::Bool(enabled)) = raw {
        return OverlayTextShadow {
            enabled: *enabled,
            ..fallback.clone()
        };
    }
    let r = as_object(raw);
    OverlayTextShadow {
        enabled: safe_bool(r.get("enabled"), fallback.enabled),
        x_px: clamp_num(r.get("xPx"), -40.0, 40.0, fallback.x_px),
        y_px: clamp_num(r.get("yPx"), -40.0, 40.0, fallback.y_px),
        blur_px: clamp_num(r.get("blurPx"), 0.0, 80.0, fallback.blur_px),
        color: safe_color(r.get("color"), &fallback.color),
        opacity: clamp_num(r.get("opacity"), 0.0, 1.0, fallback.opacity),
        extra: Extra::new(),
    }
}

fn normalize_outline(raw: Option<&Value>, fallback: &OverlayTextOutline) -> OverlayTextOutline {
    let r = as_object(raw);
    OverlayTextOutline {
        enabled: safe_bool(r.get("enabled"), fallback.enabled),
        position: safe_enum(r.get("position"), fallback.position),
        width_px: clamp_num(r.get("widthPx"), 0.0, 24.0, fallback.width_px),
        style: safe_enum(r.get("style"), fallback.style),
        color: safe_color(r.get("color"), &fallback.color),
        opacity: clamp_num(r.get("opacity"), 0.0, 1.0, fallback.opacity),
        extra: Extra::new(),
    }
}

/// `normalizeTextStyle`. The TypeScript passes `{ uppercase, shadow }` read
/// from the same object as a legacy fallback; both resolve to `r.uppercase`
/// and `r.shadow`, so they are read directly here.
fn normalize_text_style(
    raw: Option<&Value>,
    fallback: &OverlayTextStyle,
    box_fallback: &OverlayBox,
) -> OverlayTextStyle {
    let r = as_object(raw);
    let legacy_upper = safe_bool(r.get("uppercase"), false);
    let text_transform = if r.contains_key("textTransform") {
        safe_enum(r.get("textTransform"), fallback.text_transform)
    } else if legacy_upper {
        OverlayTextTransform::Uppercase
    } else {
        fallback.text_transform
    };

    OverlayTextStyle {
        font_family: safe_font_family(r.get("fontFamily"), &fallback.font_family),
        font_size_px: clamp_num(r.get("fontSizePx"), 12.0, 200.0, fallback.font_size_px),
        font_weight: clamp_num(r.get("fontWeight"), 100.0, 900.0, fallback.font_weight),
        color: safe_color(r.get("color"), &fallback.color),
        color_opacity: clamp_num(r.get("colorOpacity"), 0.0, 1.0, fallback.color_opacity),
        line_height: clamp_num(r.get("lineHeight"), 0.9, 2.5, fallback.line_height),
        letter_spacing_px: clamp_num(
            r.get("letterSpacingPx"),
            -4.0,
            20.0,
            fallback.letter_spacing_px,
        ),
        align: safe_enum(r.get("align"), fallback.align),
        vertical_align: safe_enum(r.get("verticalAlign"), fallback.vertical_align),
        text_decoration: safe_enum(r.get("textDecoration"), fallback.text_decoration),
        text_transform,
        shadow: normalize_shadow(r.get("shadow"), &fallback.shadow),
        outline: normalize_outline(r.get("outline"), &fallback.outline),
        frame: normalize_box(r.get("box"), box_fallback),
        extra: Extra::new(),
    }
}

// ─── Theme normaliser ──────────────────────────────────────────────────────────

/// `normalizeOverlayTheme`.
pub fn normalize_overlay_theme(raw: Option<&Value>) -> OverlayTheme {
    let r = as_object(raw);
    let d = &*DEFAULT_OVERLAY_THEME;

    let bg = as_object(r.get("background"));
    let verse = r.get("verse");
    let reference = r.get("reference");
    let reference_obj = as_object(reference);
    let layout = as_object(r.get("layout"));

    let position = safe_enum(layout.get("position"), d.layout.position);
    let max_width_pct = clamp_num(
        layout.get("maxWidthPct"),
        20.0,
        100.0,
        d.layout.max_width_pct,
    );
    let ref_show = safe_bool(reference_obj.get("show"), d.reference.show);
    let ref_position = safe_enum(reference_obj.get("position"), d.reference.position);
    let preset = boxes_for_layout_preset(position, ref_position, max_width_pct, ref_show);

    let media_path = match bg.get("mediaPath") {
        Some(Value::String(path)) if !js::trim(path).is_empty() => Some(js::trim(path).to_owned()),
        _ => None,
    };

    OverlayTheme {
        elements: normalize_overlay_elements(r.get("elements")),
        background: OverlayBackground {
            kind: safe_enum(bg.get("type"), d.background.kind),
            color: safe_color(bg.get("color"), &d.background.color),
            color2: bg
                .get("color2")
                .map(|c| safe_color(Some(c), &d.background.color)),
            angle_deg: bg
                .get("angleDeg")
                .map(|a| clamp_num(Some(a), 0.0, 360.0, 0.0)),
            opacity: clamp_num(bg.get("opacity"), 0.0, 1.0, d.background.opacity),
            // Path validity is enforced by the media protocol allowlist, not here.
            media_path,
            media_fit: Some(safe_enum(bg.get("mediaFit"), MediaFit::Cover)),
            media_loop: Some(safe_bool(bg.get("mediaLoop"), false)),
            media_start_seconds: None,
            media_end_seconds: None,
            media_rate: None,
            media_cue: None,
            hue: Some(clamp_num(bg.get("hue"), -180.0, 180.0, 0.0)),
            saturation: Some(clamp_num(bg.get("saturation"), 0.0, 2.0, 1.0)),
            brightness: Some(clamp_num(bg.get("brightness"), 0.25, 1.75, 1.0)),
            contrast: Some(clamp_num(bg.get("contrast"), 0.25, 1.75, 1.0)),
            blur_px: Some(clamp_num(bg.get("blurPx"), 0.0, 40.0, 0.0)),
            extra: Extra::new(),
        },
        verse: normalize_text_style(verse, &d.verse, &preset.verse),
        reference: OverlayReferenceStyle {
            show: ref_show,
            position: ref_position,
            text: normalize_text_style(reference, &d.reference.text, &preset.reference),
        },
        layout: OverlayLayout {
            position,
            max_width_pct,
            padding_px: clamp_num(layout.get("paddingPx"), 0.0, 200.0, d.layout.padding_px),
            backdrop_box: safe_bool(layout.get("backdropBox"), d.layout.backdrop_box),
            backdrop_color: safe_color(layout.get("backdropColor"), &d.layout.backdrop_color),
            // Retired: corners are always square; healed to 0 on every read.
            backdrop_radius_px: 0.0,
            auto_fit_text: safe_bool(layout.get("autoFitText"), d.layout.auto_fit_text),
            extra: Extra::new(),
        },
        extra: Extra::new(),
    }
}

// ─── Output normaliser and legacy migration ────────────────────────────────────

string_enum!(
    /// The retired `overlay.mode`.
    pub enum LegacyOverlayMode {
        Auto = "auto",
        Ndi = "ndi",
        Message = "message",
    }
);

/// `LegacyOverlayFields`: what a store without `outputs` migrates from.
#[derive(Clone, Debug, PartialEq)]
pub struct LegacyOverlayFields {
    pub mode: Option<LegacyOverlayMode>,
    pub theme: OverlayTheme,
    pub template: String,
}

/// `migrateLegacyOutputs`: older stores keep only their NDI route.
fn migrate_legacy_outputs(legacy: &LegacyOverlayFields) -> Vec<OverlayOutput> {
    vec![
        OverlayOutput {
            name: "NDI feed".into(),
            enabled: legacy.mode != Some(LegacyOverlayMode::Message),
            theme: legacy.theme.clone(),
            ..make_overlay_output("main", OverlayOutputKind::Ndi)
        },
        make_projector_output(0.0),
    ]
}

/// `themeId` as stored: a non-empty string, else `null`.
fn theme_id(v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

/// `normalizeOutputVariant`: `None` unless the value is an object (arrays
/// count, as `typeof [] === 'object'`), so "no override" survives a round trip.
fn normalize_output_variant(
    raw: Option<&Value>,
    fallback_template: &str,
) -> Option<OverlayOutputVariant> {
    if !matches!(raw, Some(Value::Object(_) | Value::Array(_))) {
        return None;
    }
    let r = as_object(raw);
    Some(OverlayOutputVariant {
        theme_id: theme_id(r.get("themeId")),
        theme: normalize_overlay_theme(r.get("theme")),
        template: safe_string(r.get("template"), fallback_template),
        extra: Extra::new(),
    })
}

/// `normalizeOverlayOutputs`. A missing or non-array list migrates from the
/// legacy fields; an explicit list is kept as saved, except that invalid
/// entries are dropped, extra NDI and Syphon senders and second screens on
/// one display are switched off, and an empty result gets the Main Screen back.
pub fn normalize_overlay_outputs(
    raw: Option<&Value>,
    legacy: &LegacyOverlayFields,
) -> Vec<OverlayOutput> {
    let Some(Value::Array(items)) = raw else {
        return migrate_legacy_outputs(legacy);
    };

    let mut seen_ids = HashSet::new();
    let mut ndi_enabled = 0;
    let mut syphon_enabled = 0;
    // Display ids are safe integers, so exact as `i64`.
    let mut screen_displays = HashSet::new();
    let mut normalized: Vec<OverlayOutput> = Vec::new();

    for item in items {
        let r = as_object(Some(item));
        let id = safe_string(r.get("id"), "");
        let id = js::trim(&id).to_owned();
        if id.is_empty() || seen_ids.contains(&id) {
            continue;
        }
        seen_ids.insert(id.clone());

        let Some(kind) = r
            .get("kind")
            .and_then(Value::as_str)
            .and_then(OverlayOutputKind::from_name)
        else {
            continue;
        };
        let device = kind == OverlayOutputKind::Device;
        let screen = kind == OverlayOutputKind::Screen;
        let local = is_local_kind(kind);
        let mut enabled = safe_bool(r.get("enabled"), false);
        if kind == OverlayOutputKind::Ndi && enabled {
            if ndi_enabled >= MAX_NDI_OUTPUTS {
                enabled = false;
            } else {
                ndi_enabled += 1;
            }
        }
        if kind == OverlayOutputKind::Syphon && enabled {
            if syphon_enabled >= MAX_SYPHON_OUTPUTS {
                enabled = false;
            } else {
                syphon_enabled += 1;
            }
        }
        let display_id = if screen {
            normalize_display_id(r.get("displayId"))
        } else {
            None
        };
        // Two windows on one display would overlap; only the first is live.
        if screen
            && enabled
            && let Some(display) = display_id
            && !screen_displays.insert(display as i64)
        {
            enabled = false;
        }

        let trimmed = |key: &str| js::trim(&safe_string(r.get(key), "")).to_owned();
        let name = safe_string(r.get("name"), &id);
        let name = js::trim(&name);

        normalized.push(OverlayOutput {
            name: if name.is_empty() {
                id.clone()
            } else {
                name.to_owned()
            },
            kind,
            enabled,
            order: clamp_num(r.get("order"), 0.0, 999.0, normalized.len() as f64),
            fallback_only: safe_bool(r.get("fallbackOnly"), false),
            theme_id: theme_id(r.get("themeId")),
            theme: normalize_overlay_theme(r.get("theme")),
            template: safe_string(r.get("template"), &legacy.template),
            lyrics: normalize_output_variant(r.get("lyrics"), &legacy.template),
            display_id,
            display_label: if screen {
                trimmed("displayLabel")
            } else {
                String::new()
            },
            device_port_id: if device {
                trimmed("devicePortId")
            } else {
                String::new()
            },
            device_port_label: if device {
                trimmed("devicePortLabel")
            } else {
                String::new()
            },
            device_mode_id: if device {
                trimmed("deviceModeId")
            } else {
                String::new()
            },
            device_key_fill: device && safe_bool(r.get("deviceKeyFill"), false),
            display_size: if screen {
                normalize_display_size(r.get("displaySize"))
            } else {
                None
            },
            aspect: if screen {
                safe_enum(r.get("aspect"), OutputAspect::Letterbox)
            } else {
                OutputAspect::Letterbox
            },
            custom_aspect: normalize_custom_aspect(r.get("customAspect")),
            show: normalize_show_filter(r.get("show")),
            source: if local {
                safe_enum_in(
                    r.get("source"),
                    &[OutputSource::Program, OutputSource::Playlist],
                    OutputSource::Program,
                )
            } else {
                OutputSource::Program
            },
            playlist_id: if local {
                trimmed("playlistId")
            } else {
                String::new()
            },
            slide_sec: math_round(clamp_num(r.get("slideSec"), 3.0, 600.0, 8.0)),
            confidence: normalize_confidence_layout(r.get("confidence")),
            id,
            extra: Extra::new(),
        });
    }

    // Never an empty Audience: a setup with no outputs gets the Main Screen back.
    if normalized.is_empty() {
        default_overlay_outputs()
    } else {
        normalized
    }
}

/// `normalizeDisplayId`: a safe integer, else `null`.
fn normalize_display_id(raw: Option<&Value>) -> Option<f64> {
    raw.and_then(Value::as_f64)
        .filter(|n| js::is_safe_integer(*n))
}

/// `normalizeDisplaySize`: positive finite width and height, else `null`.
fn normalize_display_size(raw: Option<&Value>) -> Option<Size> {
    let r = as_object(raw);
    let width = r.get("width")?.as_f64()?;
    let height = r.get("height")?.as_f64()?;
    (width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0)
        .then(|| size(width, height))
}

/// `normalizeCustomAspect`: two whole numbers, 1–100 each.
fn normalize_custom_aspect(raw: Option<&Value>) -> Size {
    let r = as_object(raw);
    size(
        math_round(clamp_num(r.get("width"), 1.0, 100.0, 16.0)),
        math_round(clamp_num(r.get("height"), 1.0, 100.0, 9.0)),
    )
}

// ─── Overlay settings normaliser ───────────────────────────────────────────────

/// `normalizeOverlaySettings`. Retired keys (`mode`, `ppVideoInputUuid`,
/// `autoClearSec`, …) are dropped.
pub fn normalize_overlay_settings(raw: Option<&Value>) -> OverlaySettings {
    let r = as_object(raw);
    let d = &*DEFAULT_OVERLAY_SETTINGS;

    let template = safe_string(r.get("template"), &d.template);
    let mode = safe_enum(r.get("mode"), LegacyOverlayMode::Auto);
    let theme = normalize_overlay_theme(r.get("theme"));
    let legacy = LegacyOverlayFields {
        mode: Some(mode),
        theme,
        template,
    };

    OverlaySettings {
        show_translation: safe_bool(r.get("showTranslation"), d.show_translation),
        show_verse_numbers: safe_bool(r.get("showVerseNumbers"), d.show_verse_numbers),
        max_verses: non_neg_number(r.get("maxVerses"), d.max_verses),
        outputs: normalize_overlay_outputs(r.get("outputs"), &legacy),
        template: legacy.template,
        theme: legacy.theme,
        extra: Extra::new(),
    }
}
