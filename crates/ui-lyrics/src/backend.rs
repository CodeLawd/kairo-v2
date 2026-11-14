//! What the Lyrics page reads and asks for. The application implements
//! [`LyricsBackend`] over its services; tests drive a scripted fake.
//!
//! Electron reached these through `window.api.lyrics`, `setlist`,
//! `libraries`, `songUsage` and `transfer`, and the renderer stores
//! (`useBootstrapStore.lyrics`, `useSetlistStore`, `useLibraries`).

use std::sync::Arc;

use kairo_domain::collections::{LibrariesCommand, LibraryState, SetlistCommand, SetlistState};
use kairo_domain::lyrics::{LyricsOnlinePreview, LyricsOnlineResult, LyricsProvider, LyricsSong};
use kairo_domain::usage::SongUsageEntry;

/// Everything the page starts from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LyricsSnapshot {
    /// The library (`useBootstrapStore.lyrics`).
    pub songs: Vec<LyricsSong>,
    /// The song libraries.
    pub libraries: LibraryState,
    /// The setlists ("playlists" on screen).
    pub setlists: SetlistState,
}

/// A push from the core.
#[derive(Clone, Debug, PartialEq)]
pub enum LyricsFeed {
    /// The library as the core holds it (an import elsewhere, a `.kairo`
    /// file, a folder sync). Merged, never replacing edits held here.
    Songs(Vec<LyricsSong>),
    Libraries(LibraryState),
    Setlists(SetlistState),
}

/// A web hit to fetch or import (`{ provider, url, title, artist }`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OnlineSource {
    pub provider: LyricsProvider,
    pub url: String,
    pub title: String,
    pub artist: String,
}

impl OnlineSource {
    pub fn of(result: &LyricsOnlineResult) -> Self {
        Self {
            provider: result.provider,
            url: result.url.clone(),
            title: result.title.clone(),
            artist: result.artist.clone(),
        }
    }
}

/// What a `.kairo` export carries (`exportKairo`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KairoExport {
    Songs(Vec<String>),
    Setlist(String),
    Library,
}

/// One request. Each runs off the UI thread and answers once.
#[derive(Clone, Debug, PartialEq)]
pub enum LyricsCall {
    /// `lyrics.search` (full text); answers [`LyricsAnswer::Songs`].
    Search(String),
    /// `lyrics.searchOnline`; answers [`LyricsAnswer::Online`].
    SearchOnline(String),
    /// `lyrics.previewOnline`; answers [`LyricsAnswer::Preview`].
    PreviewOnline(OnlineSource),
    /// `lyrics.import({ type: 'online', ... })`; answers [`LyricsAnswer::Song`].
    ImportOnline(OnlineSource),
    /// `lyrics.toggleFavorite`; answers [`LyricsAnswer::Favorite`].
    ToggleFavorite(String),
    /// `lyrics.delete`, with the setlist and library clean-up.
    Delete(String),
    /// `setlist.command`; answers [`LyricsAnswer::Setlists`].
    Setlist(SetlistCommand),
    /// `libraries.command` for songs; answers [`LyricsAnswer::Libraries`].
    Libraries(LibrariesCommand),
    /// `songUsage.list(from, to)`; answers [`LyricsAnswer::Usage`].
    UsageList { from: f64, to: f64 },
    /// `songUsage.exportCsv(from, to)`: the save dialog and the file;
    /// answers [`LyricsAnswer::Saved`] (`None` when cancelled).
    UsageExport { from: f64, to: f64 },
    /// `exportKairo(...)`.
    ExportKairo(KairoExport),
    /// `importKairo()`.
    ImportKairo,
    /// `window.open(url)` for a web hit whose lyrics cannot be imported.
    OpenUrl(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum LyricsAnswer {
    Done,
    Songs(Vec<LyricsSong>),
    Online(Vec<LyricsOnlineResult>),
    Preview(LyricsOnlinePreview),
    Song(LyricsSong),
    Favorite(bool),
    Setlists(SetlistState),
    Libraries(LibraryState),
    Usage(Vec<SongUsageEntry>),
    Saved(Option<String>),
}

pub type Reply = Box<dyn FnOnce(Result<LyricsAnswer, String>) + Send>;
pub type FeedSink = Arc<dyn Fn(LyricsFeed) + Send + Sync>;
/// Dropping it stops the feed.
pub type Listening = Box<dyn Send>;

pub trait LyricsBackend: Send + Sync + 'static {
    fn snapshot(&self) -> LyricsSnapshot;
    /// Starts pushing to `sink` until the guard drops.
    fn listen(&self, sink: FeedSink) -> Listening;
    fn call(&self, call: LyricsCall, reply: Reply);
}
