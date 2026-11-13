//! Live cameras and capture cards (prompt 057): `__setCamera` without a
//! browser.
//!
//! - [`label`]: device labels as Chromium showed them, and matching saved
//!   labels back to devices.
//! - [`format`]: the capture format nearest `1920 × 1080 at 30` (ideal).
//! - [`backend`]: the capture system (AVFoundation on macOS) and its errors.
//! - [`cameras`]: cameras shared across outputs, opened off-thread, stale
//!   streams discarded, lost devices kept on their last frame and reopened.
//! - [`mic`]: the optional audio input played with the camera.

pub mod backend;
pub mod cameras;
pub mod format;
pub mod label;
pub mod mic;

#[cfg(target_os = "macos")]
mod avf;

#[cfg(test)]
mod tests;

pub use backend::{Backend, CameraError, Capture, FrameSink, Health, system};
pub use cameras::{CameraEvent, Cameras, Feed};
pub use format::{FormatInfo, Request};
pub use label::DeviceInfo;
