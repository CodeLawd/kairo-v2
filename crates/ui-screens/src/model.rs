//! Screens edits are applied to the latest shared settings, never to a stale
//! copy captured when a field or theme editor was opened.
use kairo_domain::displays::{DisplayBinding, resolve_display};
use kairo_domain::overlay::*;
use kairo_domain::overlay_defaults::make_overlay_output;
use kairo_domain::overlay_outputs::{MAX_NDI_OUTPUTS, MAX_SYPHON_OUTPUTS};
use kairo_domain::program::{PLAYLIST_SHOW_KEYS, StageDisplayConfig};
use kairo_domain::settings::AppSettings;
use kairo_domain::theme_library::{assign_theme_to_output, with_default_themes};
use serde_json::{Value, json};

pub const SELECTION_KEY: &str = "show:selected";
pub const SHOW_KEYS: &[&str] = &["show:setup", "show:looks", "show:transition", "show:check"];

#[derive(Clone, Debug)]
pub enum Edit {
    AddOutput {
        id: String,
        kind: OverlayOutputKind,
    },
    AddStage(String),
    Output {
        id: String,
        patch: Value,
    },
    Stage {
        id: String,
        patch: Value,
    },
    Remove(String),
    Group {
        stage: bool,
        enabled: bool,
    },
    Bind {
        key: String,
        display: Option<DisplayInfo>,
    },
    MoveToCard {
        id: String,
        next_id: String,
        port: kairo_domain::video_devices::VideoPort,
    },
    Theme {
        id: String,
        kind: OverlayContentKind,
        theme: Option<String>,
    },
    Presentation(Value),
}

pub fn kind_title(kind: OverlayOutputKind) -> &'static str {
    match kind {
        OverlayOutputKind::Screen => "External display",
        OverlayOutputKind::Ndi => "NDI feed",
        OverlayOutputKind::Device => "Video card",
        OverlayOutputKind::Syphon => "Syphon",
    }
}

pub fn initial_selection(
    s: &AppSettings,
    initial: Option<&str>,
    remembered: Option<&str>,
) -> Option<String> {
    if let Some(kind) = initial.and_then(|s| s.strip_prefix("kind:")) {
        return s
            .overlay
            .outputs
            .iter()
            .find(|o| o.kind.as_str() == kind)
            .map(|o| o.id.clone());
    }
    initial
        .filter(|s| !s.starts_with("add:"))
        .or(remembered.filter(|s| SHOW_KEYS.contains(s)))
        .map(str::to_owned)
        .or_else(|| s.overlay.outputs.first().map(|o| o.id.clone()))
        .or_else(|| {
            s.presentation
                .stage_displays
                .first()
                .map(|s| format!("stage:{}", s.id))
        })
}

pub fn look_applies(output: &OverlayOutput, key: &str) -> bool {
    output.source != OutputSource::Playlist || PLAYLIST_SHOW_KEYS.iter().any(|k| k.as_str() == key)
}

pub fn binding(id: Option<f64>, label: &str, size: &Option<Size>) -> DisplayBinding {
    DisplayBinding {
        display_id: id,
        display_label: label.into(),
        display_size: size.clone(),
    }
}

pub fn display_owner(
    s: &AppSettings,
    key: &str,
    display: &DisplayInfo,
    displays: &[DisplayInfo],
) -> Option<String> {
    s.overlay
        .outputs
        .iter()
        .filter(|o| o.kind == OverlayOutputKind::Screen && o.enabled && o.id != key)
        .find(|o| {
            resolve_display(
                &binding(o.display_id, &o.display_label, &o.display_size),
                displays,
                &[],
            )
            .is_some_and(|d| d.id == display.id)
        })
        .map(|o| o.name.clone())
        .or_else(|| {
            s.presentation
                .stage_displays
                .iter()
                .filter(|o| o.enabled && format!("stage:{}", o.id) != key)
                .find(|o| {
                    resolve_display(
                        &binding(o.display_id, &o.display_label, &o.display_size),
                        displays,
                        &[],
                    )
                    .is_some_and(|d| d.id == display.id)
                })
                .map(|o| o.name.clone())
        })
}

