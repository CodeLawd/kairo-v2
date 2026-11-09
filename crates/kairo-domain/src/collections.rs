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

pub const LIBRARY_KINDS: &[LibraryKind] = LibraryKind::ALL;
pub const DEFAULT_LIBRARY_ID: &str = "default";
pub const LIBRARY_MAX: usize = 50;
pub const SETLIST_MAX: usize = 100;
pub const SETLIST_MAX_LISTS: usize = 50;
pub const DEFAULT_SETLIST_NAME: &str = "Sunday service";
pub const PASSAGES_MAX: usize = 500;

pub fn default_library_name(kind: LibraryKind) -> &'static str {
    match kind {
        LibraryKind::Songs => "All songs",
        LibraryKind::Documents => "All slides",
        LibraryKind::Scripture => "All passages",
    }
}

fn display_name(name: &str, fallback: &str) -> String {
    let no_controls: String = name.chars().filter(|&c| c >= ' ').collect();
    let collapsed = no_controls
        .split(crate::js::is_whitespace)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let value = if collapsed.is_empty() {
        fallback
    } else {
        &collapsed
    };
    let mut units = 0;
    value
        .chars()
        .take_while(|c| {
            units += c.len_utf16();
            units <= 80
        })
        .collect()
}

pub fn normalize_library_name(name: &str, fallback: Option<&str>) -> String {
    display_name(name, fallback.unwrap_or("New library"))
}

pub fn normalize_setlist_name(name: &str, fallback: Option<&str>) -> String {
    display_name(name, fallback.unwrap_or(DEFAULT_SETLIST_NAME))
}

fn coerced_name(value: &serde_json::Value) -> String {
    if value.is_null() {
        String::new()
    } else {
        crate::js::value_to_string(value)
    }
}

pub fn normalize_library_state(raw: &serde_json::Value) -> LibraryState {
    use std::collections::HashSet;
    let libraries: Vec<Library> = raw["libraries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|value| {
            let id = value["id"].as_str()?;
            if id == DEFAULT_LIBRARY_ID {
                return None;
            }
            let mut value = value.clone();
            value["name"] = normalize_library_name(&coerced_name(&value["name"]), None).into();
            serde_json::from_value(value).ok()
        })
        .collect();
    let known: HashSet<_> = libraries
        .iter()
        .map(|library| library.id.as_str())
        .collect();
    let assignments = raw["assignments"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(item, library)| {
            let library = library.as_str()?;
            known
                .contains(library)
                .then(|| (item.clone(), library.to_owned()))
        })
        .collect();
    LibraryState {
        libraries,
        assignments,
        extra: Extra::new(),
    }
}

pub fn normalize_libraries_state(raw: &serde_json::Value) -> LibrariesState {
    LibrariesState {
        songs: normalize_library_state(&raw["songs"]),
        documents: normalize_library_state(&raw["documents"]),
        scripture: normalize_library_state(&raw["scripture"]),
        extra: Extra::new(),
    }
}

pub fn library_of<'a>(state: &'a LibraryState, item_id: &str) -> &'a str {
    state
        .assignments
        .get(item_id)
        .map_or(DEFAULT_LIBRARY_ID, String::as_str)
}

pub fn items_in_library<'a, T>(
    state: &LibraryState,
    items: &'a [T],
    library_id: &str,
    id_of: impl Fn(&T) -> &str,
) -> Vec<&'a T> {
    items
        .iter()
        .filter(|item| {
            library_id == DEFAULT_LIBRARY_ID
                || state
                    .assignments
                    .get(id_of(item))
                    .is_some_and(|id| id == library_id)
        })
        .collect()
}

pub fn library_counts<T>(
    state: &LibraryState,
    items: &[T],
    id_of: impl Fn(&T) -> &str,
) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::from([(DEFAULT_LIBRARY_ID.to_owned(), items.len())]);
    counts.extend(
        state
            .libraries
            .iter()
            .map(|library| (library.id.clone(), 0)),
    );
    for item in items {
        if let Some(id) = state
            .assignments
            .get(id_of(item))
            .filter(|id| !id.is_empty())
            && let Some(count) = counts.get_mut(id)
        {
            *count += 1;
        }
    }
    counts
}

