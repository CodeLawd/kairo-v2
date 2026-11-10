//! The documents library, from `documents/index.ts`: imports under
//! `<userData>/documents/<id>`, one PNG per page and a `document.json`
//! written atomically. Rendering is here rather than in the window: PDFium
//! replaces the pdf.js loop that fed `savePage`. Every call blocks; run it
//! on a background thread.

use crate::DocumentError;
use crate::converters::{Detector, NEEDS_AN_APP, convert_power_point_to_pdf};
use crate::office_apps::OfficeProbes;
use crate::pdf::{MAX_PAGES, render_pages};
use crate::pptx_builtin::{SlidePrinter, convert_pptx_builtin};
use crate::pptx_package::read_pptx_videos;
use crate::slide_converter::SlideConverter;
use kairo_domain::documents::{
    DocumentFormat, DocumentImportInfo, DocumentNeedsConverter, DocumentPrepareOptions,
    DocumentPrepared, DocumentSlide, DocumentsCapabilities, POWERPOINT_NEEDS_CONVERTER,
    PowerpointConverterId, ProjectionDocument, SIMPLIFIED_IMPORT_WARNING, format_document_size,
    normalize_document_name, valid_document_page,
};
use kairo_domain::overlay::Size;
use regex::Regex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, LazyLock, Mutex};

const MB: u64 = 1024 * 1024;
const MAX_PAGE_PNG: usize = 40 * 1024 * 1024;
const MAX_PAGE_SIDE: u32 = 3840;

static ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-f0-9-]{36}$").unwrap());
static VIDEO_FILE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^[^/\\]+\.(mp4|m4v|webm|mov|ogv)$").unwrap());

type Result<T> = std::result::Result<T, DocumentError>;

fn message<T>(text: impl Into<String>) -> Result<T> {
    Err(DocumentError::Message(text.into()))
}

/// What `prepare` hands back.
#[derive(Clone, Debug, PartialEq)]
pub enum PrepareOutcome {
    /// Converted; render `data` with [`DocumentsService::render`].
    Prepared(DocumentPrepared),
    /// Nothing converts this deck accurately: offer the slide converter or,
    /// for .pptx, the simplified renderer, then call again with `resume`.
    NeedsConverter(DocumentNeedsConverter),
}

