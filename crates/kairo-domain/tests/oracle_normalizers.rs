//! Differential tests of every prompt 013 normaliser against the TypeScript,
//! on hostile JSON (see `common`), plus idempotence: normalising twice
//! equals normalising once.
//!
//! By default each test runs its cached cases. The full run of 100,000 cases
//! per normaliser needs Node and the Electron checkout:
//!
//! ```text
//! KAIRO_ORACLE_MODE=live KAIRO_DIFF_CASES=100000 \
//!   cargo test -p kairo-domain --release --test oracle_normalizers
//! ```

mod common;

use common::*;
use kairo_domain::json::to_js_value;
use kairo_domain::normalize::{
    as_object, clamp_num, is_plain_object, non_neg_number, safe_bool, safe_color, safe_enum_in,
    safe_number, safe_string,
};
use kairo_domain::overlay::{OverlayLayoutPosition, OverlayTheme};
use kairo_domain::overlay_defaults::{
    LegacyOverlayFields, LegacyOverlayMode, normalize_overlay_outputs, normalize_overlay_settings,
    normalize_overlay_theme,
};
use kairo_domain::overlay_elements::{normalize_overlay_element, normalize_overlay_elements};
use kairo_domain::pp_retirement::retire_pro_presenter_settings;
use kairo_domain::program::{normalize_confidence_layout, normalize_show_filter};
use kairo_domain::theme_library::normalize_theme_library;
use kairo_oracle::{assert_all_match, undefined};
use proptest::prelude::*;
use serde::Serialize;
use serde_json::{Value, json};

fn js<T: Serialize>(value: &T) -> Value {
    to_js_value(value).unwrap()
}

/// `undefined` for an absent argument.
fn arg(v: &Option<Value>) -> Value {
    v.clone().unwrap_or_else(undefined)
}

/// The top-level argument: usually the shape, sometimes anything, sometimes absent.
fn top(shape: BoxedStrategy<Value>) -> BoxedStrategy<Option<Value>> {
    prop_oneof![
        12 => shape.prop_map(Some),
        2 => hostile().prop_map(Some),
        1 => Just(None),
    ]
    .boxed()
}

/// Asserts `normalize(normalize(x)) == normalize(x)` through JSON, as a store
/// file would carry it.
fn assert_idempotent<T: Serialize>(
    label: &str,
    input: &Option<Value>,
    once: &T,
    again: impl Fn(&Value) -> T,
) {
    let once = js(once);
    let twice = js(&again(&once));
    assert_eq!(once, twice, "{label} is not idempotent for input {input:?}");
}

/// The background keys `DEFAULT_OVERLAY_THEME` lacks and the theme normaliser adds.
const FILLED_BACKGROUND_KEYS: [&str; 6] = [
    "hue",
    "saturation",
    "brightness",
    "contrast",
    "blurPx",
    "mediaLoop",
];

/// Idempotence for the output and settings normalisers, which the TypeScript
/// does not quite have: an output it creates (the Main Screen for an empty
/// list, the legacy migration) carries `DEFAULT_OVERLAY_THEME` unnormalised,
/// so the second pass adds that theme's missing background keys. Asserts that
/// those keys are the only difference, and that from the second pass on the
/// result is fixed. Recorded in DECISIONS.md (prompt 013).
fn assert_idempotent_from_second_pass<T: Serialize>(
    label: &str,
    input: &Option<Value>,
    once: &T,
    again: impl Fn(&Value) -> T,
) {
    let once = js(once);
    let twice = js(&again(&once));
    for path in kairo_oracle::compare(&once, &twice) {
        let allowed = FILLED_BACKGROUND_KEYS
            .iter()
            .any(|key| path.contains(&format!("theme.background.{key}: oracle None")));
        assert!(
            allowed,
            "{label}: unexpected second-pass change {path} for input {input:?}"
        );
    }
    let thrice = js(&again(&twice));
    assert_eq!(
        twice, thrice,
        "{label} is not idempotent from the second pass for input {input:?}"
    );
}

#[test]
fn normalize_overlay_theme_matches_and_is_idempotent() {
    chunked(13_100, cases(500), top(theme()), |inputs| {
        let cases: Vec<_> = inputs
            .iter()
            .map(|raw| {
                let out = normalize_overlay_theme(raw.as_ref());
                assert_idempotent("normalizeOverlayTheme", raw, &out, |v| {
                    normalize_overlay_theme(Some(v))
                });
                (json!([arg(raw)]), js(&out))
            })
            .collect();
        assert_all_match("overlay-defaults.normalizeOverlayTheme", &cases);
    });
}

