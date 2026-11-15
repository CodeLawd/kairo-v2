//! The media dock (prompt 083): the backgrounds library and its playlists
//! as a resizable sheet under the booth pages, with push, rename, copy,
//! cut, paste, reveal and delete, drag reorder, OS file drops, rubber-band
//! picks, private preview playback and the Media Inspector with its trim
//! timeline and colour sliders.
//!
//! The dock reaches the core only through [`MediaBackend`]; the app
//! implements it over its media service and actions.

pub mod backend;
pub mod inspector;
pub mod model;
pub mod player;
pub mod rows;
pub mod view;

pub use backend::MediaBackend;
pub use model::{MediaDock, MediaEvent};
pub use view::MediaDockView;
