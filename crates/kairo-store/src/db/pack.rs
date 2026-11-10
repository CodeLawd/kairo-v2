use super::{Error, Result, verse_counts::BIBLE_VERSE_COUNTS};
use kairo_domain::scripture::{
    translations::get_translation_definition, versification::get_verse_numbering_override,
};
use rusqlite::{Connection, OpenFlags};
use std::{fs, io::ErrorKind, path::Path, thread};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackVerse {
    pub book_id: u32,
    pub chapter: u32,
    pub verse: u32,
    pub text: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ValidatedPack {
    pub translation_id: String,
    pub translation_name: String,
    pub language: String,
    pub verses: Vec<PackVerse>,
    pub book_count: usize,
    pub chapter_count: usize,
}

/// Read and validate an external, finalized SQLite pack on a dedicated thread.
/// Only fixed queries against ordinary tables are allowed. The pack is opened
/// immutable so SQLite cannot create WAL or shared-memory files next to it.
pub async fn validate_bible_pack(
    path: &Path,
    expected_translation: Option<&str>,
) -> Result<ValidatedPack> {
    let path = path.to_owned();
    let expected_translation = expected_translation.map(str::to_uppercase);
    let (reply, result) = async_channel::bounded(1);
    thread::Builder::new()
        .name("sqlite-bible-pack".into())
        .spawn(move || {
            let _ = reply.try_send(validate(&path, expected_translation.as_deref()));
        })?;
    result.recv().await.map_err(|_| Error::WorkerStopped)?
}

fn invalid(message: impl Into<String>) -> Error {
    Error::InvalidPack(message.into())
}

fn validate(path: &Path, expected: Option<&str>) -> Result<ValidatedPack> {
    if path.to_str().is_some_and(|path| path.trim().is_empty()) {
        return Err(invalid("Select a Bible pack file to install."));
    }
    let path = fs::canonicalize(path).map_err(|error| {
        if error.kind() == ErrorKind::NotFound {
            invalid("That Bible pack file could not be found.")
        } else {
            error.into()
        }
    })?;
    if !fs::metadata(&path)?.is_file() {
        return Err(invalid("That Bible pack path is not a file."));
    }
    // Immutable connections ignore WAL. Reject a pack that still has frames
    // instead of silently validating an older snapshot of the database.
    let mut wal_path = path.as_os_str().to_owned();
    wal_path.push("-wal");
    match fs::metadata(wal_path) {
        Ok(metadata) if metadata.len() > 0 => {
            return Err(invalid("The Bible pack has an uncheckpointed WAL."));
        }
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let uri = url::Url::from_file_path(&path).map_err(|_| invalid("Invalid Bible pack path."))?;
    // SQLite treats an authority as invalid in its default build. Encode a
    // Windows UNC path as a file URI with an empty authority and two path slashes.
    let uri = match uri.host_str() {
        Some(host) => format!("file:////{host}{}?immutable=1", uri.path()),
        None => format!("{uri}?immutable=1"),
    };
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI;
    let conn = Connection::open_with_flags(uri, flags)?;
    conn.execute_batch("PRAGMA trusted_schema = OFF; PRAGMA temp_store = MEMORY;")?;
    let table_count: u32 = conn.query_row(
        "SELECT count(*) FROM pragma_table_list
         WHERE schema = 'main' AND type = 'table' AND name IN ('translations', 'verses')",
        [],
        |row| row.get(0),
    )?;
    if table_count != 2 {
        return Err(invalid(
            "That file is missing the translations or verses table.",
        ));
    }
    let check: String = conn.query_row("PRAGMA integrity_check(1)", [], |row| row.get(0))?;
    if check != "ok" {
        return Err(invalid(
            "The Bible pack failed SQLite integrity validation.",
        ));
    }

    let mut statement = conn.prepare("SELECT id, name, language FROM translations LIMIT 2")?;
    let mut rows = statement.query([])?;
    let Some(translation) = rows.next()? else {
        return Err(invalid("The Bible pack contains no translations."));
    };
    let source_id: String = translation.get(0)?;
    let name: Option<String> = translation.get(1)?;
    let language: Option<String> = translation.get(2)?;
    let translation_id = source_id.to_uppercase();
    if rows.next()?.is_some() || expected.is_some_and(|expected| expected != translation_id) {
        return Err(invalid(match expected {
            Some(expected) => format!("Expected a pack containing only {expected}."),
            None => "Expected a pack containing a single translation.".into(),
        }));
    }

    let mut statement = conn.prepare(
        "SELECT book_id, chapter, verse, text FROM verses
         WHERE translation_id = ?1 ORDER BY book_id, chapter, verse",
    )?;
    let mut rows = statement.query([source_id])?;
    let mut verses = Vec::new();
    let mut chapter_count = 0;
    // Walk the canonical numbering and the actual rows together. A missing,
    // duplicate, omitted, or out-of-bounds reference cannot match this sequence.
    for (book_index, counts) in BIBLE_VERSE_COUNTS.iter().enumerate() {
        let book_id = (book_index + 1) as u32;
        for (chapter_index, &canonical_last) in counts.iter().enumerate() {
            let chapter = (chapter_index + 1) as u32;
            chapter_count += 1;
            let numbering = get_verse_numbering_override(
                &translation_id,
                f64::from(book_id),
                f64::from(chapter),
            );
            let last = numbering.map_or(canonical_last, |numbering| numbering.last_verse);
            for verse in 1..=last {
                if numbering.is_some_and(|numbering| numbering.omitted.contains(&verse)) {
                    continue;
                }
                let Some(row) = rows.next()? else {
                    return Err(invalid(format!(
                        "Missing verse {book_id}:{chapter}:{verse}."
                    )));
                };
                let actual: (u32, u32, u32) = (row.get(0)?, row.get(1)?, row.get(2)?);
                if actual != (book_id, chapter, verse) {
                    return Err(invalid(format!(
                        "Expected verse {book_id}:{chapter}:{verse}, found {}:{}:{}.",
                        actual.0, actual.1, actual.2
                    )));
                }
                let text: String = row.get::<_, Option<String>>(3)?.unwrap_or_default();
                if text
                    .trim_matches(|c: char| (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}')
                    .is_empty()
                {
                    return Err(invalid(format!(
                        "Empty verse text at {book_id}:{chapter}:{verse}."
                    )));
                }
                verses.push(PackVerse {
                    book_id,
                    chapter,
                    verse,
                    text,
                });
            }
        }
    }
    if rows.next()?.is_some() {
        return Err(invalid("The Bible pack contains unexpected extra verses."));
    }
    let translation_name = name.filter(|name| !name.is_empty()).unwrap_or_else(|| {
        get_translation_definition(&translation_id)
            .map_or(translation_id.as_str(), |def| def.name)
            .into()
    });
    Ok(ValidatedPack {
        translation_id,
        translation_name,
        language: language
            .filter(|language| !language.is_empty())
            .unwrap_or_else(|| "en".into()),
        verses,
        book_count: BIBLE_VERSE_COUNTS.len(),
        chapter_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_lite::future::block_on;
    use rusqlite::params;

    #[test]
    fn immutable_pack_checks_exact_versification_without_side_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pack # %.db");
        let mut writer = Connection::open(&path).unwrap();
        writer.execute_batch("
            PRAGMA journal_mode = WAL;
            CREATE TABLE translations (id TEXT, name TEXT, language TEXT);
            CREATE TABLE verses (translation_id TEXT, book_id INTEGER, chapter INTEGER, verse INTEGER, text TEXT);
            CREATE VIRTUAL TABLE verses_fts USING fts5(text);
            INSERT INTO translations VALUES ('ampc', '', NULL);
        ").unwrap();
        let tx = writer.transaction().unwrap();
        {
            let mut insert = tx
                .prepare("INSERT INTO verses VALUES ('ampc', ?1, ?2, ?3, 'test')")
                .unwrap();
            for (book, counts) in BIBLE_VERSE_COUNTS.iter().enumerate() {
                for (chapter, &last) in counts.iter().enumerate() {
                    let (book, chapter) = (book + 1, chapter + 1);
                    let (last, omitted): (_, &[u32]) = match (book, chapter) {
                        (41, 9) => (50, &[44, 46]),
                        (64, 1) => (15, &[]),
                        _ => (last, &[]),
                    };
                    for verse in (1..=last).filter(|verse| !omitted.contains(verse)) {
                        insert
                            .execute(params![book as i64, chapter as i64, verse])
                            .unwrap();
                    }
                }
            }
        }
        tx.commit().unwrap();
        let pending = block_on(validate_bible_pack(&path, None)).unwrap_err();
        assert!(
            pending.to_string().contains("uncheckpointed WAL"),
            "{pending}"
        );
        drop(writer);

        let files = || {
            fs::read_dir(dir.path())
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect::<Vec<_>>()
        };
        assert_eq!(files(), [path.file_name().unwrap()]);
        let pack = block_on(validate_bible_pack(&path, Some("AmPc"))).unwrap();
        assert_eq!(pack.translation_id, "AMPC");
        assert_eq!(
            pack.translation_name,
            get_translation_definition("AMPC").unwrap().name
        );
        assert_eq!(pack.language, "en");
        assert_eq!(
            (pack.book_count, pack.chapter_count, pack.verses.len()),
            (66, 1189, 31_101)
        );
        assert_eq!(files(), [path.file_name().unwrap()]);

        let wrong_translation = block_on(validate_bible_pack(&path, Some("NKJV"))).unwrap_err();
        assert!(wrong_translation.to_string().contains("only NKJV"));
        let writer = Connection::open(&path).unwrap();
        writer
            .execute(
                "UPDATE verses SET verse = 44 WHERE book_id = 41 AND chapter = 9 AND verse = 45",
                [],
            )
            .unwrap();
        drop(writer);
        let wrong_verse = block_on(validate_bible_pack(&path, None)).unwrap_err();
        assert!(
            wrong_verse
                .to_string()
                .contains("Expected verse 41:9:45, found 41:9:44"),
            "{wrong_verse}"
        );
        assert_eq!(files(), [path.file_name().unwrap()]);
    }

    #[test]
    fn rejects_invalid_paths_non_databases_and_views() {
        let dir = tempfile::tempdir().unwrap();
        for path in [Path::new(" "), &dir.path().join("missing"), dir.path()] {
            assert!(matches!(validate(path, None), Err(Error::InvalidPack(_))));
        }
        let broken = dir.path().join("broken.db");
        fs::write(&broken, "invalid file").unwrap();
        assert!(validate(&broken, None).is_err());

        let path = dir.path().join("view.db");
        let writer = Connection::open(&path).unwrap();
        writer.execute_batch("CREATE VIEW translations AS SELECT 'test' AS id; CREATE TABLE verses (book_id INTEGER);").unwrap();
        drop(writer);
        let error = validate(&path, None).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("missing the translations or verses table")
        );
    }
}