#[test]
fn normalize_overlay_element_matches_and_is_idempotent() {
    chunked(13_200, cases(2_000), top(element()), |inputs| {
        let cases: Vec<_> = inputs
            .iter()
            .map(|raw| {
                let out = normalize_overlay_element(raw.as_ref());
                if let Some(element) = &out {
                    assert_idempotent("normalizeOverlayElement", raw, element, |v| {
                        normalize_overlay_element(Some(v))
                            .expect("a normalised element stays valid")
                    });
                }
                let expected = out.as_ref().map_or(Value::Null, js);
                (json!([arg(raw)]), expected)
            })
            .collect();
        assert_all_match("overlay-elements.normalizeOverlayElement", &cases);
    });
}

#[test]
fn normalize_overlay_elements_matches_and_is_idempotent() {
    chunked(13_300, cases(500), top(elements()), |inputs| {
        let cases: Vec<_> = inputs
            .iter()
            .map(|raw| {
                let out = normalize_overlay_elements(raw.as_ref());
                assert_idempotent("normalizeOverlayElements", raw, &out, |v| {
                    normalize_overlay_elements(Some(v))
                });
                (json!([arg(raw)]), js(&out))
            })
            .collect();
        assert_all_match("overlay-elements.normalizeOverlayElements", &cases);
    });
}

fn legacy() -> BoxedStrategy<(Option<LegacyOverlayMode>, Option<Value>, String)> {
    (
        prop::option::of(prop::sample::select(LegacyOverlayMode::ALL.to_vec())),
        prop::option::of(theme()),
        prop::sample::select(vec!["{Reference}\n{Text}", "{Text}", ""]).prop_map(str::to_owned),
    )
        .boxed()
}

fn legacy_json(fields: &LegacyOverlayFields) -> Value {
    let mut v = json!({"theme": js(&fields.theme), "template": fields.template});
    if let Some(mode) = fields.mode {
        v["mode"] = json!(mode.as_str());
    }
    v
}

#[test]
fn normalize_overlay_outputs_matches_and_is_idempotent() {
    chunked(13_400, cases(300), (top(outputs()), legacy()), |inputs| {
        let cases: Vec<_> = inputs
            .iter()
            .map(|(raw, (mode, theme, template))| {
                let fields = LegacyOverlayFields {
                    mode: *mode,
                    theme: normalize_overlay_theme(theme.as_ref()),
                    template: template.clone(),
                };
                let out = normalize_overlay_outputs(raw.as_ref(), &fields);
                assert_idempotent_from_second_pass("normalizeOverlayOutputs", raw, &out, |v| {
                    normalize_overlay_outputs(Some(v), &fields)
                });
                (json!([arg(raw), legacy_json(&fields)]), js(&out))
            })
            .collect();
        assert_all_match("overlay-defaults.normalizeOverlayOutputs", &cases);
    });
}

#[test]
fn normalize_overlay_settings_matches_and_is_idempotent() {
    chunked(13_500, cases(200), top(overlay_settings()), |inputs| {
        let cases: Vec<_> = inputs
            .iter()
            .map(|raw| {
                let out = normalize_overlay_settings(raw.as_ref());
                assert_idempotent_from_second_pass("normalizeOverlaySettings", raw, &out, |v| {
                    normalize_overlay_settings(Some(v))
                });
                (json!([arg(raw)]), js(&out))
            })
            .collect();
        assert_all_match("overlay-defaults.normalizeOverlaySettings", &cases);
    });
}

#[test]
fn normalize_theme_library_matches_and_is_idempotent() {
    let strategy = (top(theme_library()), theme(), 0.0..2e12f64);
    chunked(13_600, cases(300), strategy, |inputs| {
        let cases: Vec<_> = inputs
            .iter()
            .map(|(raw, active, now)| {
                let active: OverlayTheme = normalize_overlay_theme(Some(active));
                let out = normalize_theme_library(raw.as_ref(), &active, *now, "legacy-id");
                assert_idempotent("normalizeThemeLibrary", raw, &out, |v| {
                    normalize_theme_library(Some(v), &active, *now, "legacy-id")
                });
                (json!([arg(raw), js(&active), now, "legacy-id"]), js(&out))
            })
            .collect();
        assert_all_match("theme-library.normalizeThemeLibrary", &cases);
    });
}

