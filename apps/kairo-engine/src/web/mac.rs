//! WKWebView capture and native restrictions. Wry owns the browser; custom
//! delegates replace its default permission grant and reload a crashed renderer.

use super::{Signals, address, key_code, observed_scroll};
use block2::{DynBlock, RcBlock};
use kairo_compositor::Raster;
use kairo_engine_proto::WebInput;
use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained,
    runtime::ProtocolObject,
};
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType, NSImage};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo, CGImageByteOrderInfo,
};
use objc2_foundation::{
    NSError, NSNumber, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
};
use objc2_web_kit::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event_loop::ActiveEventLoop;
use winit::window::Window;
use wry::{BackgroundThrottlingPolicy, WebView, WebViewBuilder, WebViewExtMacOS};

const OBSERVE: &str = r#"(() => {
  let queued=false;
  const report=()=>{if(queued)return;queued=true;requestAnimationFrame(()=>{
    queued=false;window.ipc.postMessage(JSON.stringify({type:'scroll',x:scrollX,y:scrollY}));
  });};
  addEventListener('scroll',report,{passive:true});
  addEventListener('pageshow',report); report();
})()"#;

// Defense in depth: make denied promises settle even when WebKit waits for
// visibility/OS authorization before invoking its native permission delegate.
const DENY_CAPTURE: &str = r#"(() => {
  const deny=()=>Promise.reject(new DOMException('Capture is disabled in Kairo outputs','NotAllowedError'));
  const lock=(obj,key,value)=>{if(obj)try{Object.defineProperty(obj,key,{value,writable:false,configurable:false})}catch{}};
  lock(globalThis.MediaDevices?.prototype,'getUserMedia',deny);
  lock(globalThis.MediaDevices?.prototype,'getDisplayMedia',deny);
  const legacy=(_,ok,fail)=>{if(fail)fail(new DOMException('Capture is disabled','NotAllowedError'))};
  lock(navigator,'getUserMedia',legacy);lock(navigator,'webkitGetUserMedia',legacy);
})()"#;

