//! PDFium page rendering against the development library.

mod support;

use kairo_documents::DocumentError;
use kairo_documents::pdf::{TOO_MANY_PAGES, page_count, render_pages};
use std::sync::atomic::AtomicBool;
use support::pdf::{pdf, require_pdfium};

#[test]
fn renders_each_page_inside_4k_on_white() {
    if !require_pdfium() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let data = pdf(&[(960, 540), (612, 792)]);
    assert_eq!(page_count(&data).unwrap(), 2);
    let mut seen = Vec::new();
    let paths = render_pages(
        &data,
        dir.path(),
        &AtomicBool::new(false),
        &mut |done, total| seen.push((done, total)),
    )
    .unwrap();
    assert_eq!(seen.first(), Some(&(0, 2)));
    assert_eq!(seen.last(), Some(&(2, 2)));
    let first = image::open(&paths[0]).unwrap().into_rgba8();
    assert_eq!(first.dimensions(), (3840, 2160));
    // 100pt at 4× is the red square in the bottom-left; the rest is white.
    assert_eq!(first.get_pixel(10, 2150).0, [255, 0, 0, 255]);
    assert_eq!(first.get_pixel(3000, 100).0, [255, 255, 255, 255]);
    let second = image::open(&paths[1]).unwrap();
    assert_eq!((second.width(), second.height()), (1670, 2160));
}

#[test]
fn refuses_more_than_500_pages_and_honours_cancel() {
    if !require_pdfium() {
        return;
    }
    let many = pdf(&vec![(100, 100); 501]);
    assert_eq!(page_count(&many).unwrap_err().to_string(), TOO_MANY_PAGES);
    let dir = tempfile::tempdir().unwrap();
    let result = render_pages(
        &pdf(&[(100, 100)]),
        dir.path(),
        &AtomicBool::new(true),
        &mut |_, _| {},
    );
    assert!(matches!(result, Err(DocumentError::Cancelled)));
    assert!(
        page_count(b"not a pdf")
            .unwrap_err()
            .to_string()
            .starts_with("Could not read the PDF")
    );
}

/// Timing for PROGRESS.md: `cargo test --release -p kairo-documents --test
/// pdf_render -- --ignored --nocapture`. Writes the PDF it times to
/// `KAIRO_MEASURE_PDF` when set, so pdf.js can be timed on the same file.
#[test]
#[ignore]
fn measure_twenty_text_pages() {
    assert!(require_pdfium(), "PDFium is needed to measure");
    let one = support::pdf_corpus::pdfs().remove(0).1;
    let pdfium_doc = {
        // Twenty copies of the text page in one document.
        let parts = vec![one; 20];
        kairo_documents::pdf::merge_pages(&parts).unwrap()
    };
    if let Some(path) = std::env::var_os("KAIRO_MEASURE_PDF") {
        std::fs::write(path, &pdfium_doc).unwrap();
    }
    for run in 0..3 {
        let dir = tempfile::tempdir().unwrap();
        let started = std::time::Instant::now();
        let pages = render_pages(
            &pdfium_doc,
            dir.path(),
            &AtomicBool::new(false),
            &mut |_, _| {},
        )
        .unwrap();
        assert_eq!(pages.len(), 20);
        assert_eq!(image::image_dimensions(&pages[19]).unwrap(), (1670, 2160));
        println!("run {run}: 20 pages in {:?}", started.elapsed());
    }
}
