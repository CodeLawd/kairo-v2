use futures_lite::future::block_on;
use kairo_store::db::{Database, Schema};
use rusqlite::OptionalExtension;

#[test]
fn first_run_seed_never_replaces_installed_data() {
    block_on(async {
        let fixture = tempfile::tempdir().unwrap();
        let source = fixture.path().join("seed.db");
        let seed = rusqlite::Connection::open(&source).unwrap();
        seed.execute_batch("CREATE TABLE marker (value TEXT); INSERT INTO marker VALUES ('seed');")
            .unwrap();
        drop(seed);
        let profile = fixture.path().join("profile");
        let db = Database::open_bible(&profile, &source).await.unwrap();
        let value = db
            .read(|c| c.query_row("SELECT value FROM marker", [], |r| r.get::<_, String>(0)))
            .await
            .unwrap();
        assert_eq!(value, "seed");
        db.write(|c| c.execute("UPDATE marker SET value = 'installed'", []))
            .await
            .unwrap();
        drop(db);
        let db = Database::open_bible(&profile, &fixture.path().join("seed.db"))
            .await
            .unwrap();
        let value = db
            .read(|c| c.query_row("SELECT value FROM marker", [], |r| r.get::<_, String>(0)))
            .await
            .unwrap();
        assert_eq!(value, "installed");
    });
}

#[test]
fn readers_see_committed_data_while_writer_holds_transaction() {
    block_on(async {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(&dir.path().join("lyrics.db"), Schema::Lyrics)
            .await
            .unwrap();
        let (started, in_transaction) = async_channel::bounded(1);
        let (release, wait) = async_channel::bounded(1);
        let write = db.write(move |c| {
            let tx = c.transaction()?;
            tx.execute("INSERT INTO songs (id, title, created_at, updated_at) VALUES ('song', 'Title', 0, 0)", [])?;
            started.send_blocking(()).unwrap();
            wait.recv_blocking().unwrap();
            tx.commit()
        });
        let read = async {
            in_transaction.recv().await.unwrap();
            let result = db
                .read(|c| {
                    c.query_row("SELECT title FROM songs WHERE id = 'song'", [], |r| {
                        r.get::<_, String>(0)
                    })
                    .optional()
                })
                .await;
            release.send(()).await.unwrap();
            assert_eq!(result.unwrap(), None);
        };
        futures_lite::future::zip(write, read).await.0.unwrap();
        assert_eq!(
            db.read(
                |c| c.query_row("SELECT title FROM songs WHERE id = 'song'", [], |r| r
                    .get::<_, String>(0))
            )
            .await
            .unwrap(),
            "Title"
        );
        assert!(
            db.read(|c| c.execute("DELETE FROM songs", []))
                .await
                .is_err()
        );
    });
}

#[test]
fn quote_index_plaintext_lives_in_memory() {
    block_on(async {
        let db = Database::open_quote_index().await.unwrap();
        let filename = db
            .write(|c| {
                c.query_row(
                    "SELECT file FROM pragma_database_list WHERE name = 'main'",
                    [],
                    |row| row.get::<_, String>(0),
                )
            })
            .await
            .unwrap();
        assert!(filename.is_empty());
        db.write(|c| c.execute("INSERT INTO verses_fts(text) VALUES ('transient')", []))
            .await
            .unwrap();
        assert_eq!(
            db.read(|c| c.query_row(
                "SELECT count(*) FROM verses_fts WHERE verses_fts MATCH 'transient'",
                [],
                |row| row.get::<_, i32>(0)
            ))
            .await
            .unwrap(),
            1
        );
    });
}

#[test]
fn missing_seed_initializes_canonical_books() {
    block_on(async {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open_bible(dir.path(), &dir.path().join("missing.db"))
            .await
            .unwrap();
        let book = db
            .read(|conn| {
                conn.query_row(
                    "SELECT name, abbreviation, aliases FROM books WHERE id = 1",
                    [],
                    |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                        ))
                    },
                )
            })
            .await
            .unwrap();
        assert_eq!(
            book,
            ("Genesis".into(), "Gen".into(), "[\"Ge\",\"Gn\"]".into())
        );
        assert_eq!(db.read(|conn| conn.query_row("SELECT count(*) FROM books", [], |r| r.get::<_, u32>(0))).await.unwrap(), 66);
    });
}

