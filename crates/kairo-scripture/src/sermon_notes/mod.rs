//! Sermon notes: reading documents and finding the scripture references in
//! them. Ports `analyzeScriptureReferences` and `readSermonDocument` from
//! `src/main/services/scripture/sermon-plans.ts`, the text helpers in
//! `src/lib/sermon-notes-review.ts`, and the `scripture:importSermonNotes` /
//! `scripture:scanSermonNotes` handlers in `src/main/ipc/index.ts`.
//!
//! Match offsets are UTF-16 code units, as JavaScript string indexes are, so
//! the review editor can paint them on the same text.

mod analysis;
mod document;
mod docx;
mod import;
mod pdf;

pub use analysis::{analyze_scripture_references, extract_scripture_references};
pub use document::{
    ReadProgress, SermonDocumentContent, SermonDocumentError, UNSUPPORTED_DOCUMENT,
    read_sermon_document, read_sermon_document_with_progress,
};
pub use import::{
    ACCEPTED_EXTENSIONS, ImportProgress, SermonImportError, import_sermon_notes,
    import_sermon_notes_blocking, scan_sermon_notes,
};
pub use kairo_domain::scripture::notes_review::{
    SERMON_NOTES_BLOCK_SEPARATOR, escape_html, normalized_reference_label,
    plain_text_to_editor_html,
};
pub use pdf::{PDFIUM_DIR_ENV, set_pdfium_library_dir};
