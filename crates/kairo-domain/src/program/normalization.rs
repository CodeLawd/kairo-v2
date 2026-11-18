use super::*;
use crate::js::{is_safe_integer, math_round};
use crate::normalize::{clamp_num, safe_bool, safe_color, safe_enum};
use crate::workspace::{fallback_text, text};
use serde_json::Value;
use std::collections::HashSet;

pub const MAX_PROP_OBJECTS: usize = 40;
pub const MAX_PROP_TEXT_LENGTH: usize = 1000;
pub const MAX_SAVED_MESSAGES: usize = 50;
pub const MAX_SAVED_MESSAGE_LENGTH: usize = 500;
pub const MAX_PROGRAM_TIMERS: usize = 20;

fn truncate(text: &str, limit: usize) -> String {
    let mut units = 0;
    text.chars()
        .take_while(|ch| {
            units += ch.len_utf16();
            units <= limit
        })
        .collect()
}

fn array(value: &Value) -> impl Iterator<Item = &Value> {
    value.as_array().into_iter().flatten()
}

fn rounded(value: Option<&Value>, min: f64, max: f64, fallback: f64) -> f64 {
    math_round(clamp_num(value, min, max, fallback))
}

fn prop_objects(raw: &Value) -> impl Iterator<Item = PropObject> {
    let mut seen = HashSet::new();
    array(raw).filter_map(move |raw| {
        let id = text(&raw["id"]);
        if id.is_empty() || !seen.insert(id) {
            return None;
        }
        let n = |key, min, max, fallback| clamp_num(raw.get(key), min, max, fallback);
        let w = n("w", 0.01, 1.0, 0.3);
        let h = n("h", 0.01, 1.0, 0.2);
        let frame = PropFrame {
            id: id.into(),
            opacity: n("opacity", 0.0, 1.0, 1.0),
            w,
            h,
            x: n("x", 0.0, 1.0 - w, 0.05),
            y: n("y", 0.0, 1.0 - h, 0.05),
        };
        let color = || safe_color(raw.get("color"), "#ffffff");
        let align = || safe_enum(raw.get("align"), PropAlign::Center);
        let bold = || safe_bool(raw.get("bold"), true);
        let extra = Extra::new();
        Some(match raw["kind"].as_str()? {
            "text" => PropObject::Text {
                frame,
                text: truncate(
                    raw["text"].as_str().unwrap_or_default(),
                    MAX_PROP_TEXT_LENGTH,
                ),
                font_size_px: math_round(n("fontSizePx", 8.0, 400.0, 72.0)),
                color: color(),
                bold: bold(),
                align: align(),
                extra,
            },
            "timer" => PropObject::Timer {
                frame,
                font_size_px: math_round(n("fontSizePx", 8.0, 400.0, 96.0)),
                color: color(),
                bold: bold(),
                align: align(),
                overrun_color: safe_color(raw.get("overrunColor"), "#f87171"),
                extra,
            },
            "shape" => PropObject::Shape {
                frame,
                shape: safe_enum(raw.get("shape"), PropShape::Rectangle),
                fill: safe_color(raw.get("fill"), "#315ede"),
                radius_px: math_round(n("radiusPx", 0.0, 540.0, 0.0)),
                extra,
            },
            "image" => {
                let path = text(&raw["mediaPath"]);
                if path.is_empty() {
                    return None;
                }
                PropObject::Image {
                    frame,
                    media_path: path.into(),
                    fit: safe_enum(raw.get("fit"), PropImageFit::Contain),
                    extra,
                }
            }
            _ => return None,
        })
    })
}

fn legacy_prop(raw: &Value, id: &str) -> Vec<PropObject> {
    let path = text(&raw["mediaPath"]);
    if path.is_empty() {
        return Vec::new();
    }
    let w = clamp_num(raw.get("widthPct"), 2.0, 100.0, 12.0) / 100.0;
    let h = (w * 16.0 / 9.0).min(1.0);
    let mx = clamp_num(raw.get("marginPct"), 0.0, 20.0, 2.0) / 100.0;
    let my = (mx * 16.0 / 9.0).min(0.5);
    let (x, y) = match safe_enum(raw.get("position"), PropPosition::TopRight) {
        PropPosition::Center => ((1.0 - w) / 2.0, (1.0 - h) / 2.0),
        PropPosition::TopLeft => (mx, my),
        PropPosition::TopRight => (1.0 - mx - w, my),
        PropPosition::BottomLeft => (mx, 1.0 - my - h),
        PropPosition::BottomRight => (1.0 - mx - w, 1.0 - my - h),
    };
    vec![PropObject::Image {
        frame: PropFrame {
            id: format!("{id}-image"),
            x: x.max(0.0),
            y: y.max(0.0),
            w,
            h,
            opacity: clamp_num(raw.get("opacity"), 0.0, 1.0, 1.0),
        },
        media_path: path.into(),
        fit: PropImageFit::Contain,
        extra: Extra::new(),
    }]
}

