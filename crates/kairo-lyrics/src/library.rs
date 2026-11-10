use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use icu_collator::{Collator, options::CollatorOptions};
use kairo_domain::{
    js::trim,
    lyrics::{
        LyricsImportPreview, LyricsImportSource, LyricsSectionType, LyricsSong, LyricsSongSection,
        LyricsSource,
        duplicate::find_duplicate,
        section_json::{parse_section_lines, serialize_section_lines},
    },
};
use kairo_store::{
    db::{Database, Schema},
    settings::{MigrationFlag, SettingsStore},
    workspace::{Workspace, delete_song_file, read_song_files, write_song_file},
};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::import::{parse_import, parse_open_lyrics};

#[derive(Debug, thiserror::Error)]
pub enum LyricsError {
    #[error(transparent)]
    Database(#[from] kairo_store::db::Error),
    #[error("Only file and text imports can be previewed.")]
    PreviewSource,
    #[error(
        "SongSelect CCLI API import requires a licensed SongSelect account. Export the song as a .usr file from SongSelect and use the \"usr\" import type instead."
    )]
    LicensedImport,
    #[error("URL import is not yet implemented.")]
    UrlImport,
    #[error("Online lyric import requires an online lyrics provider.")]
    OnlineImport,
    #[error("Invalid OpenLyrics file: {0}")]
    OpenLyrics(String),
    /// An online source, review or translation failure, in the operator's words.
    #[error("{0}")]
    Online(String),
}

pub(crate) type Result<T> = std::result::Result<T, LyricsError>;

pub use kairo_domain::workspace::WorkspaceSongSync as FolderSync;

pub struct LyricsService {
    db: Database,
    workspace: Workspace,
    settings: Arc<Mutex<SettingsStore>>,
}

use kairo_domain::js::now_ms;

pub(crate) fn new_id(now: f64) -> String {
    format!(
        "song-{now}-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..12]
    )
}

const SONG_COLUMNS: &str =
    "id,title,artist,copyright,ccli_number,is_favorite,source,created_at,updated_at";
const SONG_BY_ID: &str = "SELECT id,title,artist,copyright,ccli_number,is_favorite,source,created_at,updated_at FROM songs WHERE id=?";

fn row_song(row: &Row<'_>) -> rusqlite::Result<LyricsSong> {
    let copyright: String = row.get(3)?;
    let source: String = row.get(6)?;
    Ok(LyricsSong {
        id: row.get(0)?,
        title: row.get(1)?,
        artist: row.get(2)?,
        copyright: (!copyright.is_empty()).then_some(copyright),
        ccli_number: row.get(4)?,
        is_favorite: Some(row.get::<_, i64>(5)? == 1),
        source: LyricsSource::from_name(&source),
        sections: Vec::new(),
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
        extra: Default::default(),
    })
}

