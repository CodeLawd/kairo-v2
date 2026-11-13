//! C ABI ports of Electron's native addons, with thin safe wrappers.
//!
//! - [`syphon`] (macOS): Syphon servers, from `native/syphon/syphon.mm`.

#[cfg(target_os = "macos")]
pub mod syphon;