fn normalize_props(raw: &Value) -> Vec<ProgramProp> {
    let mut seen = HashSet::new();
    array(raw)
        .filter_map(|raw| {
            let id = text(&raw["id"]);
            if id.is_empty() || !seen.insert(id) {
                return None;
            }
            let objects = if raw["objects"].is_array() {
                prop_objects(&raw["objects"])
                    .take(MAX_PROP_OBJECTS)
                    .collect()
            } else {
                legacy_prop(raw, id)
            };
            Some(ProgramProp {
                id: id.into(),
                name: fallback_text(&raw["name"], "Prop").into(),
                background: (!raw["background"].is_null()
                    && raw["background"].as_str() != Some(""))
                .then(|| safe_color(raw.get("background"), "#000000")),
                objects,
                extra: Extra::new(),
            })
        })
        .collect()
}

fn normalize_timers(raw: &Value, legacy: &Value) -> Vec<ProgramTimerDef> {
    let rollover = safe_bool(legacy.get("rollover"), true);
    let color = safe_color(legacy.get("color"), "#5eead4");
    let overrun_color = safe_color(legacy.get("overrunColor"), "#f87171");
    let default = ProgramTimerDef::default();
    let mut seen = HashSet::new();
    let mut timers: Vec<_> = array(raw)
        .filter_map(|raw| {
            let id = text(&raw["id"]);
            if id.is_empty() || !seen.insert(id) {
                return None;
            }
            let target = text(&raw["targetTime"]);
            let name = truncate(text(&raw["name"]), 80);
            let theme = &raw["theme"];
            let background = safe_color(theme.get("background"), "");
            let mut objects: Vec<_> = prop_objects(&theme["objects"])
                .take(MAX_PROP_OBJECTS)
                .collect();
            if !objects
                .iter()
                .any(|o| matches!(o, PropObject::Timer { .. }))
            {
                objects.extend(default_timer_theme(&color, &overrun_color).objects);
            }
            Some(ProgramTimerDef {
                id: id.into(),
                name: if name.is_empty() {
                    "Timer".into()
                } else {
                    name
                },
                kind: safe_enum(raw.get("kind"), TimerKind::Countdown),
                duration_sec: rounded(raw.get("durationSec"), 0.0, 86_400.0, default.duration_sec),
                target_time: if parse_clock_target(target).is_some() {
                    target.into()
                } else {
                    default.target_time.clone()
                },
                overrun: safe_bool(raw.get("overrun"), rollover),
                theme: TimerTheme {
                    background: (!background.is_empty()).then_some(background),
                    objects,
                    extra: Extra::new(),
                },
                extra: Extra::new(),
            })
        })
        .take(MAX_PROGRAM_TIMERS)
        .collect();
    if timers.is_empty() {
        timers.push(ProgramTimerDef {
            overrun: rollover,
            theme: default_timer_theme(&color, &overrun_color),
            ..default
        });
    }
    timers
}

fn normalize_messages(raw: &Value) -> Vec<ProgramSavedMessage> {
    let mut seen = HashSet::new();
    array(raw)
        .filter_map(|raw| {
            let id = text(&raw["id"]);
            let text = truncate(text(&raw["text"]), MAX_SAVED_MESSAGE_LENGTH);
            (!id.is_empty() && !text.is_empty() && seen.insert(id)).then(|| ProgramSavedMessage {
                id: id.into(),
                text,
                extra: Extra::new(),
            })
        })
        .take(MAX_SAVED_MESSAGES)
        .collect()
}

fn normalize_stage_displays(raw: &Value) -> Vec<StageDisplayConfig> {
    let mut seen = HashSet::new();
    let mut taken = HashSet::new();
    array(raw)
        .filter_map(|raw| {
            let id = text(&raw["id"]);
            if id.is_empty() || !seen.insert(id) {
                return None;
            }
            let display_id = raw["displayId"].as_f64().filter(|n| is_safe_integer(*n));
            let enabled = safe_bool(raw.get("enabled"), false)
                && display_id.is_none_or(|id| taken.insert(id as i64));
            let size = &raw["displaySize"];
            let display_size = size["width"]
                .as_f64()
                .zip(size["height"].as_f64())
                .filter(|(w, h)| *w > 0.0 && *h > 0.0)
                .map(|(width, height)| crate::overlay::Size {
                    width,
                    height,
                    extra: Extra::new(),
                });
            Some(StageDisplayConfig {
                id: id.into(),
                name: fallback_text(&raw["name"], "Stage display").into(),
                enabled,
                display_id,
                display_label: text(&raw["displayLabel"]).into(),
                display_size,
                mode: match raw["mode"].as_str() {
                    Some("teleprompter") => StageDisplayMode::Teleprompter,
                    _ => StageDisplayMode::ConfidenceMonitor,
                },
                show_next: safe_bool(raw.get("showNext"), true),
                show_clock: safe_bool(raw.get("showClock"), true),
                show_timer: safe_bool(raw.get("showTimer"), true),
                extra: Extra::new(),
            })
        })
        .collect()
}

