//! Background media and house audio, from `src/lib/ipc.ts` and `src/lib/tracks.ts`.

use crate::overlay::MediaFit;
use crate::serde_util::{Extra, nullable, present, required, string_enum};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// `AppSettings.media`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MediaSettings {
    pub folder: String,
    /// Deprecated: playlists now live in `<folder>/.kairo/playlists.json`.
    pub playlists: Vec<MediaPlaylist>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `AppSettings.tracks`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TracksSettings {
    pub folder: String,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrackItem {
    pub id: String,
    pub name: String,
    pub path: String,
    pub ext: String,
    pub size: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TracksLibrary {
    pub folder: String,
    pub items: Vec<TrackItem>,
    #[serde(deserialize_with = "required")]
    pub live_id: Option<String>,
    pub live_paused: bool,
    #[serde(deserialize_with = "required")]
    pub error: Option<String>,
}

string_enum!(
    pub enum MediaKind {
        Video = "video",
        Image = "image",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaItem {
    pub id: String,
    pub name: String,
    pub path: String,
    pub kind: MediaKind,
    pub ext: String,
    pub folder: String,
    pub size_bytes: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MediaFolder {
    pub id: String,
    pub name: String,
    pub count: f64,
}

/// An operator-built collection of media item ids. Persisted in the media
/// manifest and, before that, in settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaPlaylist {
    pub id: String,
    pub name: String,
    pub item_ids: Vec<String>,
    pub created_at: f64,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Per-file playback settings stored in the media manifest.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaPlayback {
    #[serde(rename = "loop")]
    pub looped: bool,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub start_seconds: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "nullable")]
    pub end_seconds: Option<Option<f64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub rate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "nullable")]
    pub fit: Option<Option<MediaFit>>,
    pub hue: f64,
    pub saturation: f64,
    pub brightness: f64,
    pub contrast: f64,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaLibrary {
    pub folder: String,
    pub folders: Vec<MediaFolder>,
    pub items: Vec<MediaItem>,
    pub playlists: Vec<MediaPlaylist>,
    pub playback: BTreeMap<String, MediaPlayback>,
    #[serde(deserialize_with = "required")]
    pub live_item_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live_playback: Option<MediaPlayback>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live_cue: Option<f64>,
    pub live_paused: bool,
    #[serde(deserialize_with = "required")]
    pub error: Option<String>,
}
