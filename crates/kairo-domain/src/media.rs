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

/// Display an elapsed time as `m:ss` or `h:mm:ss`.
pub fn format_media_clock(seconds: f64) -> String {
    let total = if seconds.is_finite() && seconds > 0.0 {
        seconds.floor()
    } else {
        0.0
    };
    let hours = (total / 3600.0).floor();
    let minutes = ((total % 3600.0) / 60.0).floor() as u8;
    let seconds = (total % 60.0) as u8;
    if hours > 0.0 {
        format!(
            "{}:{minutes:02}:{seconds:02}",
            crate::js::number_to_string(hours)
        )
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

impl Default for MediaPlayback {
    fn default() -> Self {
        Self {
            looped: false,
            start_seconds: None,
            end_seconds: None,
            rate: None,
            fit: None,
            hue: 0.0,
            saturation: 1.0,
            brightness: 1.0,
            contrast: 1.0,
            extra: Extra::new(),
        }
    }
}

pub fn normalize_media_playback(raw: &serde_json::Value) -> MediaPlayback {
    let clamp = |key: &str, min, max, fallback| {
        raw.get(key)
            .and_then(serde_json::Value::as_f64)
            .filter(|n| n.is_finite())
            .map_or(fallback, |n| crate::js::clamp(n, min, max))
    };
    let start = clamp("startSeconds", 0.0, 9_007_199_254_740_991.0, 0.0);
    let end = clamp("endSeconds", 0.0, 9_007_199_254_740_991.0, 0.0);
    let rate = clamp("rate", 0.5, 2.0, 1.0);
    MediaPlayback {
        start_seconds: (start > 0.0).then_some(start),
        end_seconds: (end > start).then_some(Some(end)),
        rate: (rate != 1.0).then_some(rate),
        fit: raw
            .get("fit")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .map(Some),
        looped: raw
            .get("loop")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        hue: clamp("hue", -180.0, 180.0, 0.0),
        saturation: clamp("saturation", 0.0, 2.0, 1.0),
        brightness: clamp("brightness", 0.25, 1.75, 1.0),
        contrast: clamp("contrast", 0.25, 1.75, 1.0),
        extra: Extra::new(),
    }
}

pub fn is_default_media_playback(playback: &MediaPlayback) -> bool {
    playback.start_seconds.unwrap_or(0.0) == 0.0
        && playback.end_seconds.flatten().unwrap_or(0.0) == 0.0
        && playback.rate.unwrap_or(1.0) == 1.0
        && playback.fit.flatten().is_none()
        && !playback.looped
        && playback.hue == 0.0
        && playback.saturation == 1.0
        && playback.brightness == 1.0
        && playback.contrast == 1.0
}

pub fn normalize_playback_map(raw: &serde_json::Value) -> BTreeMap<String, MediaPlayback> {
    raw.as_object()
        .into_iter()
        .flatten()
        .filter_map(|(id, raw)| {
            let playback = normalize_media_playback(raw);
            (!id.is_empty() && id != "__proto__" && !is_default_media_playback(&playback))
                .then(|| (id.clone(), playback))
        })
        .collect()
}

pub fn media_filter_css(playback: &MediaPlayback) -> String {
    let mut parts = Vec::new();
    for (name, value, identity, unit) in [
        ("hue-rotate", playback.hue, 0.0, "deg"),
        ("saturate", playback.saturation, 1.0, ""),
        ("brightness", playback.brightness, 1.0, ""),
        ("contrast", playback.contrast, 1.0, ""),
    ] {
        if value != identity {
            parts.push(format!(
                "{name}({}{unit})",
                crate::js::number_to_string(value)
            ));
        }
    }
    parts.join(" ")
}

pub fn apply_playback_to_background(
    mut background: crate::overlay::OverlayBackground,
    playback: &MediaPlayback,
    kind: MediaKind,
) -> crate::overlay::OverlayBackground {
    let video = kind == MediaKind::Video;
    background.media_loop = Some(!video || playback.looped);
    background.media_start_seconds = if video { playback.start_seconds } else { None };
    background.media_end_seconds = if video { playback.end_seconds } else { None };
    background.media_rate = if video { playback.rate } else { None };
    background.hue = Some(playback.hue);
    background.saturation = Some(playback.saturation);
    background.brightness = Some(playback.brightness);
    background.contrast = Some(playback.contrast);
    background
}

pub fn theme_with_live_media(
    mut theme: crate::overlay::OverlayTheme,
    item: Option<(&str, MediaKind)>,
    playback: &MediaPlayback,
    cue: Option<f64>,
) -> crate::overlay::OverlayTheme {
    use crate::overlay::OverlayBackgroundType;
    if let Some((path, kind)) = item {
        let video = kind == MediaKind::Video;
        let mut background = theme.background;
        background.kind = if video {
            OverlayBackgroundType::Video
        } else {
            OverlayBackgroundType::Image
        };
        background.media_path = Some(path.to_owned());
        background.media_fit = playback
            .fit
            .flatten()
            .or(background.media_fit)
            .or(Some(MediaFit::Cover));
        background.blur_px = Some(0.0);
        background.media_cue = if video { cue } else { None };
        theme.background = apply_playback_to_background(background, playback, kind);
    }
    theme
}

pub fn theme_owns_background(theme: &crate::overlay::OverlayTheme) -> bool {
    theme.background.kind != crate::overlay::OverlayBackgroundType::Transparent
}

pub fn without_background(mut theme: crate::overlay::OverlayTheme) -> crate::overlay::OverlayTheme {
    theme.background.kind = crate::overlay::OverlayBackgroundType::Transparent;
    theme
}

pub fn theme_for_push(
    theme: crate::overlay::OverlayTheme,
    shows_backgrounds: bool,
    live: Option<(&str, MediaKind)>,
    playback: &MediaPlayback,
    cue: Option<f64>,
    force: bool,
) -> crate::overlay::OverlayTheme {
    if !shows_backgrounds {
        without_background(theme)
    } else if !force && theme_owns_background(&theme) {
        theme
    } else {
        theme_with_live_media(theme, live, playback, cue)
    }
}

/// MIME type for the Electron media extension table.
pub fn media_mime_type(path: &str) -> &'static str {
    match path
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("mp4") => "video/mp4",
        Some("m4v") => "video/x-m4v",
        Some("webm") => "video/webm",
        Some("mov") => "video/quicktime",
        Some("ogv") => "video/ogg",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        _ => "application/octet-stream",
    }
}

