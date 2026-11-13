//! Program sound (prompt 056): everything `<audio>`, `setSinkId` and Web
//! Audio did in Electron.
//!
//! - [`mixer`]: sources with their own speaker gain and the NDI taps.
//! - [`output`]: the output device chosen by label, the clock that drives
//!   the mixer, hot-plug.
//! - [`decode`] and [`clip`]: a file's sound, played by range, rate, loop,
//!   pause and seek, for video sources, tracks and the program fallback.
//! - [`tracks`]: the house player (play, pause, stop, seek, progress).
//! - [`routing`]: which surface makes sound, and when the fallback plays.
//! - [`block`]: the NDI block and its boundary checks.

pub mod block;
pub mod clip;
pub mod decode;
pub mod mixer;
pub mod output;
pub mod routing;
pub mod tap;
#[cfg(test)]
mod tests;
pub mod tracks;

pub use block::{AudioBlock, BLOCK_FRAMES, BlockError};
pub use clip::{ClipControl, ClipPlayer, ClipSettings, ClipStatus};
pub use mixer::{MixerControl, RATE, SourceControl, SourceInput, SourceKind, TapSwitch};
pub use output::{AudioOut, OutputDevice, list_output_devices};
pub use tracks::{FallbackLive, FallbackPlayer, TrackTime, TracksPlayer};
