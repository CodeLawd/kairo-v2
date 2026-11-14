//! View → Zoom In, Zoom Out, Actual Size: Chromium's page zoom, as a
//! persisted interface scale. GPUI Kit's rem is the theme's base font size,
//! so scaling it scales type, spacing and controls together.
//!
//! Chromium remembered a zoom per origin; the app had one origin, so one
//! number per profile. It lives in `ui-scale.json` in the user-data folder,
//! apart from `settings.json`, so Electron never reads a key it does not know.

use std::path::{Path, PathBuf};

/// Chromium's zoom steps (`kPresetZoomFactors`).
pub const STEPS: [f32; 17] = [
    0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0, 5.0,
];

/// The file name in the user-data folder.
pub const FILE: &str = "ui-scale.json";

/// The interface scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiScale(f32);

impl Default for UiScale {
    fn default() -> Self {
        Self(1.0)
    }
}

impl UiScale {
    /// A scale, snapped to the nearest step. Non-finite or non-positive
    /// values are actual size.
    pub fn new(value: f32) -> Self {
        if !value.is_finite() || value <= 0.0 {
            return Self::default();
        }
        let nearest = STEPS
            .iter()
            .copied()
            .min_by(|a, b| (a - value).abs().total_cmp(&(b - value).abs()))
            .unwrap_or(1.0);
        Self(nearest)
    }

    /// The factor.
    pub fn factor(self) -> f32 {
        self.0
    }

    /// One step larger, stopping at the largest.
    pub fn zoom_in(self) -> Self {
        Self(
            STEPS
                .iter()
                .copied()
                .find(|s| *s > self.0 + 1e-4)
                .unwrap_or(self.0),
        )
    }

    /// One step smaller, stopping at the smallest.
    pub fn zoom_out(self) -> Self {
        Self(
            STEPS
                .iter()
                .rev()
                .copied()
                .find(|s| *s < self.0 - 1e-4)
                .unwrap_or(self.0),
        )
    }

    /// The path in a user-data folder.
    pub fn path(user_data: &Path) -> PathBuf {
        user_data.join(FILE)
    }

    /// Reads the saved scale. A missing or unreadable file is actual size.
    pub fn load(user_data: &Path) -> Self {
        std::fs::read_to_string(Self::path(user_data))
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .and_then(|value| value.get("scale").and_then(serde_json::Value::as_f64))
            .map(|scale| Self::new(scale as f32))
            .unwrap_or_default()
    }

    /// Saves the scale. A failed write is logged; the scale still applies
    /// for this launch, as a remembered size is only a convenience.
    pub fn save(self, user_data: &Path) {
        let text = serde_json::json!({ "scale": self.0 }).to_string();
        let path = Self::path(user_data);
        let temp = path.with_extension("json.tmp");
        let result = std::fs::write(&temp, text).and_then(|()| std::fs::rename(&temp, &path));
        if let Err(error) = result {
            tracing::warn!(%error, "[Shell] could not save the interface scale");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_up_and_down_and_stops_at_the_ends() {
        let s = UiScale::default();
        assert_eq!(s.zoom_in().factor(), 1.1);
        assert_eq!(s.zoom_out().factor(), 0.9);
        assert_eq!(UiScale::new(5.0).zoom_in().factor(), 5.0);
        assert_eq!(UiScale::new(0.25).zoom_out().factor(), 0.25);
    }

    #[test]
    fn odd_values_snap_or_reset() {
        assert_eq!(UiScale::new(1.2).factor(), 1.25);
        assert_eq!(UiScale::new(f32::NAN).factor(), 1.0);
        assert_eq!(UiScale::new(-2.0).factor(), 1.0);
    }

    #[test]
    fn saves_and_loads_and_tolerates_junk() {
        let dir = std::env::temp_dir().join(format!("kairo-ui-scale-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(UiScale::load(&dir).factor(), 1.0);
        UiScale::new(1.5).save(&dir);
        assert_eq!(UiScale::load(&dir).factor(), 1.5);
        std::fs::write(UiScale::path(&dir), "{not json").unwrap();
        assert_eq!(UiScale::load(&dir).factor(), 1.0);
        std::fs::remove_dir_all(&dir).ok();
    }
}
