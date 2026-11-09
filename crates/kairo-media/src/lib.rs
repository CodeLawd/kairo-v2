//! Folder-backed background media and house audio libraries (prompt 043).
//!
//! Ports `src/main/services/media`, `src/main/services/tracks` and
//! `src/main/services/ndi/media-allowlist.ts`.

pub mod allowlist;
pub mod clipboard;
pub mod library;
pub mod paths;
pub mod poster;
pub mod publish;
pub mod settings;
pub mod tracks;

pub use library::{MediaError, MediaService};
pub use settings::SettingsPort;
pub use tracks::TracksService;
