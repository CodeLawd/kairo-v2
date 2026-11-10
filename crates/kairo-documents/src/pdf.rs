//! PDF pages to PNG with PDFium, in place of pdf.js in `Documents.tsx`.
//!
//! Each page is scaled to fit 3840×2160 (`min(3840 / w, 2160 / h)` on its
//! point size, the canvas rounded up as pdf.js does) on white. PDFium calls
//! are serialized by the binding, so pages render one at a time while PNG
//! encoding runs on a bounded pool. A cancel stops before the next page.

use crate::DocumentError;
use pdfium_render::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock, mpsc};

pub const MAX_PAGES: usize = 500;
pub const MAX_WIDTH: f64 = 3840.0;
pub const MAX_HEIGHT: f64 = 2160.0;
pub const PDFIUM_DIR_ENV: &str = "PDFIUM_DYNAMIC_LIB_PATH";
pub const TOO_MANY_PAGES: &str =
    "Documents can contain up to 500 pages. Split this document into smaller files.";

static LIBRARY_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);
static PDFIUM: OnceLock<Result<Pdfium, String>> = OnceLock::new();

/// Where the PDFium library lives; checked first. Takes effect before the
/// first render in the process.
pub fn set_pdfium_library_dir(dir: impl Into<PathBuf>) {
    *LIBRARY_DIR.lock().unwrap_or_else(|e| e.into_inner()) = Some(dir.into());
}

fn candidate_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = LIBRARY_DIR
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
        .and_then(|p| p.parent().map(Path::to_path_buf))
    {
        dirs.push(exe_dir.join("../Frameworks"));
        dirs.push(exe_dir);
    }
    dirs
}

fn pdfium() -> Result<&'static Pdfium, DocumentError> {
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
        .map_err(|e| DocumentError::PdfiumUnavailable(e.clone()))
}

/// The canvas size pdf.js gives a page of `width`×`height` points.
pub fn page_pixels(width: f64, height: f64) -> (u32, u32) {
    let scale = (MAX_WIDTH / width).min(MAX_HEIGHT / height);
    (
        (width * scale).ceil() as u32,
        (height * scale).ceil() as u32,
    )
}

fn open<'a>(pdfium: &'a Pdfium, data: &'a [u8]) -> Result<PdfDocument<'a>, DocumentError> {
    pdfium
        .load_pdf_from_byte_slice(data, None)
        .map_err(|error| {
            DocumentError::Message(match error {
                PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError) => {
                    "Could not read the PDF: the document is password protected.".into()
                }
                other => format!("Could not read the PDF: {other}"),
            })
        })
}

/// How many pages `data` has, refusing more than 500.
pub fn page_count(data: &[u8]) -> Result<usize, DocumentError> {
    let pdfium = pdfium()?;
    let document = open(pdfium, data)?;
    let count = document.pages().len() as usize;
    if count > MAX_PAGES {
        return Err(DocumentError::Message(TOO_MANY_PAGES.into()));
    }
    Ok(count)
}

