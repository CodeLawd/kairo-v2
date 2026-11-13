//! What a capture system provides: its cameras, and frames from one.

use crate::format::Request;
use crate::label::DeviceInfo;
use kairo_compositor::YuvFrame;
use std::fmt;

/// Why a camera is not showing. The operator saw one result for all of
/// these in Electron (`__setCamera` resolved `false`); the message says which.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CameraError {
    /// Capture is not allowed (`NotAllowedError`).
    Denied,
    /// No input has this label (`Camera not found: …`).
    NotFound(String),
    /// Another program holds the device (`NotReadableError`).
    Busy,
    /// The device went away.
    Unplugged,
    /// No capture system on this platform.
    Unsupported,
    Failed(String),
}

impl fmt::Display for CameraError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Denied => f.write_str("Camera access is not allowed"),
            Self::NotFound(label) => write!(f, "Camera not found: {label}"),
            Self::Busy => f.write_str("The camera is in use by another program"),
            Self::Unplugged => f.write_str("The camera was disconnected"),
            Self::Unsupported => f.write_str("Camera capture is not available on this system"),
            Self::Failed(m) => write!(f, "The camera could not start: {m}"),
        }
    }
}

impl std::error::Error for CameraError {}

/// How an open capture is doing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Health {
    #[default]
    Running,
    Unplugged,
    Busy,
}

/// Where frames go. Called on the capture system's own thread.
pub type FrameSink = Box<dyn Fn(YuvFrame) + Send + Sync>;

/// An open capture. Dropping it stops the device.
pub trait Capture: Send {
    fn health(&self) -> Health;
    /// Width, height and frame rate the device runs at.
    fn mode(&self) -> (u32, u32, f64);
}

/// A capture system.
pub trait Backend: Send + Sync + 'static {
    /// Video inputs, after asking for permission if it was never asked.
    fn video_devices(&self) -> Result<Vec<DeviceInfo>, CameraError>;
    /// Starts `device` in the format closest to `request`.
    fn open(
        &self,
        device: &DeviceInfo,
        request: Request,
        sink: FrameSink,
    ) -> Result<Box<dyn Capture>, CameraError>;
}

/// The platform's capture system.
pub fn system() -> std::sync::Arc<dyn Backend> {
    #[cfg(target_os = "macos")]
    {
        std::sync::Arc::new(crate::avf::Avf)
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::sync::Arc::new(Unsupported)
    }
}

/// Windows (Media Foundation) and Linux (V4L2) capture are not built yet
/// (DECISIONS 057): every request reports `Unsupported`, which the
/// operator sees as an error, and the screens keep what they showed.
#[cfg(not(target_os = "macos"))]
struct Unsupported;

#[cfg(not(target_os = "macos"))]
impl Backend for Unsupported {
    fn video_devices(&self) -> Result<Vec<DeviceInfo>, CameraError> {
        Err(CameraError::Unsupported)
    }

    fn open(
        &self,
        _: &DeviceInfo,
        _: Request,
        _: FrameSink,
    ) -> Result<Box<dyn Capture>, CameraError> {
        Err(CameraError::Unsupported)
    }
}
