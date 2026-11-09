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
