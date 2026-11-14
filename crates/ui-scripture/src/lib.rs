//! The Scripture workspace (prompt 078): verse search with live suggestions,
//! the virtualised verse grid with keyboard navigation, playlists, the live
//! plan, the saved-passage library and the queue dock.
//!
//! The page reaches the core only through [`ScriptureBackend`]; the app
//! implements it over its services.

pub mod backend;
pub mod card;
pub mod model;
pub mod review;
pub mod rows;
pub mod view;

pub use backend::ScriptureBackend;
pub use model::{Scripture, ScriptureEvent};
pub use view::ScriptureWorkspace;
