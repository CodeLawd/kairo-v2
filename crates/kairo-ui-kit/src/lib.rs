//! Kairo's design system on GPUI Kit: tokens, theme, fonts, icons, motion
//! and the shared controls every operator screen uses.
//!
//! `../kairo-presenter/DESIGN.md` is the interface contract; the token values
//! come from its `index.css` and `tailwind.config.js`. Raw colours live only
//! in [`tokens`].
//!
//! Start an app with [`init`] after `gpui_kit::init`, and pass
//! [`KairoAssets`] to the application so the icons load.

pub mod brand;
pub mod components;
pub mod fonts;
pub mod gallery;
pub mod icons;
pub mod launch_mark;
pub mod lockup_paths;
pub mod motion;
pub mod theme;
pub mod tokens;

pub use icons::{KairoAssets, KairoIcon};
pub use theme::{KairoTheme, KairoThemeExt, set_appearance, set_reduced_motion};
pub use tokens::{Appearance, Hue, KairoColors};

/// Registers fonts, key bindings and the theme. Call once, after
/// `gpui_kit::init`.
pub fn init(appearance: Appearance, cx: &mut gpui_kit::App) {
    components::code_field::init(cx);
    theme::init(appearance, cx);
}
