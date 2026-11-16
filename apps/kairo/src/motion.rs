//! The system's display accessibility settings: reduce motion
//! (`prefers-reduced-motion`), and reduce transparency or increase contrast
//! (`prefers-reduced-transparency`, `prefers-contrast: more`).
//!
//! Chromium reads them from the OS; GPUI does not, so Kairo asks AppKit at
//! launch and whenever the window regains focus, and tells GPUI
//! (`App::set_reduce_motion`) and the Kairo theme. Reduce transparency or
//! increase contrast draws the chrome solid (`index.css:686`; DECISIONS 106).
//! `KAIRO_REDUCED_MOTION=1` (or `0`) and `KAIRO_SOLID_CHROME=1` (or `0`)
//! override the system for testing. Windows and Linux are not read yet and
//! report motion allowed and translucent chrome (DECISIONS.md, 072 and 106).

use gpui_kit::App;

/// Whether the system asks for reduced motion.
pub fn system_prefers_reduced_motion() -> bool {
    if let Some(value) = std::env::var_os("KAIRO_REDUCED_MOTION") {
        return value == "1";
    }
    platform::reduce_motion()
}

/// Whether the system asks for reduced transparency or increased contrast,
/// either of which makes the chrome solid.
pub fn system_prefers_solid_chrome() -> bool {
    if let Some(value) = std::env::var_os("KAIRO_SOLID_CHROME") {
        return value == "1";
    }
    platform::reduce_transparency() || platform::increase_contrast()
}

#[cfg(target_os = "macos")]
mod platform {
    use objc2_app_kit::NSWorkspace;

    pub fn reduce_motion() -> bool {
        NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
    }

    pub fn reduce_transparency() -> bool {
        NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceTransparency()
    }

    pub fn increase_contrast() -> bool {
        NSWorkspace::sharedWorkspace().accessibilityDisplayShouldIncreaseContrast()
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    pub fn reduce_motion() -> bool {
        false
    }

    pub fn reduce_transparency() -> bool {
        false
    }

    pub fn increase_contrast() -> bool {
        false
    }
}

/// Applies the system settings to GPUI and the Kairo theme.
pub fn apply(cx: &mut App) {
    let reduced = system_prefers_reduced_motion();
    if cx.reduce_motion() != reduced {
        tracing::info!(reduced, "[Motion] reduced motion");
    }
    cx.set_reduce_motion(reduced);
    kairo_ui_kit::set_reduced_motion(reduced, cx);
    let solid = system_prefers_solid_chrome();
    if kairo_ui_kit::KairoThemeExt::kairo(cx).solid_chrome != solid {
        tracing::info!(solid, "[Motion] solid chrome");
    }
    kairo_ui_kit::set_solid_chrome(solid, cx);
}
