//! Prompt 044: setlist, libraries, passages, song usage and `.kairo`
//! transfer. Bundles are round-tripped with Electron's real
//! `src/lib/kairo-bundle.ts` (`tools/ts-oracle/modules/kairo-bundle-roundtrip.ts`)
//! in both directions.

use kairo_domain::collections::{LibrariesCommand, LibraryKind, SetlistCommand};
use kairo_domain::json::to_js_value;
use kairo_domain::kairo_bundle::{
    KAIRO_BUNDLE_MAX_BYTES, PlaylistImportChoice, SongImportChoice, TransferCommitRequest,
    TransferExportRequest, parse_kairo_bundle, serialize_bundle,
};
use kairo_domain::lyrics::LyricsSong;
use kairo_domain::scripture::{PassagesCommand, ScriptureResult, SermonPlan};
use kairo_lyrics::LyricsService;
use kairo_orchestrator::live_plan::{LivePlanService, SermonPlanStore};
use kairo_store::settings::{LaunchContext, SettingsStore, random_uuid};
use kairo_transfer::{
    Clock, CommitProgress, FileDialogs, KairoOpenQueue, KairoOpened, LibrariesService,
    PassagesService, SetlistService, SongUsageService, TransferService, delete_song,
    kairo_paths_in, remove_document_from_libraries, song_usage_file_name,
};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

const NOW: f64 = 1_700_000_000_000.0;
const VERSION: &str = "1.0.40";

fn clock() -> Clock {
    Arc::new(|| NOW)
}

fn song(i: usize) -> LyricsSong {
    serde_json::from_value(json!({
        "id": format!("song-{i:03}"),
        "title": format!("Song number {i} é"),
        "artist": if i.is_multiple_of(3) { String::new() } else { format!("Artist {}", i % 7) },
        "copyright": format!("© 20{:02} Public", i % 30),
        "ccliNumber": format!("{}", 100000 + i),
        "sections": [
            {"type": "verse", "label": "Verse 1", "lines": [format!("Line one of song {i}"), "Grace upon grace".to_string()]},
            {"type": "chorus", "label": "Chorus", "lines": [format!("Chorus words {i}"), "Sing it again".to_string()], "lineColors": [null, "#ff0000"]},
        ],
        "source": "text",
        "isFavorite": i.is_multiple_of(4),
        "createdAt": 1_600_000_000_000.0 + i as f64,
        "updatedAt": 1_600_000_000_500.0 + i as f64,
    }))
    .unwrap()
}

fn plan() -> SermonPlan {
    serde_json::from_value(json!({
        "id": "plan-1",
        "title": "Romans series",
        "sourceFileName": "romans.docx",
        "sourceText": "Romans 8:28",
        "createdAt": 1_650_000_000_000.0,
        "updatedAt": 1_650_000_000_100.0,
        "items": [
            {"id": "i1", "reference": "Romans 8:28", "translation": "KJV", "available": true,
             "verses": [{"book": "Romans", "chapter": 8, "verse": 28, "text": "And we know that all things work together for good", "translation": "KJV"}]},
            {"id": "i2", "reference": "Romans 9:1", "translation": "NIV", "available": false, "error": "Not downloaded", "verses": []},
        ]
    }))
    .unwrap()
}

struct App {
    _dir: tempfile::TempDir,
    root: PathBuf,
    lyrics: Arc<LyricsService>,
    setlist: SetlistService,
    libraries: LibrariesService,
    plans: LivePlanService,
    transfer: TransferService,
}

fn settings(profile: &Path) -> Arc<Mutex<SettingsStore>> {
    Arc::new(Mutex::new(SettingsStore::open(
        profile,
        &mut LaunchContext {
            now_ms: NOW,
            new_id: &mut random_uuid,
        },
    )))
}

