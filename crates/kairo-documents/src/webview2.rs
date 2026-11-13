//! Windows slide printing: a hidden WebView2 loads `slides.html`, waits for
//! its images and fonts, then `PrintToPdf` writes one page per slide at
//! Electron's page size (`@page` in the HTML, 20 in across, no margins,
//! backgrounds on). Wry has no PDF call and needs a parent window, so this
//! drives WebView2 directly (DECISIONS.md, 045).
//!
//! WebView2 runs on one STA thread with a message pump. Each print gets its
//! own thread, hidden window, environment and controller, so a service call
//! never needs the app's UI thread.

use crate::DocumentError;
use crate::pptx_builtin::{SlidePrinter, SlidesHtml};
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_PRINT_ORIENTATION_PORTRAIT, CreateCoreWebView2EnvironmentWithOptions,
    GetAvailableCoreWebView2BrowserVersionString, ICoreWebView2, ICoreWebView2_7,
    ICoreWebView2Controller, ICoreWebView2Environment, ICoreWebView2Environment6,
    ICoreWebView2EnvironmentOptions,
};
use webview2_com::{
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    ExecuteScriptCompletedHandler, PrintToPdfCompletedHandler, take_pwstr,
};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, DispatchMessageW, MSG, PM_REMOVE, PeekMessageW,
    TranslateMessage, WINDOW_EX_STYLE, WS_POPUP,
};
use windows::core::{HSTRING, Interface, PWSTR, w};

/// Electron's `CONVERT_MS`: the whole print gives up after this.
const PRINT_TIMEOUT: Duration = Duration::from_secs(120);

/// Ready only once the slides page itself (not the initial blank page) has
/// loaded every font and image.
const READY_SCRIPT: &str = "location.protocol === 'file:' && document.readyState === 'complete' && document.fonts.status === 'loaded' && [...document.images].every((img) => img.complete) ? 'ready' : 'wait'";

/// The message shown when the WebView2 runtime is missing.
pub const RUNTIME_MISSING: &str = "Microsoft Edge WebView2 Runtime is not installed.";

/// The installed WebView2 runtime's version, or `None` when there is none.
pub fn runtime_version() -> Option<String> {
    let mut info = PWSTR::null();
    // SAFETY: a null folder asks for the installed runtime; WebView2 fills
    // `info` with a CoTaskMem string that `take_pwstr` frees.
    unsafe {
        GetAvailableCoreWebView2BrowserVersionString(windows::core::PCWSTR::null(), &mut info)
    }
    .ok()?;
    if info.is_null() {
        return None;
    }
    let version = take_pwstr(info);
    (!version.is_empty()).then_some(version)
}

/// Prints through WebView2 on a thread of its own.
#[derive(Debug, Default, Clone, Copy)]
pub struct WebView2Printer;

impl WebView2Printer {
    pub fn new() -> Self {
        Self
    }
}

impl SlidePrinter for WebView2Printer {
    fn print(&self, html: &SlidesHtml) -> Result<Vec<u8>, DocumentError> {
        if runtime_version().is_none() {
            return Err(DocumentError::Message(RUNTIME_MISSING.into()));
        }
        let html = html.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("kairo-webview2-print".into())
            .spawn(move || {
                let _ = tx.send(print_on_this_thread(&html));
            })
            .map_err(|_| failed())?;
        rx.recv_timeout(PRINT_TIMEOUT + Duration::from_secs(5))
            .map_err(|_| failed())?
    }
}

fn failed() -> DocumentError {
    DocumentError::Message("Could not render this PowerPoint file.".into())
}

/// Pumps this thread's messages until `rx` has a value or time runs out.
fn pump<T>(rx: &mpsc::Receiver<T>, deadline: Instant) -> Result<T, DocumentError> {
    let mut msg = MSG::default();
    loop {
        if let Ok(value) = rx.try_recv() {
            return Ok(value);
        }
        if Instant::now() >= deadline {
            return Err(failed());
        }
        // SAFETY: plain message pump on the thread that owns the windows.
        let had = unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool();
        if had {
            // SAFETY: as above, `msg` came from PeekMessageW.
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        } else {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

/// Owns the COM apartment and the hidden window; both end on drop.
struct Apartment {
    window: HWND,
}

impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: the window and apartment were made on this thread.
        unsafe {
            let _ = DestroyWindow(self.window);
            CoUninitialize();
        }
    }
}

fn print_on_this_thread(html: &SlidesHtml) -> Result<Vec<u8>, DocumentError> {
    let deadline = Instant::now() + PRINT_TIMEOUT;
    // SAFETY: a fresh thread enters a single-threaded apartment once.
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
        .ok()
        .map_err(|_| failed())?;
    let width = html.slide_width.round() as i32;
    let height = html.slide_height.round() as i32;
    // SAFETY: "STATIC" is a system class; the window is never shown.
    let window = match unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            w!("Kairo slides"),
            WS_POPUP,
            0,
            0,
            width,
            height,
            None,
            None,
            None,
            None,
        )
    } {
        Ok(window) => window,
        Err(_) => {
            // SAFETY: balances the CoInitializeEx above.
            unsafe { CoUninitialize() };
            return Err(failed());
        }
    };
    let _apartment = Apartment { window };

    let environment = create_environment(&html.read_access, deadline)?;
    let controller = create_controller(&environment, window, deadline)?;
    // SAFETY: COM calls on the apartment thread that made the objects.
    let view: ICoreWebView2 = unsafe {
        controller
            .SetBounds(RECT {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            })
            .map_err(|_| failed())?;
        controller.CoreWebView2().map_err(|_| failed())?
    };
    let url = url::Url::from_file_path(&html.path).map_err(|()| failed())?;
    // SAFETY: as above.
    unsafe { view.Navigate(&HSTRING::from(url.as_str())) }.map_err(|_| failed())?;
    loop {
        if evaluate(&view, READY_SCRIPT, deadline)? == "\"ready\"" {
            break;
        }
        let (_tx, idle) = mpsc::channel::<()>();
        let _ = pump(
            &idle,
            (Instant::now() + Duration::from_millis(20)).min(deadline),
        );
        if Instant::now() >= deadline {
            return Err(failed());
        }
    }
    let pdf_path = html.read_access.join("slides.pdf");
    let _ = std::fs::remove_file(&pdf_path);
    print_to_pdf(&environment, &view, html, &pdf_path, deadline)?;
    // SAFETY: as above.
    let _ = unsafe { controller.Close() };
    let bytes = std::fs::read(&pdf_path).map_err(|_| failed())?;
    let _ = std::fs::remove_file(&pdf_path);
    Ok(bytes)
}

