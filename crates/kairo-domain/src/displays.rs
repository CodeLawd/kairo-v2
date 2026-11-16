//! Display binding for `screen` outputs and output frame shapes, from
//! `src/lib/displays.ts`.
//!
//! A screen output remembers its display by id, and by label and size as a
//! second chance after a replug. There is deliberately no third chance.

use crate::js::{self, math_max, math_min, math_round};
use crate::overlay::{DisplayBounds, DisplayInfo, OutputAspect, Size};
use crate::serde_util::Extra;

/// `Pick<OverlayOutput, 'displayId' | 'displayLabel' | 'displaySize'>`.
#[derive(Clone, Debug, PartialEq)]
pub struct DisplayBinding {
    pub display_id: Option<f64>,
    pub display_label: String,
    pub display_size: Option<Size>,
}

/// `resolveDisplay`: the bound display, re-matched by label and size when its
/// id changed, or `None` when it is not connected, ambiguous, or already
/// claimed by another screen. A result with a different id should be persisted.
pub fn resolve_display<'a>(
    binding: &DisplayBinding,
    displays: &'a [DisplayInfo],
    claimed_ids: &[f64],
) -> Option<&'a DisplayInfo> {
    let display_id = binding.display_id?;
    let claimed = |id: f64| claimed_ids.contains(&id);

    if let Some(by_id) = displays.iter().find(|d| d.id == display_id) {
        return (!claimed(by_id.id)).then_some(by_id);
    }

    let label = js::trim(&binding.display_label);
    let size = binding.display_size.as_ref()?;
    if label.is_empty() {
        return None;
    }
    let matches: Vec<&DisplayInfo> = displays
        .iter()
        .filter(|d| d.label == label && d.size.width == size.width && d.size.height == size.height)
        .collect();
    // Two identical projectors: guessing would be the "some other screen" fallback.
    match matches.as_slice() {
        [only] if !claimed(only.id) => Some(only),
        _ => None,
    }
}

/// `bindingForDisplay`: what to store when an operator picks `display`.
pub fn binding_for_display(display: &DisplayInfo) -> DisplayBinding {
    DisplayBinding {
        display_id: Some(display.id),
        display_label: display.label.clone(),
        display_size: Some(size(display.size.width, display.size.height)),
    }
}

fn size(width: f64, height: f64) -> Size {
    Size {
        width,
        height,
        extra: Extra::new(),
    }
}

/// `describeDisplay`: "DELL U2720Q · 2560×1440", in physical pixels.
pub fn describe_display(label: &str, size: &Size, scale_factor: f64) -> String {
    let name = js::trim(label);
    let name = if name.is_empty() { "Display" } else { name };
    let width = math_round(size.width * scale_factor);
    let height = math_round(size.height * scale_factor);
    format!(
        "{name} · {}×{}",
        js::number_to_string(width),
        js::number_to_string(height)
    )
}

/// One `OUTPUT_ASPECTS` entry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutputAspectOption {
    pub value: OutputAspect,
    pub label: &'static str,
    pub ratio: Option<(f64, f64)>,
}

/// `OUTPUT_ASPECTS`.
pub const OUTPUT_ASPECTS: &[OutputAspectOption] = &[
    option(
        OutputAspect::Letterbox,
        "16:9 — TVs, most projectors",
        Some((16.0, 9.0)),
    ),
    option(
        OutputAspect::Ratio16x10,
        "16:10 — many projectors and laptops",
        Some((16.0, 10.0)),
    ),
    option(
        OutputAspect::Ratio4x3,
        "4:3 — older projectors",
        Some((4.0, 3.0)),
    ),
    option(
        OutputAspect::Ratio21x9,
        "21:9 — ultrawide",
        Some((21.0, 9.0)),
    ),
    option(
        OutputAspect::Ratio32x9,
        "32:9 — super-wide LED wall",
        Some((32.0, 9.0)),
    ),
    option(
        OutputAspect::Ratio9x16,
        "9:16 — portrait TV",
        Some((9.0, 16.0)),
    ),
    option(
        OutputAspect::Ratio1x1,
        "1:1 — square LED panel",
        Some((1.0, 1.0)),
    ),
    option(OutputAspect::Fill, "Match the display", None),
    option(OutputAspect::Custom, "Custom ratio…", None),
];

const fn option(
    value: OutputAspect,
    label: &'static str,
    ratio: Option<(f64, f64)>,
) -> OutputAspectOption {
    OutputAspectOption {
        value,
        label,
        ratio,
    }
}

/// `OUTPUT_ASPECT_VALUES`.
pub const OUTPUT_ASPECT_VALUES: &[OutputAspect] = OutputAspect::ALL;

/// A width and height pair for the ratio helpers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dimensions {
    pub width: f64,
    pub height: f64,
}

/// `aspectRatioOf`: width ÷ height, or `None` for `fill` on an unknown display.
pub fn aspect_ratio_of(
    aspect: OutputAspect,
    custom: Option<Dimensions>,
    display: Option<Dimensions>,
) -> Option<f64> {
    let valid = |d: Option<Dimensions>| d.filter(|d| d.width > 0.0 && d.height > 0.0);
    match aspect {
        OutputAspect::Fill => valid(display).map(|d| d.width / d.height),
        OutputAspect::Custom => Some(valid(custom).map_or(16.0 / 9.0, |d| d.width / d.height)),
        _ => {
            let (w, h) = OUTPUT_ASPECTS
                .iter()
                .find(|o| o.value == aspect)
                .and_then(|o| o.ratio)
                .unwrap_or((16.0, 9.0));
            Some(w / h)
        }
    }
}

/// `frameHeightFor`: frame height in 1920-wide theme pixels, 360 to 4320.
pub fn frame_height_for(size: Dimensions, aspect: OutputAspect, custom: Option<Dimensions>) -> f64 {
    let ratio = aspect_ratio_of(aspect, custom, Some(size)).unwrap_or(16.0 / 9.0);
    math_max(360.0, math_min(4320.0, math_round(1920.0 / ratio)))
}

/// The `testPatternLayout` result: the window and the frame inside it as
/// fractions of the window.
#[derive(Clone, Debug, PartialEq)]
pub struct TestPatternLayout {
    pub bounds: DisplayBounds,
    pub frame: Dimensions,
}

/// `testPatternLayout`: where a test pattern goes, matching where the real
/// output opens (the whole display, or the half-size rehearsal window).
pub fn test_pattern_layout(
    display: &DisplayBounds,
    rehearsal: bool,
    frame_height: f64,
) -> TestPatternLayout {
    let mut bounds = display.clone();
    if rehearsal {
        let width = math_round(display.width / 2.0);
        let height = math_round((width * frame_height) / 1920.0);
        bounds = DisplayBounds {
            width,
            height,
            x: display.x + math_round((display.width - width) / 2.0),
            y: display.y + math_round((display.height - height) / 2.0),
        };
    }
    let scale = math_min(bounds.width / 1920.0, bounds.height / frame_height);
    TestPatternLayout {
        frame: Dimensions {
            width: (1920.0 * scale) / bounds.width,
            height: (frame_height * scale) / bounds.height,
        },
        bounds,
    }
}

/// Something bound to a display (`DisplayBound`).
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct DisplayBound {
    /// Its name, for "used by …".
    pub name: String,
    /// Its binding.
    pub binding: DisplayBinding,
}

impl DisplayBound {
    pub fn new(name: impl Into<String>, binding: DisplayBinding) -> Self {
        Self {
            name: name.into(),
            binding,
        }
    }
}