struct NavState {
    original: Retained<ProtocolObject<dyn WKNavigationDelegate>>,
    signals: Rc<RefCell<Signals>>,
    last_crash: Cell<Option<Instant>>,
    crashes: Cell<u32>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "KairoProgramNavigation"]
    #[thread_kind = MainThreadOnly]
    #[ivars = NavState]
    struct Navigation;
    unsafe impl NSObjectProtocol for Navigation {}
    unsafe impl WKNavigationDelegate for Navigation {
        #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
        fn action(
            &self,
            view: &WKWebView,
            action: &WKNavigationAction,
            handler: &DynBlock<dyn Fn(WKNavigationActionPolicy)>,
        ) {
            // SAFETY: WebKit calls on the main thread with live native objects.
            unsafe {
                let target = action.targetFrame();
                let main = target.as_ref().is_some_and(|f| f.isMainFrame());
                let source_main = action.sourceFrame().is_none_or(|f| f.isMainFrame());
                let url = action
                    .request()
                    .URL()
                    .and_then(|u| u.absoluteString())
                    .map(|u| u.to_string());
                // No new browsing contexts, downloads, unsupported schemes, or
                // subframe navigation of the main page (the sandbox boundary).
                if target.is_none()
                    || action.shouldPerformDownload()
                    || (main && (!source_main || url.as_deref().and_then(address).is_none()))
                {
                    handler.call((WKNavigationActionPolicy::Cancel,));
                } else {
                    self.ivars()
                        .original
                        .webView_decidePolicyForNavigationAction_decisionHandler(
                            view, action, handler,
                        );
                }
            }
        }
        #[unsafe(method(webView:decidePolicyForNavigationResponse:decisionHandler:))]
        fn response(
            &self,
            view: &WKWebView,
            response: &WKNavigationResponse,
            handler: &DynBlock<dyn Fn(WKNavigationResponsePolicy)>,
        ) {
            unsafe {
                self.ivars()
                    .original
                    .webView_decidePolicyForNavigationResponse_decisionHandler(
                        view, response, handler,
                    );
            }
        }
        #[unsafe(method(webView:didCommitNavigation:))]
        fn commit(&self, view: &WKWebView, nav: Option<&WKNavigation>) {
            // Preserve Wry's pending-script flush on the first commit.
            unsafe {
                self.ivars().original.webView_didCommitNavigation(view, nav);
            }
        }
        #[unsafe(method(webView:didFinishNavigation:))]
        fn finish(&self, view: &WKWebView, nav: Option<&WKNavigation>) {
            unsafe {
                self.ivars().original.webView_didFinishNavigation(view, nav);
            }
            self.ivars().signals.borrow_mut().loaded = true;
        }
        #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
        fn failed_load(&self, _: &WKWebView, _: Option<&WKNavigation>, error: &NSError) {
            // -999 is a superseded navigation, not a failed page.
            if error.code() != -999 {
                self.ivars().signals.borrow_mut().error =
                    Some(error.localizedDescription().to_string());
            }
        }
        #[unsafe(method(webView:didFailNavigation:withError:))]
        fn failed(&self, view: &WKWebView, nav: Option<&WKNavigation>, error: &NSError) {
            let _ = (view, nav);
            if error.code() != -999 {
                self.ivars().signals.borrow_mut().error =
                    Some(error.localizedDescription().to_string());
            }
        }
        #[unsafe(method(webViewWebContentProcessDidTerminate:))]
        fn terminated(&self, view: &WKWebView) {
            let state = self.ivars();
            let now = Instant::now();
            let count = if state
                .last_crash
                .get()
                .is_some_and(|t| now.duration_since(t) < Duration::from_secs(60))
            {
                state.crashes.get() + 1
            } else {
                1
            };
            state.last_crash.set(Some(now));
            state.crashes.set(count);
            let mut s = state.signals.borrow_mut();
            s.crashed = true;
            s.error = Some(
                if count <= 3 {
                    "renderer terminated; reloading"
                } else {
                    "renderer repeatedly terminated; use Reload to retry"
                }
                .into(),
            );
            drop(s);
            if count <= 3 {
                unsafe {
                    view.reload();
                }
            }
        }
    }
);

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "KairoProgramPermissions"]
    #[thread_kind = MainThreadOnly]
    struct Permissions;
    unsafe impl NSObjectProtocol for Permissions {}
    unsafe impl WKUIDelegate for Permissions {
        #[unsafe(method(webView:requestMediaCapturePermissionForOrigin:initiatedByFrame:type:decisionHandler:))]
        fn capture(
            &self,
            _: &WKWebView,
            _: &WKSecurityOrigin,
            _: &WKFrameInfo,
            _: WKMediaCaptureType,
            handler: &DynBlock<dyn Fn(WKPermissionDecision)>,
        ) {
            handler.call((WKPermissionDecision::Deny,));
        }
        #[unsafe(method(webView:requestDeviceOrientationAndMotionPermissionForOrigin:initiatedByFrame:decisionHandler:))]
        fn motion(
            &self,
            _: &WKWebView,
            _: &WKSecurityOrigin,
            _: &WKFrameInfo,
            handler: &DynBlock<dyn Fn(WKPermissionDecision)>,
        ) {
            handler.call((WKPermissionDecision::Deny,));
        }
        // The default createWebView result is nil: popups cannot create windows.
    }
);

pub struct Page {
    view: WebView,
    _navigation: Retained<Navigation>,
    _permissions: Retained<Permissions>,
    window: Window,
    signals: Rc<RefCell<Signals>>,
    busy: Rc<Cell<Option<Instant>>>,
    alive: Rc<Cell<bool>>,
    generation: Rc<Cell<u64>>,
    last_error: Option<String>,
    last_url: String,
}

