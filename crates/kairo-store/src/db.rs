//! Bounded SQLite access. Every connection lives on its own OS thread.
//!
//! Writable databases use WAL and a single writer. Independent read-only
//! connections share a bounded queue. A transient quote index serializes all
//! calls on one worker so its licensed plaintext never leaves memory.

mod pack;
mod verse_counts;

pub use pack::{PackVerse, ValidatedPack, validate_bible_pack};
pub use verse_counts::BIBLE_VERSE_COUNTS;

use async_channel::{Receiver, Sender};
use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;
use std::fs::{self, File};
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

const READER_COUNT: usize = 3;
const QUEUE_CAPACITY: usize = 64;
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
const BIBLE: &str = include_str!("db/bible.sql");
const QUOTE: &str = include_str!("db/quote.sql");

type Job = Box<dyn FnOnce(&mut Connection) + Send>;
type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("SQLite worker stopped")]
    WorkerStopped,
    #[error("SQLite refused WAL mode (returned {0})")]
    WalUnavailable(String),
    #[error("Expected an API.Bible cache database")]
    NotApiBibleCache,
    #[error("Invalid Bible pack: {0}")]
    InvalidPack(String),
}

/// The on-disk layouts used by Electron.
#[derive(Clone, Copy, Debug)]
pub enum Schema {
    Bible,
    Lyrics,
    ApiBibleCache,
}

impl Schema {
    fn sql(self) -> &'static str {
        match self {
            Self::Bible => BIBLE,
            Self::Lyrics => include_str!("db/lyrics.sql"),
            Self::ApiBibleCache => include_str!("db/api_cache.sql"),
        }
    }
}

/// A cloneable handle to dedicated database workers. Dropping the last handle
/// closes the queues and lets the workers close their connections.
#[derive(Clone)]
pub struct Database {
    writer: Sender<Job>,
    readers: Sender<Job>,
    schema: Option<Schema>,
}

impl Database {
    pub async fn open(path: &Path, schema: Schema) -> Result<Self> {
        let path = path.to_owned();
        let writer_path = path.clone();
        Self::open_file(path, schema, move || open_writer(&writer_path, schema)).await
    }

    /// Install the bundled seed into the profile once. A missing bundle is
    /// allowed and creates the same empty, book-seeded database as Electron.
    pub async fn open_bible(user_data: &Path, bundled_bible: &Path) -> Result<Self> {
        let path = user_data.join("bible.db");
        let destination = path.clone();
        let bundled_bible = bundled_bible.to_owned();
        Self::open_file(path, Schema::Bible, move || {
            seed_bible_file(&bundled_bible, &destination)?;
            open_writer(&destination, Schema::Bible)
        })
        .await
    }

    async fn open_file(
        path: PathBuf,
        schema: Schema,
        open_writer: impl FnOnce() -> Result<Connection> + Send + 'static,
    ) -> Result<Self> {
        let (writer, writes) = async_channel::bounded(QUEUE_CAPACITY);
        spawn_worker("sqlite-writer", writes, open_writer).await?;
        let (readers, reads) = async_channel::bounded(QUEUE_CAPACITY);
        for _ in 0..READER_COUNT {
            let path = path.clone();
            spawn_worker("sqlite-reader", reads.clone(), move || {
                let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
                configure(&conn)?;
                Ok(conn)
            })
            .await?;
        }
        Ok(Self {
            writer,
            readers,
            schema: Some(schema),
        })
    }

    /// SQLite shared-cache memory databases can return SQLITE_LOCKED when
    /// readers overlap a write. The ephemeral index uses one queue and one
    /// connection, matching the Electron index's ownership.
    pub async fn open_quote_index() -> Result<Self> {
        let (writer, jobs) = async_channel::bounded(QUEUE_CAPACITY);
        spawn_worker("sqlite-quotes", jobs, || {
            let conn = Connection::open_in_memory()?;
            conn.execute_batch("PRAGMA temp_store = MEMORY;")?;
            conn.execute_batch(QUOTE)?;
            Ok(conn)
        })
        .await?;
        Ok(Self {
            readers: writer.clone(),
            writer,
            schema: None,
        })
    }

    /// Discard encrypted cache data atomically when its key cannot be recovered.
    /// Also clear child tables explicitly to remove any legacy orphan rows.
    pub async fn purge_api_bible_cache(&self) -> Result<()> {
        if !matches!(self.schema, Some(Schema::ApiBibleCache)) {
            return Err(Error::NotApiBibleCache);
        }
        self.write(|conn| {
            let tx = conn.transaction()?;
            tx.execute_batch(
                "DELETE FROM api_verses;
                 DELETE FROM api_ranges;
                 DELETE FROM api_chapters;
                 DELETE FROM api_translations;",
            )?;
            tx.commit()
        })
        .await
    }

