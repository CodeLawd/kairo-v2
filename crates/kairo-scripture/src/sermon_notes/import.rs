//! `scripture:importSermonNotes` and `scripture:scanSermonNotes`. The file
//! dialog belongs to the app shell: it filters on [`ACCEPTED_EXTENSIONS`] and
//! passes the chosen path, or `None` when the user cancelled. The default
//! translation comes from `settings.scripture.defaultTranslation`.

use super::analysis::analyze_scripture_references;
use super::document::{ReadProgress, SermonDocumentError, read_sermon_document_with_progress};
use kairo_domain::scripture::{SermonNotesAnalysis, SermonPlanDraft};
use std::path::{Path, PathBuf};

/// The open dialog's "Sermon notes" filter.
pub const ACCEPTED_EXTENSIONS: [&str; 5] = ["docx", "pdf", "txt", "md", "rtf"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportProgress {
    Reading(ReadProgress),
    Scanning,
}

#[derive(Debug, thiserror::Error)]
pub enum SermonImportError {
    #[error(transparent)]
    Document(#[from] SermonDocumentError),
    #[error("The import stopped unexpectedly: {0}")]
    Task(String),
}

/// `path.basename(filePath, path.extname(filePath))`.
fn title_of(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Reads and scans on the calling thread.
pub fn import_sermon_notes_blocking(
    path: &Path,
    default_translation: &str,
    progress: &mut dyn FnMut(ImportProgress),
) -> Result<SermonPlanDraft, SermonImportError> {
    let content =
        read_sermon_document_with_progress(path, &mut |p| progress(ImportProgress::Reading(p)))?;
    progress(ImportProgress::Scanning);
    let analysis = analyze_scripture_references(&content.text, default_translation);
    Ok(SermonPlanDraft {
        title: title_of(path),
        source_file_name: file_name_of(path),
        text: content.text,
        html: content.html,
        matches: analysis.matches,
        items: analysis.items,
    })
}

/// Reads and scans on a blocking thread, so a large PDF never holds up the
/// caller's actor. `None` (a cancelled dialog) resolves to `None`.
pub async fn import_sermon_notes(
    path: Option<PathBuf>,
    default_translation: String,
    mut progress: impl FnMut(ImportProgress) + Send + 'static,
) -> Result<Option<SermonPlanDraft>, SermonImportError> {
    let Some(path) = path else { return Ok(None) };
    tokio::task::spawn_blocking(move || {
        import_sermon_notes_blocking(&path, &default_translation, &mut progress)
    })
    .await
    .map_err(|e| SermonImportError::Task(e.to_string()))?
    .map(Some)
}

/// Rescans text edited in the review editor.
pub fn scan_sermon_notes(text: &str, default_translation: &str) -> SermonNotesAnalysis {
    analyze_scripture_references(text, default_translation)
}
