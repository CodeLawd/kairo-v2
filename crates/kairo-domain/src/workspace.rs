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

pub const WORKSPACE_FOLDER_NAME: &str = "Kairo Presenter";
pub const SONGS_DIR_NAME: &str = "Songs";
pub const MEDIA_DIR_NAME: &str = "Media";

pub fn slugify_title(title: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    let normalized: String = title
        .nfd()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
        .collect();
    let slug: String = normalized
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .take(60)
        .collect();
    if slug.is_empty() { "song".into() } else { slug }
}

pub fn song_file_name(song: &crate::lyrics::LyricsSong) -> String {
    format!("{}__{}{SONG_FILE_EXT}", slugify_title(&song.title), song.id)
}

pub fn song_id_from_file_name(name: &str) -> Option<&str> {
    let stem = name.strip_suffix(SONG_FILE_EXT)?;
    let id = crate::js::trim(stem.split_once("__").map_or(stem, |(_, id)| id));
    (!id.is_empty()).then_some(id)
}

pub fn is_song_file_name(name: &str) -> bool {
    name.ends_with(SONG_FILE_EXT) && !name.starts_with('.')
}

pub fn is_inside_folder(child: &str, parent: &str) -> bool {
    let parent = parent.trim_end_matches(['/', '\\']);
    let child = child.trim_end_matches(['/', '\\']);
    let separator = if parent.contains('\\') { '\\' } else { '/' };
    !parent.is_empty()
        && (parent == child
            || child
                .strip_prefix(parent)
                .is_some_and(|tail| tail.starts_with(separator)))
}

pub(crate) fn text(value: &serde_json::Value) -> &str {
    crate::js::trim(value.as_str().unwrap_or_default())
}

pub(crate) fn fallback_text<'a>(value: &'a serde_json::Value, fallback: &'a str) -> &'a str {
    let value = text(value);
    if value.is_empty() { fallback } else { value }
}

pub fn parse_song_file(
    raw: &str,
    fallback_id: &str,
    now_ms: f64,
) -> Option<crate::lyrics::LyricsSong> {
    coerce_song(&serde_json::from_str(raw).ok()?, fallback_id, now_ms)
}

pub(crate) fn coerce_song(
    value: &serde_json::Value,
    fallback_id: &str,
    now_ms: f64,
) -> Option<crate::lyrics::LyricsSong> {
    use crate::lyrics::{LyricsSong, LyricsSongSection, hotkeys::normalize_hotkey};
    use serde_json::Value;
    if !(value["kairoSong"].is_number()
        || value["title"].is_string() && value["sections"].is_array())
    {
        return None;
    }
    let id = fallback_text(&value["id"], fallback_id);
    if id.is_empty() {
        return None;
    }
    let sections = value["sections"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|section| {
            let lines: Vec<_> = section["lines"]
                .as_array()?
                .iter()
                .map(|line| {
                    if line.is_null() {
                        String::new()
                    } else {
                        crate::js::value_to_string(line)
                    }
                })
                .collect();
            let line_colors = section["lineColors"].as_array().and_then(|colors| {
                let values: Vec<_> = (0..lines.len())
                    .map(|i| {
                        colors
                            .get(i)
                            .map(text)
                            .filter(|s| !s.is_empty())
                            .map(str::to_owned)
                    })
                    .collect();
                values.iter().any(Option::is_some).then_some(values)
            });
            Some(LyricsSongSection {
                kind: serde_json::from_value(section["type"].clone())
                    .unwrap_or(LyricsSectionType::Verse),
                label: section["label"]
                    .as_str()
                    .filter(|s| !crate::js::trim(s).is_empty())
                    .unwrap_or("Verse 1")
                    .into(),
                lines,
                line_colors,
                hotkey: normalize_hotkey(section["hotkey"].as_str()),
                extra: Extra::new(),
            })
        })
        .collect();
    let positive = |key: &str, default| value[key].as_f64().filter(|n| *n > 0.0).unwrap_or(default);
    let created_at = positive("createdAt", now_ms);
    let optional = |v: &Value| Some(text(v)).filter(|s| !s.is_empty()).map(str::to_owned);
    Some(LyricsSong {
        id: id.into(),
        title: fallback_text(&value["title"], "Untitled").into(),
        artist: text(&value["artist"]).into(),
        copyright: optional(&value["copyright"]),
        ccli_number: optional(&value["ccliNumber"]),
        is_favorite: Some(value["isFavorite"].as_bool().unwrap_or(false)),
        source: Some(serde_json::from_value(value["source"].clone()).unwrap_or(LyricsSource::Text)),
        sections,
        created_at,
        updated_at: positive("updatedAt", created_at),
        extra: Extra::new(),
    })
}

pub fn serialize_song_file(song: &crate::lyrics::LyricsSong) -> serde_json::Result<String> {
    let sections = song
        .sections
        .iter()
        .map(|section| SongFileSection {
            kind: section.kind,
            label: section.label.clone(),
            lines: section.lines.clone(),
            line_colors: section.line_colors.clone().filter(|colors| {
                colors
                    .iter()
                    .any(|color| color.as_ref().is_some_and(|color| !color.is_empty()))
            }),
            hotkey: crate::lyrics::hotkeys::normalize_hotkey(section.hotkey.as_deref()),
            extra: Extra::new(),
        })
        .collect();
    let file = SongFile {
        kairo_song: SONG_FILE_VERSION,
        id: song.id.clone(),
        title: song.title.clone(),
        artist: song.artist.clone(),
        copyright: song.copyright.clone().unwrap_or_default(),
        ccli_number: song.ccli_number.clone().unwrap_or_default(),
        is_favorite: song.is_favorite.unwrap_or(false),
        source: song.source.unwrap_or(LyricsSource::Text),
        created_at: song.created_at,
        updated_at: song.updated_at,
        sections,
        extra: Extra::new(),
    };
    Ok(serde_json::to_string_pretty(&crate::json::to_js_value(&file)?)? + "\n")
}
