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

pub const WEB_FRAME_WIDTH: u32 = 1920;
pub const WEB_FRAME_HEIGHT: u32 = 1080;

pub fn web_address(input: &str, search: bool) -> Option<String> {
    use regex::Regex;
    use std::sync::LazyLock;
    static SCHEME: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)^([a-z][a-z0-9+.-]*):").unwrap());
    static PORT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[^\s/:]+:[0-9]+(/|$)").unwrap());
    static HOST: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^[^/]+\.[^/.]+(?:[:/?#]|$)|(?i:^localhost(?:[:/]|$))").unwrap()
    });
    let text = crate::js::trim(input);
    if text.is_empty() || text.encode_utf16().count() > 2048 {
        return None;
    }
    let scheme = SCHEME
        .captures(text)
        .map(|capture| capture[1].to_ascii_lowercase());
    let http = matches!(scheme.as_deref(), Some("http" | "https"));
    let search_address = || {
        search.then(|| {
            format!(
                "https://www.google.com/search?q={}",
                crate::js::encode_uri_component(text)
            )
        })
    };
    if scheme.is_some() && !http && !PORT.is_match(text) {
        return if text.contains("://") {
            None
        } else {
            search_address()
        };
    }
    if text.chars().any(crate::js::is_whitespace) || (scheme.is_none() && !HOST.is_match(text)) {
        return search_address();
    }
    let candidate = if http {
        std::borrow::Cow::Borrowed(text)
    } else {
        format!("https://{text}").into()
    };
    match url::Url::parse(&candidate) {
        Ok(url) if matches!(url.scheme(), "http" | "https") => Some(url.into()),
        Ok(_) => None,
        Err(_) => search_address(),
    }
}

pub fn without_frame_ancestors(policy: &str) -> String {
    policy
        .split(';')
        .filter(|directive| {
            !crate::js::trim(directive)
                .split(crate::js::is_whitespace)
                .next()
                .unwrap_or_default()
                .eq_ignore_ascii_case("frame-ancestors")
        })
        .collect::<Vec<_>>()
        .join(";")
}

pub fn display_address(raw: &str) -> String {
    let Ok(url) = url::Url::parse(raw) else {
        return raw.into();
    };
    let host = &url[url::Position::BeforeHost..url::Position::AfterPort];
    let path = if url.path() == "/" { "" } else { url.path() };
    match url.query() {
        Some(query) => format!("{host}{path}?{query}"),
        None => format!("{host}{path}"),
    }
}

pub fn normalize_web_settings(raw: &serde_json::Value) -> WebSettings {
    let bookmarks = raw["bookmarks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let id = entry["id"].as_str().filter(|id| !id.is_empty())?;
            let url = web_address(entry["url"].as_str()?, false)?;
            let name = crate::workspace::text(&entry["name"]);
            let name = if name.is_empty() {
                display_address(&url)
            } else {
                let mut units = 0;
                name.chars()
                    .take_while(|c| {
                        units += c.len_utf16();
                        units <= 80
                    })
                    .collect()
            };
            Some(WebBookmark {
                id: id.into(),
                name,
                url,
                extra: Extra::new(),
            })
        })
        .take(50)
        .collect();
    WebSettings {
        bookmarks,
        show_pointer: raw["showPointer"].as_bool().unwrap_or(false),
        extra: Extra::new(),
    }
}
