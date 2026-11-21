//! Ported from `local-bible-pack.test.ts`, `local-nkjv-routing.test.ts` and
//! `ampc-pack.test.ts`: install, lookup, search, status and removal of local
//! packs. The converter CLI tests stay with the converter in the Electron
//! checkout, and the IPC, preload and packaging contract tests belong to the
//! wiring and packaging prompts (see DECISIONS.md, 033).

mod common;

use common::*;
use kairo_domain::scripture::TranslationAccess;
use kairo_scripture::ScriptureError;
use kairo_scripture::api::ApiSearchHit;
use kairo_store::db::PackVerse;
use std::sync::Arc;

fn tmp() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn texts(verses: &[kairo_domain::scripture::ScriptureVerse]) -> Vec<String> {
    verses.iter().map(|v| v.text.clone()).collect()
}

#[test]
fn installer_validates_the_generated_pack_and_reads_its_counts() {
    let dir = tmp();
    let service = service_with_db(open_dest_with_kjv(dir.path()));
    let pack = write_pack(
        &dir.path().join("nkjv-pack.db"),
        "NKJV",
        "Synthetic Fixture Translation",
        &marked_fixture_verses(),
    );
    let result = block_on(service.install_local_bible_pack(&pack, None)).unwrap();
    assert_eq!(
        (
            result.translation.as_str(),
            result.verse_count,
            result.chapter_count
        ),
        ("NKJV", FIXTURE_VERSES, FIXTURE_CHAPTERS)
    );
}

#[test]
fn installer_refuses_to_overwrite_a_bundled_translation() {
    let dir = tmp();
    let db = open_dest_with_kjv(dir.path());
    let service = service_with_db(db.clone());
    let kjv = write_pack(
        &dir.path().join("kjv-pack.db"),
        "KJV",
        "KJV",
        &fixture_verses(),
    );
    let error = block_on(service.install_local_bible_pack(&kjv, None)).unwrap_err();
    assert!(error.to_string().contains("bundled KJV"), "{error}");
    assert_eq!(block_on(db.get_verse_count("KJV")).unwrap(), 2);
    // BBE ships in resources/bible.db too, with or without local rows present.
    let bbe = write_pack(
        &dir.path().join("bbe-pack.db"),
        "BBE",
        "BBE",
        &fixture_verses(),
    );
    let error = block_on(service.install_local_bible_pack(&bbe, None)).unwrap_err();
    assert!(error.to_string().contains("bundled BBE"), "{error}");
}

#[test]
fn installer_enforces_the_expected_translation_id_for_downloads() {
    let dir = tmp();
    let db = open_dest_with_kjv(dir.path());
    let service = service_with_db(db.clone());
    // A checksum proves the bytes, not the translation inside them.
    let kjv = write_pack(
        &dir.path().join("kjv-pack.db"),
        "KJV",
        "KJV",
        &fixture_verses(),
    );
    let error = block_on(service.install_local_bible_pack(&kjv, Some("NKJV"))).unwrap_err();
    assert!(
        error.to_string().contains("containing only NKJV"),
        "{error}"
    );
    assert_eq!(block_on(db.get_verse_count("KJV")).unwrap(), 2);

    let esv = write_pack(
        &dir.path().join("esv-pack.db"),
        "ESV",
        "ESV",
        &fixture_verses(),
    );
    let installed = block_on(service.install_local_bible_pack(&esv, Some("ESV"))).unwrap();
    assert_eq!(installed.translation, "ESV");
    assert_eq!(block_on(db.get_verse_count("ESV")).unwrap(), 31_102);
}

