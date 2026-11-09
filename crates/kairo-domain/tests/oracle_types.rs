//! Comparisons with the TypeScript originals through `kairo-oracle`: defaults
//! for each settings section, enum value lists, and files the Electron code
//! writes. Runs from `tests/oracle-cache` unless `KAIRO_ORACLE_MODE` says
//! otherwise; refresh with `cargo run -p ts-oracle -- --regenerate`.

mod common;

use common::round_trip;
use kairo_domain::audience::{AudienceQuestionState, AudienceRoomSettings};
use kairo_domain::brand;
use kairo_domain::capture::{CaptureFrameRate, CaptureSettings, CaptureStatus};
use kairo_domain::cloud::{CrashKind, FeedbackKind, FeedbackStatus};
use kairo_domain::collections::LibraryKind;
use kairo_domain::documents::DocumentsSettings;
use kairo_domain::import_menu::ImportKind;
use kairo_domain::json::to_js_value;
use kairo_domain::lyrics::{LyricsSectionType, LyricsSong, LyricsSource};
use kairo_domain::overlay::{OutputAspect, OverlayTheme};
use kairo_domain::program::{
    ConfidenceLayout, OutputShowFilter, PresentationSettings, ProgramState, VideoSettings,
    default_timer_theme,
};
use kairo_domain::services::ServiceUploadState;
use kairo_domain::settings::{
    AppSettings, DEFAULT_TRANSLATION_ID, OnboardingState, OverlaySettings, SECTION_COUNT,
    ShortcutCommand, StoredSettings,
};
use kairo_domain::usage::{UsageErrorKind, UsageFeature};
use kairo_domain::web::WebSettings;
use kairo_domain::workspace::SongFile;
use kairo_oracle::{assert_matches_oracle, generate, oracle_value, oracle_values, strip_undefined};
use proptest::prelude::*;
use serde::Serialize;
use serde_json::{Value, json};

/// A TypeScript constant or zero-argument result, as written to disk.
fn ts(name: &str) -> Value {
    strip_undefined(oracle_value(name, json!([])))
}

fn js<T: Serialize>(value: &T) -> Value {
    to_js_value(value).unwrap()
}

#[test]
fn settings_defaults_match_db_index_section_by_section() {
    let expected = ts("oracle:settings-defaults.settingsDefaults");
    // DEFAULT_OVERLAY_SETTINGS is built by overlay-defaults.ts (prompt 013),
    // so the overlay section is taken from TypeScript and only round tripped.
    let overlay: OverlaySettings = round_trip("overlay default", &expected["overlay"]);
    let actual = js(&AppSettings::defaults(overlay));
    let (Value::Object(expected), Value::Object(actual)) = (&expected, &actual) else {
        panic!("defaults are not objects");
    };
    let mut sections: Vec<_> = expected.keys().chain(actual.keys()).collect();
    sections.sort();
    sections.dedup();
    assert_eq!(sections.len(), SECTION_COUNT, "sections: {sections:?}");
    for section in sections {
        assert_eq!(
            actual.get(section),
            expected.get(section),
            "the default of settings section `{section}` differs from db/index.ts"
        );
    }
}

