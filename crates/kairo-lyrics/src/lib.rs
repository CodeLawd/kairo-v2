//! Local SQLite lyrics library and workspace song-file reconciliation.

mod import;
mod library;
pub mod online;
mod online_service;

pub use import::{parse_open_lyrics, parse_section_label};
pub use library::{FolderSync, LyricsError, LyricsService};
pub use online_service::song_match_key;