#[test]
fn installer_rejects_non_database_unstructured_and_mixed_packs() {
    let dir = tmp();
    let service = service_with_db(open_dest_with_kjv(dir.path()));
    let install = |path: &std::path::Path| block_on(service.install_local_bible_pack(path, None));

    let not_db = dir.path().join("notes.txt");
    std::fs::write(&not_db, "not a database").unwrap();
    assert!(install(&not_db).is_err());
    let missing = install(&dir.path().join("missing.db")).unwrap_err();
    assert!(
        missing.to_string().contains("could not be found"),
        "{missing}"
    );

    let unstructured = dir.path().join("empty.db");
    rusqlite::Connection::open(&unstructured)
        .unwrap()
        .execute_batch("CREATE TABLE notes (x)")
        .unwrap();
    let error = install(&unstructured).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("missing the translations or verses table"),
        "{error}"
    );

    // A pack holding two translations is refused even when NKJV is complete.
    let mixed = write_pack_with(
        &dir.path().join("mixed.db"),
        &[("NKJV", "NKJV"), ("KJV", "KJV")],
        "NKJV",
        &fixture_verses(),
        &[("KJV", 43, 3, 16, "An extra translation row.")],
    );
    let error = install(&mixed).unwrap_err();
    assert!(error.to_string().contains("single translation"), "{error}");
}

#[test]
fn installer_rejects_an_incomplete_pack_without_touching_the_installed_copy() {
    let dir = tmp();
    let db = open_dest_with_kjv(dir.path());
    let service = service_with_db(db.clone());
    let pack = write_pack(
        &dir.path().join("nkjv.db"),
        "NKJV",
        "Fixture",
        &marked_fixture_verses(),
    );
    block_on(service.install_local_bible_pack(&pack, None)).unwrap();
    let before = block_on(db.get_verse("NKJV", "John", 3.0, 16.0))
        .unwrap()
        .unwrap()
        .text;

    let short: Vec<PackVerse> = marked_fixture_verses()
        .into_iter()
        .filter(|r| r.book_id != 66)
        .collect();
    let short = write_pack(&dir.path().join("short.db"), "NKJV", "Fixture", &short);
    assert!(block_on(service.install_local_bible_pack(&short, None)).is_err());
    assert_eq!(block_on(db.get_verse_count("NKJV")).unwrap(), 31_102);
    let after = block_on(db.get_verse("NKJV", "John", 3.0, 16.0))
        .unwrap()
        .unwrap()
        .text;
    assert_eq!(after, before);
}

#[test]
fn status_reports_nkjv_absent_before_install_and_present_after() {
    let dir = tmp();
    let service = service_with_db(open_dest_with_kjv(dir.path()));
    let before = block_on(service.get_local_bible_pack_status("NKJV")).unwrap();
    assert!(!before.installed);
    assert_eq!(before.verse_count, 0.0);

    let pack = write_pack(
        &dir.path().join("nkjv.db"),
        "NKJV",
        "Fixture",
        &marked_fixture_verses(),
    );
    let result = block_on(service.install_local_bible_pack(&pack, None)).unwrap();
    assert_eq!(
        serde_json::to_value(&result).unwrap(),
        serde_json::json!({ "translation": "NKJV", "verseCount": 31102.0, "chapterCount": 1189.0, "installed": true })
    );
    let after = block_on(service.get_local_bible_pack_status("nkjv")).unwrap();
    assert!(after.installed);
    assert_eq!(
        (after.verse_count, after.chapter_count),
        (FIXTURE_VERSES, FIXTURE_CHAPTERS)
    );
    assert_eq!(after.translation, "NKJV");
}

#[test]
fn installed_nkjv_serves_reference_lookup_and_phrase_search_locally() {
    let dir = tmp();
    let service = service_with_db(open_dest_with_kjv(dir.path()));
    let pack = write_pack(
        &dir.path().join("nkjv.db"),
        "NKJV",
        "Fixture",
        &marked_fixture_verses(),
    );
    block_on(service.install_local_bible_pack(&pack, None)).unwrap();

    let verses = block_on(service.lookup_verses("John", 3.0, 16.0, None, Some("NKJV"))).unwrap();
    assert_eq!(verses.len(), 1);
    assert!(verses[0].text.contains(FIXTURE_MARKER));

    let results = block_on(service.search("John 3:16", Some("NKJV"))).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].translation, "NKJV");
    assert_eq!(results[0].reference, "John 3:16");
    assert!(results[0].verses[0].text.contains(FIXTURE_MARKER));

    let results =
        block_on(service.search(&format!("{FIXTURE_MARKER} covenant"), Some("NKJV"))).unwrap();
    assert!(!results.is_empty());
    assert_eq!(results[0].translation, "NKJV");
    assert!(results[0].verses[0].text.contains(FIXTURE_MARKER));
}