async fn app_with(songs: &[LyricsSong], plans: &[SermonPlan]) -> App {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_owned();
    if !plans.is_empty() {
        std::fs::write(
            root.join("kairo-sermon-plans.json"),
            serde_json::to_string(&json!({"plans": plans, "livePlanId": null})).unwrap(),
        )
        .unwrap();
    }
    let lyrics = Arc::new(
        LyricsService::open(&root, root.join("workspace"), settings(&root))
            .await
            .unwrap(),
    );
    for s in songs {
        lyrics.add_song(s.clone()).await.unwrap();
    }
    let setlist = SetlistService::open(&root, clock());
    let libraries = LibrariesService::open(&root, clock());
    let plans = LivePlanService::new(Box::new(SermonPlanStore::open(&root)));
    let transfer = TransferService::new(
        lyrics.clone(),
        setlist.clone(),
        plans.clone(),
        VERSION,
        clock(),
    );
    App {
        _dir: dir,
        root,
        lyrics,
        setlist,
        libraries,
        plans,
        transfer,
    }
}

struct Dialogs {
    save: Option<PathBuf>,
    open: Vec<PathBuf>,
    asked: Mutex<Vec<String>>,
}

impl FileDialogs for Dialogs {
    fn save_kairo<'a>(
        &'a self,
        default_name: &'a str,
    ) -> Pin<Box<dyn Future<Output = Option<PathBuf>> + Send + 'a>> {
        self.asked.lock().unwrap().push(default_name.to_owned());
        Box::pin(async move { self.save.clone() })
    }
    fn open_kairo(&self) -> Pin<Box<dyn Future<Output = Vec<PathBuf>> + Send + '_>> {
        Box::pin(async move { self.open.clone() })
    }
}

fn dialogs(save: Option<PathBuf>, open: Vec<PathBuf>) -> Dialogs {
    Dialogs {
        save,
        open,
        asked: Mutex::new(Vec::new()),
    }
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
}

// ─── Round trips with Electron ───────────────────────────────────────────────

#[test]
fn rust_exports_read_identically_in_electron() {
    rt().block_on(async {
        let songs: Vec<_> = (0..12).map(song).collect();
        let app = app_with(&songs, &[plan()]).await;
        let list = app
            .setlist
            .apply(SetlistCommand::Create {
                name: "Sunday AM".into(),
            })
            .unwrap()
            .active_id
            .unwrap();
        for id in ["song-004", "song-001", "song-009", "song-001"] {
            app.setlist
                .apply(SetlistCommand::Add {
                    song_id: id.into(),
                    index: None,
                    list_id: Some(list.clone()),
                })
                .unwrap();
        }
        let requests = [
            TransferExportRequest::Library,
            TransferExportRequest::Songs {
                song_ids: vec!["song-002".into(), "missing".into(), "song-007".into()],
                name: Some("  ".into()),
            },
            TransferExportRequest::Songs {
                song_ids: vec!["song-005".into()],
                name: None,
            },
            TransferExportRequest::Setlist { list_id: list },
            TransferExportRequest::Playlist {
                plan_id: "plan-1".into(),
            },
        ];
        for request in requests {
            let target = app.root.join("export");
            let d = dialogs(Some(target.clone()), vec![]);
            let result = app.transfer.export(request.clone(), &d).await.unwrap();
            assert!(result.saved);
            let written = app.root.join(result.file_name.clone().unwrap());
            assert_eq!(
                written,
                app.root.join("export.kairo"),
                "{request:?}: .kairo added"
            );
            let text = std::fs::read_to_string(&written).unwrap();

            // Electron reads it back to the same bundle Rust reads...
            let electron = kairo_oracle::oracle_value(
                "oracle:kairo-bundle-roundtrip.parse",
                json!([text, NOW]),
            );
            let rust = to_js_value(&parse_kairo_bundle(&text, NOW).unwrap()).unwrap();
            let diff = kairo_oracle::compare(&electron["bundle"], &rust);
            assert!(diff.is_empty(), "{request:?}: {diff:?}");
            // ...and exporting what it read gives the file byte for byte.
            let again = kairo_oracle::oracle_value(
                "oracle:kairo-bundle-roundtrip.reexport",
                json!([text, NOW]),
            );
            // Reading a playlist drops fields the reader does not know
            // (`sourceText`, a verse's `translation`), in Electron too.
            let rust_again = serialize_bundle(&parse_kairo_bundle(&text, NOW).unwrap()).unwrap();
            assert_eq!(again.as_str().unwrap(), rust_again, "{request:?}");
            if !matches!(request, TransferExportRequest::Playlist { .. }) {
                assert_eq!(rust_again, text, "{request:?}: re-export is the file");
            }
            std::fs::remove_file(written).unwrap();
        }
    });
}