fn merge(target: &mut Value, patch: &Value) {
    if let (Some(target), Some(patch)) = (target.as_object_mut(), patch.as_object()) {
        for (key, value) in patch {
            if value.is_object() && target.get(key).is_some_and(Value::is_object) {
                merge(target.get_mut(key).unwrap(), value);
            } else {
                target.insert(key.clone(), value.clone());
            }
        }
    }
}
fn patch<T: serde_json_compat::Record>(value: &mut T, changes: &Value) -> Result<(), String> {
    serde_json_compat::patch(value, changes)
}

// Kept local so callers cannot bypass the edit contract with a whole-settings replacement.
mod serde_json_compat {
    pub trait Record: serde::Serialize + serde::de::DeserializeOwned {}
    impl<T: serde::Serialize + serde::de::DeserializeOwned> Record for T {}
    pub fn patch<T: Record>(value: &mut T, changes: &serde_json::Value) -> Result<(), String> {
        let mut raw = serde_json::to_value(&*value).map_err(|e| e.to_string())?;
        super::merge(&mut raw, changes);
        *value = serde_json::from_value(raw).map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// Returns the section to persist. Invalid edits leave the caller's settings
/// untouched (the application edits a fresh read before committing).
pub fn apply(s: &mut AppSettings, edit: &Edit) -> Result<&'static str, String> {
    match edit {
        Edit::AddOutput { id, kind } => {
            let count = s.overlay.outputs.iter().filter(|o| o.kind == *kind).count();
            let limit = match kind {
                OverlayOutputKind::Ndi => MAX_NDI_OUTPUTS,
                OverlayOutputKind::Syphon => MAX_SYPHON_OUTPUTS,
                _ => usize::MAX,
            };
            if count >= limit {
                return Err(format!(
                    "The maximum number of {} outputs is {limit}.",
                    kind_title(*kind)
                ));
            }
            if s.overlay.outputs.iter().any(|o| o.id == *id) {
                return Err("That screen already exists.".into());
            }
            let mut output = make_overlay_output(id, *kind);
            output.name = if count == 0 {
                kind_title(*kind).into()
            } else {
                format!("{} {}", kind_title(*kind), count + 1)
            };
            output.enabled = true;
            output.order = s.overlay.outputs.len() as f64;
            s.overlay.outputs.push(with_default_themes(
                &output,
                &s.theme_library,
                Some(&kairo_domain::theme_library::ThemeDefaults {
                    scripture: s.theme_defaults.scripture.clone(),
                    lyrics: s.theme_defaults.lyrics.clone(),
                }),
            ));
        }
        Edit::AddStage(id) => {
            let count = s.presentation.stage_displays.len();
            s.presentation.stage_displays.push(StageDisplayConfig {
                id: id.clone(),
                name: if count == 0 {
                    "Stage display".into()
                } else {
                    format!("Stage display {}", count + 1)
                },
                enabled: true,
                display_id: None,
                display_label: String::new(),
                display_size: None,
                mode: Default::default(),
                show_next: true,
                show_clock: true,
                show_timer: true,
                extra: Default::default(),
            });
            return Ok("presentation");
        }
        Edit::Output { id, patch: changes } => {
            let output = s
                .overlay
                .outputs
                .iter_mut()
                .find(|o| o.id == *id)
                .ok_or("This screen no longer exists.")?;
            patch(output, changes)?;
            output.slide_sec = output.slide_sec.clamp(3., 600.);
            output.custom_aspect.width = output.custom_aspect.width.round().clamp(1., 100.);
            output.custom_aspect.height = output.custom_aspect.height.round().clamp(1., 100.);
        }
        Edit::Stage { id, patch: changes } => {
            patch(
                s.presentation
                    .stage_displays
                    .iter_mut()
                    .find(|o| o.id == *id)
                    .ok_or("This stage display no longer exists.")?,
                changes,
            )?;
            return Ok("presentation");
        }
        Edit::Remove(key) => {
            if let Some(id) = key.strip_prefix("stage:") {
                s.presentation.stage_displays.retain(|s| s.id != id);
                return Ok("presentation");
            }
            if !s.overlay.outputs.iter().any(|o| o.id == *key) {
                return Err("This screen no longer exists.".into());
            }
            if s.overlay.outputs.len() <= 1 {
                return Err(
                    "Audience needs at least one screen. Add another before deleting this one."
                        .into(),
                );
            }
            s.overlay.outputs.retain(|o| o.id != *key);
        }
        Edit::Group { stage, enabled } => {
            if *stage {
                for s in &mut s.presentation.stage_displays {
                    s.enabled = *enabled;
                }
                return Ok("presentation");
            }
            for o in &mut s.overlay.outputs {
                o.enabled = *enabled;
            }
        }
        Edit::Bind { key, display } => {
            if let Some(display) = display
                && let Some(owner) = display_owner(s, key, display, std::slice::from_ref(display))
            {
                return Err(format!(
                    "{owner} is already on that display. Move it first."
                ));
            }
            let changes = display.as_ref().map_or(
                json!({"displayId":null,"displayLabel":"","displaySize":null}),
                |d| json!({"displayId":d.id,"displayLabel":d.label,"displaySize":d.size}),
            );
            return if let Some(id) = key.strip_prefix("stage:") {
                apply(
                    s,
                    &Edit::Stage {
                        id: id.into(),
                        patch: changes,
                    },
                )
            } else {
                apply(
                    s,
                    &Edit::Output {
                        id: key.clone(),
                        patch: changes,
                    },
                )
            };
        }
        Edit::MoveToCard { id, next_id, port } => {
            if let Some(reason) = &port.unavailable_reason {
                return Err(reason.clone());
            }
            if s.overlay.outputs.iter().any(|o| {
                o.kind == OverlayOutputKind::Device && o.id != *id && o.device_port_id == port.id
            }) {
                return Err("That video output is already in use.".into());
            }
            let o = s
                .overlay
                .outputs
                .iter_mut()
                .find(|o| o.id == *id)
                .ok_or("This screen no longer exists.")?;
            o.id = next_id.clone();
            o.kind = OverlayOutputKind::Device;
            o.display_id = None;
            o.display_label.clear();
            o.display_size = None;
            o.device_port_id = port.id.clone();
            o.device_port_label = format!("{} · {}", port.device_name, port.port_name);
            o.device_mode_id = port.modes.first().map(|m| m.id.clone()).unwrap_or_default();
            o.device_key_fill = false;
        }
        Edit::Theme { id, kind, theme } => {
            let o = s
                .overlay
                .outputs
                .iter_mut()
                .find(|o| o.id == *id)
                .ok_or("This screen no longer exists.")?;
            if let Some(id) = theme {
                let theme = s
                    .theme_library
                    .iter()
                    .find(|t| t.id == *id && t.kind == *kind)
                    .ok_or("This theme no longer exists.")?;
                *o = assign_theme_to_output(o, theme);
            } else if *kind == OverlayContentKind::Lyrics {
                o.lyrics = None;
            } else {
                o.theme_id = None;
            }
        }
        Edit::Presentation(changes) => {
            patch(&mut s.presentation, changes)?;
            return Ok("presentation");
        }
    }
    Ok("overlay")
}

// ─── Sidebar and header text (`ScreenConfiguration.tsx` at af978c5) ──────────

/// A status's colour in the sidebar dot (`StatusDot`): live, waiting or off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Live,
    Warn,
    Off,
}

