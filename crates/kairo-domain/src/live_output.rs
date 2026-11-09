//! The payload pushed to outputs, from `src/lib/live-output.ts`.

use crate::overlay::OverlayContentKind;
use crate::scripture::{ScriptureTranslation, ScriptureVerse};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveOutputPayload {
    pub kind: OverlayContentKind,
    pub reference: String,
    pub text: String,
    /// `(string | undefined)[]`: an `undefined` element is written by
    /// `JSON.stringify` as `null`, so it reads as `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_colors: Option<Vec<Option<String>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verses: Option<Vec<ScriptureVerse>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translation: Option<ScriptureTranslation>,
}

pub fn build_lyrics_live_output_payload(
    song: &crate::lyrics::LyricsSong,
    slide: Option<&crate::lyrics::slides::LyricSlide>,
) -> LiveOutputPayload {
    let reference = slide
        .filter(|slide| !slide.section_label.is_empty())
        .map_or_else(
            || song.title.clone(),
            |slide| format!("{} · {}", song.title, slide.section_label),
        );
    LiveOutputPayload {
        kind: OverlayContentKind::Lyrics,
        reference,
        text: slide.map_or_else(String::new, |slide| slide.lines.join("\n")),
        line_colors: slide.map(|slide| slide.line_colors.clone()),
        verses: None,
        translation: None,
    }
}
