//! The documents library end to end, with no office apps installed and a
//! printer that stands in for the web view.

mod support;

use kairo_documents::DocumentError;
use kairo_documents::office_apps::OfficeProbes;
use kairo_documents::pptx_builtin::{SlidePrinter, SlidesHtml};
use kairo_documents::service::{DocumentsService, PrepareOutcome};
use kairo_documents::slide_converter::SlideConverter;
use kairo_domain::documents::{
    DocumentFormat, DocumentPrepareOptions, PowerpointConverterId, SIMPLIFIED_IMPORT_WARNING,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use support::deck::{Slide, png, pptx};
use support::pdf::{pdf, require_pdfium};

/// A machine with no office apps.
struct NoApps;

impl OfficeProbes for NoApps {
    fn platform(&self) -> &str {
        "linux"
    }
    fn read_dir(&self, _: &Path) -> std::io::Result<Vec<String>> {
        Err(std::io::ErrorKind::NotFound.into())
    }
    fn read_file(&self, _: &Path) -> std::io::Result<String> {
        Err(std::io::ErrorKind::NotFound.into())
    }
    fn exists(&self, _: &Path) -> bool {
        false
    }
    fn home(&self) -> PathBuf {
        PathBuf::from("/nonexistent")
    }
    fn env(&self, _: &str) -> Option<String> {
        None
    }
    fn mdfind(&self, _: &str) -> Option<String> {
        None
    }
    fn command_on_path(&self, _: &str) -> Option<String> {
        None
    }
}

/// Prints one 1440×810 page per slide.
struct FakePrinter;

impl SlidePrinter for FakePrinter {
    fn print(&self, html: &SlidesHtml) -> Result<Vec<u8>, DocumentError> {
        assert!(html.path.is_file());
        Ok(pdf(&vec![(1440, 810); html.slides]))
    }
}

fn service(user_data: &Path) -> Arc<DocumentsService> {
    let converter = SlideConverter::with_build(user_data, reqwest::Client::new(), None);
    DocumentsService::new(
        user_data,
        Arc::new(NoApps),
        converter,
        Arc::new(FakePrinter),
    )
}

fn prepared(outcome: Option<PrepareOutcome>) -> kairo_domain::documents::DocumentPrepared {
    match outcome {
        Some(PrepareOutcome::Prepared(p)) => p,
        other => panic!("expected a prepared import, got {other:?}"),
    }
}

#[test]
fn pdf_import_lists_pages_and_renames() {
    if !require_pdfium() {
        return;
    }
    let user = tempfile::tempdir().unwrap();
    let docs = service(user.path());
    assert_eq!(
        docs.capabilities().converters,
        [PowerpointConverterId::Builtin]
    );
    assert!(docs.list().unwrap().is_empty());
    let source = user.path().join("Sunday Notes.PDF");
    std::fs::write(&source, pdf(&[(612, 792), (960, 540)])).unwrap();
    let mut preparing = Vec::new();
    let p = prepared(
        docs.prepare(
            Some("pdf"),
            Some(source),
            &DocumentPrepareOptions::default(),
            &mut |info| preparing.push(info),
        )
        .unwrap(),
    );
    assert_eq!(
        (p.name.as_str(), p.format),
        ("Sunday Notes", DocumentFormat::Pdf)
    );
    assert_eq!(preparing[0].max_size_bytes, 100.0 * 1024.0 * 1024.0);
    // Not listed until finished.
    assert!(docs.list().unwrap().is_empty());
    let mut progress = Vec::new();
    docs.render(&p.id, &p.data, &AtomicBool::new(false), &mut |d, t| {
        progress.push((d, t))
    })
    .unwrap();
    assert_eq!(progress.last(), Some(&(2, 2)));
    let list = docs.finish(&p.id).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].pages.len(), 2);
    let dir = user.path().join("documents").join(&p.id);
    assert!(dir.join("document.json").is_file() && !dir.join("document.json.tmp").exists());
    let slide = docs.slide(&p.id, 1.0).unwrap();
    assert_eq!((slide.width, slide.height), (3840.0, 2160.0));
    assert_eq!(docs.page(&p.id, 0.0).unwrap(), dir.join("0.png"));
    for bad in [2.0, -1.0, 0.5] {
        assert_eq!(
            docs.page(&p.id, bad).unwrap_err().to_string(),
            "Document page not found."
        );
    }
    let renamed = docs.rename(&p.id, "  Morning   Service\u{7} ").unwrap();
    assert_eq!(renamed[0].name, "Morning Service");
    assert_eq!(
        docs.rename(&p.id, "a/b").unwrap_err().to_string(),
        "Document names cannot include slashes."
    );
    assert!(docs.remove(&p.id).unwrap().is_empty());
    assert_eq!(
        docs.remove(&p.id).unwrap_err().to_string(),
        "Document not found."
    );
    assert_eq!(
        docs.rename(&p.id, "x").unwrap_err().to_string(),
        "Document not found."
    );
}

