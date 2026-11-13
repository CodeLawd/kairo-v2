//! Native code that is not a Rust crate: macOS frameworks reached through a
//! C ABI compiled by `build.rs`.
//!
//! - [`recorder`]: Live → Start Capture's movie writer (prompt 064).

#[cfg(target_os = "macos")]
pub mod recorder;