#[test]
fn a_failed_import_rolls_back_and_preserves_the_previous_nkjv() {
    let dir = tmp();
    let db = open_dest_with_kjv(dir.path());
    let service = service_with_db(db.clone());
    let pack = write_pack(
        &dir.path().join("nkjv.db"),
        "NKJV",
        "Fixture",
        &marked_fixture_verses(),
    );
    block_on(service.install_local_bible_pack(&pack, None)).unwrap();
    let before = block_on(db.get_verse("NKJV", "John", 3.0, 16.0))
        .unwrap()
        .unwrap()
        .text;

    // An unknown book violates the foreign key part way through.
    let mut rows = fixture_verses();
    rows[15_000].book_id = 99;
    assert!(block_on(db.import_translation_pack("NKJV", "Broken", "en", rows)).is_err());
    assert_eq!(block_on(db.get_verse_count("NKJV")).unwrap(), 31_102);
    assert_eq!(
        block_on(db.get_verse("NKJV", "John", 3.0, 16.0))
            .unwrap()
            .unwrap()
            .text,
        before
    );
    assert!(
        block_on(service.get_local_bible_pack_status("NKJV"))
            .unwrap()
            .installed
    );
}

#[test]
fn removing_nkjv_leaves_kjv_verses_fts_rows_and_default_untouched() {
    let dir = tmp();
    let db = open_dest_with_kjv(dir.path());
    let service = service_with_db(db.clone());
    let pack = write_pack(
        &dir.path().join("nkjv.db"),
        "NKJV",
        "Fixture",
        &marked_fixture_verses(),
    );
    block_on(service.install_local_bible_pack(&pack, None)).unwrap();

    let status = block_on(service.remove_local_bible_translation("NKJV")).unwrap();
    assert!(!status.installed);
    assert_eq!(status.verse_count, 0.0);
    assert_eq!(block_on(db.get_verse_count("NKJV")).unwrap(), 0);
    assert_eq!(block_on(db.get_verse_count("KJV")).unwrap(), 2);
    assert_eq!(
        block_on(db.get_verse("KJV", "John", 3.0, 16.0))
            .unwrap()
            .unwrap()
            .text,
        "Synthetic KJV stand-in for John 3:16."
    );
    let translations = block_on(db.get_all_translations()).unwrap();
    assert!(translations.iter().any(|t| t.id == "KJV" && t.is_default));
    assert!(!translations.iter().any(|t| t.id == "NKJV"));
    assert!(
        !block_on(service.search("stand-in", Some("KJV")))
            .unwrap()
            .is_empty()
    );

    for bundled in ["KJV", "BBE"] {
        let error = block_on(service.remove_local_bible_translation(bundled)).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("The bundled {bundled} copy cannot be removed.")
        );
    }
    assert_eq!(block_on(db.get_verse_count("KJV")).unwrap(), 2);
}

#[test]
fn reinstalling_over_an_existing_nkjv_replaces_it_without_duplicating_rows() {
    let dir = tmp();
    let db = open_dest_with_kjv(dir.path());
    let service = service_with_db(db.clone());
    for name in ["a.db", "b.db"] {
        let pack = write_pack(
            &dir.path().join(name),
            "NKJV",
            "Fixture",
            &marked_fixture_verses(),
        );
        block_on(service.install_local_bible_pack(&pack, None)).unwrap();
    }
    assert_eq!(block_on(db.get_verse_count("NKJV")).unwrap(), 31_102);
    let fts: i64 = block_on(db.database().read(|c| {
        c.query_row(
            "SELECT count(*) FROM verses_fts WHERE translation_id = 'NKJV'",
            [],
            |r| r.get(0),
        )
    }))
    .unwrap();
    assert_eq!(fts, 31_102);
    assert_eq!(block_on(db.get_verse_count("KJV")).unwrap(), 2);
}