#[test]
fn powerpoint_without_a_converter_waits_then_takes_the_simplified_renderer() {
    if !require_pdfium() {
        return;
    }
    let user = tempfile::tempdir().unwrap();
    let docs = service(user.path());
    let source = user.path().join("Deck.pptx");
    let slides = [
        Slide {
            fill: "FF0000",
            picture: Some(png([0, 0, 255])),
        },
        Slide {
            fill: "00FF00",
            picture: None,
        },
    ];
    std::fs::write(&source, pptx(&slides, 12_192_000, 6_858_000)).unwrap();
    let options = DocumentPrepareOptions::default();
    let Some(PrepareOutcome::NeedsConverter(waiting)) = docs
        .prepare(None, Some(source.clone()), &options, &mut |_| {})
        .unwrap()
    else {
        panic!("expected needsConverter");
    };
    assert_eq!(
        (waiting.name.as_str(), waiting.format),
        ("Deck", DocumentFormat::Pptx)
    );
    let resume = DocumentPrepareOptions {
        resume: Some(waiting.token.clone()),
        simplified: Some(true),
    };
    let p = prepared(docs.prepare(None, None, &resume, &mut |_| {}).unwrap());
    // A token works once.
    assert_eq!(
        docs.prepare(None, None, &resume, &mut |_| {})
            .unwrap_err()
            .to_string(),
        "Choose the file again."
    );
    docs.render(&p.id, &p.data, &AtomicBool::new(false), &mut |_, _| {})
        .unwrap();
    let list = docs.finish(&p.id).unwrap();
    assert_eq!(
        list[0].warnings.as_deref(),
        Some(&[SIMPLIFIED_IMPORT_WARNING.to_owned()][..])
    );
    assert_eq!(list[0].videos.as_ref().map(Vec::len), Some(2));
    let slide = docs.slide(&p.id, 0.0).unwrap();
    // The deck's size in EMU, as `readPptxVideos` reports it.
    assert_eq!((slide.width, slide.height), (12_192_000.0, 6_858_000.0));
}

