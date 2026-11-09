use kairo_domain::{
    audience, capture, collections, documents, json::to_js_value, kairo_bundle, media, web,
};
use kairo_oracle::assert_all_match;
use serde::Serialize;
use serde_json::{Value, json};

fn inputs(values: &[Value], implementation: impl Fn(&Value) -> Value) -> Vec<(Value, Value)> {
    values
        .iter()
        .map(|value| (json!([value]), implementation(value)))
        .collect()
}

fn value(value: impl Serialize) -> Value {
    to_js_value(&value).unwrap()
}

#[test]
fn bundle_parsing_matches_for_edits_errors_and_old_versions() {
    let song = json!({"id":"song-1","title":"  Ẹ ṣé  ","sections":[{"label":"Verse 1","type":"verse","lines":["a",null,42,{}],"lineColors":[null," #fff "],"hotkey":"v"}]});
    let other = json!({"title":"New song","sections":[],"createdAt":1});
    let raw = [
        "{".into(),
        "[]".into(),
        "{}".into(),
        json!({"kairoBundle":2,"kind":"songs"}).to_string(),
        json!({"kairoBundle":1,"kind":"bad"}).to_string(),
        json!({"kairoBundle":1,"kind":"songs","songs":[null,{}]}).to_string(),
        json!({"kairoBundle":0,"kind":"songs","songs":[song,song,other]}).to_string(),
        json!({"kairoSong":1,"id":"legacy","sections":[]}).to_string(),
        json!({"kairoBundle":1,"kind":"setlist","name":"Sunday","songs":[song,other],"setlist":{"songIds":["imported-2","gone","song-1","song-1"]}}).to_string(),
        json!({"kairoBundle":1,"kind":"scripture-playlist","playlist":{"items":[{"reference":" John 3:16 ","verses":[{"book":"John","chapter":"3","verse":[16],"text":" text "},{"book":"John","verse":17,"text":"missing chapter"}]},{"reference":"John 4:1"},{"reference":""}]}}).to_string(),
        json!({"kairoBundle":1,"kind":"scripture-playlist"}).to_string(),
    ];
    let now = 123_456.0;
    let cases: Vec<_> = raw
        .iter()
        .map(|raw| {
            let result = match kairo_bundle::parse_kairo_bundle(raw, now) {
                Ok(bundle) => {
                    let round_trip = kairo_bundle::serialize_bundle(&bundle).unwrap();
                    assert_eq!(
                        kairo_bundle::parse_kairo_bundle(&round_trip, now).unwrap(),
                        bundle
                    );
                    json!({"result": value(bundle)})
                }
                Err(error) => json!({"error": error}),
            };
            (json!([raw, now]), result)
        })
        .collect();
    assert_all_match("oracle:shared-domain.parseBundle", &cases);
}

#[test]
fn media_playback_matches_for_identity_and_hostile_values() {
    let raw = [
        Value::Null,
        json!({}),
        json!([]),
        json!({"startSeconds":-1,"endSeconds":4,"rate":0.1,"fit":"fill","loop":true}),
        json!({"startSeconds":5,"endSeconds":4,"rate":1,"fit":"stretch","loop":"yes"}),
        json!({"hue":600,"saturation":-2,"brightness":0,"contrast":2}),
        json!({"startSeconds":1e50,"endSeconds":1e80,"rate":5,"fit":"contain"}),
        json!({"startSeconds":"4","hue":"-20","brightness":null}),
    ];
    assert_all_match(
        "media-playback.normalizeMediaPlayback",
        &inputs(&raw, |raw| value(media::normalize_media_playback(raw))),
    );
    let normalized: Vec<_> = raw.iter().map(media::normalize_media_playback).collect();
    let cases: Vec<_> = normalized
        .iter()
        .map(|playback| {
            (
                json!([value(playback)]),
                json!(media::media_filter_css(playback)),
            )
        })
        .collect();
    assert_all_match("media-playback.mediaFilterCss", &cases);
}