#[test]
fn electron_exports_import_identically_in_rust() {
    let songs: Vec<Value> = (0..8).map(|i| to_js_value(&song(i)).unwrap()).collect();
    let plan = to_js_value(&plan()).unwrap();
    for (kind, name) in [
        ("songs", "Eight songs"),
        ("setlist", "Evening"),
        ("scripture-playlist", ""),
    ] {
        let text = kairo_oracle::oracle_value(
            "oracle:kairo-bundle-roundtrip.exportText",
            json!([kind, name, songs, plan, VERSION, NOW]),
        );
        let text = text.as_str().unwrap();
        let bundle = parse_kairo_bundle(text, NOW).unwrap();
        let electron_again = kairo_oracle::oracle_value(
            "oracle:kairo-bundle-roundtrip.reexport",
            json!([text, NOW]),
        );
        assert_eq!(
            serialize_bundle(&bundle).unwrap(),
            electron_again.as_str().unwrap(),
            "{kind}: Rust re-export differs from Electron's"
        );
        let electron =
            kairo_oracle::oracle_value("oracle:kairo-bundle-roundtrip.parse", json!([text, NOW]));
        assert!(
            kairo_oracle::compare(&electron["bundle"], &to_js_value(&bundle).unwrap()).is_empty(),
            "{kind}"
        );
    }
}

#[test]
fn electron_file_imports_through_the_service() {
    rt().block_on(async {
        let songs: Vec<Value> = (0..6).map(|i| to_js_value(&song(i)).unwrap()).collect();
        let text = kairo_oracle::oracle_value(
            "oracle:kairo-bundle-roundtrip.exportText",
            json!(["setlist", "Evening", songs, Value::Null, VERSION, NOW]),
        );
        // The library already has song 0 (same id) and a renamed copy of song 1's lyrics.
        let mut copy = song(1);
        copy.id = "local-copy".into();
        let app = app_with(&[song(0), copy], &[]).await;
        app.setlist
            .apply(SetlistCommand::Create {
                name: "Evening".into(),
            })
            .unwrap();
        let file = app.root.join("Evening.kairo");
        std::fs::write(&file, text.as_str().unwrap()).unwrap();

        let preview = app.transfer.preview_file(&file).await.unwrap();
        assert_eq!(preview.file_name, "Evening.kairo");
        assert_eq!(preview.setlist.as_ref().unwrap().song_count, 6);
        let reasons: Vec<_> = preview
            .songs
            .iter()
            .map(|s| {
                s.duplicate
                    .as_ref()
                    .map(|d| (d.song_id.clone(), to_js_value(&d.reason).unwrap()))
            })
            .collect();
        assert_eq!(reasons[0], Some(("song-000".into(), json!("same-song"))));
        assert_eq!(
            reasons[1].as_ref().map(|r| r.0.as_str()),
            Some("local-copy")
        );
        assert!(reasons[2..].iter().all(Option::is_none));

        let mut choices = std::collections::BTreeMap::new();
        choices.insert("song-000".into(), SongImportChoice::Replace);
        choices.insert("song-001".into(), SongImportChoice::Skip);
        choices.insert("song-002".into(), SongImportChoice::KeepBoth);
        choices.insert("song-003".into(), SongImportChoice::Add);
        // song-004 and song-005 have no choice: skipped, and not in the library.
        let mut seen = Vec::new();
        let result = app
            .transfer
            .commit(
                TransferCommitRequest {
                    token: preview.token.clone(),
                    songs: choices,
                    create_setlist: true,
                    playlist: None,
                },
                |p| seen.push(p),
            )
            .await
            .unwrap();
        assert_eq!((result.added, result.replaced, result.skipped), (2, 1, 3));
        assert_eq!(result.setlist_name.as_deref(), Some("Evening (2)"));
        assert_eq!(seen.last(), Some(&CommitProgress { done: 6, total: 6 }));
        let lists = app.setlist.snapshot().lists;
        assert_eq!(lists[0].name, "Evening (2)");
        assert_eq!(
            lists[0].song_ids,
            ["song-000", "local-copy", "song-002", "song-003"]
        );
        // Same token again: expired.
        let again = app
            .transfer
            .commit(
                TransferCommitRequest {
                    token: preview.token,
                    songs: Default::default(),
                    create_setlist: false,
                    playlist: None,
                },
                |_| {},
            )
            .await
            .unwrap_err();
        assert_eq!(
            again.to_string(),
            "This import has expired. Open the file again."
        );
    });
}

