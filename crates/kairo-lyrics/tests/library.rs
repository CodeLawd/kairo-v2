use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    time::Instant,
};

use futures_lite::future::block_on;
use kairo_domain::{
    lyrics::{LyricsImportSource, LyricsSectionType, LyricsSong, LyricsSongSection, LyricsSource},
    workspace::{serialize_song_file, song_file_name},
};
use kairo_lyrics::{LyricsService, parse_open_lyrics, parse_section_label};
use kairo_store::settings::{LaunchContext, SettingsStore, random_uuid};
use serde_json::json;

fn settings(profile: &Path) -> Arc<Mutex<SettingsStore>> {
    Arc::new(Mutex::new(SettingsStore::open(
        profile,
        &mut LaunchContext {
            now_ms: 1_700_000_000_000.0,
            new_id: &mut random_uuid,
        },
    )))
}

fn usr(content: &str, filename: Option<&str>) -> LyricsImportSource {
    LyricsImportSource::Usr {
        content: content.to_owned(),
        filename: filename.map(str::to_owned),
    }
}

fn fixture(index: usize) -> LyricsSong {
    let adjective = [
        "Blessed",
        "Éternel",
        "Ánimo",
        "Great",
        "Ìyìn",
        "Adoración",
        "Amazing",
        "Mighty",
        "Santo",
        "Blessed",
    ][index % 10];
    let phrase = [
        "grace", "morning", "love", "river", "sing", "faith", "praise",
    ][index / 10 % 7];
    LyricsSong {
        id: format!("fixture-{index:04}"),
        title: format!("{adjective} {phrase} {index:04}"),
        artist: format!("Choir {}", index / 10 % 143),
        copyright: None,
        ccli_number: None,
        is_favorite: Some(false),
        source: Some(LyricsSource::Text),
        sections: vec![LyricsSongSection {
            kind: LyricsSectionType::Verse,
            label: "Verse 1".into(),
            lines: vec![
                format!("We gather for {phrase} and lift our voices"),
                format!("Evening number {index}"),
            ],
            line_colors: None,
            hotkey: None,
            extra: Default::default(),
        }],
        created_at: 1_700_000_000_000.0,
        updated_at: 1_700_000_000_000.0,
        extra: Default::default(),
    }
}

#[test]
fn reconciles_folder_and_preserves_identity_across_edits() {
    block_on(async {
        let profile = tempfile::tempdir().unwrap();
        let dir = profile.path().join("Songs");
        let service = LyricsService::open(
            profile.path(),
            profile.path().to_owned(),
            settings(profile.path()),
        )
        .await
        .unwrap();
        let song = service.import_song(usr(
            "\u{feff}Title: Grace\r\nAuthor= Choir\r\nCCLI#=12-345\r\n\r\n[V1]\r\nSome words\r\n[Chorus]\r\nMore words",
            Some("Old_Title.usr"),
        )).await.unwrap();
        assert_eq!(song.title, "Grace");
        assert_eq!(song.ccli_number.as_deref(), Some("12345"));
        assert_eq!(song.sections[1].kind, LyricsSectionType::Chorus);
        let duplicate = service
            .import_song(usr("CCLI#=12345\nOther song", None))
            .await
            .unwrap();
        assert_eq!(song.id, duplicate.id);
        let preview = service
            .preview_import(usr("CCLI#=12345\nPreview", None))
            .await
            .unwrap();
        assert!(preview.duplicate.is_some());
        assert_eq!(service.get_library().await.unwrap().len(), 1);
        assert!(service.toggle_favorite(&song.id).await.unwrap());

        let mut edit = song.clone();
        edit.title = "New name".into();
        edit.is_favorite = Some(false);
        edit.source = Some(LyricsSource::Web);
        let saved = service.update_song(&song.id, edit).await.unwrap().unwrap();
        assert_eq!(saved.is_favorite, Some(true));
        assert_eq!(saved.source, Some(LyricsSource::Usr));
        assert_eq!(saved.created_at, song.created_at);
        assert!(!dir.join(song_file_name(&song)).exists());
        assert!(dir.join(song_file_name(&saved)).exists());
        assert_eq!(service.search("new name").await.unwrap()[0].id, song.id);
        assert!(service.search("grace").await.unwrap().is_empty());

        let dropped = fixture(1);
        std::fs::write(
            dir.join("renamed.song.json"),
            serialize_song_file(&dropped).unwrap(),
        )
        .unwrap();
        std::fs::remove_file(dir.join(song_file_name(&saved))).unwrap();
        let summary = service.sync_from_folder().await.unwrap();
        assert_eq!(
            (summary.imported, summary.removed, summary.exported),
            (1.0, 1.0, 0.0)
        );
        assert!(service.get_song(&saved.id).await.unwrap().is_none());
        assert!(service.delete_song(&dropped.id).await.unwrap());
        assert!(!dir.join("renamed.song.json").exists());
        assert_eq!(service.sync_from_folder().await.unwrap().imported, 0.0);
        assert!(!service.delete_song(&dropped.id).await.unwrap());
        assert!(!service.toggle_favorite(&dropped.id).await.unwrap());
    });
}

