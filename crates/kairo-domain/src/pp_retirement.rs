//! ProPresenter data removal, from `src/lib/pp-retirement.ts`. Runs on the
//! raw settings file on every launch, before any normaliser, and is idempotent.

use crate::js;
use crate::normalize::as_object;
use crate::overlay::OverlayOutputKind;
use serde_json::{Map, Value, json};

/// `retireProPresenterSettings`: drops ProPresenter sections and bindings and
/// output kinds that are no longer rendered. A store with no `outputs` array
/// gets one: an enabled NDI feed for the old `ndi`/`auto` modes, otherwise
/// empty, so a message-only setup never becomes an enabled output.
pub fn retire_pro_presenter_settings(raw: Option<&Value>) -> Map<String, Value> {
    let mut settings = as_object(raw).clone();
    let legacy = as_object(settings.get("overlay")).clone();
    settings.remove("propresenter");
    settings.remove("retirementNotice");
    settings.remove("propresenterResources");
    let mut overlay = legacy.clone();
    overlay.remove("mode");
    overlay.remove("ppVideoInputUuid");

    let kept: Vec<Value> = match legacy.get("outputs") {
        Some(Value::Array(outputs)) => outputs
            .iter()
            .map(|o| as_object(Some(o)))
            // `String(o.kind)`: coerced, so `["ndi"]` passes here and is
            // rejected later by the output normaliser.
            .filter(|o| {
                let kind = o
                    .get("kind")
                    .map_or_else(|| "undefined".to_owned(), js::value_to_string);
                OverlayOutputKind::from_name(&kind).is_some()
            })
            .map(|o| {
                let mut clean = o.clone();
                clean.remove("lookId");
                clean.remove("ppVideoInputUuid");
                Value::Object(clean)
            })
            .collect(),
        _ => {
            let mode = legacy.get("mode").and_then(Value::as_str);
            if matches!(mode, Some("ndi" | "auto")) {
                let mut main = json!({"id": "main", "kind": "ndi", "enabled": true});
                if let Some(theme) = legacy.get("theme") {
                    main["theme"] = theme.clone();
                }
                vec![main]
            } else {
                Vec::new()
            }
        }
    };
    overlay.insert("outputs".into(), Value::Array(kept));
    settings.insert("overlay".into(), Value::Object(overlay));
    settings
}