// ─── Transfer edge cases ─────────────────────────────────────────────────────

#[test]
fn import_errors_caps_and_batches() {
    rt().block_on(async {
        let app = app_with(&[], &[plan()]).await;
        let root = &app.root;
        let write = |name: &str, body: &str| {
            let p = root.join(name);
            std::fs::write(&p, body).unwrap();
            p
        };
        let one = |path: PathBuf| {
            let t = app.transfer.clone();
            async move { t.preview_file(&path).await.unwrap_err().to_string() }
        };
        assert_eq!(
            one(write("bad.kairo", "{nope")).await,
            "This file is not a Kairo export — it could not be read."
        );
        assert_eq!(
            one(write("foreign.json", "{\"a\":1}")).await,
            "This file is not a Kairo export."
        );
        assert_eq!(
            one(write(
                "future.kairo",
                "{\"kairoBundle\":2,\"kind\":\"songs\"}"
            ))
            .await,
            "This file was made by a newer version of Kairo. Update Kairo to open it."
        );
        assert_eq!(
            one(write(
                "empty.kairo",
                "{\"kairoBundle\":1,\"kind\":\"songs\",\"songs\":[]}"
            ))
            .await,
            "This Kairo file has no songs in it."
        );
        assert!(one(root.join("missing.kairo")).await.starts_with("ENOENT"));

        // Over 50 MB is refused before reading.
        let big = root.join("big.kairo");
        std::fs::File::create(&big)
            .unwrap()
            .set_len(KAIRO_BUNDLE_MAX_BYTES as u64 + 1)
            .unwrap();
        assert_eq!(
            one(big.clone()).await,
            "This file is too large to be a Kairo export."
        );
        let exact = root.join("exact.kairo");
        std::fs::File::create(&exact)
            .unwrap()
            .set_len(KAIRO_BUNDLE_MAX_BYTES as u64)
            .unwrap();
        assert_ne!(
            one(exact).await,
            "This file is too large to be a Kairo export."
        );

        // A batch skips what it cannot read, merges songs and keeps the first duplicate id.
        let a = write(
            "a.kairo",
            &serialize_bundle(&kairo_domain::kairo_bundle::songs_bundle(
                "A".into(),
                vec![song(1), song(2)],
                None,
                NOW,
            ))
            .unwrap(),
        );
        let b = write(
            "b.kairo",
            &serialize_bundle(&kairo_domain::kairo_bundle::songs_bundle(
                "B".into(),
                vec![song(2), song(3)],
                None,
                NOW,
            ))
            .unwrap(),
        );
        let preview = app
            .transfer
            .preview_files(&[a.clone(), big.clone(), b.clone()])
            .await
            .unwrap();
        assert_eq!(preview.file_name, "3 files");
        assert_eq!(
            preview
                .songs
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>(),
            ["song-001", "song-002", "song-003"]
        );
        let skipped = preview.skipped_files.unwrap();
        assert_eq!(skipped[0].file_name, "big.kairo");
        let none = app
            .transfer
            .preview_files(&[big.clone(), root.join("bad.kairo")])
            .await
            .unwrap_err();
        assert_eq!(
            none.to_string(),
            "None of the 2 files could be read. This file is too large to be a Kairo export."
        );

        // A playlist can't be batched with other files.
        let p = write(
            "plan.kairo",
            &serialize_bundle(&kairo_domain::kairo_bundle::playlist_bundle(
                plan(),
                None,
                NOW,
            ))
            .unwrap(),
        );
        let mixed = app
            .transfer
            .preview_files(&[a, p.clone()])
            .await
            .unwrap_err();
        assert_eq!(
            mixed.to_string(),
            "Song and scripture playlists import one file at a time. Select just that file."
        );

        // Playlist: exists, then keep-both makes a renamed copy; replace overwrites.
        let preview = app.transfer.preview_file(&p).await.unwrap();
        assert!(preview.playlist.as_ref().unwrap().exists);
        let kept = app
            .transfer
            .commit(
                TransferCommitRequest {
                    token: preview.token,
                    songs: Default::default(),
                    create_setlist: false,
                    playlist: Some(PlaylistImportChoice::KeepBoth),
                },
                |_| {},
            )
            .await
            .unwrap();
        assert_eq!(
            kept.playlist_title.as_deref(),
            Some("Romans series (imported)")
        );
        assert_eq!(app.plans.list_plans().len(), 2);
        let preview = app.transfer.preview_file(&p).await.unwrap();
        app.transfer
            .commit(
                TransferCommitRequest {
                    token: preview.token,
                    songs: Default::default(),
                    create_setlist: false,
                    playlist: Some(PlaylistImportChoice::Replace),
                },
                |_| {},
            )
            .await
            .unwrap();
        assert_eq!(app.plans.list_plans().len(), 2);
        let preview = app.transfer.preview_file(&p).await.unwrap();
        app.transfer.discard(&preview.token);
        assert!(
            app.transfer
                .commit(
                    TransferCommitRequest {
                        token: preview.token,
                        songs: Default::default(),
                        create_setlist: false,
                        playlist: None
                    },
                    |_| {}
                )
                .await
                .is_err()
        );

        // Cancelled pickers.
        assert!(
            app.transfer
                .pick_and_preview(&dialogs(None, vec![]))
                .await
                .unwrap()
                .is_none()
        );
        let saved = app
            .transfer
            .export(
                TransferExportRequest::Playlist {
                    plan_id: "plan-1".into(),
                },
                &dialogs(None, vec![]),
            )
            .await
            .unwrap();
        assert!(!saved.saved && saved.file_name.is_none());
    });
}