fn normalize_video(raw: &Value) -> VideoSettings {
    let default = VideoSettings::default();
    let width = clamp_num(raw.get("sizePct"), 10.0, 60.0, default.frame.width_pct);
    let margin = clamp_num(raw.get("marginPct"), 0.0, 8.0, 2.0);
    let corner = raw["corner"]
        .as_str()
        .filter(|c| {
            matches!(
                *c,
                "top-left" | "top-right" | "bottom-left" | "bottom-right"
            )
        })
        .unwrap_or("bottom-right");
    let frame = &raw["box"];
    let width_pct = clamp_num(frame.get("widthPct"), 10.0, 90.0, width);
    let height_pct = clamp_num(frame.get("heightPct"), 10.0, 90.0, width);
    let far = 100.0 - width - margin;
    VideoSettings {
        layout: safe_enum(raw.get("layout"), default.layout),
        frame: VideoBox {
            x_pct: clamp_num(
                frame.get("xPct"),
                0.0,
                100.0 - width_pct,
                if corner.ends_with("right") {
                    far
                } else {
                    margin
                },
            ),
            y_pct: clamp_num(
                frame.get("yPct"),
                0.0,
                100.0 - height_pct,
                if corner.starts_with("bottom") {
                    far
                } else {
                    margin
                },
            ),
            width_pct,
            height_pct,
            extra: Extra::new(),
        },
        radius_px: rounded(raw.get("radiusPx"), 0.0, 48.0, default.radius_px),
        side: safe_enum(raw.get("side"), default.side),
        split_pct: rounded(raw.get("splitPct"), 20.0, 80.0, default.split_pct),
        extra: Extra::new(),
    }
}

pub fn normalize_presentation_settings(raw: &Value) -> PresentationSettings {
    let mut settings = PresentationSettings::default();
    let t = &raw["transition"];
    settings.transition.kind = safe_enum(t.get("kind"), settings.transition.kind);
    settings.transition.duration_ms = rounded(
        t.get("durationMs"),
        50.0,
        3000.0,
        settings.transition.duration_ms,
    );
    let q = &raw["question"];
    let question = &mut settings.question;
    question.position = safe_enum(q.get("position"), question.position);
    question.align = safe_enum(q.get("align"), question.align);
    question.font_family = safe_enum(q.get("fontFamily"), question.font_family);
    question.font_size_px = rounded(q.get("fontSizePx"), 24.0, 100.0, question.font_size_px);
    question.color = safe_color(q.get("color"), &question.color);
    question.background = safe_color(q.get("background"), &question.background);
    question.background_opacity = rounded(
        q.get("backgroundOpacity"),
        0.0,
        100.0,
        question.background_opacity,
    );
    question.border_radius_px = rounded(
        q.get("borderRadiusPx"),
        0.0,
        48.0,
        question.border_radius_px,
    );
    question.width_pct = rounded(q.get("widthPct"), 50.0, 100.0, question.width_pct);
    question.show_author = safe_bool(q.get("showAuthor"), question.show_author);
    question.join_headline = truncate(
        q["joinHeadline"]
            .as_str()
            .unwrap_or(&question.join_headline),
        80,
    );
    question.qr_size_px = rounded(q.get("qrSizePx"), 160.0, 400.0, question.qr_size_px);
    question.show_join_url = safe_bool(q.get("showJoinUrl"), question.show_join_url);
    let m = &raw["message"];
    let message = &mut settings.message;
    message.position = safe_enum(m.get("position"), message.position);
    message.font_size_px = rounded(m.get("fontSizePx"), 16.0, 160.0, message.font_size_px);
    message.color = safe_color(m.get("color"), &message.color);
    message.background = safe_color(m.get("background"), &message.background);
    message.scroll = safe_bool(m.get("scroll"), message.scroll);
    settings.messages = normalize_messages(&raw["messages"]);
    settings.props = normalize_props(&raw["props"]);
    settings.logo.media_path = text(&raw["logo"]["mediaPath"]).into();
    settings.logo.background = safe_color(raw["logo"].get("background"), &settings.logo.background);
    settings.stage_displays = normalize_stage_displays(&raw["stageDisplays"]);
    let a = &raw["audio"];
    settings.audio.enabled = safe_bool(a.get("enabled"), settings.audio.enabled);
    settings.audio.volume = clamp_num(a.get("volume"), 0.0, 1.0, settings.audio.volume);
    settings.audio.output_label = text(&a["outputLabel"]).into();
    settings.audio.ndi = safe_bool(a.get("ndi"), settings.audio.ndi);
    settings.video = normalize_video(if raw["video"].is_null() {
        &raw["camera"]
    } else {
        &raw["video"]
    });
    settings.timers = normalize_timers(&raw["timers"], &raw["timer"]);
    settings
}

pub fn presentation_media_paths(raw: &Value) -> Vec<String> {
    let settings = normalize_presentation_settings(raw);
    std::iter::once(settings.logo.media_path)
        .chain(
            settings
                .props
                .into_iter()
                .flat_map(|p| p.objects)
                .chain(
                    settings
                        .timers
                        .into_iter()
                        .flat_map(|timer| timer.theme.objects),
                )
                .filter_map(|object| match object {
                    PropObject::Image { media_path, .. } => Some(media_path),
                    _ => None,
                }),
        )
        .filter(|path| !path.is_empty())
        .collect()
}