fn locale_compare(a: &str, b: &str) -> std::cmp::Ordering {
    thread_local! {
        static COLLATOR: std::cell::RefCell<feruca::Collator> = std::cell::RefCell::new(
            feruca::Collator::new(feruca::Tailoring::Cldr(feruca::Locale::Root), false, false),
        );
    }
    COLLATOR.with(|c| c.borrow_mut().collate(a, b))
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `document.json`, written as `JSON.stringify` would and replaced atomically.
fn write_atomic(dir: &Path, document: &ProjectionDocument) -> Result<()> {
    let json = kairo_domain::json::to_js_string(document)
        .map_err(|e| DocumentError::Message(e.to_string()))?;
    kairo_store::fs_atomic::write_atomic(&dir.join("document.json"), json.as_bytes())?;
    Ok(())
}

pub struct DocumentsService {
    root: PathBuf,
    probes: Arc<dyn OfficeProbes + Send + Sync>,
    slide_converter: Arc<SlideConverter>,
    printer: Arc<dyn SlidePrinter>,
    pending: Mutex<HashMap<String, ProjectionDocument>>,
    /// PowerPoints picked while no converter was ready, waiting for the
    /// operator's choice.
    waiting: Mutex<HashMap<String, PathBuf>>,
}

impl DocumentsService {
    pub fn new(
        user_data: &Path,
        probes: Arc<dyn OfficeProbes + Send + Sync>,
        slide_converter: Arc<SlideConverter>,
        printer: Arc<dyn SlidePrinter>,
    ) -> Arc<Self> {
        let service = Arc::new(Self {
            root: user_data.join("documents"),
            probes,
            slide_converter,
            printer,
            pending: Mutex::default(),
            waiting: Mutex::default(),
        });
        let weak = Arc::downgrade(&service);
        service
            .slide_converter
            .set_detect_others(move || weak.upgrade()?.detector().other_converter());
        service
    }

    fn detector(&self) -> Detector<'_> {
        Detector {
            probes: self.probes.as_ref(),
            downloaded_libre_office: self.slide_converter.binary(),
        }
    }

    fn directory(&self, id: &str) -> Result<PathBuf> {
        if !ID.is_match(id) {
            return message("Invalid document ID.");
        }
        Ok(self.root.join(id))
    }

    fn pending(&self) -> std::sync::MutexGuard<'_, HashMap<String, ProjectionDocument>> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn capabilities(&self) -> DocumentsCapabilities {
        let mut converters = self.detector().converters();
        let libre_office = converters.contains(&PowerpointConverterId::LibreOffice);
        converters.push(PowerpointConverterId::Builtin);
        DocumentsCapabilities {
            converters,
            can_convert_power_point: true,
            libre_office,
        }
    }

    fn read_document(&self, id: &str) -> Result<(PathBuf, ProjectionDocument)> {
        let dir = self.directory(id)?;
        let text = std::fs::read_to_string(dir.join("document.json"))?;
        let document =
            serde_json::from_str(&text).map_err(|e| DocumentError::Message(e.to_string()))?;
        Ok((dir, document))
    }

    fn check_videos(document: &ProjectionDocument) -> Result<()> {
        for video in document.videos.iter().flatten().flatten() {
            if !VIDEO_FILE.is_match(&video.file) {
                return message("Invalid document video filename.");
            }
        }
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<ProjectionDocument>> {
        std::fs::create_dir_all(&self.root)?;
        let mut documents = Vec::new();
        for entry in std::fs::read_dir(&self.root)? {
            let id = entry?.file_name().to_string_lossy().into_owned();
            // Incomplete imports are never listed.
            let Ok((dir, mut document)) = self.read_document(&id) else {
                continue;
            };
            let count = document.pages.len();
            if !(1..=MAX_PAGES).contains(&count) || Self::check_videos(&document).is_err() {
                continue;
            }
            document.id = id;
            document.pages = (0..count)
                .map(|i| dir.join(format!("{i}.png")).to_string_lossy().into_owned())
                .collect();
            documents.push(document);
        }
        documents.sort_by(|a, b| locale_compare(&a.name, &b.name));
        Ok(documents)
    }

    /// `picked` is the file the operator chose (`None`: the picker was
    /// cancelled). `kind` is `pdf`, `powerpoint` or absent. `on_preparing` is
    /// the `documents:preparing` push.
    pub fn prepare(
        &self,
        kind: Option<&str>,
        picked: Option<PathBuf>,
        options: &DocumentPrepareOptions,
        on_preparing: &mut dyn FnMut(DocumentImportInfo),
    ) -> Result<Option<PrepareOutcome>> {
        if kind.is_some_and(|k| k != "pdf" && k != "powerpoint") {
            return message("Unsupported document type.");
        }
        let source = match &options.resume {
            Some(token) => {
                let waiting = self
                    .waiting
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(token);
                match waiting {
                    Some(source) => source,
                    None => return message("Choose the file again."),
                }
            }
            None => match picked {
                Some(path) => path,
                None => return Ok(None),
            },
        };
        let extension = source
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let Some(format) = DocumentFormat::from_name(&extension) else {
            return message("Choose a PDF or PowerPoint file.");
        };
        let simplified = options.simplified == Some(true);
        if format != DocumentFormat::Pdf && !simplified {
            let darwin = self.probes.platform() == "darwin";
            let usable = self
                .detector()
                .converters()
                .into_iter()
                .filter(|id| *id != PowerpointConverterId::Wps || !darwin)
                .count();
            if usable == 0 {
                let token = uuid::Uuid::new_v4().to_string();
                self.waiting
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(token.clone(), source.clone());
                return Ok(Some(PrepareOutcome::NeedsConverter(
                    DocumentNeedsConverter {
                        needs_converter: Default::default(),
                        token,
                        name: file_stem(&source),
                        format,
                    },
                )));
            }
        }
        self.import_from(&source, format, on_preparing, simplified)
            .map(|p| Some(PrepareOutcome::Prepared(p)))
    }

    fn import_from(
        &self,
        source: &Path,
        format: DocumentFormat,
        on_preparing: &mut dyn FnMut(DocumentImportInfo),
        simplified: bool,
    ) -> Result<DocumentPrepared> {
        let max_mb = if format == DocumentFormat::Pptx {
            500
        } else {
            100
        };
        let size_bytes = std::fs::metadata(source)?.len();
        let name = file_stem(source);
        on_preparing(DocumentImportInfo {
            name: name.clone(),
            format,
            size_bytes: size_bytes as f64,
            max_size_bytes: (max_mb * MB) as f64,
        });
        if size_bytes > max_mb * MB {
            return message(format!(
                "This document is {}. The maximum for {} files is {max_mb} MB.",
                format_document_size(size_bytes as f64),
                format.as_str().to_uppercase()
            ));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let dir = self.directory(&id)?;
        std::fs::create_dir_all(&dir)?;
        let result = self.convert(source, format, simplified, &id, &name, &dir);
        if result.is_err() {
            let _ = std::fs::remove_dir_all(&dir);
        }
        result
    }

    fn convert(
        &self,
        source: &Path,
        format: DocumentFormat,
        simplified: bool,
        id: &str,
        name: &str,
        dir: &Path,
    ) -> Result<DocumentPrepared> {
        let copy = dir.join(format!("source.{}", format.as_str()));
        std::fs::copy(source, &copy)?;
        let mut document = ProjectionDocument {
            id: id.to_owned(),
            name: name.to_owned(),
            format,
            pages: Vec::new(),
            slide_size: None,
            videos: None,
            warnings: None,
        };
        match format {
            DocumentFormat::Pptx => {
                let embedded = read_pptx_videos(std::fs::read(&copy)?)?;
                document.slide_size = Some(Size {
                    width: embedded.width,
                    height: embedded.height,
                    extra: Default::default(),
                });
                document.videos = Some(embedded.slides);
                document.warnings = Some(embedded.warnings);
                let media = dir.join("media");
                std::fs::create_dir_all(&media)?;
                for (file, bytes) in embedded.media {
                    std::fs::write(media.join(file), bytes)?;
                }
            }
            DocumentFormat::Ppt => {
                document.warnings = Some(vec![
                    "Embedded video playback requires .pptx. Save this presentation as .pptx and import again to play its videos.".into(),
                ]);
            }
            DocumentFormat::Pdf => {}
        }
        let mut pdf = copy.clone();
        if format != DocumentFormat::Pdf {
            let dest = dir.join("source.pdf");
            let converted = if simplified {
                message("simplified")
            } else {
                convert_power_point_to_pdf(&self.detector(), &copy, &dest, dir)
            };
            let used = match converted {
                Ok(used) => used,
                Err(error) if format != DocumentFormat::Pptx => {
                    return Err(if error.to_string() == NEEDS_AN_APP {
                        DocumentError::Message(POWERPOINT_NEEDS_CONVERTER.into())
                    } else {
                        error
                    });
                }
                Err(_) => {
                    convert_pptx_builtin(&copy, &dest, dir, self.printer.as_ref())?;
                    let mut warnings = vec![SIMPLIFIED_IMPORT_WARNING.to_owned()];
                    warnings.extend(document.warnings.take().unwrap_or_default());
                    document.warnings = Some(warnings);
                    PowerpointConverterId::Builtin
                }
            };
            tracing::info!(
                converter = used.as_str(),
                format = format.as_str(),
                "[Documents] Converted PowerPoint"
            );
            pdf = dest;
        }
        if std::fs::metadata(&pdf)?.len() > 100 * MB {
            return message("Converted PDF exceeds 100 MB.");
        }
        let data = std::fs::read(&pdf)?;
        self.pending().insert(id.to_owned(), document);
        Ok(DocumentPrepared {
            id: id.to_owned(),
            data,
            name: name.to_owned(),
            format,
        })
    }

    /// Renders a prepared import's pages, in place of the window's pdf.js
    /// loop of `savePage` calls. `progress` gets pages done and the total.
    pub fn render(
        &self,
        id: &str,
        data: &[u8],
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<()> {
        let dir = self.directory(id)?;
        if !self.pending().get(id).is_some_and(|d| d.pages.is_empty()) {
            return message("Invalid import page.");
        }
        let pages = render_pages(data, &dir, cancel, progress)?;
        // A cancel during rendering removed the import.
        let mut pending = self.pending();
        let Some(document) = pending.get_mut(id) else {
            return Err(DocumentError::Cancelled);
        };
        document.pages = pages
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        Ok(())
    }

    /// One rendered page, as `documents:savePage` took it.
    pub fn save_page(&self, id: &str, page: f64, png: &[u8]) -> Result<()> {
        let dir = self.directory(id)?;
        let mut pending = self.pending();
        let Some(document) = pending.get_mut(id) else {
            return message("Invalid import page.");
        };
        if page != document.pages.len() as f64 || !valid_document_page(page, MAX_PAGES as f64) {
            return message("Invalid import page.");
        }
        if png.len() > MAX_PAGE_PNG {
            return message("Page image is too large.");
        }
        let image = match image::load_from_memory(png) {
            Ok(image) if image.width() > 0 && image.height() > 0 => image,
            _ => return message("Invalid page image."),
        };
        if image.width() > MAX_PAGE_SIDE || image.height() > MAX_PAGE_SIDE {
            return message("Invalid page image.");
        }
        let path = dir.join(format!("{}.png", page as usize));
        image
            .save_with_format(&path, image::ImageFormat::Png)
            .map_err(|e| DocumentError::Message(e.to_string()))?;
        document.pages.push(path.to_string_lossy().into_owned());
        Ok(())
    }

    pub fn finish(&self, id: &str) -> Result<Vec<ProjectionDocument>> {
        let document = match self.pending().get(id) {
            Some(d) if !d.pages.is_empty() => d.clone(),
            _ => return message("No pages were rendered."),
        };
        if document
            .videos
            .as_ref()
            .is_some_and(|v| v.len() != document.pages.len())
        {
            return message(
                "PowerPoint conversion changed the slide count. Export this deck again before importing.",
            );
        }
        write_atomic(&self.directory(id)?, &document)?;
        self.pending().remove(id);
        self.list()
    }

    pub fn cancel(&self, id: &str) -> Result<()> {
        if self.pending().remove(id).is_none() {
            return Ok(());
        }
        let _ = std::fs::remove_dir_all(self.directory(id)?);
        Ok(())
    }

    pub fn rename(&self, id: &str, name: &str) -> Result<Vec<ProjectionDocument>> {
        let next = normalize_document_name(name).map_err(|e| DocumentError::Message(e.into()))?;
        let dir = self.directory(id)?;
        let Ok((_, mut document)) = self.read_document(id) else {
            return message("Document not found.");
        };
        if document.name == next {
            return self.list();
        }
        document.name = next;
        document.id = id.to_owned();
        write_atomic(&dir, &document)?;
        self.list()
    }

    pub fn remove(&self, id: &str) -> Result<Vec<ProjectionDocument>> {
        if !self.list()?.iter().any(|d| d.id == id) {
            return message("Document not found.");
        }
        let _ = std::fs::remove_dir_all(self.directory(id)?);
        self.list()
    }

    /// One page's path, reading only this document's metadata.
    pub fn page(&self, id: &str, page: f64) -> Result<PathBuf> {
        let dir = self.directory(id)?;
        let Ok((_, document)) = self.read_document(id) else {
            return message("Document page not found.");
        };
        if !valid_document_page(page, document.pages.len() as f64) {
            return message("Document page not found.");
        }
        let path = dir.join(format!("{}.png", page as usize));
        std::fs::metadata(&path)?;
        Ok(path)
    }

    pub fn slide(&self, id: &str, page: f64) -> Result<DocumentSlide> {
        let path = self.page(id, page)?;
        let (dir, document) = self.read_document(id)?;
        Self::check_videos(&document)?;
        let mut videos = document
            .videos
            .as_ref()
            .and_then(|v| v.get(page as usize))
            .cloned()
            .unwrap_or_default();
        for video in &mut videos {
            let file = dir.join("media").join(&video.file);
            std::fs::metadata(&file)?;
            video.file = file.to_string_lossy().into_owned();
        }
        let (width, height) = match &document.slide_size {
            Some(size) => (size.width, size.height),
            None => image::image_dimensions(&path)
                .map(|(w, h)| (f64::from(w), f64::from(h)))
                .map_err(|e| DocumentError::Message(e.to_string()))?,
        };
        Ok(DocumentSlide {
            id: id.to_owned(),
            page,
            path: path.to_string_lossy().into_owned(),
            width,
            height,
            videos,
        })
    }
}