#[test]
fn previews_title_fallback_and_openlyrics_order() {
    block_on(async {
        let profile = tempfile::tempdir().unwrap();
        let service = LyricsService::open(
            profile.path(),
            profile.path().to_owned(),
            settings(profile.path()),
        )
        .await
        .unwrap();
        let draft = service
            .preview_import(usr(
                "Chorus: sing it loud\nAgain and again",
                Some("Praise__Song.txt"),
            ))
            .await
            .unwrap();
        assert_eq!(draft.song.title, "Praise Song");
        assert_eq!(draft.song.sections[0].lines[0], "Chorus: sing it loud");
        let metadata = service
            .preview_import(usr("\n\u{feff}Title=Grace\n[V1]\nWords", None))
            .await
            .unwrap();
        assert_eq!(metadata.song.title, "Grace");
        assert_eq!(metadata.song.sections[0].lines, ["Words"]);
        assert!(service.get_library().await.unwrap().is_empty());
        assert_eq!(parse_section_label("END."), None);
        assert_eq!(
            parse_section_label("{E2}"),
            Some((LyricsSectionType::Ending, 2))
        );
        let xml = r#"<song xmlns="http://openlyrics.info/namespace/2009/song"><properties>
          <titles><title>Example &amp; Praise</title></titles><authors><author>Choir</author></authors>
          <verseOrder>v1 c1 v1</verseOrder></properties><lyrics>
          <verse name="v1"><lines>First<br/>Second</lines></verse>
          <verse name="c1"><lines>Chorus line</lines></verse></lyrics></song>"#;
        let imported = service.import_open_lyrics(xml).await.unwrap();
        assert_eq!(imported.title, "Example & Praise");
        assert_eq!(imported.sections.len(), 3);
        assert_eq!(imported.sections[0].lines, ["First", "Second"]);
        assert_eq!(imported.sections[0], imported.sections[2]);
        assert!(parse_open_lyrics("<song>", 0.0).is_err());
        assert!(parse_open_lyrics("<other/>", 0.0).is_err());
    });
}