    pub async fn read<T: Send + 'static>(
        &self,
        query: impl FnOnce(&Connection) -> rusqlite::Result<T> + Send + 'static,
    ) -> Result<T> {
        submit(&self.readers, move |conn| query(conn)).await
    }

    /// A started write runs through completion even if its caller is
    /// cancelled. Group related mutations in a transaction in the closure.
    pub async fn write<T: Send + 'static>(
        &self,
        query: impl FnOnce(&mut Connection) -> rusqlite::Result<T> + Send + 'static,
    ) -> Result<T> {
        submit(&self.writer, query).await
    }
}

async fn submit<T: Send + 'static>(
    jobs: &Sender<Job>,
    query: impl FnOnce(&mut Connection) -> rusqlite::Result<T> + Send + 'static,
) -> Result<T> {
    let (reply, result) = async_channel::bounded(1);
    jobs.send(Box::new(move |conn| {
        if !reply.is_closed() {
            let _ = reply.try_send(query(conn));
        }
    }))
    .await
    .map_err(|_| Error::WorkerStopped)?;
    Ok(result.recv().await.map_err(|_| Error::WorkerStopped)??)
}

async fn spawn_worker(
    name: &str,
    jobs: Receiver<Job>,
    open: impl FnOnce() -> Result<Connection> + Send + 'static,
) -> Result<()> {
    let (ready, opened) = async_channel::bounded(1);
    thread::Builder::new().name(name.into()).spawn(move || {
        let mut conn = match open() {
            Ok(conn) => conn,
            Err(error) => {
                let _ = ready.try_send(Err(error));
                return;
            }
        };
        if ready.try_send(Ok(())).is_ok() {
            while let Ok(job) = jobs.recv_blocking() {
                job(&mut conn);
            }
        }
    })?;
    opened.recv().await.map_err(|_| Error::WorkerStopped)?
}

fn configure(conn: &Connection) -> rusqlite::Result<()> {
    conn.busy_timeout(BUSY_TIMEOUT)?;
    conn.pragma_update(None, "foreign_keys", true)?;
    Ok(())
}

fn open_writer(path: &Path, schema: Schema) -> Result<Connection> {
    if let Some(parent) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE;
    let mut conn = Connection::open_with_flags(path, flags)?;
    configure(&conn)?;
    // SQLite recovers committed WAL frames on open; never remove side files.
    let mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
    if mode != "wal" {
        return Err(Error::WalUnavailable(mode));
    }
    let synchronous = match schema {
        Schema::Bible | Schema::Lyrics => "NORMAL",
        Schema::ApiBibleCache => "FULL",
    };
    conn.pragma_update(None, "synchronous", synchronous)?;
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    tx.execute_batch(schema.sql())?;
    if matches!(schema, Schema::Bible) {
        seed_books(&tx)?;
    }
    tx.commit()?;
    Ok(conn)
}

fn seed_bible_file(source: &Path, destination: &Path) -> io::Result<()> {
    match destination.symlink_metadata() {
        Ok(_) => return Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let mut source = match File::open(source) {
        Ok(source) => source,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    let parent = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    fs::create_dir_all(parent)?;
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    io::copy(&mut source, &mut staged)?;
    staged.as_file().sync_all()?;
    match staged.persist_noclobber(destination) {
        Ok(_) => Ok(()),
        Err(error) if error.error.kind() == ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error.error),
    }
}

/// The 66 canonical books (`BOOKS` in `bible-db.ts`): id, name, abbr, testament, aliases.
pub const BOOKS_JSON: &str = include_str!("db/books.json");

#[derive(Deserialize)]
struct Book {
    id: u32,
    name: String,
    abbr: String,
    testament: String,
    aliases: Vec<String>,
}

fn seed_books(conn: &Connection) -> Result<()> {
    let count: u32 = conn.query_row("SELECT count(*) FROM books", [], |row| row.get(0))?;
    if count != 66 {
        let books: Vec<Book> = serde_json::from_str(BOOKS_JSON)?;
        let mut insert = conn.prepare(
            "INSERT OR REPLACE INTO books (id, name, abbreviation, testament, aliases)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for book in books {
            insert.execute(rusqlite::params![
                book.id,
                book.name,
                book.abbr,
                book.testament,
                serde_json::to_string(&book.aliases)?,
            ])?;
        }
    }
    Ok(())
}
