//! Pure settings transformations used by first-run setup.
use crate::displays::{DisplayBinding, DisplayBound};
use crate::overlay::OverlayOutputKind;
use crate::overlay_defaults::make_projector_output;
use crate::program::{PresentationSettings, StageDisplayConfig};
use crate::settings::{AppSettings, OverlaySettings};

/// The church profile to save before setup moves on: the org's name and
/// this computer's zone, filling only what is empty. `None` when nothing
/// changes.
pub fn seeded_church(
    settings: &AppSettings,
    org: Option<&str>,
    local_zone: &str,
) -> Option<crate::settings::ChurchProfile> {
    let church = &settings.church;
    let mut next = church.clone();
    if church.name.trim().is_empty() {
        next.name = org.map(str::trim).unwrap_or("").to_owned();
    } else {
        next.name = church.name.trim().to_owned();
    }
    if church.timezone.is_empty() {
        next.timezone = local_zone.to_owned();
    }
    (next.name != church.name || next.timezone != church.timezone).then_some(next)
}

/// The two rows' targets and what else is on displays.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct OutputTargets {
    /// The audience screen.
    pub audience: DisplayBound,
    /// Displays taken for the audience row.
    pub audience_others: Vec<DisplayBound>,
    /// The first stage display.
    pub stage: DisplayBound,
    /// Displays taken for the stage row.
    pub stage_others: Vec<DisplayBound>,
}

fn bound_of(
    name: &str,
    id: Option<f64>,
    label: &str,
    size: Option<crate::overlay::Size>,
) -> DisplayBound {
    DisplayBound {
        name: name.into(),
        binding: DisplayBinding {
            display_id: id,
            display_label: label.into(),
            display_size: size,
        },
    }
}

/// `StepOutput`'s targets from settings.
pub fn output_targets(settings: &AppSettings) -> OutputTargets {
    let outputs = &settings.overlay.outputs;
    let screen = outputs
        .iter()
        .find(|o| o.kind == OverlayOutputKind::Screen)
        .cloned()
        .unwrap_or_else(|| make_projector_output(0.0));
    let stages = &settings.presentation.stage_displays;
    let enabled_screens: Vec<_> = outputs
        .iter()
        .filter(|o| o.kind == OverlayOutputKind::Screen && o.enabled)
        .collect();
    let enabled_stages: Vec<_> = stages.iter().filter(|s| s.enabled).collect();
    let screen_bound = |o: &crate::overlay::OverlayOutput| {
        bound_of(
            &o.name,
            o.display_id,
            &o.display_label,
            o.display_size.clone(),
        )
    };
    let stage_bound = |s: &StageDisplayConfig| {
        bound_of(
            &s.name,
            s.display_id,
            &s.display_label,
            s.display_size.clone(),
        )
    };
    let audience_others = enabled_screens
        .iter()
        .filter(|o| o.id != screen.id)
        .map(|o| screen_bound(o))
        .chain(enabled_stages.iter().map(|s| stage_bound(s)))
        .collect();
    let first_stage = stages.first();
    let stage_others = enabled_screens
        .iter()
        .map(|o| screen_bound(o))
        .chain(
            enabled_stages
                .iter()
                .filter(|s| Some(s.id.as_str()) != first_stage.map(|f| f.id.as_str()))
                .map(|s| stage_bound(s)),
        )
        .collect();
    OutputTargets {
        audience: bound_of(
            "Audience screen",
            screen.display_id,
            &screen.display_label,
            screen.display_size.clone(),
        ),
        audience_others,
        stage: match first_stage {
            Some(s) => stage_bound(s),
            None => bound_of("Stage display", None, "", None),
        },
        stage_others,
    }
}

/// `stage-<Date.now() base 36>`.
pub fn new_stage_display(binding: &DisplayBinding, now: u64) -> StageDisplayConfig {
    let mut config: StageDisplayConfig = serde_default_stage();
    config.id = format!("stage-{}", base36(now));
    config.name = "Stage display".into();
    config.enabled = true;
    config.display_id = binding.display_id;
    config.display_label = binding.display_label.clone();
    config.display_size = binding.display_size.clone();
    config.show_next = true;
    config.show_clock = true;
    config.show_timer = true;
    config
}

fn serde_default_stage() -> StageDisplayConfig {
    StageDisplayConfig {
        id: String::new(),
        name: String::new(),
        enabled: false,
        display_id: None,
        display_label: String::new(),
        display_size: None,
        show_next: true,
        show_clock: true,
        show_timer: true,
        extra: Default::default(),
    }
}