#[test]
fn discarded_api_cache_removes_every_licensed_row() {
    block_on(async {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(&dir.path().join("cache.db"), Schema::ApiBibleCache)
            .await
            .unwrap();
        db.write(|conn| {
            conn.execute_batch(
                "
            INSERT INTO api_translations (bible_id, translation_id, name, copyright, status)
            VALUES ('id', 'TX', 'name', 'copyright', 'ready');
            INSERT INTO api_verses VALUES ('id', 1, 'Genesis', 1, 1, 'ciphertext', 'iv', 'tag', 0);
            INSERT INTO api_ranges VALUES ('id', 1, 1, 1, 1, 0);
            INSERT INTO api_chapters VALUES ('id', 'chapter', 'ready', 0);",
            )
        })
        .await
        .unwrap();
        db.purge_api_bible_cache().await.unwrap();
        assert_eq!(
            db.read(|conn| conn.query_row(
                "
            SELECT (SELECT count(*) FROM api_translations)
                 + (SELECT count(*) FROM api_verses)
                 + (SELECT count(*) FROM api_ranges)
                 + (SELECT count(*) FROM api_chapters)",
                [],
                |r| r.get::<_, u32>(0)
            ))
            .await
            .unwrap(),
            0
        );
    });
}

#[test]
fn wal_recovery_discards_uncommitted_changes_after_process_kill() {
    const CHILD: &str = "KAIRO_SQLITE_CRASH_TEST_DIR";
    if let Some(dir) = std::env::var_os(CHILD) {
        let dir = std::path::PathBuf::from(dir);
        block_on(async {
            let db = Database::open(&dir.join("lyrics.db"), Schema::Lyrics)
                .await
                .unwrap();
            db.write(move |c| -> rusqlite::Result<()> {
                c.execute_batch(
                    "PRAGMA wal_autocheckpoint = 0;
                    CREATE TABLE recovery (id INTEGER);
                    INSERT INTO recovery VALUES (1);",
                )?;
                let tx = c.transaction()?;
                tx.execute("INSERT INTO recovery VALUES (2)", [])?;
                std::fs::write(dir.join("ready"), []).unwrap();
                loop {
                    std::thread::park();
                }
            })
            .await
            .unwrap()
        });
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("wal_recovery_discards_uncommitted_changes_after_process_kill")
        .env(CHILD, dir.path())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !dir.path().join("ready").exists() && std::time::Instant::now() < deadline {
        if let Some(exit) = child.try_wait().unwrap() {
            panic!("Crash fixture exited before writing its WAL: {exit}");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(dir.path().join("ready").exists());
    assert!(dir.path().join("lyrics.db-wal").metadata().unwrap().len() > 0);
    block_on(async {
        let db = Database::open(&dir.path().join("lyrics.db"), Schema::Lyrics)
            .await
            .unwrap();
        let values = db
            .read(|c| {
                let mut stmt = c.prepare("SELECT id FROM recovery ORDER BY id")?;
                stmt.query_map([], |r| r.get::<_, u32>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .await
            .unwrap();
        assert_eq!(values, [1]);
    });
}

#[test]
#[ignore = "Requires the Electron checkout, bundled Bible and matching better-sqlite3 ABI"]
fn bundled_fts_oracle_and_point_lookup_latency() {
    use serde_json::{Value, json};
    let electron = kairo_oracle::electron_dir();
    let source = electron.join("resources/bible.db");
    let temp = tempfile::tempdir().unwrap();
    let oracle_copy = temp.path().join("oracle.db");
    std::fs::copy(&source, &oracle_copy).unwrap();
    block_on(async {
        let db = Database::open_bible(temp.path(), &source).await.unwrap();
        let tokens: Vec<String> = db.read(|conn| {
            conn.execute_batch("CREATE VIRTUAL TABLE temp.words USING fts5vocab(main, verses_fts, row);")?;
            let mut stmt = conn.prepare("SELECT term FROM temp.words WHERE length(term) >= 4 AND doc >= 3 ORDER BY term LIMIT 500")?;
            stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()
        }).await.unwrap();
        let cases: Vec<_> = tokens
            .into_iter()
            .flat_map(|word| {
                [
                    (format!("\"{word}\""), None::<String>),
                    (format!("\"{word}\"*"), Some("BSB".to_string())),
                ]
            })
            .collect();
        assert_eq!(cases.len(), 1_000);

        let mut oracle =
            kairo_oracle::Oracle::new(kairo_oracle::Mode::Live, temp.path().into(), electron);
        // Call in batches to keep both ends' JSONL payloads manageable.
        for batch in cases.chunks(25) {
            let expected = oracle
                .results(
                    "oracle:bible-sqlite.ftsMatches",
                    &[json!([oracle_copy, batch])],
                )
                .unwrap();
            let actual = db
                .read({
                    let batch = batch.to_vec();
                    move |conn| {
                        let mut stmt = conn.prepare_cached(
                            "
                        SELECT b.name, b.abbreviation, f.translation_id, CAST(f.book_id AS INTEGER),
                               CAST(f.chapter AS INTEGER), CAST(f.verse AS INTEGER), f.text
                          FROM verses_fts f JOIN books b ON b.id = CAST(f.book_id AS INTEGER)
                         WHERE verses_fts MATCH ?1 AND (?2 IS NULL OR f.translation_id = ?2)
                         ORDER BY rank LIMIT 50",
                        )?;
                        let mut results = Vec::new();
                        for (query, translation) in batch {
                            let matches = stmt
                                .query_map(rusqlite::params![query, translation], |row| {
                                    Ok(json!({
                                        "book_name": row.get::<_, String>(0)?,
                                        "book_abbr": row.get::<_, String>(1)?,
                                        "translation_id": row.get::<_, String>(2)?,
                                        "book_id": row.get::<_, u32>(3)?,
                                        "chapter": row.get::<_, u32>(4)?,
                                        "verse": row.get::<_, u32>(5)?,
                                        "text": row.get::<_, String>(6)?,
                                    }))
                                })?
                                .collect::<rusqlite::Result<Vec<Value>>>()?;
                            results.push(matches);
                        }
                        Ok(json!(results))
                    }
                })
                .await
                .unwrap();
            assert_eq!(actual, expected[0], "queries beginning with {:?}", batch[0]);
        }

        let mut timings = Vec::new();
        for i in 0..1_000u32 {
            let start = std::time::Instant::now();
            let text: String = db
                .read(move |conn| {
                    conn.prepare_cached(
                        "SELECT text FROM verses
                    WHERE translation_id = 'BSB' AND book_id = 1 AND chapter = 1 AND verse = ?1",
                    )?
                    .query_row([i % 31 + 1], |r| r.get(0))
                })
                .await
                .unwrap();
            timings.push(start.elapsed());
            assert!(!text.is_empty());
        }
        timings.sort_unstable();
        let p99 = timings[989];
        eprintln!("1,000 FTS queries match Electron, point lookup p99 {p99:?} (1,000 samples)");
        assert!(p99 < std::time::Duration::from_millis(1));
    });
}

#[test]
#[ignore = "60-second SQLite concurrency soak"]
fn sustained_concurrent_readers_and_writer() {
    use std::time::{Duration, Instant};
    block_on(async {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(&dir.path().join("lyrics.db"), Schema::Lyrics)
            .await
            .unwrap();
        db.write(|conn| {
            conn.execute_batch("CREATE TABLE soak (count INTEGER); INSERT INTO soak VALUES (0);")
        })
        .await
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        let writer = async {
            let mut writes = 0;
            while Instant::now() < deadline {
                db.write(|conn| {
                    let tx = conn.transaction()?;
                    tx.execute("UPDATE soak SET count = count + 1", [])?;
                    tx.commit()
                })
                .await
                .unwrap();
                writes += 1;
            }
            writes
        };
        let reader = || async {
            let mut reads = 0;
            let mut last = 0;
            while Instant::now() < deadline {
                let count: u32 = db
                    .read(|conn| conn.query_row("SELECT count FROM soak", [], |r| r.get(0)))
                    .await
                    .unwrap();
                assert!(count >= last);
                last = count;
                reads += 1;
            }
            reads
        };
        use futures_lite::future::zip;
        let (writes, (reads1, (reads2, reads3))) =
            zip(writer, zip(reader(), zip(reader(), reader()))).await;
        eprintln!("60-second WAL soak: {writes} writes, reader counts {reads1}/{reads2}/{reads3}");
        assert!(writes > 1_000 && reads1 > 1_000 && reads2 > 1_000 && reads3 > 1_000);
    });
}

#[test]
#[ignore = "Requires the Electron checkout and its real AMPC pack asset"]
fn validates_real_electron_bible_pack() {
    use std::process::{Command, Stdio};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ampc.db");
    let asset = kairo_oracle::electron_dir().join("bible-packs/ampc-pack.db.gz");
    let file = std::fs::File::create(&path).unwrap();
    let status = Command::new("gzip")
        .arg("-cd")
        .arg(asset)
        .stdout(Stdio::from(file))
        .status()
        .unwrap();
    assert!(status.success());
    let pack = block_on(kairo_store::db::validate_bible_pack(&path, Some("AMPC"))).unwrap();
    assert_eq!(pack.translation_id, "AMPC");
    assert_eq!(
        (pack.book_count, pack.chapter_count, pack.verses.len()),
        (66, 1189, 31_101)
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}