#[test]
fn closed_service_reports_not_open_or_nothing() {
    let service = kairo_scripture::ScriptureService::new(Arc::new(NoNetwork));
    assert!(matches!(
        block_on(service.get_local_bible_pack_status("NKJV")),
        Err(ScriptureError::NotOpen)
    ));
    assert!(matches!(
        block_on(service.remove_local_bible_translation("NKJV")),
        Err(ScriptureError::NotOpen)
    ));
    assert!(
        block_on(service.search("John 3:16", None))
            .unwrap()
            .is_empty()
    );
    assert!(
        block_on(service.lookup_chapter("John", 3.0, None))
            .unwrap()
            .is_empty()
    );
    assert!(!block_on(service.is_local_translation("KJV")).unwrap());
    assert_eq!(
        block_on(service.get_translation_options(false, false))
            .unwrap()
            .len(),
        17
    );
}

// ─── local-nkjv-routing.test.ts ─────────────────────────────────────────────

fn service_with_local_nkjv(
    dir: &std::path::Path,
) -> (
    kairo_scripture::ScriptureService,
    kairo_scripture::bible_db::BibleDb,
) {
    let db = open_empty(dir);
    block_on(db.import_translation_pack(
        "NKJV",
        "Synthetic Fixture Translation",
        "en",
        marked_fixture_verses(),
    ))
    .unwrap();
    (service_with_db(db.clone()), db)
}

fn stub(hits: Vec<ApiSearchHit>) -> Arc<StubApi> {
    Arc::new(StubApi {
        hits,
        authorized: ["NKJV".to_string()].into(),
        ..Default::default()
    })
}

#[test]
fn local_nkjv_reference_and_phrase_search_make_zero_api_calls() {
    let dir = tmp();
    let (service, _db) = service_with_local_nkjv(dir.path());
    let api = stub(Vec::new());
    service.attach_api_bible(Some(api.clone()));

    let results = block_on(service.search("John 3:16", Some("NKJV"))).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].translation, "NKJV");
    assert!(results[0].verses[0].text.contains(FIXTURE_MARKER));

    let results =
        block_on(service.search(&format!("{FIXTURE_MARKER} covenant"), Some("NKJV"))).unwrap();
    assert!(!results.is_empty());
    assert_eq!(results[0].translation, "NKJV");
    assert!(api.calls().is_empty(), "{:?}", api.calls());
}

#[test]
fn local_nkjv_needs_no_api_for_reference_or_phrase_search() {
    let dir = tmp();
    let (service, _db) = service_with_local_nkjv(dir.path());
    assert_eq!(
        block_on(service.search("John 3:16", Some("NKJV")))
            .unwrap()
            .len(),
        1
    );
    assert!(
        !block_on(service.search(&format!("{FIXTURE_MARKER} covenant"), Some("NKJV")))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn api_bible_fallback_still_serves_nkjv_when_no_local_copy_exists() {
    let dir = tmp();
    let service = service_with_db(open_empty(dir.path()));
    let api = stub(vec![ApiSearchHit {
        reference: "John 3:16".into(),
        text: "Synthetic API fallback verse text.".into(),
        book_id: Some("JHN".into()),
        chapter: Some(3.0),
        verse: Some(16.0),
    }]);
    service.attach_api_bible(Some(api.clone()));
    let results =
        block_on(service.search("grace upon grace remembered words", Some("NKJV"))).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].translation, "NKJV");
    assert_eq!(
        results[0].verses[0].text,
        "Synthetic API fallback verse text."
    );
    assert_eq!(results[0].verses[0].book, "John");
    assert!(api.calls().contains(&"search".to_string()));

    // A reference in an API translation goes to API.Bible, never another local Bible.
    let results = block_on(service.search("John 3:16", Some("NKJV"))).unwrap();
    assert_eq!(results[0].reference, "John 3:16");
    assert!(api.calls().contains(&"lookupPassage".to_string()));
    let error = block_on(service.search("John 3:16", Some("NIV"))).unwrap_err();
    assert_eq!(
        error.to_string(),
        "NIV is not authorized for this API.Bible key."
    );
    let results = block_on(service.search("Psalm 23", Some("NKJV"))).unwrap();
    assert_eq!(results[0].reference, "Psalms 23");
}