pub fn with_assignment(
    mut state: LibraryState,
    item_ids: &[String],
    library_id: &str,
) -> LibraryState {
    for item_id in item_ids {
        if library_id == DEFAULT_LIBRARY_ID {
            state.assignments.remove(item_id);
        } else {
            state.assignments.insert(item_id.clone(), library_id.into());
        }
    }
    state
}

pub fn without_library(mut state: LibraryState, library_id: &str) -> LibraryState {
    state.libraries.retain(|library| library.id != library_id);
    state.assignments.retain(|_, id| id != library_id);
    state
}

pub fn without_item(mut state: LibraryState, item_id: &str) -> LibraryState {
    state.assignments.remove(item_id);
    state
}

pub fn with_song(song_ids: &[String], song_id: &str, index: Option<f64>) -> Vec<String> {
    if song_id.is_empty() {
        return song_ids.to_vec();
    }
    let mut rest = without_song(song_ids, song_id);
    if index.is_none_or(|index| index >= rest.len() as f64) {
        if rest.len() >= SETLIST_MAX {
            return song_ids.to_vec();
        }
        rest.push(song_id.into());
    } else {
        rest.insert(index.unwrap_or_default() as usize, song_id.into());
    }
    rest
}

pub fn without_song(song_ids: &[String], song_id: &str) -> Vec<String> {
    song_ids
        .iter()
        .filter(|id| *id != song_id)
        .cloned()
        .collect()
}

pub fn pruned_setlist<'a>(
    song_ids: &[String],
    library_ids: impl IntoIterator<Item = &'a str>,
) -> Vec<String> {
    let known: std::collections::HashSet<_> = library_ids.into_iter().collect();
    song_ids
        .iter()
        .filter(|id| known.contains(id.as_str()))
        .cloned()
        .collect()
}

pub fn migrate_setlist_state(raw: &serde_json::Value, now_ms: f64) -> SetlistState {
    let ids = |value: &serde_json::Value| -> Vec<String> {
        value
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|id| id.as_str().map(str::to_owned))
            .collect()
    };
    let mut lists: Vec<SongSetlist> = raw["lists"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|value| {
            value["id"].as_str()?;
            let mut value = value.clone();
            value["name"] = normalize_setlist_name(&coerced_name(&value["name"]), None).into();
            value["songIds"] = serde_json::json!(ids(&value["songIds"]));
            serde_json::from_value(value).ok()
        })
        .collect();
    if lists.is_empty() && raw["songIds"].as_array().is_some_and(|ids| !ids.is_empty()) {
        let date = raw["updatedAt"].as_f64().unwrap_or(now_ms);
        lists.push(SongSetlist {
            id: "setlist-legacy".into(),
            name: DEFAULT_SETLIST_NAME.into(),
            song_ids: ids(&raw["songIds"]),
            created_at: date,
            updated_at: date,
            extra: Extra::new(),
        });
    }
    let active_id = raw["activeId"]
        .as_str()
        .filter(|id| lists.iter().any(|list| list.id == *id))
        .or_else(|| lists.first().map(|list| list.id.as_str()))
        .map(str::to_owned);
    SetlistState {
        lists,
        active_id,
        extra: Extra::new(),
    }
}

pub fn passage_key(reference: &str, translation: &str) -> String {
    format!(
        "{}|{}",
        crate::js::trim(reference).to_lowercase(),
        translation.to_lowercase()
    )
}

pub fn with_passage(
    mut passages: Vec<crate::scripture::SavedPassage>,
    mut passage: crate::scripture::SavedPassage,
) -> Vec<crate::scripture::SavedPassage> {
    let key = passage_key(&passage.reference, &passage.translation);
    if let Some(existing) = passages
        .iter()
        .find(|item| passage_key(&item.reference, &item.translation) == key)
    {
        passage.id.clone_from(&existing.id);
        passage.saved_at = existing.saved_at;
        for item in passages.iter_mut().filter(|item| item.id == passage.id) {
            item.clone_from(&passage);
        }
    } else if passages.len() < PASSAGES_MAX {
        passages.insert(0, passage);
    }
    passages
}

pub fn passage_to_result(
    passage: crate::scripture::SavedPassage,
) -> crate::scripture::ScriptureResult {
    crate::scripture::ScriptureResult {
        reference: passage.reference,
        translation: passage.translation,
        verses: passage.verses,
    }
}