pub const TRACK_FILE_EXTENSIONS: &[&str] = &["mp3", "wav", "m4a", "aac", "ogg", "flac"];

pub fn is_track_extension(extension: &str) -> bool {
    TRACK_FILE_EXTENSIONS
        .iter()
        .any(|ext| ext.eq_ignore_ascii_case(extension.strip_prefix('.').unwrap_or(extension)))
}

pub fn sibling_audio_folder(media_folder: &str) -> String {
    let trimmed = media_folder.trim_end_matches(['/', '\\']);
    let sep = if trimmed.contains('\\') && !trimmed.contains('/') {
        "\\"
    } else {
        "/"
    };
    let mut parts: Vec<_> = trimmed
        .split(['/', '\\'])
        .filter(|p| !p.is_empty())
        .collect();
    let Some(last) = parts.last_mut() else {
        return trimmed.into();
    };
    *last = "Audio";
    let drive = parts[0].as_bytes();
    if matches!(drive, [letter, b':'] if letter.is_ascii_alphabetic()) {
        return format!("{}\\{}", parts[0], parts[1..].join("\\"));
    }
    format!(
        "{}{}",
        if trimmed.starts_with('/') { "/" } else { "" },
        parts.join(sep)
    )
}

pub fn track_file_name(raw: &str, extension: &str) -> String {
    let suffix = format!(".{}", extension.strip_prefix('.').unwrap_or(extension));
    let trimmed = crate::js::trim(raw);
    let name = trimmed
        .len()
        .checked_sub(suffix.len())
        .and_then(|i| {
            trimmed
                .get(i..)
                .filter(|tail| tail.eq_ignore_ascii_case(&suffix))
                .map(|_| &trimmed[..i])
        })
        .unwrap_or(trimmed);
    let clean: String = name
        .chars()
        .map(|c| if "/\\:*?\"<>|".contains(c) { ' ' } else { c })
        .collect();
    let clean = clean
        .split(crate::js::is_whitespace)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let mut units = 0;
    let clean: String = clean
        .trim_end_matches(['.', ' '])
        .chars()
        .take_while(|ch| {
            units += ch.len_utf16();
            units <= 120
        })
        .collect();
    crate::js::trim(&clean).into()
}