#[test]
fn capture_and_slideshow_settings_match() {
    let capture_cases = [
        Value::Null,
        json!({"codec":"bad","bitrateMbps":"800","resolution":"720p","frameRate":"60","sourceOutputId":7}),
        json!({"codec":"prores4444","bitrateMbps":null,"frameRate":23,"audioDeviceName":"device"}),
        json!({"codec":"hevc","bitrateMbps":2.5,"frameRate":[25]}),
    ];
    assert_all_match(
        "capture.normalizeCaptureSettings",
        &inputs(&capture_cases, |raw| {
            value(capture::normalize_capture_settings(raw))
        }),
    );
    let slideshow = [
        Value::Null,
        json!({"slideshowSec":"9.5","slideshowLoop":false}),
        json!({"slideshowSec":null,"slideshowAutoStart":true}),
        json!({"slideshowSec":99999}),
    ];
    assert_all_match(
        "documents.normalizeDocumentsSettings",
        &inputs(&slideshow, |raw| {
            value(documents::normalize_documents_settings(raw))
        }),
    );
    let bytes = [
        0.0,
        262_144.0,
        -262_144.0,
        1_205_862.4,
        0.0001,
        15_554_281.0,
    ];
    let cases = bytes
        .into_iter()
        .map(|bytes| {
            (
                json!([bytes]),
                json!(documents::format_document_size(bytes)),
            )
        })
        .collect::<Vec<_>>();
    assert_all_match("documents.formatDocumentSize", &cases);
}

#[test]
fn audience_event_validation_matches() {
    let events = [
        "{".into(),
        json!({"type":"snapshot","extra":1}).to_string(),
        json!({"type":"ask","requestId":"req-1","text":"hello","author":"Sam"}).to_string(),
        json!({"type":"ask","requestId":"req-1","text":"hello","author":null}).to_string(),
        json!({"type":"vote","requestId":"invalid id","id":"a","voted":true}).to_string(),
        json!({"type":"state","requestId":"req-1","id":"a","state":"hidden"}).to_string(),
        json!({"type":"poll-vote","requestId":"req-1","id":"a","options":["1","2"]}).to_string(),
        json!({"type":"poll-state","requestId":"req-1","id":"a","state":"open","showResults":false}).to_string(),
        json!({"type":"poll-save","requestId":"req-1","poll":{"title":"What?"}}).to_string(),
        json!({"type":"settings","requestId":"req-1","settings":{"review":"false","names":"off"}}).to_string(),
        json!({"type":"end","requestId":"x".repeat(65)}).to_string(),
        json!({"type":"ask","requestId":"req-1","text":"😀".repeat(4097)}).to_string(),
    ];
    let cases: Vec<_> = events
        .iter()
        .map(|raw| (json!([raw]), value(audience::parse_audience_event(raw))))
        .collect();
    assert_all_match("audience-protocol.parseAudienceEvent", &cases);
}

#[test]
fn web_addresses_and_bookmarks_match() {
    let cases: Vec<_> = [
        "",
        " youtube.com ",
        "localhost:8080/foo",
        "example.com:443/a#b",
        "foo bar",
        "file:///tmp/x",
        "javascript:alert(1)",
        "https://example.org:8080/a?q=x",
        "Ọ̀pẹ́ 🙏",
    ]
    .into_iter()
    .flat_map(|input| {
        [false, true].into_iter().map(move |search| {
            (
                json!([input, search]),
                json!(web::web_address(input, search)),
            )
        })
    })
    .collect();
    assert_all_match("web.webAddress", &cases);
    let bookmarks = [
        json!({"bookmarks":[{"id":"a","url":"youtube.com","name":" "},{"id":"b","url":"file:///tmp/x"},{"id":"c","url":"https://host/foo","name":"   Live   "}],"showPointer":true}),
    ];
    assert_all_match(
        "web.normalizeWebSettings",
        &inputs(&bookmarks, |raw| value(web::normalize_web_settings(raw))),
    );
}

#[test]
fn saved_collections_keep_order_and_repair_old_state() {
    let cases = [
        json!({"songIds":["a",7,"b"],"updatedAt":12}),
        json!({"lists":[{"id":"a","name":"  Sunday   Service  ","songIds":[1,"x"],"createdAt":3,"updatedAt":3}],"activeId":"missing"}),
        Value::Null,
    ].into_iter().map(|raw| (json!([raw, 100]), value(collections::migrate_setlist_state(&raw, 100.0)))).collect::<Vec<_>>();
    assert_all_match("setlist.migrateSetlistState", &cases);
    let raw = [
        json!({"libraries":[{"id":"live","name":"  Live  ","createdAt":1,"updatedAt":1},{"id":"default","name":"wrong"}],"assignments":{"a":"live","b":"missing","c":"default"}}),
        Value::Null,
    ];
    assert_all_match(
        "libraries.normalizeLibraryState",
        &inputs(&raw, |raw| value(collections::normalize_library_state(raw))),
    );
}