#[test]
#[ignore = "requires Electron checkout, installed dependencies and matching SQLite ABI"]
fn search_matches_electron_for_thousand_queries() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let electron = std::env::var_os("KAIRO_ELECTRON_DIR")
        .map_or_else(|| root.join("../kairo-presenter"), std::path::PathBuf::from)
        .canonicalize()
        .unwrap();
    let native = tempfile::tempdir().unwrap();
    let oracle = tempfile::tempdir().unwrap();
    let folder = native.path().join("Songs");
    std::fs::create_dir(&folder).unwrap();
    for index in 0..5_000 {
        let song = fixture(index);
        std::fs::write(
            folder.join(song_file_name(&song)),
            serialize_song_file(&song).unwrap(),
        )
        .unwrap();
    }
    let queries: Vec<_> = (0..1_000)
        .map(|i| match i % 12 {
            0 => format!("{:04}", i * 43 % 5_000),
            1 => format!("Choir {}", i % 143),
            2 => format!("Evening number {}", i * 19 % 5_000),
            3 => format!("Éternel {:04}", i * 7 % 5_000),
            4 => format!("lift our voices {}", i % 143),
            5 => format!("blessed OR {}", i % 21),
            6 => format!("choir NOT {}", i % 143),
            7 => format!("Great -{:04}", i * 13 % 5_000),
            8 => format!("voice* {}", i % 49),
            9 => format!("Choir _{}", i % 10),
            10 => format!("{} NEAR {}", i % 31, i % 27),
            _ => format!("river {:04}", i * 11 % 5_000),
        })
        .collect();
    let stubs = root.join("tools/ts-oracle/modules/electron-launch");
    let mut child = Command::new(electron.join("node_modules/.bin/electron"))
        .arg("--require")
        .arg(stubs.join("preload.cjs"))
        .arg("--import")
        .arg(stubs.join("register.mjs"))
        .args(["--import", "tsx"])
        .arg(root.join("tools/ts-oracle/modules/lyrics-library/child.mts"))
        .current_dir(electron)
        .env("ELECTRON_RUN_AS_NODE", "1")
        .env("KAIRO_STUB_USER_DATA", oracle.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&json!({"folder": folder, "queries": queries})).unwrap())
        .unwrap();

    let start = Instant::now();
    let service = block_on(LyricsService::open(
        native.path(),
        native.path().to_owned(),
        settings(native.path()),
    ))
    .unwrap();
    let open = start.elapsed();
    let start = Instant::now();
    assert_eq!(block_on(service.get_library()).unwrap().len(), 5_000);
    eprintln!(
        "5,000-song index open {open:?}, first library read {:?}",
        start.elapsed()
    );

    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected: Vec<Vec<String>> = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|err| panic!("{err}: {}", String::from_utf8_lossy(&output.stdout)));
    assert_eq!(expected.len(), queries.len());
    let mut samples = Vec::new();
    for (query, expected) in queries.iter().zip(expected) {
        let start = Instant::now();
        let actual = block_on(service.search(query)).unwrap();
        samples.push(start.elapsed());
        assert_eq!(
            actual.into_iter().map(|song| song.id).collect::<Vec<_>>(),
            expected,
            "{query}"
        );
    }
    samples.sort_unstable();
    eprintln!(
        "search p50 {:?}, p95 {:?}, p99 {:?}, max {:?}",
        samples[499], samples[949], samples[989], samples[999]
    );
}

#[test]
fn exports_legacy_rows_once_and_tracks_the_configured_workspace() {
    use kairo_store::{
        db::{Database, Schema},
        settings::MigrationFlag,
    };

    block_on(async {
        let profile = tempfile::tempdir().unwrap();
        let old = profile.path().join("old");
        let dir = old.join("Songs");
        let config = settings(profile.path());
        let db = Database::open(&profile.path().join("lyrics.db"), Schema::Lyrics)
            .await
            .unwrap();
        db.write(|db| {
            db.execute(
                "INSERT INTO songs(id,title,created_at,updated_at) VALUES('legacy','Legacy',1,1)",
                [],
            )?;
            Ok(())
        })
        .await
        .unwrap();
        drop(db);
        let service = LyricsService::open(profile.path(), old, config.clone())
            .await
            .unwrap();
        let song = service.get_song("legacy").await.unwrap().unwrap();
        assert!(dir.join(song_file_name(&song)).exists());
        assert!(
            config
                .lock()
                .unwrap()
                .migration_done(MigrationFlag::SongsFolderExport)
        );

        let new = profile.path().join("new");
        let new_songs = new.join("Songs");
        std::fs::create_dir_all(&new_songs).unwrap();
        std::fs::rename(
            dir.join(song_file_name(&song)),
            new_songs.join(song_file_name(&song)),
        )
        .unwrap();
        config
            .lock()
            .unwrap()
            .merge_fields("workspace", [("folder", json!(new))]);
        let summary = service.sync_from_folder().await.unwrap();
        assert_eq!(
            (summary.imported, summary.removed, summary.exported),
            (1.0, 0.0, 0.0)
        );
        assert!(service.toggle_favorite("legacy").await.unwrap());
        assert!(new_songs.join(song_file_name(&song)).exists());
        std::fs::remove_file(new_songs.join(song_file_name(&song))).unwrap();
        assert_eq!(service.sync_from_folder().await.unwrap().removed, 1.0);
    });
}

