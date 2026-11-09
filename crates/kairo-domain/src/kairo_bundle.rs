//! Self-contained song, setlist and scripture playlist exchanges.

use crate::lyrics::LyricsSong;
use crate::scripture::{ScriptureVerse, SermonPlan, SermonScriptureItem};
use crate::workspace::{coerce_song, fallback_text, slugify_title, text};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

pub const KAIRO_BUNDLE_EXT: &str = ".kairo";
pub const KAIRO_BUNDLE_VERSION: u32 = 1;
pub const KAIRO_BUNDLE_MAX_BYTES: usize = 50 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum KairoBundleKind {
    Songs,
    Setlist,
    ScripturePlaylist,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleSetlist {
    pub name: String,
    pub song_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KairoBundle {
    pub kairo_bundle: u32,
    pub kind: KairoBundleKind,
    pub name: String,
    pub exported_at: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_version: Option<String>,
    pub songs: Vec<LyricsSong>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setlist: Option<BundleSetlist>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playlist: Option<SermonPlan>,
}

pub fn songs_bundle(
    name: String,
    songs: Vec<LyricsSong>,
    app_version: Option<String>,
    now_ms: f64,
) -> KairoBundle {
    KairoBundle {
        kairo_bundle: KAIRO_BUNDLE_VERSION,
        kind: KairoBundleKind::Songs,
        name,
        songs,
        exported_at: now_ms,
        app_version: app_version.filter(|v| !v.is_empty()),
        setlist: None,
        playlist: None,
    }
}

pub fn setlist_bundle(
    name: String,
    songs: Vec<LyricsSong>,
    app_version: Option<String>,
    now_ms: f64,
) -> KairoBundle {
    let setlist = BundleSetlist {
        name: name.clone(),
        song_ids: songs.iter().map(|song| song.id.clone()).collect(),
    };
    KairoBundle {
        kind: KairoBundleKind::Setlist,
        setlist: Some(setlist),
        ..songs_bundle(name, songs, app_version, now_ms)
    }
}

pub fn playlist_bundle(plan: SermonPlan, app_version: Option<String>, now_ms: f64) -> KairoBundle {
    KairoBundle {
        kind: KairoBundleKind::ScripturePlaylist,
        playlist: Some(plan.clone()),
        ..songs_bundle(plan.title, Vec::new(), app_version, now_ms)
    }
}

pub fn serialize_bundle(bundle: &KairoBundle) -> serde_json::Result<String> {
    Ok(serde_json::to_string_pretty(&crate::json::to_js_value(bundle)?)? + "\n")
}

pub fn bundle_file_name(name: &str) -> String {
    format!("{}{KAIRO_BUNDLE_EXT}", slugify_title(name))
}

fn coerce_songs(value: &Value, now_ms: f64) -> Vec<LyricsSong> {
    let mut seen = HashSet::new();
    value
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(index, value)| coerce_song(value, &format!("imported-{}", index + 1), now_ms))
        .filter(|song| seen.insert(song.id.clone()))
        .collect()
}

fn coerce_verse(raw: &Value) -> Option<ScriptureVerse> {
    let number = |key| {
        let n = crate::js::value_to_number(raw.get(key));
        n.is_finite().then_some(n)
    };
    let book = text(&raw["book"]);
    let verse_text = text(&raw["text"]);
    if book.is_empty() || verse_text.is_empty() {
        return None;
    }
    Some(ScriptureVerse {
        book: book.into(),
        chapter: number("chapter")?,
        verse: number("verse")?,
        text: verse_text.into(),
        extra: Default::default(),
    })
}

fn coerce_item(index: usize, raw: &Value) -> Option<SermonScriptureItem> {
    let reference = text(&raw["reference"]);
    if reference.is_empty() {
        return None;
    }
    let verses: Vec<_> = raw["verses"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(coerce_verse)
        .collect();
    Some(SermonScriptureItem {
        id: fallback_text(&raw["id"], &format!("item-{}", index + 1)).into(),
        reference: reference.into(),
        translation: fallback_text(&raw["translation"], "KJV").into(),
        available: !verses.is_empty(),
        error: verses
            .is_empty()
            .then(|| "No verse text in the imported file".into()),
        verses,
        extra: Default::default(),
    })
}

fn coerce_plan(raw: &Value, now_ms: f64) -> Option<SermonPlan> {
    if !raw.is_object() && !raw.is_array() {
        return None;
    }
    Some(SermonPlan {
        source_text: None,
        id: fallback_text(
            &raw["id"],
            &format!("plan-{}", crate::js::number_to_string(now_ms)),
        )
        .into(),
        title: fallback_text(&raw["title"], "Imported playlist").into(),
        source_file_name: text(&raw["sourceFileName"]).into(),
        items: raw["items"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
            .filter_map(|(i, raw)| coerce_item(i, raw))
            .collect(),
        created_at: raw["createdAt"].as_f64().unwrap_or(now_ms),
        updated_at: raw["updatedAt"].as_f64().unwrap_or(now_ms),
        extra: Default::default(),
    })
}

pub fn parse_kairo_bundle(raw: &str, now_ms: f64) -> Result<KairoBundle, &'static str> {
    let value: Value = serde_json::from_str(raw)
        .map_err(|_| "This file is not a Kairo export — it could not be read.")?;
    if value["kairoSong"].is_number() {
        let song = coerce_song(&value, "imported-1", now_ms)
            .ok_or("This song file has no usable song in it.")?;
        return Ok(songs_bundle(song.title.clone(), vec![song], None, 0.0));
    }
    let version = value["kairoBundle"]
        .as_f64()
        .ok_or("This file is not a Kairo export.")?;
    if version > f64::from(KAIRO_BUNDLE_VERSION) {
        return Err("This file was made by a newer version of Kairo. Update Kairo to open it.");
    }
    let kind = serde_json::from_value(value["kind"].clone())
        .map_err(|_| "This Kairo file contains something this version cannot import.")?;
    let songs = coerce_songs(&value["songs"], now_ms);
    let version = text(&value["appVersion"]);
    let mut bundle = songs_bundle(
        fallback_text(&value["name"], "Kairo export").into(),
        songs,
        Some(version.into()),
        value["exportedAt"].as_f64().unwrap_or(0.0),
    );
    bundle.kind = kind;
    match kind {
        KairoBundleKind::ScripturePlaylist => {
            bundle.playlist = Some(
                coerce_plan(&value["playlist"], now_ms)
                    .ok_or("This playlist file has no playlist in it.")?,
            );
            return Ok(bundle);
        }
        KairoBundleKind::Setlist => {
            let raw = &value["setlist"];
            let song_ids = raw["songIds"].as_array().map_or_else(
                || bundle.songs.iter().map(|song| song.id.clone()).collect(),
                |ids| {
                    let known: HashSet<_> =
                        bundle.songs.iter().map(|song| song.id.as_str()).collect();
                    ids.iter()
                        .map(crate::js::value_to_string)
                        .filter(|id| known.contains(id.as_str()))
                        .collect()
                },
            );
            bundle.setlist = Some(BundleSetlist {
                name: fallback_text(&raw["name"], &bundle.name).into(),
                song_ids,
            });
        }
        KairoBundleKind::Songs => {}
    }
    if bundle.songs.is_empty() {
        return Err("This Kairo file has no songs in it.");
    }
    Ok(bundle)
}

pub fn merge_kairo_bundles(mut bundles: Vec<KairoBundle>) -> Result<KairoBundle, &'static str> {
    if bundles.is_empty() {
        return Err("No files to import.");
    }
    if bundles.len() == 1 {
        return Ok(bundles.remove(0));
    }
    if bundles
        .iter()
        .any(|bundle| bundle.kind != KairoBundleKind::Songs)
    {
        return Err(
            "Song and scripture playlists import one file at a time. Select just that file.",
        );
    }
    let mut seen = HashSet::new();
    let songs: Vec<_> = bundles
        .into_iter()
        .flat_map(|bundle| bundle.songs)
        .filter(|song| seen.insert(song.id.clone()))
        .collect();
    let suffix = if songs.len() == 1 { "" } else { "s" };
    Ok(songs_bundle(
        format!("{} song{suffix}", songs.len()),
        songs,
        None,
        0.0,
    ))
}

crate::serde_util::string_enum!(
    pub enum SongImportChoice {
        Add = "add",
        Skip = "skip",
        Replace = "replace",
        KeepBoth = "keep-both",
    }
);
crate::serde_util::string_enum!(
    pub enum PlaylistImportChoice {
        Replace = "replace",
        KeepBoth = "keep-both",
        Skip = "skip",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum TransferExportRequest {
    Songs {
        song_ids: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    Library,
    Setlist {
        list_id: String,
    },
    Playlist {
        plan_id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferExportResult {
    pub saved: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferSongPreview {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub section_count: usize,
    #[serde(deserialize_with = "crate::serde_util::required")]
    pub duplicate: Option<crate::lyrics::DuplicateMatch>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferSetlistPreview {
    pub name: String,
    pub song_count: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferPlaylistPreview {
    pub title: String,
    pub item_count: usize,
    pub exists: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferSkippedFile {
    pub file_name: String,
    pub error: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferPreview {
    pub token: String,
    pub file_name: String,
    pub kind: KairoBundleKind,
    pub name: String,
    pub exported_at: f64,
    pub songs: Vec<TransferSongPreview>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setlist: Option<TransferSetlistPreview>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playlist: Option<TransferPlaylistPreview>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped_files: Option<Vec<TransferSkippedFile>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferCommitRequest {
    pub token: String,
    pub songs: std::collections::BTreeMap<String, SongImportChoice>,
    pub create_setlist: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playlist: Option<PlaylistImportChoice>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferCommitResult {
    pub added: usize,
    pub replaced: usize,
    pub skipped: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setlist_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playlist_title: Option<String>,
}
