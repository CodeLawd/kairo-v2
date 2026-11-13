//! Linux slide printing: an offscreen WebKitGTK view loads `slides.html`,
//! waits for its images and fonts, then a `WebKitPrintOperation` prints to a
//! PDF file at Electron's page size (20 in across, no margins, backgrounds
//! on). Wry has no PDF call and needs a window, so this drives WebKitGTK
//! directly (DECISIONS.md, 045).
//!
//! GTK must run on the thread that called `gtk::init`, so, as on macOS, the
//! printer takes a dispatcher that runs the job there.

use crate::DocumentError;
use crate::pptx_builtin::{SlidePrinter, SlidesHtml};
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use webkit2gtk::{PrintOperation, PrintOperationExt, SettingsExt, WebView, WebViewExt};

/// Electron's `CONVERT_MS`: the whole print gives up after this.
const PRINT_TIMEOUT: Duration = Duration::from_secs(120);

/// Ready only once the slides page itself has loaded every font and image.
const READY_SCRIPT: &str = "location.protocol === 'file:' && document.readyState === 'complete' && document.fonts.status === 'loaded' && [...document.images].every((img) => img.complete) ? 'ready' : 'wait'";

type MainThreadJob = Box<dyn FnOnce() + Send>;

/// Prints on the GTK thread through `dispatch`, which must run the job
/// there (the app passes GPUI's foreground executor).
pub struct WebKitGtkPrinter {
    dispatch: Box<dyn Fn(MainThreadJob) + Send + Sync>,
}

impl WebKitGtkPrinter {
    pub fn new(dispatch: impl Fn(MainThreadJob) + Send + Sync + 'static) -> Self {
        Self {
            dispatch: Box::new(dispatch),
        }
    }
}

impl SlidePrinter for WebKitGtkPrinter {
    fn print(&self, html: &SlidesHtml) -> Result<Vec<u8>, DocumentError> {
        let (tx, rx) = mpsc::channel();
        let html = html.clone();
        (self.dispatch)(Box::new(move || {
            let result = if gtk::is_initialized_main_thread() {
                print_slides(&html)
            } else {
                Err(DocumentError::Message(
                    "Slides must be printed on the GTK thread.".into(),
                ))
            };
            let _ = tx.send(result);
        }));
        rx.recv_timeout(PRINT_TIMEOUT + Duration::from_secs(5))
            .map_err(|_| failed())?
    }
}

fn failed() -> DocumentError {
    DocumentError::Message("Could not render this PowerPoint file.".into())
}

/// Runs the default main context until `done` returns a value or time runs out.
fn spin<T>(deadline: Instant, mut done: impl FnMut() -> Option<T>) -> Result<T, DocumentError> {
    let context = glib::MainContext::default();
    loop {
        if let Some(value) = done() {
            return Ok(value);
        }
        if Instant::now() >= deadline {
            return Err(failed());
        }
        if !context.iteration(false) {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

fn evaluate(
    view: &WebView,
    script: &str,
    deadline: Instant,
) -> Result<Option<String>, DocumentError> {
    let slot: Rc<RefCell<Option<Option<String>>>> = Rc::default();
    let sink = slot.clone();
    // `run_javascript` is the 2.38 call Wry also builds against.
    view.run_javascript(
        script,
        None::<&webkit2gtk::gio::Cancellable>,
        move |result| {
            let text = result
                .ok()
                .and_then(|result| result.js_value())
                .filter(javascriptcore::ValueExt::is_string)
                .map(|value| javascriptcore::ValueExt::to_str(&value).to_string());
            *sink.borrow_mut() = Some(text);
        },
    );
    spin(deadline, || slot.borrow_mut().take())
}

/// Loads `html` in an offscreen view and prints it. GTK thread only; runs
/// the main context while it waits.
pub fn print_slides(html: &SlidesHtml) -> Result<Vec<u8>, DocumentError> {
    let deadline = Instant::now() + PRINT_TIMEOUT;
    let url = url::Url::from_file_path(&html.path).map_err(|()| failed())?;
    let pdf_path = html.read_access.join("slides.pdf");
    let pdf_url = url::Url::from_file_path(&pdf_path).map_err(|()| failed())?;
    let _ = std::fs::remove_file(&pdf_path);

    let view = WebView::new();
    if let Some(settings) = WebViewExt::settings(&view) {
        settings.set_print_backgrounds(true);
        settings.set_allow_file_access_from_file_urls(true);
    }
    let window = gtk::OffscreenWindow::new();
    window.set_default_size(
        html.slide_width.round() as i32,
        html.slide_height.round() as i32,
    );
    window.add(&view);
    window.show_all();
    view.load_uri(url.as_str());
    let result = (|| {
        loop {
            if !view.is_loading()
                && evaluate(&view, READY_SCRIPT, deadline)?.as_deref() == Some("ready")
            {
                break;
            }
            let until = Instant::now() + Duration::from_millis(20);
            spin(deadline, || (Instant::now() >= until).then_some(()))?;
        }
        print_to_file(&view, html, pdf_url.as_str(), deadline)?;
        std::fs::read(&pdf_path).map_err(|_| failed())
    })();
    view.stop_loading();
    // SAFETY: the window and view are not used after this.
    unsafe { window.destroy() };
    let _ = std::fs::remove_file(&pdf_path);
    result
}

fn print_to_file(
    view: &WebView,
    html: &SlidesHtml,
    uri: &str,
    deadline: Instant,
) -> Result<(), DocumentError> {
    let paper = gtk::PaperSize::new_custom(
        "kairo-slide",
        "Slide",
        html.page_width_pt,
        html.page_height_pt,
        gtk::Unit::Points,
    );
    let page = gtk::PageSetup::new();
    page.set_paper_size(&paper);
    // Width is already the long side, so portrait keeps it across.
    page.set_orientation(gtk::PageOrientation::Portrait);
    page.set_top_margin(0.0, gtk::Unit::Points);
    page.set_bottom_margin(0.0, gtk::Unit::Points);
    page.set_left_margin(0.0, gtk::Unit::Points);
    page.set_right_margin(0.0, gtk::Unit::Points);
    let settings = gtk::PrintSettings::new();
    settings.set_printer("Print to File");
    settings.set(gtk::PRINT_SETTINGS_OUTPUT_FILE_FORMAT, Some("pdf"));
    settings.set(gtk::PRINT_SETTINGS_OUTPUT_URI, Some(uri));
    settings.set_paper_size(&paper);
    settings.set_orientation(gtk::PageOrientation::Portrait);
    settings.set_scale(100.0);

    let operation = PrintOperation::new(view);
    operation.set_page_setup(&page);
    operation.set_print_settings(&settings);
    let slot: Rc<RefCell<Option<bool>>> = Rc::default();
    let finished = slot.clone();
    operation.connect_finished(move |_| {
        finished.borrow_mut().get_or_insert(true);
    });
    let failed_slot = slot.clone();
    operation.connect_failed(move |_, error| {
        tracing::warn!("WebKitGTK print failed: {error}");
        *failed_slot.borrow_mut() = Some(false);
    });
    operation.print();
    if spin(deadline, || *slot.borrow())? {
        Ok(())
    } else {
        Err(failed())
    }
}
