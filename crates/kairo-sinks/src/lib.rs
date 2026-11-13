//! Output sinks the engine sends frames to (prompts 061 to 064).
//!
//! - [`ndi`]: the NDI 6 runtime, loaded at run time from the app folder.
//! - [`output`]: one NDI feed per output: sender retries, the repeat
//!   cadence, the transparent clear frame and program sound.
//! - [`syphon`]: Syphon servers on macOS (prompt 063).

pub mod ndi;
pub mod output;
pub mod syphon;

pub use output::{Make, NdiOutput, Status, Timing, Transmit};
