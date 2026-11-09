//! How a section's lines are stored in the SQLite cache (`sections.lines`).
//! Port of `src/lib/lyrics-section-json.ts`.
//!
//! A plain section is a JSON array of lines, the original format. The object
//! form `{ lines, lineColors?, hotkey? }` is used only when an extra is set,
//! so older and newer rows read the same way.

use super::hotkeys::normalize_hotkey;
use super::js_text::{number_to_string, trim};
use serde::Serialize;
use serde_json::Value;

/// A section's lines as read from the cache.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionLines {
    pub lines: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_colors: Option<Vec<Option<String>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hotkey: Option<String>,
}

/// Writes lines as `JSON.stringify` would: a plain array unless a colour or a
/// valid hotkey is set. Colours are trimmed; blank ones become `null`.
pub fn serialize_section_lines(
    lines: &[String],
    line_colors: Option<&[Option<String>]>,
    hotkey: Option<&str>,
) -> String {
    let key = normalize_hotkey(hotkey);
    let has_colors = line_colors.is_some_and(|colors| {
        colors
            .iter()
            .any(|c| c.as_deref().is_some_and(|c| !trim(c).is_empty()))
    });
    if !has_colors && key.is_none() {
        return to_json(&lines);
    }
    let colors = line_colors.filter(|_| has_colors).map(|colors| {
        (0..lines.len())
            .map(|i| {
                colors
                    .get(i)
                    .and_then(|c| c.as_deref())
                    .map(trim)
                    .filter(|c| !c.is_empty())
                    .map(str::to_string)
            })
            .collect()
    });
    to_json(&SectionLines {
        lines: lines.to_vec(),
        line_colors: colors,
        hotkey: key,
    })
}

/// serde_json escapes exactly what `JSON.stringify` escapes for strings.
fn to_json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "[]".into())
}

/// JavaScript's `String(x ?? '')` for a parsed JSON value.
fn js_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => number_to_string(n.as_f64().unwrap_or(f64::NAN)),
        Value::String(s) => s.clone(),
        Value::Array(items) => items.iter().map(js_string).collect::<Vec<_>>().join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

/// Reads either stored form. Anything unreadable gives no lines.
pub fn parse_section_lines(raw: &str) -> SectionLines {
    let Ok(parsed) = serde_json::from_str::<Value>(raw) else {
        return SectionLines::default();
    };
    match parsed {
        Value::Array(items) => SectionLines {
            lines: items.iter().map(js_string).collect(),
            ..SectionLines::default()
        },
        Value::Object(obj) => {
            let Some(Value::Array(items)) = obj.get("lines") else {
                return SectionLines::default();
            };
            let lines: Vec<String> = items.iter().map(js_string).collect();
            let line_colors = match obj.get("lineColors") {
                Some(Value::Array(colors)) => Some(
                    (0..lines.len())
                        .map(|i| match colors.get(i) {
                            Some(Value::String(c)) if !trim(c).is_empty() => {
                                Some(trim(c).to_string())
                            }
                            _ => None,
                        })
                        .collect(),
                ),
                _ => None,
            };
            let hotkey = match obj.get("hotkey") {
                Some(Value::String(k)) => normalize_hotkey(Some(k)),
                _ => None,
            };
            SectionLines {
                lines,
                line_colors,
                hotkey,
            }
        }
        _ => SectionLines::default(),
    }
}
