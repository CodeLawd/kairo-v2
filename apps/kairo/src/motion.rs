//! The system's reduce-motion setting (`prefers-reduced-motion`).
//!
//! Chromium reads it from the OS; GPUI does not, so Kairo asks AppKit at
//! launch and whenever the window regains focus, and tells both GPUI
//! (`App::set_reduce_motion`) and the Kairo theme. `KAIRO_REDUCED_MOTION=1`
//! (or `0`) overrides the system for testing. Windows and Linux are not
//! read yet and report motion allowed (DECISIONS.md, 072).

use gpui_kit::App;

/// Whether the system asks for reduced motion.
pub fn system_prefers_reduced_motion() -> bool {
    if let Some(value) = std::env::var_os("KAIRO_REDUCED_MOTION") {
        return value == "1";
    }
    platform()
}

#[cfg(target_os = "macos")]
fn platform() -> bool {
    use objc2_app_kit::NSWorkspace;
    NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}

#[cfg(not(target_os = "macos"))]
fn platform() -> bool {
    false
}

/// Applies the system setting to GPUI and the Kairo theme.
pub fn apply(cx: &mut App) {
    let reduced = system_prefers_reduced_motion();
    if cx.reduce_motion() != reduced {
        tracing::info!(reduced, "[Motion] reduced motion");
    }
    cx.set_reduce_motion(reduced);
    kairo_ui_kit::set_reduced_motion(reduced, cx);
}