#[test]
fn export_errors_come_before_the_dialog() {
    rt().block_on(async {
        let app = app_with(&[], &[]).await;
        let d = dialogs(Some(app.root.join("x")), vec![]);
        for (request, message) in [
            (TransferExportRequest::Library, "The song library is empty."),
            (
                TransferExportRequest::Songs {
                    song_ids: vec!["nope".into()],
                    name: None,
                },
                "Choose at least one song to export.",
            ),
            (
                TransferExportRequest::Setlist {
                    list_id: "nope".into(),
                },
                "That playlist no longer exists.",
            ),
            (
                TransferExportRequest::Playlist {
                    plan_id: "nope".into(),
                },
                "That playlist no longer exists.",
            ),
        ] {
            assert_eq!(
                app.transfer
                    .export(request, &d)
                    .await
                    .unwrap_err()
                    .to_string(),
                message
            );
        }
        assert!(d.asked.lock().unwrap().is_empty());
    });
}

#[test]
fn opened_files_queue_until_ready_and_bad_files_name_themselves() {
    rt().block_on(async {
        let app = app_with(&[], &[]).await;
        let good = app.root.join("Good Songs.KAIRO");
        std::fs::write(&good, serialize_bundle(&kairo_domain::kairo_bundle::songs_bundle("Good".into(), vec![song(1)], None, NOW)).unwrap()).unwrap();
        let bad = app.root.join("broken.kairo");
        std::fs::write(&bad, "not json").unwrap();
        let args = vec!["kairo".to_owned(), good.display().to_string(), bad.display().to_string(), app.root.join("gone.kairo").display().to_string(), "x.txt".into()];
        let paths = kairo_paths_in(&args);
        assert_eq!(paths, [good.clone(), bad.clone()]);

        let queue = KairoOpenQueue::new(app.transfer.clone());
        queue.open(paths).await;
        assert_eq!(queue.pending(), 2);
        let got = Arc::new(Mutex::new(Vec::new()));
        let sink = got.clone();
        queue.ready(move |m| sink.lock().unwrap().push(m)).await;
        let got = got.lock().unwrap().clone();
        assert_eq!(got.len(), 2);
        assert!(matches!(&got[0], KairoOpened::Preview(p) if p.file_name == "Good Songs.KAIRO"));
        assert_eq!(got[1].to_json(), json!({"error": "This file is not a Kairo export — it could not be read.", "fileName": "broken.kairo"}));
        assert_eq!(queue.pending(), 0);

        queue.detach();
        queue.open(vec![good]).await;
        assert_eq!(queue.pending(), 1);
    });
}

