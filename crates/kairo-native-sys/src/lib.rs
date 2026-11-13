//! C ABI ports of Electron's native addons, with thin safe wrappers.
//!
//! - [`syphon`] (macOS): Syphon servers, from `native/syphon/syphon.mm`.
//! - [`recorder`] (macOS): Live → Start Capture's movie writer (prompt 064).

#[cfg(target_os = "macos")]
pub mod recorder;
#[cfg(target_os = "macos")]
pub mod syphon;
