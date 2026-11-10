//! PDF text through PDFium (`pdfium-render`), replacing `pdf-parse`.
//!
//! PDFium is a shared library loaded once per process. It is looked for in
//! the directory set by [`set_pdfium_library_dir`], then `PDFIUM_DYNAMIC_LIB_PATH`,
//! then next to the executable (and the macOS bundle's `Frameworks`), then
//! the system library path.

use super::document::{ReadProgress, SermonDocumentError};
use pdfium_render::prelude::*;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// Directory holding the PDFium shared library.
pub const PDFIUM_DIR_ENV: &str = "PDFIUM_DYNAMIC_LIB_PATH";

static CONFIGURED_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);
static PDFIUM: OnceLock<Result<Pdfium, String>> = OnceLock::new();

/// Where to find PDFium. Takes effect only before the first PDF is read.
pub fn set_pdfium_library_dir(dir: impl Into<PathBuf>) {
    *CONFIGURED_DIR.lock().unwrap_or_else(|e| e.into_inner()) = Some(dir.into());
}

fn candidate_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = CONFIGURED_DIR
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
    {
        dirs.push(dir);
    }
    if let Some(dir) = std::env::var_os(PDFIUM_DIR_ENV) {
        dirs.push(PathBuf::from(dir));
    }
    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(PathBuf::from))
    {
        dirs.push(exe_dir.join("../Frameworks"));
        dirs.push(exe_dir);
    }
    dirs
}

fn pdfium() -> Result<&'static Pdfium, SermonDocumentError> {
    PDFIUM
        .get_or_init(|| {
            let mut tried = Vec::new();
            for dir in candidate_dirs() {
                let path = Pdfium::pdfium_platform_library_name_at_path(&dir);
                if !path.is_file() {
                    continue;
                }
                match Pdfium::bind_to_library(&path) {
                    Ok(bindings) => return Ok(Pdfium::new(bindings)),
                    Err(error) => tried.push(format!("{}: {error}", path.display())),
                }
            }
            match Pdfium::bind_to_system_library() {
                Ok(bindings) => Ok(Pdfium::new(bindings)),
                Err(error) => {
                    tried.push(format!("system library: {error}"));
                    Err(tried.join("; "))
                }
            }
        })
        .as_ref()
        .map_err(|e| SermonDocumentError::PdfiumUnavailable(e.clone()))
}

/// Every page's text, each page preceded by a blank line as `pdf-parse`
/// joins them. PDFium ends lines with CRLF; they become LF like `pdf-parse`.
pub fn extract_text(
    bytes: &[u8],
    progress: &mut dyn FnMut(ReadProgress),
) -> Result<String, SermonDocumentError> {
    let pdfium = pdfium()?;
    let document = pdfium
        .load_pdf_from_byte_slice(bytes, None)
        .map_err(|e| SermonDocumentError::Pdf(describe(&e)))?;
    let pages = document.pages();
    let total = usize::try_from(pages.len()).unwrap_or(0);
    let mut text = String::new();
    for (index, page) in pages.iter().enumerate() {
        let page_text = page
            .text()
            .map_err(|e| SermonDocumentError::Pdf(describe(&e)))?
            .all();
        text.push_str("\n\n");
        text.push_str(&page_text.replace("\r\n", "\n").replace('\r', "\n"));
        progress(ReadProgress {
            done: index + 1,
            total,
        });
    }
    Ok(text)
}

fn describe(error: &PdfiumError) -> String {
    match error {
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError) => {
            "the document is password protected".into()
        }
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::FormatError) => {
            "the file is not a valid PDF".into()
        }
        other => other.to_string(),
    }
}