// ─── Small services ──────────────────────────────────────────────────────────

#[test]
fn setlist_commands_match_electron_rules() {
    let dir = tempfile::tempdir().unwrap();
    let s = SetlistService::open(dir.path(), clock());
    let changes = Arc::new(Mutex::new(0));
    let c = changes.clone();
    s.on_changed(move |_| *c.lock().unwrap() += 1);
    assert_eq!(
        s.apply(SetlistCommand::Add {
            song_id: "a".into(),
            index: None,
            list_id: None
        })
        .unwrap_err()
        .to_string(),
        "Create a playlist first."
    );
    let first = s
        .apply(SetlistCommand::Create { name: "  ".into() })
        .unwrap();
    assert_eq!(first.lists[0].name, "Sunday service");
    let a = first.active_id.unwrap();
    let second = s
        .apply(SetlistCommand::Create {
            name: "Evening".into(),
        })
        .unwrap();
    let b = second.active_id.clone().unwrap();
    assert_eq!(
        second.lists[0].id, b,
        "new lists go first and become active"
    );
    s.apply(SetlistCommand::Add {
        song_id: "x".into(),
        index: None,
        list_id: None,
    })
    .unwrap();
    s.apply(SetlistCommand::Add {
        song_id: "y".into(),
        index: Some(0.0),
        list_id: None,
    })
    .unwrap();
    s.apply(SetlistCommand::Add {
        song_id: "x".into(),
        index: None,
        list_id: Some(a.clone()),
    })
    .unwrap();
    assert_eq!(s.snapshot().lists[0].song_ids, ["y", "x"]);
    // Electron writes `activeId: null`; the next read falls back to the
    // first list, as `migrateSetlistState` does.
    let selected = s
        .apply(SetlistCommand::Select {
            list_id: Some("ghost".into()),
        })
        .unwrap();
    assert_eq!(selected.active_id, None);
    let state = s.snapshot();
    assert_eq!(state.active_id, Some(state.lists[0].id.clone()));
    s.forget_song("x");
    let state = s.snapshot();
    assert!(
        state
            .lists
            .iter()
            .all(|l| !l.song_ids.contains(&"x".to_string()))
    );
    s.apply(SetlistCommand::Select {
        list_id: Some(b.clone()),
    })
    .unwrap();
    s.apply(SetlistCommand::Delete { list_id: b }).unwrap();
    assert_eq!(s.snapshot().active_id.as_deref(), Some(a.as_str()));
    let n = *changes.lock().unwrap();
    s.forget_song("never-there");
    assert_eq!(*changes.lock().unwrap(), n, "no write when nothing changes");
    for i in 0..49 {
        s.apply(SetlistCommand::Create {
            name: format!("L{i}"),
        })
        .unwrap();
    }
    assert_eq!(
        s.apply(SetlistCommand::Create {
            name: "one more".into()
        })
        .unwrap_err()
        .to_string(),
        "Delete a playlist before creating another."
    );
    assert_eq!(
        s.import_list("x", &[]).unwrap_err().to_string(),
        "Delete a playlist before importing another."
    );

    // Same file, same shape: Electron's electron-store reads what we wrote.
    s.flush().unwrap();
    let raw: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path().join("kairo-setlist.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(raw["lists"].as_array().unwrap().len(), 50);
    assert!(raw["activeId"].is_string());
}

#[test]
fn legacy_single_list_setlist_migrates() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("kairo-setlist.json"),
        r#"{"songIds":["a","b"],"updatedAt":5}"#,
    )
    .unwrap();
    let s = SetlistService::open(dir.path(), clock());
    let state = s.snapshot();
    assert_eq!(state.lists.len(), 1);
    assert_eq!(state.lists[0].song_ids, ["a", "b"]);
}