/// Renders every page of `data` to `dir/<index>.png`. `progress` gets the
/// number of pages finished and the total. Blocking.
pub fn render_pages(
    data: &[u8],
    dir: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<Vec<PathBuf>, DocumentError> {
    let pdfium = pdfium()?;
    let document = open(pdfium, data)?;
    let total = document.pages().len() as usize;
    if total > MAX_PAGES {
        return Err(DocumentError::Message(TOO_MANY_PAGES.into()));
    }
    if total == 0 {
        return Err(DocumentError::Message("No pages were rendered.".into()));
    }
    let workers = std::thread::available_parallelism()
        .map_or(2, |n| n.get())
        .clamp(1, 4);
    let (jobs, inbox) = mpsc::sync_channel::<(usize, image::RgbaImage)>(workers);
    let inbox = std::sync::Arc::new(Mutex::new(inbox));
    let (done, finished) = mpsc::channel::<Result<usize, DocumentError>>();
    let paths: Vec<PathBuf> = (0..total).map(|i| dir.join(format!("{i}.png"))).collect();
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let inbox = inbox.clone();
            let done = done.clone();
            let paths = &paths;
            scope.spawn(move || {
                loop {
                    let job = inbox.lock().unwrap_or_else(|e| e.into_inner()).recv();
                    let Ok((index, image)) = job else { return };
                    let result = image
                        .save_with_format(&paths[index], image::ImageFormat::Png)
                        .map(|()| index)
                        .map_err(|e| {
                            DocumentError::Message(format!(
                                "Could not save page {}: {e}",
                                index + 1
                            ))
                        });
                    if done.send(result).is_err() {
                        return;
                    }
                }
            });
        }
        drop(done);
        let mut completed = 0;
        let mut failure = None;
        let collect = |completed: &mut usize,
                       failure: &mut Option<DocumentError>,
                       progress: &mut dyn FnMut(usize, usize),
                       block: bool| {
            loop {
                let next = if block {
                    finished.recv().ok()
                } else {
                    finished.try_recv().ok()
                };
                match next {
                    Some(Ok(_)) => {
                        *completed += 1;
                        progress(*completed, total);
                    }
                    Some(Err(e)) => {
                        failure.get_or_insert(e);
                    }
                    None => return,
                }
            }
        };
        progress(0, total);
        for (index, page) in document.pages().iter().enumerate() {
            if cancel.load(Ordering::Acquire) {
                failure.get_or_insert(DocumentError::Cancelled);
                break;
            }
            let (w, h) = page_pixels(
                f64::from(page.width().value),
                f64::from(page.height().value),
            );
            let config = PdfRenderConfig::new()
                .set_target_size(w as i32, h as i32)
                .set_clear_color(PdfColor::WHITE)
                .render_form_data(true)
                .render_annotations(true);
            match page.render_with_config(&config) {
                Ok(bitmap) => match bitmap.as_image() {
                    Ok(image) => {
                        if jobs.send((index, image.into_rgba8())).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        failure.get_or_insert(DocumentError::Message(format!(
                            "Could not render page {}: {e}",
                            index + 1
                        )));
                        break;
                    }
                },
                Err(e) => {
                    failure.get_or_insert(DocumentError::Message(format!(
                        "Could not render page {}: {e}",
                        index + 1
                    )));
                    break;
                }
            }
            collect(&mut completed, &mut failure, progress, false);
            if failure.is_some() {
                break;
            }
        }
        drop(jobs);
        collect(&mut completed, &mut failure, progress, true);
        match failure {
            Some(e) => Err(e),
            None => Ok(paths.clone()),
        }
    })
}

/// Joins one-page PDFs into one document.
pub fn merge_pages(parts: &[Vec<u8>]) -> Result<Vec<u8>, DocumentError> {
    let pdfium = pdfium()?;
    let failed = |e: PdfiumError| {
        DocumentError::Message(format!("Could not render this PowerPoint file: {e}"))
    };
    let mut joined = pdfium.create_new_pdf().map_err(failed)?;
    for part in parts {
        let page = open(pdfium, part)?;
        joined.pages_mut().append(&page).map_err(failed)?;
    }
    joined.save_to_bytes().map_err(failed)
}

/// Each page's size in points.
pub fn page_sizes(data: &[u8]) -> Result<Vec<(f32, f32)>, DocumentError> {
    let pdfium = pdfium()?;
    let document = open(pdfium, data)?;
    Ok(document
        .pages()
        .iter()
        .map(|p| (p.width().value, p.height().value))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_fit_inside_4k() {
        assert_eq!(page_pixels(960.0, 540.0), (3840, 2160));
        assert_eq!(page_pixels(612.0, 792.0), (1670, 2160));
        assert_eq!(page_pixels(1440.0, 810.0), (3840, 2160));
        assert_eq!(page_pixels(800.0, 200.0), (3840, 960));
    }
}