/// The tone `outputStatus` / `stageDisplayStatus` give their text.
pub fn tone(status: &str, stage: bool) -> Tone {
    match status {
        "Off" => Tone::Off,
        "Live" => Tone::Live,
        "Ready" | "Playing its own playlist" if !stage => Tone::Live,
        _ => Tone::Warn,
    }
}

/// `describeBinding`: "EPSON PJ · 1920×1080", or why there is no display.
pub fn describe_binding(
    id: Option<f64>,
    label: &str,
    size: &Option<Size>,
    displays: &[DisplayInfo],
) -> String {
    if id.is_none() {
        return "No display chosen".into();
    }
    match resolve_display(&binding(id, label, size), displays, &[]) {
        Some(d) => kairo_domain::displays::describe_display(&d.label, &d.size, d.scale_factor),
        None => format!(
            "{} · not connected",
            if label.is_empty() { "Display" } else { label }
        ),
    }
}

/// `describeCard`: "DeckLink Duo (2) · 1080p25", or `None` with no port name.
pub fn describe_card(
    output: &OverlayOutput,
    devices: &kairo_domain::video_devices::VideoDeviceSnapshot,
) -> Option<String> {
    let port = devices.ports.iter().find(|p| p.id == output.device_port_id);
    let mode = port.and_then(|p| p.modes.iter().find(|m| m.id == output.device_mode_id));
    let name = port
        .map(|p| p.port_name.as_str())
        .unwrap_or(output.device_port_label.as_str());
    (!name.is_empty()).then(|| match mode {
        Some(mode) if !mode.label.is_empty() => format!("{name} · {}", mode.label),
        _ => name.to_owned(),
    })
}