#[test]
fn libraries_passages_and_deletes() {
    rt().block_on(async {
        let app = app_with(&[song(1), song(2)], &[]).await;
        let libs = &app.libraries;
        let created = libs.apply(LibrariesCommand::Create { kind: LibraryKind::Songs, name: "\u{1}Hymns\t\tOld ".into() }).unwrap();
        let hymns = created.songs.libraries[0].clone();
        // Tabs are C0 controls, so they are stripped before spaces collapse.
        assert_eq!(hymns.name, "HymnsOld");
        libs.apply(LibrariesCommand::Assign { kind: LibraryKind::Songs, item_ids: vec!["song-001".into(), "song-002".into()], library_id: hymns.id.clone() }).unwrap();
        let doc_lib = libs.apply(LibrariesCommand::Create { kind: LibraryKind::Documents, name: "Slides".into() }).unwrap().documents.libraries[0].id.clone();
        libs.apply(LibrariesCommand::Assign { kind: LibraryKind::Documents, item_ids: vec!["doc-1".into()], library_id: doc_lib }).unwrap();

        let list = app.setlist.apply(SetlistCommand::Create { name: "S".into() }).unwrap().active_id.unwrap();
        app.setlist.apply(SetlistCommand::Add { song_id: "song-001".into(), index: None, list_id: Some(list) }).unwrap();

        // Deleting a song clears its setlist row and its library assignment.
        assert!(delete_song(&app.lyrics, &app.setlist, libs, "song-001").await.unwrap());
        assert!(app.setlist.snapshot().lists[0].song_ids.is_empty());
        let state = libs.snapshot();
        assert!(!state.songs.assignments.contains_key("song-001"));
        assert!(state.songs.assignments.contains_key("song-002"));
        assert!(!delete_song(&app.lyrics, &app.setlist, libs, "song-001").await.unwrap());

        remove_document_from_libraries(libs, "doc-1");
        assert!(libs.snapshot().documents.assignments.is_empty());

        // Deleting a library sends its items home.
        libs.apply(LibrariesCommand::Delete { kind: LibraryKind::Songs, library_id: hymns.id }).unwrap();
        assert!(libs.snapshot().songs.assignments.is_empty());

        // Passages: save into a library, re-save keeps the id, remove forgets it.
        let passages = PassagesService::open(&app.root, libs.clone(), clock());
        let scripture_lib = libs.apply(LibrariesCommand::Create { kind: LibraryKind::Scripture, name: "Favourites".into() }).unwrap().scripture.libraries[0].id.clone();
        let result: ScriptureResult = serde_json::from_value(json!({
            "reference": "John 3:16", "translation": "KJV",
            "verses": [{"book": "John", "chapter": 3, "verse": 16, "text": "For God so loved the world", "translation": "KJV"}]
        })).unwrap();
        assert_eq!(
            passages.apply(PassagesCommand::Save { result: ScriptureResult { verses: vec![], ..result.clone() }, library_id: None }).unwrap_err().to_string(),
            "Look up a passage before saving it."
        );
        let saved = passages.apply(PassagesCommand::Save { result: result.clone(), library_id: Some(scripture_lib.clone()) }).unwrap();
        let id = saved[0].id.clone();
        assert_eq!(libs.snapshot().scripture.assignments.get(&id), Some(&scripture_lib));
        let again = passages.apply(PassagesCommand::Save { result, library_id: None }).unwrap();
        assert_eq!((again.len(), again[0].id.as_str()), (1, id.as_str()));
        passages.apply(PassagesCommand::Remove { passage_id: id.clone() }).unwrap();
        assert!(passages.snapshot().is_empty());
        assert!(!libs.snapshot().scripture.assignments.contains_key(&id));
    });
}

