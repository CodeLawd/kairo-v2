//! Each pane's state and drawing, as `impl SettingsWindow` blocks: General,
//! Verses on screen, Scripture (with the Bible library and verse timing),
//! Audio, Keyboard Shortcuts and Account. Licence & Devices is 073's
//! `LicensePanel`.

mod account;
mod audio;
mod general;
mod overlay;
mod scripture;
mod shortcuts;

pub use account::AccountPane;
pub use audio::AudioPane;
pub use general::{WorkspaceBusy, WorkspacePane};
pub use scripture::{LibraryPane, TracePane};
pub use shortcuts::ShortcutsPane;

/// Element ids, for tests.
pub mod ids {
    pub use super::account::ids as account;
    pub use super::audio::ids as audio;
    pub use super::general::ids as general;
    pub use super::overlay::ids as overlay;
    pub use super::scripture::ids as scripture;
    pub use super::shortcuts::ids as shortcuts;
}