#[test]
fn rejected_imports_leave_nothing_behind() {
    let user = tempfile::tempdir().unwrap();
    let docs = service(user.path());
    let none = DocumentPrepareOptions::default();
    let err = |r: Result<Option<PrepareOutcome>, DocumentError>| r.unwrap_err().to_string();
    assert_eq!(
        err(docs.prepare(Some("doc"), None, &none, &mut |_| {})),
        "Unsupported document type."
    );
    assert_eq!(
        docs.prepare(Some("pdf"), None, &none, &mut |_| {}).unwrap(),
        None
    );
    let text = user.path().join("notes.txt");
    std::fs::write(&text, "x").unwrap();
    assert_eq!(
        err(docs.prepare(None, Some(text), &none, &mut |_| {})),
        "Choose a PDF or PowerPoint file."
    );
    let resume = DocumentPrepareOptions {
        resume: Some("nope".into()),
        simplified: None,
    };
    assert_eq!(
        err(docs.prepare(None, None, &resume, &mut |_| {})),
        "Choose the file again."
    );

    // A legacy .ppt with nothing to convert it.
    let ppt = user.path().join("Old.ppt");
    std::fs::write(&ppt, "x").unwrap();
    let Some(PrepareOutcome::NeedsConverter(waiting)) =
        docs.prepare(None, Some(ppt), &none, &mut |_| {}).unwrap()
    else {
        panic!("expected needsConverter");
    };
    // Resuming with still nothing installed asks again; the simplified
    // renderer reads .pptx only.
    let resume = DocumentPrepareOptions {
        resume: Some(waiting.token),
        simplified: None,
    };
    assert!(matches!(
        docs.prepare(None, None, &resume, &mut |_| {}).unwrap(),
        Some(PrepareOutcome::NeedsConverter(_))
    ));

    // Over the PDF limit: a sparse file reports its size.
    let big = user.path().join("big.pdf");
    std::fs::File::create(&big)
        .unwrap()
        .set_len(100 * 1024 * 1024 + 1)
        .unwrap();
    assert_eq!(
        err(docs.prepare(None, Some(big), &none, &mut |_| {})),
        "This document is 100.0 MB. The maximum for PDF files is 100 MB."
    );
    // A damaged .pptx fails before conversion.
    let broken = user.path().join("broken.pptx");
    std::fs::write(&broken, "not a zip").unwrap();
    let simplified = DocumentPrepareOptions {
        resume: None,
        simplified: Some(true),
    };
    assert!(
        docs.prepare(None, Some(broken), &simplified, &mut |_| {})
            .is_err()
    );
    let left: Vec<_> = std::fs::read_dir(user.path().join("documents"))
        .unwrap()
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

#[test]
fn pages_cancel_and_finish_rules() {
    if !require_pdfium() {
        return;
    }
    let user = tempfile::tempdir().unwrap();
    let docs = service(user.path());
    let source = user.path().join("a.pdf");
    std::fs::write(&source, pdf(&[(100, 100)])).unwrap();
    let p = prepared(
        docs.prepare(
            None,
            Some(source.clone()),
            &DocumentPrepareOptions::default(),
            &mut |_| {},
        )
        .unwrap(),
    );
    assert_eq!(
        docs.finish(&p.id).unwrap_err().to_string(),
        "No pages were rendered."
    );
    assert_eq!(
        docs.save_page(&p.id, 1.0, &png([1, 2, 3]))
            .unwrap_err()
            .to_string(),
        "Invalid import page."
    );
    assert_eq!(
        docs.save_page(&p.id, 0.0, b"junk").unwrap_err().to_string(),
        "Invalid page image."
    );
    let wide = {
        let mut out = std::io::Cursor::new(Vec::new());
        image::RgbImage::new(3841, 1)
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    };
    assert_eq!(
        docs.save_page(&p.id, 0.0, &wide).unwrap_err().to_string(),
        "Invalid page image."
    );
    docs.save_page(&p.id, 0.0, &png([1, 2, 3])).unwrap();
    assert_eq!(docs.finish(&p.id).unwrap().len(), 1);

    let p = prepared(
        docs.prepare(
            None,
            Some(source),
            &DocumentPrepareOptions::default(),
            &mut |_| {},
        )
        .unwrap(),
    );
    let dir = user.path().join("documents").join(&p.id);
    let cancelled = docs.render(&p.id, &p.data, &AtomicBool::new(true), &mut |_, _| {});
    assert!(matches!(cancelled, Err(DocumentError::Cancelled)));
    docs.cancel(&p.id).unwrap();
    assert!(!dir.exists());
    docs.cancel(&p.id).unwrap();
    assert_eq!(
        docs.page("../etc", 0.0).unwrap_err().to_string(),
        "Invalid document ID."
    );
    // Unfinished or malformed folders are never listed.
    std::fs::create_dir_all(user.path().join("documents/not-an-id")).unwrap();
    let stray = user
        .path()
        .join("documents/00000000-0000-0000-0000-000000000000");
    std::fs::create_dir_all(&stray).unwrap();
    std::fs::write(
        stray.join("document.json"),
        r#"{"id":"x","name":"n","format":"pdf","pages":[]}"#,
    )
    .unwrap();
    assert_eq!(docs.list().unwrap().len(), 1);
}

#[test]
fn imagine_decks_become_documents_page_by_page() {
    let dir = tempfile::tempdir().unwrap();
    let svc = service(dir.path());
    let png = |shade: u8| {
        let mut out = Vec::new();
        image::RgbaImage::from_pixel(32, 18, image::Rgba([shade, 0, 0, 255]))
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    };
    let list = svc
        .create_from_images("AI for media", &[png(10), png(20), png(30)])
        .unwrap();
    let doc = list.iter().find(|d| d.name == "AI for media").unwrap();
    assert_eq!(doc.pages.len(), 3);
    assert!(svc.page(&doc.id, 2.).is_ok());

    // A bad page leaves no half-made document.
    assert!(
        svc.create_from_images("Broken", &[png(1), b"not a png".to_vec()])
            .is_err()
    );
    assert!(svc.create_from_images("Empty", &[]).is_err());
    assert_eq!(svc.list().unwrap().len(), 1);
}
