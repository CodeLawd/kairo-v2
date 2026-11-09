//! The workspace folder and the song file format, from `src/lib/workspace.ts`.

use crate::lyrics::{LyricsSectionType, LyricsSource};
use crate::serde_util::{Extra, present, required};
use serde::{Deserialize, Serialize};

/// `AppSettings.workspace`. `folder` is absolute, or `''` for the default.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceSettings {
    pub folder: String,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceInfo {
    pub root: String,
    pub songs_dir: String,
    pub media_dir: String,
    pub is_default: bool,
    pub ready: bool,
    #[serde(deserialize_with = "required")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaFolderMigration {
    pub current_folder: String,
    pub target_folder: String,
    pub needed: bool,
    pub file_count: f64,
}

/// What a Songs-folder rescan changed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceSongSync {
    pub imported: f64,
    pub removed: f64,
    pub exported: f64,
}

/// `SONG_FILE_VERSION`.
pub const SONG_FILE_VERSION: f64 = 1.0;
/// `SONG_FILE_EXT`.
pub const SONG_FILE_EXT: &str = ".song.json";

/// One `<Songs>/<slug>.song.json` file, as `serializeSongFile` writes it.
///
/// Files are hand-edited, so `parseSongFile` coerces rather than trusts them
/// (prompt 014). This type is the canonical shape; unknown keys survive.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SongFile {
    /// Format version, [`SONG_FILE_VERSION`].
    pub kairo_song: f64,
    pub id: String,
    pub title: String,
    pub artist: String,
    pub copyright: String,
    pub ccli_number: String,
    pub is_favorite: bool,
    pub source: LyricsSource,
    pub created_at: f64,
    pub updated_at: f64,
    pub sections: Vec<SongFileSection>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SongFileSection {
    #[serde(rename = "type")]
    pub kind: LyricsSectionType,
    pub label: String,
    pub lines: Vec<String>,
    /// Written only when some line has a colour.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub line_colors: Option<Vec<Option<String>>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub hotkey: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}