impl Page {
    pub fn new(
        el: &ActiveEventLoop,
        url: &str,
        restore: Option<(f64, f64)>,
    ) -> Result<Self, String> {
        let mtm = MainThreadMarker::new().ok_or("web page must be created on the main thread")?;
        let window = el
            .create_window(
                Window::default_attributes()
                    .with_title("Kairo live web source")
                    .with_visible(true)
                    .with_active(false)
                    .with_inner_size(LogicalSize::new(1920., 1080.)),
            )
            .map_err(|e| e.to_string())?;
        // A mapped window is essential: WKWebView never schedules animation
        // frames or resolves some media requests in an unmapped (hidden) view.
        // Keep the source at the back, non-interactive; outputs draw its capture.
        crate::mac::place_web_source(&window);
        let signals = Rc::new(RefCell::new(Signals::default()));
        let observe = signals.clone();
        let view = WebViewBuilder::new()
            .with_bounds(wry::Rect {
                position: LogicalPosition::new(0., 0.).into(),
                size: LogicalSize::new(1920., 1080.).into(),
            })
            .with_background_color((255, 255, 255, 255))
            .with_background_throttling(BackgroundThrottlingPolicy::Disabled)
            .with_autoplay(true)
            .with_initialization_script(OBSERVE)
            .with_initialization_script_for_main_only(DENY_CAPTURE, false)
            .with_ipc_handler(move |request| {
                if let Some(point) = observed_scroll(request.body()) {
                    observe.borrow_mut().scroll = Some(point);
                }
            })
            .with_navigation_handler(|url| {
                address(&url).is_some() || url == "about:blank" || url.starts_with("data:")
            })
            .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
            .with_download_started_handler(|_, _| false)
            .build_as_child(&window)
            .map_err(|e| e.to_string())?;
        // Configure all native permissions BEFORE the first untrusted load.
        let native = view.webview();
        let original =
            unsafe { native.navigationDelegate() }.ok_or("Wry supplied no navigation delegate")?;
        let allocated = Navigation::alloc(mtm).set_ivars(NavState {
            original,
            signals: signals.clone(),
            last_crash: Cell::new(None),
            crashes: Cell::new(0),
        });
        let navigation: Retained<Navigation> = unsafe { msg_send![super(allocated), init] };
        let permissions: Retained<Permissions> =
            unsafe { msg_send![Permissions::alloc(mtm), init] };
        unsafe {
            // Captured outputs completely cover this source. WebKit otherwise
            // stops requestAnimationFrame independently of inactive scheduling.
            // This is private WebKit SPI, isolated to our own WKWebView. Check
            // both selectors and read back the policy: an incompatible runtime
            // must report an error instead of silently streaming frozen frames.
            // See DECISIONS.md, "059 WebKit capture visibility".
            if !native.respondsToSelector(objc2::sel!(_setWindowOcclusionDetectionEnabled:))
                || !native.respondsToSelector(objc2::sel!(_windowOcclusionDetectionEnabled))
            {
                return Err("Live web capture is unavailable on this macOS WebKit version".into());
            }
            let _: () = msg_send![&*native, _setWindowOcclusionDetectionEnabled: false];
            let enabled: objc2::runtime::Bool =
                msg_send![&*native, _windowOcclusionDetectionEnabled];
            if enabled.as_bool() {
                return Err(
                    "WebKit could not enable live capture while the source is covered".into(),
                );
            }
            native
                .configuration()
                .preferences()
                .setElementFullscreenEnabled(true);
            native.setNavigationDelegate(Some(ProtocolObject::from_ref(&*navigation)));
            native.setUIDelegate(Some(ProtocolObject::from_ref(&*permissions)));
        }
        view.load_url(url).map_err(|e| e.to_string())?;
        let page = Self {
            view,
            _navigation: navigation,
            _permissions: permissions,
            window,
            signals,
            busy: Rc::new(Cell::new(None)),
            alive: Rc::new(Cell::new(true)),
            generation: Rc::new(Cell::new(0)),
            last_error: None,
            last_url: url.to_owned(),
        };
        if let Some((x, y)) = restore {
            page.scroll(x, y);
        }
        Ok(page)
    }

