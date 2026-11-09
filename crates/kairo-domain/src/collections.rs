//! Libraries and setlists, from `src/lib/libraries.ts` and `src/lib/setlist.ts`.

use crate::serde_util::{Extra, required, string_enum};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

string_enum!(
    pub enum LibraryKind {
        Songs = "songs",
        Documents = "documents",
        Scripture = "scripture",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    pub id: String,
    pub name: String,
    pub created_at: f64,
    pub updated_at: f64,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LibraryState {
    pub libraries: Vec<Library>,
    /// Item id to library id.
    pub assignments: BTreeMap<String, String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `LibrariesState = Record<LibraryKind, LibraryState>`. Persisted as
/// `kairo-libraries.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LibrariesState {
    pub songs: LibraryState,
    pub documents: LibraryState,
    pub scripture: LibraryState,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `LibrariesCommand`, tagged by `action`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum LibrariesCommand {
    List,
    Create {
        kind: LibraryKind,
        name: String,
    },
    #[serde(rename_all = "camelCase")]
    Rename {
        kind: LibraryKind,
        library_id: String,
        name: String,
    },
    #[serde(rename_all = "camelCase")]
    Delete {
        kind: LibraryKind,
        library_id: String,
    },
    #[serde(rename_all = "camelCase")]
    Assign {
        kind: LibraryKind,
        item_ids: Vec<String>,
        library_id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SongSetlist {
    pub id: String,
    pub name: String,
    pub song_ids: Vec<String>,
    pub created_at: f64,
    pub updated_at: f64,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Persisted as `kairo-setlist.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetlistState {
    pub lists: Vec<SongSetlist>,
    #[serde(deserialize_with = "required")]
    pub active_id: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `SetlistCommand`, tagged by `action`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum SetlistCommand {
    List,
    Create {
        name: String,
    },
    #[serde(rename_all = "camelCase")]
    Rename {
        list_id: String,
        name: String,
    },
    #[serde(rename_all = "camelCase")]
    Delete {
        list_id: String,
    },
    #[serde(rename_all = "camelCase")]
    Select {
        #[serde(deserialize_with = "required")]
        list_id: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Add {
        song_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        index: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        list_id: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    RemoveSong {
        song_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        list_id: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Clear {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        list_id: Option<String>,
    },
}
