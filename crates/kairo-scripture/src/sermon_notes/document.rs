//! `readSermonDocument`: text for reference scanning and HTML for the review
//! editor. DOCX keeps its formatting (replacing `mammoth`); PDF text comes
//! from PDFium (replacing `pdf-parse`); TXT, MD and RTF are read as UTF-8,
//! with RTF markup reduced to plain text.

use super::{docx, pdf};
use kairo_domain::lyrics::rtf::{looks_like_rtf, rtf_to_plain_text};
use kairo_domain::scripture::notes_review::plain_text_to_editor_html;
use std::path::Path;

/// The TypeScript's message for any other extension.
pub const UNSUPPORTED_DOCUMENT: &str =
    "Unsupported document. Choose a DOCX, PDF, TXT, MD, or RTF file.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SermonDocumentContent {
    pub text: String,
    pub html: String,
}

#[derive(Debug, thiserror::Error)]
pub enum SermonDocumentError {
    #[error("{UNSUPPORTED_DOCUMENT}")]
    Unsupported,
    #[error("Could not read {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("Could not read the Word document: {0}")]
    Docx(String),
    #[error("Could not read the PDF: {0}")]
    Pdf(String),
    #[error("PDF support is unavailable: {0}")]
    PdfiumUnavailable(String),
}

/// Pages read so far, for large PDFs. Other formats report one step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadProgress {
    pub done: usize,
    pub total: usize,
}

pub fn read_sermon_document(path: &Path) -> Result<SermonDocumentContent, SermonDocumentError> {
    read_sermon_document_with_progress(path, &mut |_| {})
}

/// Blocking. Run it off the async runtime (see `import_sermon_notes`).
pub fn read_sermon_document_with_progress(
    path: &Path,
    progress: &mut dyn FnMut(ReadProgress),
) -> Result<SermonDocumentContent, SermonDocumentError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let io = |source| SermonDocumentError::Io {
        path: path.display().to_string(),
        source,
    };
    // `path.extname` treats a leading dot as part of the name: `.docx` has no
    // extension. `Path::extension` agrees.
    match ext.as_str() {
        "docx" => {
            let bytes = std::fs::read(path).map_err(io)?;
            let doc = docx::read(&bytes).map_err(SermonDocumentError::Docx)?;
            progress(ReadProgress { done: 1, total: 1 });
            let html = if doc.html.is_empty() {
                plain_text_to_editor_html(&doc.text)
            } else {
                doc.html
            };
            Ok(SermonDocumentContent {
                text: doc.text,
                html,
            })
        }
        "pdf" => {
            let bytes = std::fs::read(path).map_err(io)?;
            let text = pdf::extract_text(&bytes, progress)?;
            let html = plain_text_to_editor_html(&text);
            Ok(SermonDocumentContent { text, html })
        }
        "txt" | "md" | "rtf" => {
            let bytes = std::fs::read(path).map_err(io)?;
            // Node's utf8 decoding: invalid sequences become U+FFFD, a BOM stays.
            let raw = String::from_utf8_lossy(&bytes);
            // TextEdit and WordPad save RTF markup; scanning it raw leaves `\'96`
            // escapes between verse numbers, which cuts every range short.
            let text = if ext == "rtf" || looks_like_rtf(&raw) {
                rtf_to_plain_text(&raw)
            } else {
                raw.into_owned()
            };
            progress(ReadProgress { done: 1, total: 1 });
            let html = plain_text_to_editor_html(&text);
            Ok(SermonDocumentContent { text, html })
        }
        _ => Err(SermonDocumentError::Unsupported),
    }
}