/// The line under an audience row's name.
pub fn row_detail(
    output: &OverlayOutput,
    displays: &[DisplayInfo],
    devices: &kairo_domain::video_devices::VideoDeviceSnapshot,
) -> String {
    match output.kind {
        OverlayOutputKind::Screen if output.source == OutputSource::Playlist => {
            "Own playlist".into()
        }
        OverlayOutputKind::Screen => describe_binding(
            output.display_id,
            &output.display_label,
            &output.display_size,
            displays,
        ),
        OverlayOutputKind::Device => {
            describe_card(output, devices).unwrap_or_else(|| "Video card".into())
        }
        OverlayOutputKind::Ndi => "NDI".into(),
        OverlayOutputKind::Syphon => "Syphon".into(),
    }
}

/// The audience list's order: screens, video cards, NDI feeds, Syphon.
pub fn audience_order(outputs: &[OverlayOutput]) -> Vec<&OverlayOutput> {
    let rank = |kind: OverlayOutputKind| match kind {
        OverlayOutputKind::Screen => 0,
        OverlayOutputKind::Device => 1,
        OverlayOutputKind::Ndi => 2,
        OverlayOutputKind::Syphon => 3,
    };
    let mut ordered: Vec<&OverlayOutput> = outputs.iter().collect();
    ordered.sort_by_key(|o| rank(o.kind));
    ordered
}

/// The detail header's second line: the display or card (else the kind),
/// then the status unless it is live, without repeats.
pub fn subtitle(resolution: Option<&str>, kind: &str, status: &str, live: bool) -> String {
    let first = resolution.unwrap_or(kind);
    if live || status.is_empty() || status == first {
        first.to_owned()
    } else {
        format!("{first} · {status}")
    }
}

#[cfg(test)]
mod sidebar_tests {
    use super::*;

    #[test]
    fn tones_follow_the_status_text() {
        assert_eq!(tone("Off", false), Tone::Off);
        assert_eq!(tone("Live", false), Tone::Live);
        assert_eq!(tone("Ready", false), Tone::Live);
        assert_eq!(tone("Playing its own playlist", false), Tone::Live);
        assert_eq!(tone("Ready", true), Tone::Warn);
        assert_eq!(tone("Checking…", false), Tone::Warn);
        assert_eq!(tone("Device disconnected", false), Tone::Warn);
    }

    #[test]
    fn subtitles_drop_a_live_status_and_repeats() {
        assert_eq!(
            subtitle(
                Some("DeckLink Duo (2)"),
                "Video card",
                "Device disconnected",
                false
            ),
            "DeckLink Duo (2) · Device disconnected"
        );
        assert_eq!(subtitle(None, "NDI", "Ready", true), "NDI");
        assert_eq!(subtitle(None, "Syphon", "Syphon", false), "Syphon");
        assert_eq!(subtitle(None, "Screen", "", false), "Screen");
    }

    #[test]
    fn unbound_and_missing_displays_say_why() {
        assert_eq!(describe_binding(None, "", &None, &[]), "No display chosen");
        assert_eq!(
            describe_binding(Some(4.0), "EPSON", &None, &[]),
            "EPSON · not connected"
        );
        assert_eq!(
            describe_binding(Some(4.0), "", &None, &[]),
            "Display · not connected"
        );
    }
}
