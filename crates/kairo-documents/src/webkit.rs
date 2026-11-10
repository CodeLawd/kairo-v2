//! macOS slide printing: an offscreen `WKWebView` loads `slides.html`, waits
//! for its images and fonts, then `createPDF` captures each slide at
//! Electron's page size (page zoom 0.75). PDFium joins the captures. Wry has no PDF
//! call and needs a window, so this uses WebKit directly (DECISIONS.md).

use crate::DocumentError;
use crate::pptx_builtin::{SlidePrinter, SlidesHtml};
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{NSData, NSDate, NSError, NSRunLoop, NSString, NSURL};
use objc2_web_kit::{WKPDFConfiguration, WKWebView, WKWebViewConfiguration};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Electron's `CONVERT_MS`: the whole print gives up after this.
const PRINT_TIMEOUT: Duration = Duration::from_secs(120);

const READY_SCRIPT: &str = "document.readyState === 'complete' && document.fonts.status === 'loaded' && [...document.images].every((img) => img.complete) ? 'ready' : 'wait'";

type MainThreadJob = Box<dyn FnOnce() + Send>;

/// Prints on the main thread through `dispatch`, which must run the job
/// there (the app passes GPUI's foreground executor).
pub struct WebKitPrinter {
    dispatch: Box<dyn Fn(MainThreadJob) + Send + Sync>,
}

impl WebKitPrinter {
    pub fn new(dispatch: impl Fn(MainThreadJob) + Send + Sync + 'static) -> Self {
        Self {
            dispatch: Box::new(dispatch),
        }
    }
}

impl SlidePrinter for WebKitPrinter {
    fn print(&self, html: &SlidesHtml) -> Result<Vec<u8>, DocumentError> {
        let (tx, rx) = mpsc::channel();
        let html = html.clone();
        (self.dispatch)(Box::new(move || {
            let result = match MainThreadMarker::new() {
                Some(mtm) => print_slides(&html, mtm),
                None => Err(DocumentError::Message(
                    "Slides must be printed on the main thread.".into(),
                )),
            };
            let _ = tx.send(result);
        }));
        rx.recv_timeout(PRINT_TIMEOUT + Duration::from_secs(5))
            .map_err(|_| DocumentError::Message("Could not render this PowerPoint file.".into()))?
    }
}

fn failed() -> DocumentError {
    DocumentError::Message("Could not render this PowerPoint file.".into())
}

/// Runs the main run loop until `done` returns a value or time runs out.
fn spin<T>(deadline: Instant, mut done: impl FnMut() -> Option<T>) -> Result<T, DocumentError> {
    let run_loop = NSRunLoop::currentRunLoop();
    loop {
        if let Some(value) = done() {
            return Ok(value);
        }
        if Instant::now() >= deadline {
            return Err(failed());
        }
        let until = NSDate::dateWithTimeIntervalSinceNow(0.01);
        run_loop.runUntilDate(&until);
    }
}

fn evaluate(
    view: &WKWebView,
    script: &str,
    deadline: Instant,
) -> Result<Option<String>, DocumentError> {
    let slot: Rc<RefCell<Option<Option<String>>>> = Rc::default();
    let sink = slot.clone();
    let handler = RcBlock::new(move |value: *mut AnyObject, _error: *mut NSError| {
        // SAFETY: WebKit passes a valid object or null for the call's result.
        let text = unsafe { value.as_ref() }
            .and_then(|v| v.downcast_ref::<NSString>())
            .map(|s| s.to_string());
        *sink.borrow_mut() = Some(text);
    });
    // SAFETY: called on the main thread that owns `view`; the handler lives
    // until WebKit releases it.
    unsafe {
        view.evaluateJavaScript_completionHandler(&NSString::from_str(script), Some(&handler))
    };
    spin(deadline, || slot.borrow_mut().take())
}

fn capture(
    view: &WKWebView,
    rect: CGRect,
    mtm: MainThreadMarker,
    deadline: Instant,
) -> Result<Vec<u8>, DocumentError> {
    let slot: Rc<RefCell<Option<Option<Vec<u8>>>>> = Rc::default();
    let sink = slot.clone();
    let handler = RcBlock::new(move |data: *mut NSData, _error: *mut NSError| {
        // SAFETY: WebKit passes valid PDF data or null on failure.
        let bytes = unsafe { data.as_ref() }.map(NSData::to_vec);
        *sink.borrow_mut() = Some(bytes);
    });
    // SAFETY: main thread; `rect` lies inside the loaded page.
    unsafe {
        let config = WKPDFConfiguration::new(mtm);
        config.setRect(rect);
        view.createPDFWithConfiguration_completionHandler(Some(&config), &handler);
    }
    spin(deadline, || slot.borrow_mut().take())?.ok_or_else(failed)
}

/// Loads `html` in an offscreen web view and prints it. Main thread only;
/// runs the run loop while it waits.
pub fn print_slides(html: &SlidesHtml, mtm: MainThreadMarker) -> Result<Vec<u8>, DocumentError> {
    let deadline = Instant::now() + PRINT_TIMEOUT;
    let page = NSURL::from_file_path(&html.path).ok_or_else(failed)?;
    let read_access = NSURL::from_directory_path(&html.read_access).ok_or_else(failed)?;
    let frame = CGRect::new(
        CGPoint::new(0.0, 0.0),
        CGSize::new(html.page_width_pt, html.page_height_pt),
    );
    // SAFETY: all WebKit calls happen on the main thread named by `mtm`.
    let view: Retained<WKWebView> = unsafe {
        let config = WKWebViewConfiguration::new(mtm);
        let view = WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), frame, &config);
        // 1920 CSS pixels across 1440 points: the PDF comes out at page size.
        view.setPageZoom(html.page_width_pt / html.slide_width);
        view
    };
    // SAFETY: as above.
    unsafe { view.loadFileURL_allowingReadAccessToURL(&page, &read_access) }.ok_or_else(failed)?;
    loop {
        // SAFETY: as above.
        let loading = unsafe { view.isLoading() };
        if !loading && evaluate(&view, READY_SCRIPT, deadline)?.as_deref() == Some("ready") {
            break;
        }
        let mut waited = false;
        spin(deadline, || {
            std::mem::replace(&mut waited, true).then_some(())
        })?;
    }
    let mut parts = Vec::with_capacity(html.slides);
    for index in 0..html.slides {
        let origin = CGPoint::new(0.0, index as f64 * html.page_height_pt);
        let rect = CGRect::new(origin, CGSize::new(html.page_width_pt, html.page_height_pt));
        parts.push(capture(&view, rect, mtm, deadline)?);
    }
    // SAFETY: as above.
    unsafe { view.stopLoading() };
    drop(view);
    crate::pdf::merge_pages(&parts)
}
