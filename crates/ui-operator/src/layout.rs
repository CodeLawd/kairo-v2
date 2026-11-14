//! Panel widths and remembered frame sizes (`src/lib/operator-layout.ts`,
//! `useLiveRailWidth`, `App.tsx` `readFrameSize`/`writeFrameSize`).
//!
//! Electron kept these in `localStorage`; here they live in `ui-state.json`
//! under the same keys, through [`UiState`].

use serde_json::Value;

/// `operator-preview-width`: the live rail, shared by every page that shows it.
pub const PREVIEW_WIDTH_KEY: &str = "operator-preview-width";
/// `operator-transcript-width`: the Operator's left transcript panel.
pub const TRANSCRIPT_WIDTH_KEY: &str = "operator-transcript-width";

/// The keys `DraggableFrame` remembers sizes under (`App.tsx`).
pub const FRAME_SCREENS_KEY: &str = "kairo.frame.screens.v4";
pub const FRAME_PROPS_KEY: &str = "kairo.frame.props";
pub const FRAME_SETTINGS_KEY: &str = "kairo.frame.settings.v2";

/// The arrow-key step of a resize handle.
pub const KEYBOARD_STEP: f32 = 16.0;

/// The persisted view preferences (`ui-state.json`). Reads of a missing or
/// unreadable value return `None`; writes never fail the caller, matching
/// `localStorage` failures that Electron ignored.
pub trait UiState: Send + Sync + 'static {
    fn get(&self, key: &str) -> Option<Value>;
    fn set(&self, key: &str, value: Value);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelSide {
    /// The transcript panel; dragging right widens it.
    Left,
    /// The live rail; dragging left widens it.
    Right,
}

impl PanelSide {
    /// `OPERATOR_PANEL_DEFAULTS`.
    pub fn default_width(self) -> f32 {
        match self {
            PanelSide::Left => 280.0,
            PanelSide::Right => 460.0,
        }
    }

    /// `OPERATOR_PANEL_BOUNDS`.
    pub fn bounds(self) -> (f32, f32) {
        match self {
            PanelSide::Left => (180.0, 420.0),
            PanelSide::Right => (420.0, 680.0),
        }
    }

    /// Earlier startup sizes, treated as unset so the new defaults land once.
    fn legacy_defaults(self) -> &'static [f32] {
        match self {
            PanelSide::Left => &[240.0],
            PanelSide::Right => &[320.0],
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            PanelSide::Left => TRANSCRIPT_WIDTH_KEY,
            PanelSide::Right => PREVIEW_WIDTH_KEY,
        }
    }
}

/// `resizeOperatorPanel`: the width after a drag of `delta_x`, clamped and
/// rounded.
pub fn resize_panel(side: PanelSide, start: f32, delta_x: f32) -> f32 {
    let direction = match side {
        PanelSide::Left => 1.0,
        PanelSide::Right => -1.0,
    };
    let (min, max) = side.bounds();
    // `Math.round` rounds halves up; `f32::round` rounds them away from zero,
    // which only differs below zero, and the clamp floor is positive.
    (start + delta_x * direction).round().clamp(min, max)
}

/// `normalizeOperatorPanelWidth` over a stored value. Electron stored the
/// width as a string; a number written by this app is read too.
pub fn normalize_panel_width(side: PanelSide, stored: Option<&Value>) -> f32 {
    let fallback = side.default_width();
    let parsed = match stored {
        None | Some(Value::Null) => return resize_panel(side, fallback, 0.0),
        Some(Value::Number(n)) => n.as_f64(),
        // `Number(string)`: blank is 0, otherwise a finite decimal or NaN.
        Some(Value::String(s)) if s.trim().is_empty() => Some(0.0),
        Some(Value::String(s)) => s.trim().parse::<f64>().ok(),
        Some(_) => None,
    };
    let Some(parsed) = parsed.filter(|v| v.is_finite()) else {
        return resize_panel(side, fallback, 0.0);
    };
    let parsed = parsed as f32;
    if side.legacy_defaults().contains(&parsed) {
        return resize_panel(side, side.default_width(), 0.0);
    }
    resize_panel(side, parsed, 0.0)
}

/// A panel width read from and written to `ui-state`.
pub fn load_panel_width(state: &dyn UiState, side: PanelSide) -> f32 {
    normalize_panel_width(side, state.get(side.key()).as_ref())
}

/// Stored as a string, as `localStorage.setItem(key, String(width))` did, so
/// a profile moved back to Electron keeps it.
pub fn save_panel_width(state: &dyn UiState, side: PanelSide, width: f32) {
    state.set(side.key(), Value::String(format_width(width)));
}

fn format_width(width: f32) -> String {
    if width.fract() == 0.0 {
        format!("{}", width as i64)
    } else {
        width.to_string()
    }
}

/// `FrameSize`: a floating window's remembered size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameSize {
    pub w: f32,
    pub h: f32,
}

