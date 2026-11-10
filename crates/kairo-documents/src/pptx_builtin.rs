//! The simplified PowerPoint renderer from `pptx-builtin.ts`: the deck becomes
//! one HTML page of slides, which a hidden web view prints to PDF.

use crate::DocumentError;
use crate::pdf::MAX_PAGES;
use crate::pptx_package::{Package, pptx_slide_paths, related_part};
use crate::pptx_slides::{
    Colors, Inherited, SLIDE_W, parse_slide, slide_html, slide_size, theme_colors,
};
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Electron's `printToPDF` page width: 508000 µm, 20 inches.
const PAGE_WIDTH_MICRONS: f64 = 508_000.0;

static CLR_MAP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<p:clrMap\b[^>]*>").unwrap());
static CLR_MAP_ENTRY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\b(bg[12]|tx[12]|accent[1-6]|hlink|folHlink)="([^"]+)""#).unwrap()
});

fn read_if_exists(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn rels_of(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.parent()
        .unwrap_or(path)
        .join("_rels")
        .join(format!("{name}.rels"))
}

/// The deck as printable HTML, ready for a web view.
#[derive(Clone, Debug)]
pub struct SlidesHtml {
    /// `slides.html` inside the work folder.
    pub path: PathBuf,
    /// The folder the page may read images from.
    pub read_access: PathBuf,
    pub slides: usize,
    /// One slide in CSS pixels, 1920 wide.
    pub slide_width: f64,
    pub slide_height: f64,
    /// One PDF page in points, as Electron's `printToPDF` sized it.
    pub page_width_pt: f64,
    pub page_height_pt: f64,
}

/// Extracts `source` into `work_dir/pptx` and writes `work_dir/slides.html`.
pub fn build_slides_html(source: &Path, work_dir: &Path) -> Result<SlidesHtml, DocumentError> {
    let extract = work_dir.join("pptx");
    let opened = std::fs::read(source)
        .map_err(DocumentError::from)
        .and_then(Package::open)
        .and_then(|mut package| package.extract_static(&extract));
    if opened.is_err() {
        return Err(DocumentError::Message(
            "This PowerPoint file could not be opened. Save it as .pptx or export a PDF and import that instead.".into(),
        ));
    }
    let ppt = extract.join("ppt");
    let slides_dir = ppt.join("slides");
    let names = std::fs::read_to_string(ppt.join("presentation.xml"))
        .and_then(|xml| Ok((xml, std::fs::read_to_string(ppt.join("_rels/presentation.xml.rels"))?)))
        .map_err(DocumentError::from)
        .and_then(|(xml, rels)| pptx_slide_paths(&xml, &rels))
        .map_err(|_| {
            DocumentError::Message(
                "This older .ppt file needs PowerPoint, WPS, Keynote, or LibreOffice. Save it as .pptx or PDF and import that instead.".into(),
            )
        })?
        .into_iter()
        .map(|path| path.rsplit('/').next().unwrap_or_default().to_owned())
        .collect::<Vec<_>>();
    if names.is_empty() || names.len() > MAX_PAGES {
        return Err(DocumentError::Message(crate::pdf::TOO_MANY_PAGES.into()));
    }
    let size = slide_size(&read_if_exists(&ppt.join("presentation.xml")));
    let colors = theme_colors(&read_if_exists(&ppt.join("theme/theme1.xml")));
    let media_dir = ppt.join("media");
    let mut slides = Vec::with_capacity(names.len());
    for name in &names {
        let slide_path = slides_dir.join(name);
        let xml = std::fs::read_to_string(&slide_path)?;
        let rels = read_if_exists(&slides_dir.join("_rels").join(format!("{name}.rels")));
        let layout_path = related_part(&extract, &slide_path, "slideLayout")?;
        let master_path = match &layout_path {
            Some(layout) => related_part(&extract, layout, "slideMaster")?,
            None => None,
        };
        let theme_path = match &master_path {
            Some(master) => related_part(&extract, master, "theme")?,
            None => None,
        };
        let master = master_path
            .as_deref()
            .map(read_if_exists)
            .unwrap_or_default();
        let layout = layout_path
            .as_deref()
            .map(read_if_exists)
            .unwrap_or_default();
        let mut slide_colors: Colors = match &theme_path {
            Some(theme) => theme_colors(&read_if_exists(theme)),
            None => colors.clone(),
        };
        let mapping = CLR_MAP.find(&master).map_or("", |m| m.as_str());
        let snapshot = slide_colors.clone();
        for entry in CLR_MAP_ENTRY.captures_iter(mapping) {
            if let Some(color) = snapshot.get(&entry[2]).filter(|c| !c.is_empty()) {
                slide_colors.insert(entry[1].to_owned(), color.clone());
            }
        }
        let inherited = Inherited {
            layout,
            master,
            layout_rels: layout_path
                .as_deref()
                .map(|p| read_if_exists(&rels_of(p)))
                .unwrap_or_default(),
            master_rels: master_path
                .as_deref()
                .map(|p| read_if_exists(&rels_of(p)))
                .unwrap_or_default(),
        };
        slides.push(parse_slide(&xml, &rels, size, &slide_colors, &inherited));
    }
    let height = (SLIDE_W * size.cy / size.cx).round();
    let href = |file: &str| {
        url::Url::from_file_path(media_dir.join(file))
            .map_or_else(|()| String::new(), |u| u.to_string())
    };
    let html = slide_html(&slides, &href, &colors, size);
    let path = work_dir.join("slides.html");
    std::fs::write(&path, html)?;
    let page_height_microns = (height * PAGE_WIDTH_MICRONS / SLIDE_W).round();
    Ok(SlidesHtml {
        path,
        read_access: work_dir.to_path_buf(),
        slides: slides.len(),
        slide_width: SLIDE_W,
        slide_height: height,
        page_width_pt: PAGE_WIDTH_MICRONS * 72.0 / 25_400.0,
        page_height_pt: page_height_microns * 72.0 / 25_400.0,
    })
}

/// Prints `slides.html` to PDF bytes: one page per slide, every image loaded
/// first. The platform web view implements it.
pub trait SlidePrinter: Send + Sync {
    fn print(&self, html: &SlidesHtml) -> Result<Vec<u8>, DocumentError>;
}

/// The built-in conversion: `source` to `dest_pdf` through `printer`.
pub fn convert_pptx_builtin(
    source: &Path,
    dest_pdf: &Path,
    work_dir: &Path,
    printer: &dyn SlidePrinter,
) -> Result<(), DocumentError> {
    let html = build_slides_html(source, work_dir)?;
    let pdf = printer.print(&html)?;
    if !pdf.starts_with(b"%PDF-") {
        return Err(DocumentError::Message(
            "Could not render this PowerPoint file.".into(),
        ));
    }
    std::fs::write(dest_pdf, pdf)?;
    Ok(())
}