fn sections(db: &Connection, mut song: LyricsSong) -> rusqlite::Result<LyricsSong> {
    let mut query = db.prepare_cached(
        "SELECT type,label,lines FROM song_sections WHERE song_id=? ORDER BY sort_order",
    )?;
    song.sections = query
        .query_map([&song.id], |row| {
            let kind: String = row.get(0)?;
            let text: String = row.get(2)?;
            let parsed = parse_section_lines(&text);
            Ok(LyricsSongSection {
                kind: LyricsSectionType::from_name(&kind).unwrap_or(LyricsSectionType::Verse),
                label: row.get(1)?,
                lines: parsed.lines,
                line_colors: parsed.line_colors,
                hotkey: parsed.hotkey,
                extra: Default::default(),
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(song)
}

fn get(db: &Connection, id: &str) -> rusqlite::Result<Option<LyricsSong>> {
    db.prepare_cached(SONG_BY_ID)?
        .query_row([id], row_song)
        .optional()?
        .map(|song| sections(db, song))
        .transpose()
}

fn library(db: &Connection) -> rusqlite::Result<Vec<LyricsSong>> {
    let mut query = db.prepare_cached(&format!(
        "SELECT {SONG_COLUMNS} FROM songs ORDER BY title COLLATE NOCASE"
    ))?;
    query
        .query_map([], row_song)?
        .map(|song| sections(db, song?))
        .collect()
}

fn write_contents(db: &Connection, song: &LyricsSong, replace_fts: bool) -> rusqlite::Result<()> {
    db.execute("DELETE FROM song_sections WHERE song_id=?", [&song.id])?;
    let mut insert = db.prepare_cached("INSERT INTO song_sections(song_id,type,label,section_idx,lines,sort_order) VALUES(?,?,?,?,?,?)")?;
    for (index, section) in song.sections.iter().enumerate() {
        insert.execute(params![
            song.id,
            section.kind.as_str(),
            section.label,
            index as i64 + 1,
            serialize_section_lines(
                &section.lines,
                section.line_colors.as_deref(),
                section.hotkey.as_deref()
            ),
            index as i64
        ])?;
    }
    if replace_fts {
        db.execute("DELETE FROM songs_fts WHERE song_id=?", [&song.id])?;
    }
    let text = song
        .sections
        .iter()
        .flat_map(|s| s.lines.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ");
    db.execute(
        "INSERT INTO songs_fts(song_id,title,artist,lyrics) VALUES(?,?,?,?)",
        params![song.id, song.title, song.artist, text],
    )?;
    Ok(())
}

fn put(db: &Connection, song: &LyricsSong, replace_fts: bool) -> rusqlite::Result<()> {
    db.execute("INSERT OR REPLACE INTO songs(id,title,artist,copyright,ccli_number,is_favorite,source,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?)",
        params![song.id, song.title, song.artist, song.copyright.as_deref().unwrap_or(""),
            song.ccli_number, song.is_favorite.unwrap_or(false), song.source.unwrap_or(LyricsSource::Text).as_str(), song.created_at, song.updated_at])?;
    write_contents(db, song, replace_fts)
}

fn remove(db: &Connection, id: &str) -> rusqlite::Result<bool> {
    let changed = db.execute("DELETE FROM songs WHERE id=?", [id])? != 0;
    db.execute("DELETE FROM songs_fts WHERE song_id=?", [id])?;
    Ok(changed)
}

fn by_ccli(db: &Connection, number: &str) -> rusqlite::Result<Option<LyricsSong>> {
    db.prepare_cached(&format!(
        "SELECT {SONG_COLUMNS} FROM songs WHERE ccli_number=? AND ccli_number!='' LIMIT 1"
    ))?
    .query_row([number], row_song)
    .optional()?
    .map(|song| sections(db, song))
    .transpose()
}

fn search(db: &Connection, query: &str) -> rusqlite::Result<Vec<LyricsSong>> {
    if query.is_empty() {
        return library(db);
    }
    let fts = query.replace(['"', '*', '^', '(', ')', '-'], " ");
    let ranked = (|| -> rusqlite::Result<Vec<String>> {
        let mut stmt = db.prepare_cached(
            "SELECT song_id FROM songs_fts WHERE songs_fts MATCH ? ORDER BY rank LIMIT 50",
        )?;
        stmt.query_map([trim(&fts)], |row| row.get(0))?.collect()
    })()
    .unwrap_or_default();
    let mut stmt = db.prepare_cached(
        "SELECT DISTINCT s.id FROM songs s LEFT JOIN song_sections sec ON sec.song_id=s.id
         WHERE s.title LIKE ?1 COLLATE NOCASE OR s.artist LIKE ?1 COLLATE NOCASE OR sec.lines LIKE ?1 LIMIT 50",
    )?;
    let pattern = format!("%{query}%");
    let fallback = stmt.query_map([pattern], |row| row.get::<_, String>(0))?;
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for id in ranked.into_iter().map(Ok).chain(fallback) {
        let id = id?;
        if seen.insert(id.clone())
            && let Some(song) = get(db, &id)?
        {
            result.push(song);
        }
    }
    let collator = Collator::try_new(Default::default(), CollatorOptions::default())
        .expect("compiled Unicode collation data");
    result.sort_by(|a, b| collator.compare(&a.title, &b.title));
    Ok(result)
}

impl LyricsService {
    /// Opens the SQLite workers and reconciles the workspace before returning.
    pub async fn open(
        profile: &Path,
        default_workspace: PathBuf,
        settings: Arc<Mutex<SettingsStore>>,
    ) -> Result<Self> {
        let db = Database::open(&profile.join("lyrics.db"), Schema::Lyrics).await?;
        let service = Self {
            db,
            workspace: Workspace::new(default_workspace),
            settings,
        };
        service.sync_from_folder().await?;
        Ok(service)
    }

    fn songs_dir(&self) -> PathBuf {
        self.workspace.songs_dir(
            &self
                .settings
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
        )
    }

    pub async fn sync_from_folder(&self) -> Result<FolderSync> {
        let (dir, settings) = (self.songs_dir(), self.settings.clone());
        Ok(self
            .db
            .write(move |db| {
                let mut entries = read_song_files(&dir, now_ms());
                // libuv's readdirSync sorts file names. FTS rank ties depend
                // on the resulting insertion order before applying LIMIT.
                entries.sort_by(|a, b| a.file.file_name().cmp(&b.file.file_name()));
                let on_disk: HashSet<_> =
                    entries.iter().map(|entry| entry.song.id.as_str()).collect();
                let first_export = !settings
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .migration_done(MigrationFlag::SongsFolderExport);
                let tx = db.transaction()?;
                let mut summary = FolderSync {
                    imported: 0.0,
                    removed: 0.0,
                    exported: 0.0,
                };
                let mut export_complete = first_export;
                let ids = serde_json::to_string(&on_disk)
                    .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
                // Unindexed song_id requires a scan to delete. Remove all
                // replaced rows in one scan and insert in the original order.
                tx.execute(
                    "DELETE FROM songs_fts WHERE song_id IN (SELECT value FROM json_each(?))",
                    [ids],
                )?;
                let mut imported = HashSet::new();
                for entry in &entries {
                    put(&tx, &entry.song, !imported.insert(entry.song.id.as_str()))?;
                    summary.imported += 1.0;
                }
                let remaining = {
                    let mut stmt =
                        tx.prepare_cached("SELECT id FROM songs ORDER BY title COLLATE NOCASE")?;
                    stmt.query_map([], |row| row.get::<_, String>(0))?
                        .collect::<rusqlite::Result<Vec<_>>>()?
                };
                for id in remaining {
                    if on_disk.contains(id.as_str()) {
                        continue;
                    }
                    if first_export {
                        if let Some(song) = get(&tx, &id)? {
                            if write_song_file(&dir, &song).is_some() {
                                summary.exported += 1.0;
                            } else {
                                export_complete = false;
                            }
                        }
                    } else {
                        remove(&tx, &id)?;
                        summary.removed += 1.0;
                    }
                }
                tx.commit()?;
                if export_complete {
                    let mut settings = settings.lock().unwrap_or_else(|e| e.into_inner());
                    settings.mark_migration_done(MigrationFlag::SongsFolderExport);
                    settings.flush().map_err(|err| {
                        rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(
                            err,
                        )))
                    })?;
                }
                Ok(summary)
            })
            .await?)
    }

    pub async fn get_library(&self) -> Result<Vec<LyricsSong>> {
        Ok(self
            .db
            .read(|db| library(&*db.unchecked_transaction()?))
            .await?)
    }

    pub async fn get_song(&self, id: &str) -> Result<Option<LyricsSong>> {
        let id = id.to_owned();
        Ok(self
            .db
            .read(move |db| get(&*db.unchecked_transaction()?, &id))
            .await?)
    }

    pub async fn search(&self, query: &str) -> Result<Vec<LyricsSong>> {
        let query = trim(query).to_owned();
        Ok(self
            .db
            .read(move |db| search(&*db.unchecked_transaction()?, &query))
            .await?)
    }

    pub async fn add_song(&self, song: LyricsSong) -> Result<()> {
        let dir = self.songs_dir();
        Ok(self
            .db
            .write(move |db| {
                let tx = db.transaction()?;
                put(&tx, &song, true)?;
                tx.commit()?;
                write_song_file(&dir, &song);
                Ok(())
            })
            .await?)
    }

    pub async fn add_imported_song(
        &self,
        mut song: LyricsSong,
        fresh_id: bool,
    ) -> Result<LyricsSong> {
        let now = now_ms();
        if fresh_id {
            song.id = new_id(now);
            song.created_at = now;
        }
        song.updated_at = now;
        song.source.get_or_insert(LyricsSource::Text);
        song.is_favorite.get_or_insert(false);
        self.add_song(song.clone()).await?;
        Ok(song)
    }

    pub async fn update_song(&self, id: &str, mut song: LyricsSong) -> Result<Option<LyricsSong>> {
        let (id, dir) = (id.to_owned(), self.songs_dir());
        Ok(self.db.write(move |db| {
            let tx = db.transaction()?;
            let Some(existing) = get(&tx, &id)? else { return Ok(None) };
            song.id = id;
            song.source = existing.source;
            song.is_favorite = existing.is_favorite;
            song.created_at = existing.created_at;
            song.updated_at = now_ms();
            tx.execute("UPDATE songs SET title=?,artist=?,copyright=?,ccli_number=?,updated_at=? WHERE id=?",
                params![song.title, song.artist, song.copyright.as_deref().unwrap_or(""), song.ccli_number, song.updated_at, song.id])?;
            write_contents(&tx, &song, true)?;
            let saved = get(&tx, &song.id)?;
            tx.commit()?;
            if let Some(saved) = &saved { write_song_file(&dir, saved); }
            Ok(saved)
        }).await?)
    }

    pub async fn delete_song(&self, id: &str) -> Result<bool> {
        let (id, dir) = (id.to_owned(), self.songs_dir());
        Ok(self
            .db
            .write(move |db| {
                let tx = db.transaction()?;
                let changed = remove(&tx, &id)?;
                tx.commit()?;
                delete_song_file(&dir, &id);
                Ok(changed)
            })
            .await?)
    }

    pub async fn toggle_favorite(&self, id: &str) -> Result<bool> {
        let (id, dir) = (id.to_owned(), self.songs_dir());
        Ok(self
            .db
            .write(move |db| {
                db.execute(
                    "UPDATE songs SET is_favorite=1-is_favorite,updated_at=? WHERE id=?",
                    params![now_ms(), id],
                )?;
                let Some(saved) = get(db, &id)? else {
                    return Ok(false);
                };
                write_song_file(&dir, &saved);
                Ok(saved.is_favorite.unwrap_or(false))
            })
            .await?)
    }

    pub async fn find_by_ccli(&self, number: &str) -> Result<Option<LyricsSong>> {
        let number = number.to_owned();
        Ok(self
            .db
            .read(move |db| by_ccli(&*db.unchecked_transaction()?, &number))
            .await?)
    }

    pub async fn preview_import(&self, source: LyricsImportSource) -> Result<LyricsImportPreview> {
        if !matches!(
            source,
            LyricsImportSource::Usr { .. } | LyricsImportSource::Text { .. }
        ) {
            return Err(LyricsError::PreviewSource);
        }
        let song = parse_import(source, now_ms())?;
        let duplicate = find_duplicate(&song, &self.get_library().await?);
        Ok(LyricsImportPreview { song, duplicate })
    }

    pub async fn import_song(&self, source: LyricsImportSource) -> Result<LyricsSong> {
        self.save_import(parse_import(source, now_ms())?).await
    }

    pub async fn import_open_lyrics(&self, xml: &str) -> Result<LyricsSong> {
        self.save_import(parse_open_lyrics(xml, now_ms())?).await
    }

    pub(crate) async fn save_import(&self, song: LyricsSong) -> Result<LyricsSong> {
        let dir = self.songs_dir();
        Ok(self
            .db
            .write(move |db| {
                let tx = db.transaction()?;
                if let Some(number) = &song.ccli_number
                    && let Some(existing) = by_ccli(&tx, number)?
                {
                    return Ok(existing);
                }
                put(&tx, &song, true)?;
                tx.commit()?;
                write_song_file(&dir, &song);
                Ok(song)
            })
            .await?)
    }
}
