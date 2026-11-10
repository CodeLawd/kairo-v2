//! Building `ScriptureSuggestion` values the orchestrator publishes.

use kairo_domain::scripture::{ScriptureSuggestion, ScriptureVerse};
use serde_json::{Value, json};

/// A suggestion from typed fields; optional fields left out when `None`.
pub(crate) fn from_fields(mut fields: serde_json::Map<String, Value>) -> ScriptureSuggestion {
    fields.retain(|_, v| !v.is_null());
    serde_json::from_value(Value::Object(fields))
        .expect("suggestion fields form a valid ScriptureSuggestion")
}

/// A manual (operator or voice) suggestion.
pub fn manual(
    id: String,
    reference: String,
    verses: Vec<ScriptureVerse>,
    translation: String,
    trigger_text: String,
    passage_id: Option<String>,
    passage_index: Option<f64>,
) -> ScriptureSuggestion {
    let Value::Object(fields) = json!({
        "id": id,
        "reference": reference,
        "verses": verses,
        "translation": translation,
        "confidence": 1,
        "source": "manual",
        "triggerText": trigger_text,
        "passageId": passage_id,
        "passageIndex": passage_index,
    }) else {
        unreachable!()
    };
    from_fields(fields)
}
