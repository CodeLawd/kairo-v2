//! The Lyrics workspace, library portion (prompt 080): the sources rail
//! (libraries, Favorites, playlists), the song list with search and the
//! web fallback, the web preview, delete, the song usage report and the ⌘F
//! song finder. The slide grid (081) and the editors (082) build on this.
//!
//! The page reaches the core only through [`LyricsBackend`]; the app
//! implements it over its services.

pub mod backend;
pub mod model;
pub mod quick_open;
pub mod rows;
pub mod view;

pub use backend::LyricsBackend;
pub use model::{Lyrics, LyricsEvent};
pub use quick_open::{QuickOpen, QuickOpenEvent};
pub use view::LyricsWorkspace;