#[test]
fn retire_pro_presenter_settings_matches_and_is_idempotent() {
    chunked(13_700, cases(1_000), top(retire_input()), |inputs| {
        let cases: Vec<_> = inputs
            .iter()
            .map(|raw| {
                let out = Value::Object(retire_pro_presenter_settings(raw.as_ref()));
                let twice = Value::Object(retire_pro_presenter_settings(Some(&out)));
                assert_eq!(
                    out, twice,
                    "retireProPresenterSettings is not idempotent for {raw:?}"
                );
                (json!([arg(raw)]), out)
            })
            .collect();
        assert_all_match("pp-retirement.retireProPresenterSettings", &cases);
    });
}

#[test]
fn normalize_show_filter_and_confidence_layout_match() {
    let show = output().prop_map(|o| o.get("show").cloned());
    chunked(13_800, cases(2_000), (show, top(hostile())), |inputs| {
        let mut show_cases = Vec::new();
        let mut confidence_cases = Vec::new();
        for (show, other) in &inputs {
            let out = normalize_show_filter(show.as_ref());
            assert_idempotent("normalizeShowFilter", show, &out, |v| {
                normalize_show_filter(Some(v))
            });
            show_cases.push((json!([arg(show)]), js(&out)));
            let out = normalize_confidence_layout(other.as_ref());
            assert_idempotent("normalizeConfidenceLayout", other, &out, |v| {
                normalize_confidence_layout(Some(v))
            });
            confidence_cases.push((json!([arg(other)]), js(&out)));
        }
        assert_all_match("program.normalizeShowFilter", &show_cases);
        assert_all_match("program.normalizeConfidenceLayout", &confidence_cases);
    });
    // Confidence layouts with plausible fields too.
    let confidence = output().prop_map(|o| o.get("confidence").cloned());
    chunked(13_850, cases(2_000), confidence, |inputs| {
        let cases: Vec<_> = inputs
            .iter()
            .map(|raw| {
                (
                    json!([arg(raw)]),
                    js(&normalize_confidence_layout(raw.as_ref())),
                )
            })
            .collect();
        assert_all_match("program.normalizeConfidenceLayout", &cases);
    });
}

/// The primitives in `normalize.ts`, each on any value and a fallback.
#[test]
fn normalize_primitives_match() {
    let value =
        prop_oneof![4 => hostile().prop_map(Some), 2 => color().prop_map(Some), 1 => Just(None)];
    let strategy = (value, -10.0..10.0f64, -10.0..10.0f64, any::<bool>());
    chunked(13_900, cases(2_000), strategy, |inputs| {
        let mut by_fn: Vec<(&str, Vec<(Value, Value)>)> = [
            "isPlainObject",
            "asObject",
            "safeString",
            "safeBool",
            "safeNumber",
            "clampNum",
            "nonNegNumber",
            "safeColor",
            "safeEnum",
        ]
        .into_iter()
        .map(|name| (name, Vec::new()))
        .collect();
        for (v, a, b, flag) in &inputs {
            let (min, max) = if a <= b { (*a, *b) } else { (*b, *a) };
            let x = v.as_ref();
            let u = arg(v);
            let rows: [(Value, Value); 9] = [
                (json!([u]), json!(is_plain_object(x))),
                (json!([u]), Value::Object(as_object(x).clone())),
                (json!([u, "fallback"]), json!(safe_string(x, "fallback"))),
                (json!([u, flag]), json!(safe_bool(x, *flag))),
                (json!([u, a]), json!(safe_number(x, *a))),
                (json!([u, min, max, a]), json!(clamp_num(x, min, max, *a))),
                (json!([u, a]), json!(non_neg_number(x, *a))),
                (json!([u, "#000"]), json!(safe_color(x, "#000"))),
                (
                    json!([u, ["lower-third", "center", "top", "full"], "center"]),
                    json!(
                        safe_enum_in(x, OverlayLayoutPosition::ALL, OverlayLayoutPosition::Center)
                            .as_str()
                    ),
                ),
            ];
            for ((_, cases), row) in by_fn.iter_mut().zip(rows) {
                cases.push(row);
            }
        }
        for (name, cases) in &by_fn {
            assert_all_match(&format!("normalize.{name}"), cases);
        }
    });
}