/// `Number.prototype.toString(36)`.
pub fn base36(mut value: u64) -> String {
    if value == 0 {
        return "0".into();
    }
    let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    while value > 0 {
        out.push(digits[(value % 36) as usize]);
        value /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

#[derive(Clone, Debug)]
pub enum SetupOutputChange {
    Overlay(Box<OverlaySettings>),
    Presentation(Box<PresentationSettings>),
}

/// Bind only setup's audience output or first stage display. The caller supplies time.
pub fn bind_setup_output(
    settings: &AppSettings,
    audience: bool,
    binding: &DisplayBinding,
    now: u64,
) -> Option<SetupOutputChange> {
    let enabled = binding.display_id.is_some();
    let save = if audience {
        let mut overlay = settings.overlay.clone();
        let screen = overlay
            .outputs
            .iter()
            .find(|o| o.kind == OverlayOutputKind::Screen)
            .cloned()
            .unwrap_or_else(|| make_projector_output(0.0));
        let mut next = screen.clone();
        next.display_id = binding.display_id;
        next.display_label = binding.display_label.clone();
        next.display_size = binding.display_size.clone();
        next.enabled = enabled;
        if overlay.outputs.iter().any(|o| o.id == screen.id) {
            for output in overlay.outputs.iter_mut() {
                if output.id == screen.id {
                    *output = next.clone();
                }
            }
        } else {
            overlay.outputs.push(next);
        }
        SetupOutputChange::Overlay(Box::new(overlay))
    } else {
        let mut presentation = settings.presentation.clone();
        if let Some(first) = presentation.stage_displays.first_mut() {
            first.display_id = binding.display_id;
            first.display_label = binding.display_label.clone();
            first.display_size = binding.display_size.clone();
            first.enabled = enabled;
        } else if enabled {
            presentation.stage_displays = vec![new_stage_display(binding, now)];
        } else {
            return None;
        }
        SetupOutputChange::Presentation(Box::new(presentation))
    };
    Some(save)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn settings() -> AppSettings {
        AppSettings::defaults(crate::overlay_defaults::DEFAULT_OVERLAY_SETTINGS.clone())
    }
    fn binding(id: Option<f64>) -> DisplayBinding {
        DisplayBinding {
            display_id: id,
            display_label: "Projector".into(),
            display_size: None,
        }
    }
    #[test]
    fn binding_a_stage_preserves_other_settings_and_uses_supplied_id_time() {
        let original = settings();
        let Some(SetupOutputChange::Presentation(next)) =
            bind_setup_output(&original, false, &binding(Some(7.)), 36)
        else {
            panic!("stage change");
        };
        assert_eq!(next.stage_displays.len(), 1);
        assert_eq!(next.stage_displays[0].id, "stage-10");
        assert_eq!(next.stage_displays[0].display_id, Some(7.));
        assert!(
            next.stage_displays[0].show_next
                && next.stage_displays[0].show_clock
                && next.stage_displays[0].show_timer
        );
        assert!(original.presentation.stage_displays.is_empty());
        assert!(bind_setup_output(&original, false, &binding(None), 0).is_none());
    }
    #[test]
    fn audience_binding_changes_only_the_selected_output() {
        let mut original = settings();
        original
            .overlay
            .outputs
            .push(crate::overlay_defaults::make_overlay_output(
                "other",
                OverlayOutputKind::Screen,
            ));
        let other = original.overlay.outputs.last().unwrap().clone();
        let Some(SetupOutputChange::Overlay(next)) =
            bind_setup_output(&original, true, &binding(Some(4.)), 0)
        else {
            panic!("audience change");
        };
        assert_eq!(next.outputs[0].display_id, Some(4.));
        assert_eq!(next.outputs.last(), Some(&other));
        let targets = output_targets(&AppSettings {
            overlay: *next,
            ..original
        });
        assert_eq!(targets.audience.binding.display_id, Some(4.));
    }
    #[test]
    fn seeding_church_is_idempotent_and_preserves_user_values() {
        let mut settings = settings();
        let church = seeded_church(&settings, Some(" Grace "), "Africa/Lagos").unwrap();
        assert_eq!(church.name, "Grace");
        settings.church = church;
        assert!(seeded_church(&settings, Some("Other"), "Europe/London").is_none());
    }
}