#[test]
fn passages_drop_malformed_entries_and_cap_reference() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("kairo-passages.json"),
        r#"{"passages":[{"id":1,"verses":[]},{"id":"ok","reference":"R","translation":"KJV","verses":[],"savedAt":1},{"id":"x","verses":"no"}]}"#,
    )
    .unwrap();
    let libs = LibrariesService::open(dir.path(), clock());
    let p = PassagesService::open(dir.path(), libs, clock());
    assert_eq!(p.snapshot().len(), 1);
    let long = "😀".repeat(150);
    let result: ScriptureResult = serde_json::from_value(json!({"reference": long, "translation": "KJV", "verses": [{"book": "A", "chapter": 1, "verse": 1, "text": "t", "translation": "KJV"}]})).unwrap();
    let saved = p
        .apply(PassagesCommand::Save {
            result,
            library_id: None,
        })
        .unwrap();
    assert_eq!(saved[0].reference.encode_utf16().count(), 200);
}

#[test]
fn song_usage_records_once_per_service_and_exports_csv() {
    rt().block_on(async {
        let dir = tempfile::tempdir().unwrap();
        let usage = SongUsageService::new(dir.path(), clock());
        let s = song(3);
        usage.record_at(&s, NOW);
        usage.record_at(&s, NOW + 60_000.0);
        usage.record_at(&song(4), NOW + 1.0);
        assert_eq!(usage.list(None, Some(NOW + 10.0)).len(), 2);
        usage.record_at(&s, NOW + 7.0 * 3_600_000.0);
        assert_eq!(
            usage.list(Some(f64::NAN), Some(f64::INFINITY)).len(),
            0,
            "non-finite bounds become 0 and now; `to` is exclusive"
        );
        assert_eq!(usage.list(None, Some(NOW + 1e9)).len(), 3);
        let path = dir.path().join(song_usage_file_name(NOW));
        assert_eq!(path.file_name().unwrap(), "song-usage-2023-11-14.csv");
        let written = usage
            .export_csv(None, Some(NOW + 1e9), &chrono::Utc, Some(path.clone()))
            .await
            .unwrap();
        assert_eq!(written, Some(path.clone()));
        let csv = std::fs::read_to_string(&path).unwrap();
        assert_eq!(csv.lines().count(), 4, "{csv}");
        assert_eq!(
            usage
                .export_csv(None, None, &chrono::Utc, None)
                .await
                .unwrap(),
            None
        );
        usage.clear_before(Some(NOW + 2.0));
        assert_eq!(usage.list(None, Some(NOW + 1e9)).len(), 1);
        usage.flush().unwrap();
        let reopened = SongUsageService::new(dir.path(), clock());
        assert_eq!(reopened.list(None, Some(NOW + 1e9)).len(), 1);
    });
}

/// `cargo test -p kairo-transfer --release --test transfer import_500_songs -- --ignored --nocapture`
#[test]
#[ignore = "timing, run by hand"]
fn import_500_songs() {
    rt().block_on(async {
        let app = app_with(&[], &[]).await;
        let songs: Vec<_> = (0..500).map(song).collect();
        let file = app.root.join("all.kairo");
        std::fs::write(
            &file,
            serialize_bundle(&kairo_domain::kairo_bundle::songs_bundle(
                "All".into(),
                songs,
                None,
                NOW,
            ))
            .unwrap(),
        )
        .unwrap();
        let start = std::time::Instant::now();
        let preview = app.transfer.preview_file(&file).await.unwrap();
        let previewed = start.elapsed();
        let choices = preview
            .songs
            .iter()
            .map(|s| (s.id.clone(), SongImportChoice::Add))
            .collect();
        let start = std::time::Instant::now();
        let mut ticks = 0;
        let result = app
            .transfer
            .commit(
                TransferCommitRequest {
                    token: preview.token,
                    songs: choices,
                    create_setlist: false,
                    playlist: None,
                },
                |_| ticks += 1,
            )
            .await
            .unwrap();
        let committed = start.elapsed();
        assert_eq!((result.added, ticks), (500, 500));
        eprintln!("500 songs: preview {previewed:?}, commit {committed:?}");
    });
}