    pub fn reload(&mut self) {
        self.generation.set(self.generation.get().wrapping_add(1));
        self.busy.set(None);
        self._navigation.ivars().crashes.set(0);
        self.signals.borrow_mut().frame = None;
        let current = self.location();
        let result = if current.is_none() || self.last_error.is_some() {
            self.view
                .load_url(current.as_deref().unwrap_or(&self.last_url))
        } else {
            self.view.reload()
        };
        if let Err(e) = result {
            self.signals.borrow_mut().error = Some(e.to_string());
        }
    }

    pub fn scroll(&self, x: f64, y: f64) {
        if let Err(e) = self.view.evaluate_script(&format!(
            "scrollTo({{left:{x},top:{y},behavior:'instant'}})"
        )) {
            self.signals.borrow_mut().error = Some(e.to_string());
        }
    }

    pub fn input(&self, input: &WebInput) {
        let view = self.view.webview();
        let ns = self.view.ns_window();
        match input {
            WebInput::Move { x, y } | WebInput::Click { x, y } => {
                // Input in page coordinates. AppKit events are in window points,
                // with the origin at the bottom; convert from the flipped view.
                let point = view.convertPoint_toView(NSPoint::new(*x, *y), None);
                let Some(target) = view.hitTest(NSPoint::new(*x, *y)) else {
                    return;
                };
                if matches!(input, WebInput::Move { .. }) {
                    if let Some(e) = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(NSEventType::MouseMoved,point,NSEventModifierFlags::empty(),0.,ns.windowNumber(),None,0,0,0.) { target.mouseMoved(&e); }
                    return;
                }
                ns.makeFirstResponder(Some(&target));
                for (kind, down) in [
                    (NSEventType::LeftMouseDown, true),
                    (NSEventType::LeftMouseUp, false),
                ] {
                    if let Some(e) = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(kind,point,NSEventModifierFlags::empty(),0.,ns.windowNumber(),None,0,1,if down {1.} else {0.}) {
                        if down { target.mouseDown(&e); } else { target.mouseUp(&e); }
                    }
                }
            }
            WebInput::Text { text } => {
                // WK's focused editor is the first responder. Do not execute text.
                if let Some(responder) = ns.firstResponder() {
                    unsafe {
                        if !responder.tryToPerform_with(
                            objc2::sel!(insertText:),
                            Some(&NSString::from_str(text)),
                        ) {
                            self.signals.borrow_mut().error =
                                Some("the focused page does not accept text".into());
                        }
                    }
                }
            }
            WebInput::Key { key } => {
                if let Some((code, ch)) = key_code(key) {
                    let chars = NSString::from_str(&ch.to_string());
                    for (kind, down) in [(NSEventType::KeyDown, true), (NSEventType::KeyUp, false)]
                    {
                        if let Some(e) = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(kind,NSPoint::ZERO,NSEventModifierFlags::empty(),0.,ns.windowNumber(),None,&chars,&chars,false,code)
                            && let Some(responder) = ns.firstResponder() { if down { responder.keyDown(&e); } else { responder.keyUp(&e); } }
                    }
                }
            }
        }
    }