#[test]
fn api_hits_without_a_reference_book_use_the_book_id_and_drop_incomplete_hits() {
    let dir = tmp();
    let service = service_with_db(open_dest_with_kjv(dir.path()));
    let hit =
        |reference: &str, book: Option<&str>, chapter: Option<f64>, text: &str| ApiSearchHit {
            reference: reference.into(),
            text: text.into(),
            book_id: book.map(Into::into),
            chapter,
            verse: Some(1.0),
        };
    let api = Arc::new(StubApi {
        hits: vec![
            hit("JHN.3", Some("JHN"), Some(3.0), "one"),
            hit("  Psalms   23:1 ", None, Some(23.0), "two"),
            hit("x", Some("XYZ"), Some(1.0), "three"),
            hit("x", None, Some(1.0), "four"),
            hit("John 3:1", None, Some(0.0), "dropped"),
            hit("John 3:1", None, None, "dropped"),
            hit("John 3:1", None, Some(3.0), ""),
        ],
        authorized: ["NIV".to_string()].into(),
        ..Default::default()
    });
    service.attach_api_bible(Some(api));
    let results = block_on(service.search("remembered words", Some("NIV"))).unwrap();
    let summary: Vec<_> = results
        .iter()
        .map(|r| (r.reference.as_str(), r.verses[0].book.as_str()))
        .collect();
    assert_eq!(
        summary,
        [
            ("John 3:1", "John"),
            ("Psalms 23:1", "Psalms"),
            ("XYZ 1:1", "XYZ"),
            ("Unknown 1:1", "Unknown")
        ]
    );
}

#[test]
fn a_failing_api_phrase_search_falls_back_to_local_bibles() {
    let dir = tmp();
    let service = service_with_db(open_dest_with_kjv(dir.path()));
    // NIV is not authorized, so the API search fails and local text answers.
    service.attach_api_bible(Some(stub(Vec::new())));
    let results = block_on(service.search("stand-in", Some("NIV"))).unwrap();
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| r.translation == "KJV"));
}

#[test]
fn nkjv_is_available_without_a_key_once_installed() {
    let dir = tmp();
    let db = open_empty(dir.path());
    let service = service_with_db(db.clone());
    let nkjv = |options: Vec<kairo_domain::scripture::ScriptureTranslationOption>| {
        options.into_iter().find(|o| o.id == "NKJV").unwrap()
    };
    assert!(!nkjv(block_on(service.get_translation_options(false, false)).unwrap()).available);
    block_on(db.import_translation_pack(
        "NKJV",
        "Synthetic Fixture Translation",
        "en",
        fixture_verses(),
    ))
    .unwrap();
    let after = nkjv(block_on(service.get_translation_options(false, false)).unwrap());
    assert!(after.available);
    assert!(!after.requires_api_key);
    // The catalogue keeps NKJV API-capable so fallback keeps working.
    assert_eq!(after.access, TranslationAccess::Api);
    assert_eq!(after.downloadable, Some(true));
    assert_eq!(after.download_approx.as_deref(), Some("about 5 MB"));
}

#[test]
fn installed_pack_listing_covers_custom_packs_and_excludes_bundled_bibles() {
    let dir = tmp();
    let (service, db) = service_with_local_nkjv(dir.path());
    block_on(db.import_translation_pack("ZZT", "Custom Test Translation", "en", fixture_verses()))
        .unwrap();
    block_on(db.import_translation_pack("KJV", "King James Version", "en", fixture_verses()))
        .unwrap();

    let listed = block_on(service.list_installed_local_bible_packs()).unwrap();
    let mut ids: Vec<_> = listed
        .iter()
        .map(|p| p.status.translation.clone())
        .collect();
    ids.sort();
    assert_eq!(ids, ["NKJV", "ZZT"]);
    let custom = listed
        .iter()
        .find(|p| p.status.translation == "ZZT")
        .unwrap();
    assert_eq!(custom.name, "Custom Test Translation");
    assert!(custom.status.installed);
    assert_eq!(custom.status.verse_count, 31_102.0);

    let removed = block_on(service.remove_local_bible_translation("zzt")).unwrap();
    assert!(!removed.installed);
    let listed = block_on(service.list_installed_local_bible_packs()).unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|p| p.status.translation.as_str())
            .collect::<Vec<_>>(),
        ["NKJV"]
    );
    let error = block_on(service.remove_local_bible_translation("KJV")).unwrap_err();
    assert!(error.to_string().contains("cannot be removed"));
}