#[test]
fn exported_defaults_match() {
    let cases: Vec<(&str, Value)> = vec![
        (
            "program.DEFAULT_PRESENTATION_SETTINGS",
            js(&PresentationSettings::default()),
        ),
        (
            "program.DEFAULT_VIDEO_SETTINGS",
            js(&VideoSettings::default()),
        ),
        (
            "program.DEFAULT_SHOW_FILTER",
            js(&OutputShowFilter::default()),
        ),
        (
            "program.DEFAULT_CONFIDENCE_LAYOUT",
            js(&ConfidenceLayout::default()),
        ),
        ("program.EMPTY_PROGRAM_STATE", js(&ProgramState::default())),
        (
            "program.defaultTimerTheme",
            js(&default_timer_theme("#5eead4", "#f87171")),
        ),
        (
            "capture.DEFAULT_CAPTURE_SETTINGS",
            js(&CaptureSettings::default()),
        ),
        ("capture.IDLE_CAPTURE_STATUS", js(&CaptureStatus::default())),
        (
            "documents.DEFAULT_DOCUMENTS_SETTINGS",
            js(&DocumentsSettings::default()),
        ),
        ("web.DEFAULT_WEB_SETTINGS", js(&WebSettings::default())),
        (
            "cloud/onboarding.DEFAULT_ONBOARDING_STATE",
            js(&OnboardingState::default()),
        ),
        (
            "audience-protocol.DEFAULT_AUDIENCE_ROOM_SETTINGS",
            js(&AudienceRoomSettings::default()),
        ),
        (
            "service-records.idleUpload",
            js(&ServiceUploadState::default()),
        ),
        (
            "bible-translations.DEFAULT_TRANSLATION_ID",
            json!(DEFAULT_TRANSLATION_ID),
        ),
        ("brand.PRODUCT_NAME", json!(brand::PRODUCT_NAME)),
        ("brand.PRODUCT_TAGLINE", json!(brand::PRODUCT_TAGLINE)),
        ("brand.NDI_SENDER_NAME", json!(brand::NDI_SENDER_NAME)),
        (
            "brand.BRAND",
            json!({
                "primary": brand::colors::PRIMARY, "white": brand::colors::WHITE,
                "muted": brand::colors::MUTED, "live": brand::colors::LIVE,
            }),
        ),
    ];
    for (name, rust) in cases {
        // Exact: same keys, values and number text, as the file would hold.
        assert_eq!(rust, ts(name), "{name}");
    }
    let themed = default_timer_theme("#123456", "#abcdef");
    assert_matches_oracle(
        "program.defaultTimerTheme",
        json!(["#123456", "#abcdef"]),
        js(&themed),
    );
}

/// Each Rust enum lists the same values, in the same order, as the
/// TypeScript constant it ports.
#[test]
fn enum_value_lists_match() {
    fn strings<T: Copy>(all: &[T], text: fn(T) -> &'static str) -> Value {
        Value::Array(all.iter().map(|v| json!(text(*v))).collect())
    }
    // (oracle name, id key when the constant lists objects, Rust values)
    let cases = [
        (
            "cloud/usage.USAGE_FEATURES",
            None,
            strings(UsageFeature::ALL, UsageFeature::as_str),
        ),
        (
            "cloud/usage.USAGE_ERRORS",
            None,
            strings(UsageErrorKind::ALL, UsageErrorKind::as_str),
        ),
        (
            "cloud/crashes.CRASH_KINDS",
            None,
            strings(CrashKind::ALL, CrashKind::as_str),
        ),
        (
            "cloud/feedback.FEEDBACK_KINDS",
            None,
            strings(FeedbackKind::ALL, FeedbackKind::as_str),
        ),
        (
            "cloud/feedback.FEEDBACK_STATUSES",
            None,
            strings(FeedbackStatus::ALL, FeedbackStatus::as_str),
        ),
        (
            "libraries.LIBRARY_KINDS",
            None,
            strings(LibraryKind::ALL, LibraryKind::as_str),
        ),
        (
            "audience-protocol.AUDIENCE_STATES",
            None,
            strings(AudienceQuestionState::ALL, AudienceQuestionState::as_str),
        ),
        (
            "displays.OUTPUT_ASPECT_VALUES",
            None,
            strings(OutputAspect::ALL, OutputAspect::as_str),
        ),
        (
            "keyboard-shortcuts.SHORTCUT_COMMANDS",
            Some("id"),
            strings(ShortcutCommand::ALL, ShortcutCommand::as_str),
        ),
        (
            "import-menu.IMPORT_OPTIONS",
            Some("kind"),
            strings(ImportKind::ALL, ImportKind::as_str),
        ),
        (
            "capture.CAPTURE_FRAME_RATES",
            None,
            Value::Array(
                CaptureFrameRate::ALL
                    .iter()
                    .map(|r| json!(r.value()))
                    .collect(),
            ),
        ),
    ];
    for (name, key, actual) in cases {
        let mut expected = ts(name);
        if let Some(key) = key {
            expected = expected
                .as_array()
                .unwrap()
                .iter()
                .map(|item| item[key].clone())
                .collect();
        }
        assert_eq!(actual, expected, "{name}");
    }
}

#[test]
fn overlay_defaults_parse_without_loss() {
    round_trip::<OverlaySettings>(
        "DEFAULT_OVERLAY_SETTINGS",
        &ts("overlay-defaults.DEFAULT_OVERLAY_SETTINGS"),
    );
    round_trip::<OverlayTheme>(
        "DEFAULT_OVERLAY_THEME",
        &ts("overlay-defaults.DEFAULT_OVERLAY_THEME"),
    );
}