    pub fn capture(
        &mut self,
        now: Instant,
        feed: &kairo_compositor::thread::SourceFeed,
        ids: Vec<u64>,
    ) {
        if let Some(start) = self.busy.get() {
            if now.duration_since(start) < Duration::from_secs(2) {
                return;
            }
            // Invalidate an overdue callback, then permit a new request.
            self.generation.set(self.generation.get().wrapping_add(1));
            self.signals.borrow_mut().error = Some("snapshot timed out; retrying".into());
        }
        self.busy.set(Some(now));
        let mtm = MainThreadMarker::new().expect("winit main thread");
        let config = unsafe { WKSnapshotConfiguration::new(mtm) };
        unsafe {
            config.setSnapshotWidth(Some(&NSNumber::new_f64(1920. / self.window.scale_factor())));
        }
        let signals = self.signals.clone();
        let busy = self.busy.clone();
        let alive = self.alive.clone();
        let generation = self.generation.clone();
        let expected = generation.get();
        let feed = feed.clone();
        let callback = RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
            if !alive.get() || generation.get() != expected {
                return;
            }
            busy.set(None);
            let frame = if !error.is_null() {
                Err(unsafe { (&*error).localizedDescription().to_string() })
            } else if image.is_null() {
                Err("snapshot returned no image".into())
            } else {
                unsafe { pixels(&*image) }
            };
            let mut s = signals.borrow_mut();
            match frame {
                Ok(frame) => {
                    let ready = Instant::now();
                    for id in &ids {
                        feed.rgba_frame(*id, frame.clone(), now, ready);
                    }
                    s.timing = Some((now, ready));
                }
                Err(error) => s.error = Some(error),
            }
        });
        unsafe {
            self.view
                .webview()
                .takeSnapshotWithConfiguration_completionHandler(Some(&config), &callback);
        }
    }

    pub fn location(&self) -> Option<String> {
        self.view.url().ok().as_deref().and_then(address)
    }

    pub fn drain(&mut self) -> (Signals, Option<String>) {
        let mut signals = std::mem::take(&mut *self.signals.borrow_mut());
        if signals.crashed {
            self.generation.set(self.generation.get().wrapping_add(1));
            self.busy.set(None);
        }
        // Suppress identical repeated failures; a successful load permits a new report.
        if signals.loaded {
            self.last_error = None;
        }
        if let Some(error) = signals.error.as_ref() {
            if self.last_error.as_ref() == Some(error) {
                signals.error = None;
            } else {
                self.last_error = Some(error.clone());
            }
        }
        let location = self.location();
        if let Some(url) = &location {
            self.last_url = url.clone();
        }
        (signals, location)
    }
}

impl Drop for Page {
    fn drop(&mut self) {
        self.alive.set(false);
        // Both delegate properties are weak; detach before releasing their owners.
        unsafe {
            let view = self.view.webview();
            view.stopLoading();
            view.setNavigationDelegate(None);
            view.setUIDelegate(None);
        }
    }
}

unsafe fn pixels(image: &NSImage) -> Result<std::sync::Arc<Raster>, String> {
    let cg =
        unsafe { image.CGImageForProposedRect_context_hints(std::ptr::null_mut(), None, None) }
            .ok_or("snapshot has no CGImage")?;
    if CGImage::width(Some(&cg)) != 1920 || CGImage::height(Some(&cg)) != 1080 {
        return Err(format!(
            "snapshot dimensions {}×{}; expected 1920×1080",
            CGImage::width(Some(&cg)),
            CGImage::height(Some(&cg))
        ));
    }
    // Force premultiplied RGBA instead of guessing NSBitmapImageRep byte order.
    let mut data = vec![0u8; 1920 * 1080 * 4];
    let color = CGColorSpace::new_device_rgb().ok_or("cannot make RGB color space")?;
    let bitmap = unsafe {
        CGBitmapContextCreate(
            data.as_mut_ptr().cast(),
            1920,
            1080,
            8,
            1920 * 4,
            Some(&color),
            CGImageAlphaInfo::PremultipliedLast.0 | CGImageByteOrderInfo::Order32Big.0,
        )
    }
    .ok_or("cannot make snapshot bitmap")?;
    CGContext::draw_image(
        Some(&bitmap),
        NSRect::new(NSPoint::ZERO, NSSize::new(1920., 1080.)),
        Some(&cg),
    );
    drop(bitmap); // the context must release its borrowed storage before Vec moves
    Ok(Raster::new(1920, 1080, data))
}