fn create_environment(
    work: &Path,
    deadline: Instant,
) -> Result<ICoreWebView2Environment, DocumentError> {
    // The profile lives in the work folder, which the import removes.
    let data = HSTRING::from(work.join("webview2").as_os_str());
    let (tx, rx) = mpsc::channel();
    let handler = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
        move |code, environment| {
            let _ = tx.send(code.ok().and(environment));
            Ok(())
        },
    ));
    // SAFETY: arguments outlive the call; the handler is reference counted.
    unsafe {
        CreateCoreWebView2EnvironmentWithOptions(
            windows::core::PCWSTR::null(),
            &data,
            None::<&ICoreWebView2EnvironmentOptions>,
            &handler,
        )
    }
    .map_err(|_| failed())?;
    pump(&rx, deadline)?.ok_or_else(failed)
}

fn create_controller(
    environment: &ICoreWebView2Environment,
    window: HWND,
    deadline: Instant,
) -> Result<ICoreWebView2Controller, DocumentError> {
    let (tx, rx) = mpsc::channel();
    let handler =
        CreateCoreWebView2ControllerCompletedHandler::create(Box::new(move |code, controller| {
            let _ = tx.send(code.ok().and(controller));
            Ok(())
        }));
    // SAFETY: `window` lives until the apartment drops.
    unsafe { environment.CreateCoreWebView2Controller(window, &handler) }.map_err(|_| failed())?;
    pump(&rx, deadline)?.ok_or_else(failed)
}

fn evaluate(
    view: &ICoreWebView2,
    script: &str,
    deadline: Instant,
) -> Result<String, DocumentError> {
    let (tx, rx) = mpsc::channel();
    let handler = ExecuteScriptCompletedHandler::create(Box::new(move |code, json| {
        let _ = tx.send(code.ok().map(|()| json));
        Ok(())
    }));
    // SAFETY: as in `print_on_this_thread`.
    unsafe { view.ExecuteScript(&HSTRING::from(script), &handler) }.map_err(|_| failed())?;
    pump(&rx, deadline)?.ok_or_else(failed)
}

fn print_to_pdf(
    environment: &ICoreWebView2Environment,
    view: &ICoreWebView2,
    html: &SlidesHtml,
    path: &Path,
    deadline: Instant,
) -> Result<(), DocumentError> {
    let environment: ICoreWebView2Environment6 = environment.cast().map_err(|_| failed())?;
    let view: ICoreWebView2_7 = view.cast().map_err(|_| failed())?;
    // SAFETY: as in `print_on_this_thread`.
    let settings = unsafe {
        let settings = environment.CreatePrintSettings().map_err(|_| failed())?;
        // Width is already the long side, so portrait keeps it across.
        settings
            .SetOrientation(COREWEBVIEW2_PRINT_ORIENTATION_PORTRAIT)
            .map_err(|_| failed())?;
        settings
            .SetPageWidth(html.page_width_pt / 72.0)
            .map_err(|_| failed())?;
        settings
            .SetPageHeight(html.page_height_pt / 72.0)
            .map_err(|_| failed())?;
        settings.SetScaleFactor(1.0).map_err(|_| failed())?;
        settings.SetMarginTop(0.0).map_err(|_| failed())?;
        settings.SetMarginBottom(0.0).map_err(|_| failed())?;
        settings.SetMarginLeft(0.0).map_err(|_| failed())?;
        settings.SetMarginRight(0.0).map_err(|_| failed())?;
        settings
            .SetShouldPrintBackgrounds(true)
            .map_err(|_| failed())?;
        settings
            .SetShouldPrintHeaderAndFooter(false)
            .map_err(|_| failed())?;
        settings
            .SetShouldPrintSelectionOnly(false)
            .map_err(|_| failed())?;
        settings
    };
    let (tx, rx) = mpsc::channel();
    let handler = PrintToPdfCompletedHandler::create(Box::new(move |code, ok| {
        let _ = tx.send(code.is_ok() && ok);
        Ok(())
    }));
    // SAFETY: as above; `path` is copied by WebView2 during the call.
    unsafe { view.PrintToPdf(&HSTRING::from(path.as_os_str()), &settings, &handler) }
        .map_err(|_| failed())?;
    if pump(&rx, deadline)? {
        Ok(())
    } else {
        Err(failed())
    }
}