/// The file a fresh install writes on first launch, with the built-in themes,
/// is canonical in every section and round trips exactly.
#[test]
fn fresh_install_settings_are_canonical() {
    let file = ts("oracle:settings-defaults.freshInstall");
    let stored: StoredSettings = round_trip("fresh install", &file);
    assert_eq!(stored.non_canonical_sections(), Vec::<&str>::new());
    let settings = stored.into_canonical().unwrap();
    assert!(
        settings.theme_library.len() >= 2,
        "built-in themes are seeded"
    );
    assert_eq!(js(&settings), file);
}

fn text() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::new()),
        "[a-zA-Z0-9 ,.'’é-]{1,24}",
        Just("  padded  ".to_owned())
    ]
}

fn song() -> impl Strategy<Value = Value> {
    let section = (
        prop::sample::select(
            LyricsSectionType::ALL
                .iter()
                .map(|t| t.as_str())
                .collect::<Vec<_>>(),
        ),
        text(),
        prop::collection::vec(text(), 0..5),
        prop::option::of(prop::collection::vec(
            prop::option::of("#[0-9a-f]{6}"),
            0..5,
        )),
        prop::option::of(prop::sample::select(vec!["1", "c", "Shift+2", ""])),
    )
        .prop_map(|(kind, label, lines, colors, hotkey)| {
            let mut s = json!({"type": kind, "label": label, "lines": lines});
            if let Some(colors) = colors {
                s["lineColors"] = json!(colors);
            }
            if let Some(hotkey) = hotkey {
                s["hotkey"] = json!(hotkey);
            }
            s
        });
    (
        "[a-z0-9-]{1,12}",
        text(),
        text(),
        prop::option::of(text()),
        prop::option::of("[0-9]{0,7}"),
        prop::option::of(any::<bool>()),
        prop::option::of(prop::sample::select(
            LyricsSource::ALL
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>(),
        )),
        prop::collection::vec(section, 0..4),
        1u64..2_000_000_000_000,
    )
        .prop_map(
            |(id, title, artist, copyright, ccli, favorite, source, sections, at)| {
                let mut song = json!({
                    "id": id, "title": title, "artist": artist, "sections": sections,
                    "createdAt": at, "updatedAt": at + 1,
                });
                for (key, value) in [
                    ("copyright", copyright.map(Value::from)),
                    ("ccliNumber", ccli.map(Value::from)),
                    ("isFavorite", favorite.map(Value::from)),
                    ("source", source.map(Value::from)),
                ] {
                    if let Some(value) = value {
                        song[key] = value;
                    }
                }
                song
            },
        )
}

/// Song files written by `serializeSongFile` parse as [`SongFile`] without
/// loss, and `parseSongFile` returns a [`LyricsSong`] that round trips.
#[test]
fn song_files_written_by_electron_round_trip() {
    let songs = generate(1201, 200, song());
    let inputs: Vec<_> = songs.iter().map(|song| json!([song])).collect();
    let texts = oracle_values("workspace.serializeSongFile", &inputs);
    let inputs: Vec<_> = texts.iter().map(|text| json!([text, "fallback"])).collect();
    let read_backs = oracle_values("workspace.parseSongFile", &inputs);
    for (index, ((song, text), read_back)) in songs.iter().zip(&texts).zip(read_backs).enumerate() {
        round_trip::<LyricsSong>(&format!("generated song {index}"), song);
        let file: Value = serde_json::from_str(text.as_str().expect("a string")).unwrap();
        let parsed: SongFile = round_trip(&format!("song file {index}"), &file);
        assert_eq!(
            parsed.kairo_song,
            kairo_domain::workspace::SONG_FILE_VERSION
        );
        round_trip::<LyricsSong>(
            &format!("parsed song file {index}"),
            &strip_undefined(read_back),
        );
    }
}

#[test]
fn literal_true_fields_reject_false() {
    let segment = json!({"id": "t", "text": "", "words": [], "isFinal": false, "timestamp": 0, "duration": 0});
    assert!(serde_json::from_value::<kairo_domain::transcript::TranscriptResult>(segment).is_err());
}
