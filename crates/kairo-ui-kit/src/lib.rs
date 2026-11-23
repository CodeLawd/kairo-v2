//! Kairo's design system on GPUI Kit: tokens, theme, fonts, icons, motion
//! and the shared controls every operator screen uses.
//!
//! `../kairo-presenter/DESIGN.md` is the interface contract; the token values
//! come from its `index.css` and `tailwind.config.js`. Raw colours live only
//! in [`tokens`].
//!
//! Start an app with [`init`] after `gpui_kit::init`, and pass
//! [`KairoAssets`] to the application so the icons load.

#[cfg(any(test, feature = "test-support"))]
pub mod a11y_audit;
pub mod brand;
pub mod coach;
pub mod components;
pub mod control;
pub mod fonts;
pub mod gallery;
pub mod icons;
pub mod launch_mark;
pub mod lockup_paths;
pub mod motion;
pub mod onboarding;
pub mod theme;
pub mod tokens;

pub use control::ControlSize;
pub use icons::{KairoAssets, KairoIcon};
pub use theme::{KairoTheme, KairoThemeExt, set_appearance, set_reduced_motion, set_solid_chrome};
pub use tokens::{Appearance, Hue, KairoColors};

/// Shared width of the right-hand Live Rail, so the header seam stays aligned
/// with the rail's left border across every workspace.
pub struct LiveRailWidth(pub f32);
impl gpui_kit::Global for LiveRailWidth {}

/// Registers fonts, key bindings and the theme. Call once, after
/// `gpui_kit::init`.
/// How full a level meter is, 0 to 1, on a decibel scale from -60 dBFS
/// (empty) to 0 dBFS (full). A linear RMS scale leaves speech and music
/// (RMS around 0.02 to 0.1) in the first segment or two, so a working input
/// looked silent.
pub fn meter_fill(rms: f64) -> f64 {
    const FLOOR_DB: f64 = -60.0;
    // Silence, and NaN from a broken buffer, read as empty.
    if rms.is_nan() || rms <= 0.0 {
        return 0.0;
    }
    let db = 20.0 * rms.min(1.0).log10();
    ((db - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0)
}

pub fn init(appearance: Appearance, cx: &mut gpui_kit::App) {
    components::code_field::init(cx);
    theme::init(appearance, cx);
}
