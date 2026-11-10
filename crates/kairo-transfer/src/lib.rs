//! The small data services and `.kairo` exchange, ported from
//! `src/main/services/{setlist,libraries,passages,transfer}/index.ts`,
//! `src/main/services/song-usage.ts`, and the transfer, setlist, libraries,
//! passages, song-usage and delete handlers in `src/main/ipc/index.ts`.
//!
//! Each service writes the same electron-store file as Electron
//! (`kairo-setlist.json`, `kairo-libraries.json`, `kairo-passages.json`,
//! `song-usage.json`) through `kairo_store::settings::open_store`, and
//! normalises on every read as the TypeScript does.

mod cleanup;
mod libraries;
mod open_queue;
mod passages;
mod setlist;
mod song_usage;
mod transfer;

pub use cleanup::{delete_song, remove_document_from_libraries};
pub use libraries::LibrariesService;
pub use open_queue::{KairoOpenQueue, KairoOpened, kairo_paths_in};
pub use passages::PassagesService;
pub use setlist::SetlistService;
pub use song_usage::{SongUsageService, song_usage_file_name};
pub use transfer::{CommitProgress, FileDialogs, TransferService};

use std::sync::Arc;

/// Epoch milliseconds. Injected so tests can pin time.
pub type Clock = Arc<dyn Fn() -> f64 + Send + Sync>;

pub fn system_clock() -> Clock {
    Arc::new(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |d| d.as_millis() as f64)
    })
}

/// Errors carry the TypeScript's user-facing messages unchanged.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    User(&'static str),
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Lyrics(#[from] kairo_lyrics::LyricsError),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

type Listeners<T> = Vec<Box<dyn Fn(&T) + Send>>;

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

use kairo_domain::js::lock;

/// `store.store = state`: the whole file becomes the state's JSON.
fn replace_store<T: serde::Serialize>(store: &mut kairo_store::json_store::JsonStore, state: &T) {
    if let Ok(serde_json::Value::Object(map)) = kairo_domain::json::to_js_value(state) {
        store.replace(map);
    }
}

fn open(user_data: &std::path::Path, name: &str) -> kairo_store::json_store::JsonStore {
    let spec = kairo_store::settings::store_spec(name).expect("known store");
    kairo_store::settings::open_store(user_data, spec)
}
