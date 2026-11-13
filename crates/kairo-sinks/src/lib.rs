//! Output sinks the engine sends frames to (prompts 061 to 064).
//!
//! - [`ndi`]: the NDI 6 runtime, loaded at run time from the app folder.
//! - [`output`]: one NDI feed per output: sender retries, the repeat
//!   cadence, the transparent clear frame and program sound.
//! - [`syphon`]: Syphon servers on macOS (prompt 063).
//! - [`video_devices`]: the video card service (prompt 062).
//! - [`decklink`]: DeckLink cards through the out-of-process helper (prompt 062).

#[cfg(unix)]
pub mod decklink;
pub mod ndi;
pub mod output;
pub mod syphon;
pub mod video_devices;

pub use output::{Make, NdiOutput, Status, Timing, Transmit};
