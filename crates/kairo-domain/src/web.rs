//! Web tab settings, from `src/lib/web.ts`.

use crate::serde_util::Extra;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WebBookmark {
    pub id: String,
    pub name: String,
    pub url: String,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `AppSettings.web`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSettings {
    pub bookmarks: Vec<WebBookmark>,
    pub show_pointer: bool,
    #[serde(flatten)]
    pub extra: Extra,
}