#[test]
fn failed_legacy_export_retries_without_deleting_the_library() {
    use kairo_store::{
        db::{Database, Schema},
        settings::MigrationFlag,
    };

    block_on(async {
        let profile = tempfile::tempdir().unwrap();
        let config = settings(profile.path());
        let folder = profile.path().join("Songs");
        std::fs::write(&folder, "blocked folder").unwrap();
        let db = Database::open(&profile.path().join("lyrics.db"), Schema::Lyrics)
            .await
            .unwrap();
        db.write(|db| {
            db.execute(
                "INSERT INTO songs(id,title,created_at,updated_at) VALUES('legacy','Legacy',1,1)",
                [],
            )?;
            Ok(())
        })
        .await
        .unwrap();
        drop(db);
        let service =
            LyricsService::open(profile.path(), profile.path().to_owned(), config.clone())
                .await
                .unwrap();
        assert!(service.get_song("legacy").await.unwrap().is_some());
        assert!(
            !config
                .lock()
                .unwrap()
                .migration_done(MigrationFlag::SongsFolderExport)
        );
        std::fs::remove_file(&folder).unwrap();
        let summary = service.sync_from_folder().await.unwrap();
        assert_eq!(summary.exported, 1.0);
        assert_eq!(summary.removed, 0.0);
        assert!(
            config
                .lock()
                .unwrap()
                .migration_done(MigrationFlag::SongsFolderExport)
        );
        assert_eq!(service.sync_from_folder().await.unwrap().imported, 1.0);
    });
}

#[test]
fn propresenter_import_preserves_source_ccli_and_slide_breaks() {
    block_on(async {
        let dir = tempfile::tempdir().unwrap();
        let service = LyricsService::open(dir.path(), dir.path().to_owned(), settings(dir.path()))
            .await
            .unwrap();
        let source = LyricsImportSource::ProPresenter {
            title: "Alpha Omega".into(),
            artist: "Author".into(),
            text: "[Verse 1]\nAlpha Omega\n\nIwo ni mo f\u{2019}iyin fun o Baba\n\n[Chorus]\nHallelujah".into(),
            copyright: Some("© 2024 Publisher".into()),
            ccli_number: Some("7115744".into()),
        };
        let preview = service.preview_import(source.clone()).await.unwrap();
        assert!(preview.duplicate.is_none());
        let saved = service.import_song(source).await.unwrap();
        assert_eq!(saved.source, Some(LyricsSource::ProPresenter));
        assert_eq!(saved.ccli_number.as_deref(), Some("7115744"));
        assert_eq!(saved.sections.len(), 2);
        assert_eq!(saved.sections[0].kind, LyricsSectionType::Verse);
        assert_eq!(
            saved.sections[0].lines,
            vec!["Alpha Omega", "", "Iwo ni mo f\u{2019}iyin fun o Baba"]
        );
        assert_eq!(saved.sections[1].kind, LyricsSectionType::Chorus);
    });
}