#[test]
fn cloud_pairing_redirects_and_onboarding_match() {
    use kairo_domain::cloud;
    let codes = [
        json!("k2x7"),
        json!("PROA-k2x7"),
        json!("PROA-K2X77"),
        json!("proa k2o7"),
        json!(""),
    ];
    assert_all_match(
        "cloud/device-code.formatUserCode",
        &inputs(&codes, |v| {
            json!(cloud::format_user_code(v.as_str().unwrap()))
        }),
    );
    assert_all_match(
        "cloud/device-code.isValidUserCode",
        &inputs(&codes, |v| {
            json!(cloud::is_valid_user_code(v.as_str().unwrap()))
        }),
    );
    let urls = [
        json!("/activate?userCode=ABCD-EFGH"),
        json!("//evil.test"),
        json!("/\\evil.test"),
        json!("/%5cevil.test"),
        json!("https://evil.test"),
        json!("/%0aevil"),
        json!("/%oops"),
        json!("/foo/../bar?q=1#hash"),
        Value::Null,
    ];
    assert_all_match(
        "cloud/return-path.safeReturnPath",
        &inputs(&urls, |v| {
            json!(cloud::safe_return_path(v.as_str(), "/dashboard"))
        }),
    );
    let states = [
        Value::Null,
        json!({"completedSteps":["church","output","output","welcome"],"skippedSteps":["apiKeys","account"],"currentStep":"apiKeys","completedAt":17,"source":"legacy"}),
        json!({"completedSteps":true,"currentStep":"account","completedAt":"12"}),
    ];
    assert_all_match(
        "cloud/onboarding.normalizeOnboardingState",
        &inputs(&states, |v| value(cloud::normalize_onboarding_state(v))),
    );
    let zones = [
        "Africa/Lagos",
        "America/Argentina/La_Rioja",
        "America/Los_Angeles",
        "America/New_York",
        "Asia/Kolkata",
    ]
    .map(String::from);
    let cases = ["", "  los-angeles", "america", "York", "lagos", "no match"]
        .into_iter()
        .map(|q| {
            (
                json!([zones, q, 3]),
                json!(cloud::match_timezones(&zones, q, 3)),
            )
        })
        .collect::<Vec<_>>();
    assert_all_match("cloud/timezone-search.matchTimezones", &cases);
}

#[test]
fn secret_patch_preserves_empty_values_until_explicitly_cleared() {
    use kairo_domain::{cloud, settings::SettingsSecretClearKey as Key};
    let current =
        json!({"apiKey":"test-current","anthropicApiKey":"test-old","language":"en","future":7});
    let raw = [
        json!({"apiKey":" ","language":"yo"}),
        json!({"apiKey":" test-new ","clearKeys":["anthropicApiKey"]}),
        json!({"apiKey":null,"anthropicApiKey":42,"clearKeys":["apiKey"]}),
    ];
    let keys = [Key::ApiKey, Key::AnthropicApiKey];
    let cases = raw
        .iter()
        .map(|incoming| {
            let out = cloud::merge_secret_section(
                current.as_object().unwrap(),
                incoming.as_object().unwrap(),
                &keys,
            );
            (json!([current, incoming, keys]), Value::Object(out))
        })
        .collect::<Vec<_>>();
    assert_all_match("cloud/org-secrets.mergeSecretSection", &cases);
}

#[test]
fn panel_widths_match_legacy_migration_and_drag_bounds() {
    use kairo_domain::operator_layout::{
        OperatorPanelSide, normalize_operator_panel_width, resize_operator_panel,
    };
    let mut resize = Vec::new();
    let mut normalize = Vec::new();
    for (name, side) in [
        ("left", OperatorPanelSide::Left),
        ("right", OperatorPanelSide::Right),
    ] {
        for delta in [-1000.0, -20.4, 0.0, 50.5, 1000.0] {
            resize.push((
                json!([name, 460, delta]),
                json!(resize_operator_panel(side, 460.0, delta)),
            ));
        }
        for stored in [
            None,
            Some(""),
            Some("NaN"),
            Some("240"),
            Some("320"),
            Some("0x220"),
            Some("650.5"),
        ] {
            normalize.push((
                json!([name, stored]),
                json!(normalize_operator_panel_width(side, stored, None)),
            ));
        }
    }
    assert_all_match("operator-layout.resizeOperatorPanel", &resize);
    assert_all_match("operator-layout.normalizeOperatorPanelWidth", &normalize);
}
