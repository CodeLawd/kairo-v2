use serde::{Deserialize, Serialize};

pub const MAX_SCRIPT_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Script {
    pub id: String,
    pub title: String,
    pub body: String,
    pub updated_at_ms: i64,
}

/// Map a guide position through a replacement bounded by the common prefix
/// and suffix. An anchor inside the replacement follows the nearer edge.
pub fn remap_anchor(old: &str, new: &str, mut anchor: usize) -> usize {
    anchor = anchor.min(old.len());
    while !old.is_char_boundary(anchor) {
        anchor -= 1;
    }
    let prefix = old
        .chars()
        .zip(new.chars())
        .take_while(|(a, b)| a == b)
        .map(|(c, _)| c.len_utf8())
        .sum::<usize>();
    let suffix = old[prefix..]
        .chars()
        .rev()
        .zip(new[prefix..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(c, _)| c.len_utf8())
        .sum::<usize>();
    let old_end = old.len() - suffix;
    let new_end = new.len() - suffix;
    if anchor >= old_end {
        new_end + (anchor - old_end)
    } else if anchor <= prefix || anchor - prefix <= old_end - anchor {
        anchor.min(prefix)
    } else {
        new_end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_unchanged_text_and_anchors_inside_unicode_replacements() {
        assert_eq!(remap_anchor("Olá mundo", "Olá belo mundo", 5), 10);
        assert_eq!(remap_anchor("Olá mundo", "Olá belo mundo", 2), 2);
        assert_eq!(remap_anchor("a世界z", "a🌎z", 4), 1);
        assert_eq!(remap_anchor("a世界éz", "a🌎z", 7), 5);
        assert_eq!(remap_anchor("a世界z", "a🌎z", 8), 6);
        assert_eq!(remap_anchor("世界", "", 2), 0);
    }
}

#[cfg(test)]
#[test]
fn legacy_stage_settings_default_to_confidence_monitor() {
    use crate::program::{StageDisplayConfig, StageDisplayMode, normalize_presentation_settings};

    let mut legacy = serde_json::json!({
        "id": "stage", "name": "Stage", "enabled": true,
        "displayId": null, "displayLabel": "", "displaySize": null,
        "showNext": true, "showClock": true, "showTimer": true
    });
    let stage: StageDisplayConfig = serde_json::from_value(legacy.clone()).unwrap();
    assert_eq!(stage.mode, StageDisplayMode::ConfidenceMonitor);
    assert_eq!(serde_json::to_value(stage).unwrap(), legacy);
    let normalize = |stage: &serde_json::Value| {
        normalize_presentation_settings(&serde_json::json!({"stageDisplays": [stage]}))
            .stage_displays[0]
            .mode
    };
    assert_eq!(normalize(&legacy), StageDisplayMode::ConfidenceMonitor);
    legacy["mode"] = serde_json::json!("teleprompter");
    let stage: StageDisplayConfig = serde_json::from_value(legacy.clone()).unwrap();
    assert_eq!(stage.mode, StageDisplayMode::Teleprompter);
    assert_eq!(normalize(&legacy), StageDisplayMode::Teleprompter);
    assert_eq!(serde_json::to_value(stage).unwrap(), legacy);
}

/// Stage content on the native engine connection. Confidence monitor
/// payloads from older clients remain readable during an engine restart.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StagePayload {
    Tagged(StageContent),
    LegacyConfidence(crate::program::StageView),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", content = "view", rename_all = "camelCase")]
pub enum StageContent {
    ConfidenceMonitor(crate::program::StageView),
    Teleprompter(TeleprompterView),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeleprompterView {
    #[serde(default)]
    pub script_id: String,
    pub revision: u64,
    pub text: String,
    pub font_px: f32,
    #[serde(default)]
    pub anchor: usize,
    /// Scroll speed in shaped lines per second. Carried with the content so
    /// a reload, a reopened window or an engine restart keeps the operator's
    /// speed.
    #[serde(default = "default_speed")]
    pub speed: f64,
    /// Flip the text left to right for a beam-splitter rig.
    #[serde(default)]
    pub mirror: bool,
}

/// About 160 words a minute at the default text size.
pub const DEFAULT_SPEED: f64 = 0.4;
pub const MIN_SPEED: f64 = 0.1;
pub const MAX_SPEED: f64 = 20.0;

fn default_speed() -> f64 {
    DEFAULT_SPEED
}

/// A speed the engine can use: finite and inside the supported range.
pub fn clamp_speed(speed: f64) -> f64 {
    if speed.is_finite() {
        speed.clamp(MIN_SPEED, MAX_SPEED)
    } else {
        DEFAULT_SPEED
    }
}