#[test]
fn is_local_translation_reflects_the_installed_nkjv() {
    let dir = tmp();
    let db = open_empty(dir.path());
    let service = service_with_db(db.clone());
    assert!(!block_on(service.is_local_translation("NKJV")).unwrap());
    block_on(db.import_translation_pack("NKJV", "Fixture", "en", fixture_verses())).unwrap();
    assert!(block_on(service.is_local_translation("nkjv")).unwrap());
    assert!(!block_on(service.is_local_translation("NIV")).unwrap());
}

// ─── ampc-pack.test.ts ──────────────────────────────────────────────────────

#[test]
fn ampc_pack_installs_searches_and_serves_ranges_without_renumbering_or_changing_kjv() {
    let dir = tmp();
    let db = open_empty(dir.path());
    block_on(db.import_translation(
        "KJV",
        "KJV",
        "en",
        vec![PackVerse {
            book_id: 43,
            chapter: 3,
            verse: 16,
            text: "Synthetic KJV retained.".into(),
        }],
        true,
    ))
    .unwrap();
    let pack = write_pack(
        &dir.path().join("ampc.db"),
        "AMPC",
        "Synthetic AMPC",
        &ampc_verses(),
    );
    let service = service_with_db(db.clone());
    assert!(
        block_on(service.install_local_bible_pack(&pack, Some("AMPC")))
            .unwrap()
            .verse_count
            == 31_101.0
    );
    assert!(
        block_on(service.get_local_bible_pack_status("AMPC"))
            .unwrap()
            .installed
    );
    let options = block_on(service.get_translation_options(false, false)).unwrap();
    let option = options.iter().find(|o| o.id == "AMPC").unwrap();
    assert!(option.available && !option.requires_api_key);

    let range =
        block_on(service.lookup_verses("Mark", 9.0, 43.0, Some(48.0), Some("AMPC"))).unwrap();
    assert_eq!(
        range.iter().map(|v| v.verse).collect::<Vec<_>>(),
        [43.0, 45.0, 47.0, 48.0]
    );
    assert!(
        block_on(service.lookup_verses("Mark", 9.0, 44.0, None, Some("AMPC")))
            .unwrap()
            .is_empty()
    );
    let result = block_on(service.search("3 John 1:15", Some("AMPC"))).unwrap();
    assert_eq!(result[0].translation, "AMPC");
    assert!(texts(&result[0].verses)[0].contains("final greeting"));
    assert!(
        !block_on(service.search("zephyrquill", Some("AMPC")))
            .unwrap()
            .is_empty()
    );
    assert_eq!(block_on(db.get_verse_count("KJV")).unwrap(), 1);
    block_on(service.remove_local_bible_translation("AMPC")).unwrap();
    assert_eq!(block_on(db.get_verse_count("AMPC")).unwrap(), 0);
    assert_eq!(block_on(db.get_verse_count("KJV")).unwrap(), 1);
}

#[test]
fn ampc_numbering_is_rejected_for_other_translations() {
    let dir = tmp();
    let service = service_with_db(open_empty(dir.path()));
    let pack = write_pack(
        &dir.path().join("ampc-as-nkjv.db"),
        "NKJV",
        "Fixture",
        &ampc_verses(),
    );
    assert!(block_on(service.install_local_bible_pack(&pack, None)).is_err());
    let mut moved = ampc_verses();
    let at = moved
        .iter()
        .position(|r| (r.book_id, r.chapter, r.verse) == (41, 9, 45))
        .unwrap();
    moved[at].verse = 44;
    let pack = write_pack(&dir.path().join("moved.db"), "AMPC", "Fixture", &moved);
    assert!(block_on(service.install_local_bible_pack(&pack, None)).is_err());
}
