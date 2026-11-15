//! Output and display configuration (prompt 075).
mod arrangement;
pub mod backend;
mod fields;
pub mod model;
mod window;
pub use backend::{Command, Reply, ScreensBackend, Snapshot};
pub use window::{FRAME, FRAME_MIN, ScreensEvent, ScreensWindow};
