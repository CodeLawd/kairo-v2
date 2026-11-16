//! The Settings window (prompt 074): `components/settings/`.
//!
//! [`SettingsWindow`] is the view; [`SettingsBackend`] is what it asks of the
//! app; [`model`] holds the rules, testable without a window.

pub mod audio_rail;
pub mod backend;
pub mod model;
pub mod panes;
pub mod widgets;
pub mod window;

pub use audio_rail::{AudioRail, OpenAudioSettings};
pub use backend::{
    BackendEvent, InputDevice, MeterEvent, MeterHandle, Reply, SettingsBackend, UpdateAction,
};
pub use model::Section;
pub use window::{FRAME, FRAME_MIN, SettingsEvent, SettingsWindow, ids};
