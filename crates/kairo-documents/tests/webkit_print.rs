//! Prints a hand-built deck through the real platform web view: `WKWebView`
//! on macOS, WebKitGTK on Linux. Both need the main thread, so this test
//! owns `main` (`harness = false`).

#[cfg(any(target_os = "macos", target_os = "linux"))]
mod support;

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn main() {
    use kairo_documents::pdf::{page_sizes, render_pages, set_pdfium_library_dir};
    use kairo_documents::pptx_builtin::build_slides_html;
    #[cfg(target_os = "macos")]
    use kairo_documents::webkit::print_slides;
    #[cfg(target_os = "linux")]
    use kairo_documents::webkitgtk::print_slides;
    use std::path::Path;
    use std::sync::atomic::AtomicBool;
    use support::deck::{Slide, png, pptx};

    let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.cache/pdfium/lib");
    if dev.join("libpdfium.dylib").is_file() || dev.join("libpdfium.so").is_file() {
        set_pdfium_library_dir(dev);
    } else if std::env::var_os("PDFIUM_DYNAMIC_LIB_PATH").is_none() {
        assert!(
            std::env::var_os("KAIRO_REQUIRE_PDFIUM").is_none(),
            "KAIRO_REQUIRE_PDFIUM is set but PDFium was not found"
        );
        eprintln!("PDFium not found: WebKit print skipped");
        return;
    }
    #[cfg(target_os = "macos")]
    let mtm = objc2::MainThreadMarker::new().expect("test main runs on the main thread");
    #[cfg(target_os = "linux")]
    gtk::init().expect("GTK starts (needs a display, for example Xvfb)");
    let work = tempfile::tempdir().unwrap();
    let source = work.path().join("deck.pptx");
    let slides = [
        Slide {
            fill: "FF0000",
            picture: Some(png([0, 0, 255])),
        },
        Slide {
            fill: "00FF00",
            picture: None,
        },
        Slide {
            fill: "0000FF",
            picture: Some(png([255, 255, 0])),
        },
    ];
    std::fs::write(&source, pptx(&slides, 12_192_000, 6_858_000)).unwrap();
    let html = build_slides_html(&source, work.path()).unwrap();
    let started = std::time::Instant::now();
    #[cfg(target_os = "macos")]
    let pdf = print_slides(&html, mtm).unwrap();
    #[cfg(target_os = "linux")]
    let pdf = print_slides(&html).unwrap();
    eprintln!("printed {} slides in {:?}", html.slides, started.elapsed());
    assert!(pdf.starts_with(b"%PDF-"));
    let sizes = page_sizes(&pdf).unwrap();
    assert_eq!(sizes.len(), 3);
    for (w, h) in sizes {
        assert!(
            (w - 1440.0).abs() < 0.5 && (h - 810.0).abs() < 0.5,
            "page {w}×{h}"
        );
    }
    let out = tempfile::tempdir().unwrap();
    let pages = render_pages(&pdf, out.path(), &AtomicBool::new(false), &mut |_, _| {}).unwrap();
    let expect = [
        ([0, 0, 255], [255, 0, 0]),
        ([0, 255, 0], [0, 255, 0]),
        ([255, 255, 0], [0, 0, 255]),
    ];
    for (path, (picture, fill)) in pages.iter().zip(expect) {
        let image = image::open(path).unwrap().into_rgb8();
        assert_eq!(image.dimensions(), (3840, 2160));
        let at = |x, y| image.get_pixel(x, y).0;
        let near = |a: [u8; 3], b: [u8; 3]| a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 8);
        assert!(
            near(at(400, 400), picture),
            "{}: picture {:?}",
            path.display(),
            at(400, 400)
        );
        assert!(
            near(at(3000, 1800), fill),
            "{}: fill {:?}",
            path.display(),
            at(3000, 1800)
        );
    }
    println!("webkit_print: ok");
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn main() {}