/// `clampFrameSize`: never larger than the app window less a 16 px margin on
/// each side, never smaller than `min`.
pub fn clamp_frame_size(size: FrameSize, min: FrameSize, window: (f32, f32)) -> FrameSize {
    let max_w = min.w.max(window.0 - 32.0);
    let max_h = min.h.max(window.1 - 32.0);
    FrameSize {
        w: size.w.max(min.w).min(max_w).round(),
        h: size.h.max(min.h).min(max_h).round(),
    }
}

/// `readFrameSize`: `None` unless both sides are finite numbers.
pub fn read_frame_size(state: &dyn UiState, key: &str) -> Option<FrameSize> {
    let value = match state.get(key)? {
        // Electron stored `JSON.stringify(size)` text.
        Value::String(text) => serde_json::from_str::<Value>(&text).ok()?,
        other => other,
    };
    let w = value.get("w")?.as_f64()?;
    let h = value.get("h")?.as_f64()?;
    (w.is_finite() && h.is_finite()).then_some(FrameSize {
        w: w as f32,
        h: h as f32,
    })
}

/// `writeFrameSize`, as the same JSON text Electron wrote.
pub fn write_frame_size(state: &dyn UiState, key: &str, size: FrameSize) {
    let text = serde_json::json!({ "w": size.w, "h": size.h }).to_string();
    state.set(key, Value::String(text));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{collections::HashMap, sync::Mutex};

    #[derive(Default)]
    struct Memory(Mutex<HashMap<String, Value>>);
    impl UiState for Memory {
        fn get(&self, key: &str) -> Option<Value> {
            self.0.lock().unwrap().get(key).cloned()
        }
        fn set(&self, key: &str, value: Value) {
            self.0.lock().unwrap().insert(key.into(), value);
        }
    }

    #[test]
    fn resize_follows_the_side_and_clamps() {
        assert_eq!(resize_panel(PanelSide::Left, 280.0, 30.4), 310.0);
        assert_eq!(resize_panel(PanelSide::Right, 460.0, 30.0), 430.0);
        assert_eq!(resize_panel(PanelSide::Right, 460.0, -1000.0), 680.0);
        assert_eq!(resize_panel(PanelSide::Left, 280.0, -1000.0), 180.0);
        assert_eq!(resize_panel(PanelSide::Left, 280.0, 0.5), 281.0);
    }

    #[test]
    fn stored_widths_normalize_like_electron() {
        let n = |side, v: Option<Value>| normalize_panel_width(side, v.as_ref());
        assert_eq!(n(PanelSide::Right, None), 460.0);
        assert_eq!(n(PanelSide::Right, Some(json!("abc"))), 460.0);
        assert_eq!(
            n(PanelSide::Right, Some(json!("320"))),
            460.0,
            "legacy default"
        );
        assert_eq!(
            n(PanelSide::Left, Some(json!("240"))),
            280.0,
            "legacy default"
        );
        assert_eq!(n(PanelSide::Right, Some(json!("500"))), 500.0);
        assert_eq!(n(PanelSide::Right, Some(json!(9999))), 680.0);
        assert_eq!(
            n(PanelSide::Left, Some(json!(""))),
            180.0,
            "Number('') is 0"
        );
        assert_eq!(n(PanelSide::Left, Some(json!(true))), 280.0);
    }

    #[test]
    fn widths_round_trip_as_strings() {
        let state = Memory::default();
        save_panel_width(&state, PanelSide::Left, 333.0);
        assert_eq!(state.get(TRANSCRIPT_WIDTH_KEY), Some(json!("333")));
        assert_eq!(load_panel_width(&state, PanelSide::Left), 333.0);
        assert_eq!(load_panel_width(&state, PanelSide::Right), 460.0);
    }

    #[test]
    fn frame_sizes_are_remembered_and_clamped() {
        let state = Memory::default();
        assert_eq!(read_frame_size(&state, FRAME_PROPS_KEY), None);
        write_frame_size(&state, FRAME_PROPS_KEY, FrameSize { w: 900.0, h: 600.0 });
        assert_eq!(
            read_frame_size(&state, FRAME_PROPS_KEY),
            Some(FrameSize { w: 900.0, h: 600.0 })
        );
        state.set(FRAME_SCREENS_KEY, json!("{\"w\":\"x\",\"h\":2}"));
        assert_eq!(read_frame_size(&state, FRAME_SCREENS_KEY), None);
        state.set(FRAME_SCREENS_KEY, json!("not json"));
        assert_eq!(read_frame_size(&state, FRAME_SCREENS_KEY), None);
        let min = FrameSize { w: 400.0, h: 300.0 };
        assert_eq!(
            clamp_frame_size(
                FrameSize {
                    w: 2000.0,
                    h: 100.0
                },
                min,
                (1280.0, 800.0)
            ),
            FrameSize {
                w: 1248.0,
                h: 300.0
            }
        );
        assert_eq!(
            clamp_frame_size(
                FrameSize {
                    w: 2000.0,
                    h: 2000.0
                },
                min,
                (300.0, 200.0)
            ),
            min,
            "a window smaller than min keeps min"
        );
    }
}
