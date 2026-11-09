//! Songs and lyric imports, from `src/lib/ipc.ts` and `src/lib/lyrics-duplicate.ts`.

use crate::serde_util::{Extra, present, required, string_enum};
use serde::{Deserialize, Serialize};

string_enum!(
    pub enum LyricsProvider {
        Genius = "genius",
        Lrclib = "lrclib",
        Web = "web",
    }
);
string_enum!(
    pub enum LyricsSectionType {
        Verse = "verse",
        Chorus = "chorus",
        Bridge = "bridge",
        PreChorus = "pre-chorus",
        Tag = "tag",
        Intro = "intro",
        Outro = "outro",
        Ending = "ending",
    }
);
string_enum!(
    pub enum LyricsSource {
        Usr = "usr",
        Text = "text",
        ProPresenter = "propresenter",
        Ccli = "ccli",
        Genius = "genius",
        Lrclib = "lrclib",
        Web = "web",
    }
);

/// `LyricsImportSource`, tagged by `type`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum LyricsImportSource {
    Url {
        url: String,
    },
    #[serde(rename_all = "camelCase")]
    Ccli {
        ccli_number: String,
        api_key: String,
    },
    Text {
        title: String,
        artist: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        copyright: Option<String>,
    },
    Usr {
        content: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
    },
    Online {
        provider: LyricsProvider,
        url: String,
        title: String,
        artist: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsOnlineResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lyrics_available: Option<bool>,
    pub id: String,
    pub provider: LyricsProvider,
    pub title: String,
    pub artist: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_year: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub existing_song_id: Option<String>,
}

string_enum!(
    pub enum UnverifiedReason {
        MissingKey = "missing-key",
        Unavailable = "unavailable",
        InvalidResponse = "invalid-response",
        TooLarge = "too-large",
    }
);

/// `LyricsReview`, tagged by `status`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum LyricsReview {
    #[serde(rename_all = "camelCase")]
    Reviewed {
        removed_lines: f64,
    },
    Unverified {
        reason: UnverifiedReason,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LyricsOnlinePreview {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<LyricsReview>,
    pub title: String,
    pub artist: String,
    pub provider: LyricsProvider,
    pub url: String,
    pub sections: Vec<LyricsSongSection>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsSongSection {
    #[serde(rename = "type")]
    pub kind: LyricsSectionType,
    pub label: String,
    pub lines: Vec<String>,
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

/// A song in the library. Persisted in SQLite and in song files.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsSong {
    pub id: String,
    pub title: String,
    pub artist: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub copyright: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub ccli_number: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub is_favorite: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub source: Option<LyricsSource>,
    pub sections: Vec<LyricsSongSection>,
    pub created_at: f64,
    pub updated_at: f64,
    #[serde(flatten)]
    pub extra: Extra,
}

string_enum!(
    pub enum DuplicateReason {
        SameSong = "same-song",
        Ccli = "ccli",
        TitleArtist = "title-artist",
        Lyrics = "lyrics",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateMatch {
    pub song_id: String,
    pub title: String,
    pub artist: String,
    pub reason: DuplicateReason,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LyricsImportPreview {
    pub song: LyricsSong,
    #[serde(deserialize_with = "required")]
    pub duplicate: Option<DuplicateMatch>,
}

string_enum!(
    pub enum CopyrightPosition {
        Each = "each",
        Last = "last",
        Separate = "separate",
        None = "none",
    }
);

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SongPresentOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines_per_slide: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_chars_per_line: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copyright_position: Option<CopyrightPosition>,
}
