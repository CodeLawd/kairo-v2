//! PDF and PowerPoint import: documents become page images (prompt 045).

pub mod converters;
pub mod office_apps;
pub mod pdf;
pub mod pptx_builtin;
pub mod pptx_package;
pub mod pptx_slides;
mod process;
pub mod service;
pub mod slide_converter;
#[cfg(target_os = "macos")]
pub mod webkit;

/// A documents failure; the message is what the import screen shows.
#[derive(Debug, thiserror::Error)]
pub enum DocumentError {
    #[error("{0}")]
    Message(String),
    #[error("The import was cancelled.")]
    Cancelled,
    #[error("PDF rendering is unavailable: {0}")]
    PdfiumUnavailable(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}
