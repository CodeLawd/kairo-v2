//! The engine's displays as the operator and the stored bindings see them.

use kairo_domain::overlay::{DisplayBounds, DisplayInfo, Size};
use kairo_engine_proto as proto;

/// Electron's `toDisplayInfo`: bounds and size in display-independent
/// pixels, as `screen.getAllDisplays()` gave them and as bindings store them.
/// The engine reports physical pixels.
pub fn display_info(display: &proto::DisplayInfo) -> DisplayInfo {
    let scale = if display.scale_factor.is_finite() && display.scale_factor > 0. {
        display.scale_factor
    } else {
        1.
    };
    let (width, height) = display.logical_size();
    DisplayInfo {
        id: display.id as f64,
        label: display.label.clone(),
        bounds: DisplayBounds {
            x: (display.bounds.x as f64 / scale).round(),
            y: (display.bounds.y as f64 / scale).round(),
            width: width as f64,
            height: height as f64,
        },
        size: Size {
            width: width as f64,
            height: height as f64,
            extra: Default::default(),
        },
        scale_factor: display.scale_factor,
        primary: display.primary,
        hosts_main_window: display.hosts_main_window,
    }
}

/// `listDisplays`.
pub fn list(displays: &[proto::DisplayInfo]) -> Vec<DisplayInfo> {
    displays.iter().map(display_info).collect()
}
