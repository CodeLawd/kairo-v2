//! macOS window behaviour winit does not offer: output windows that never
//! take focus and sit above the menu bar, as Electron's did.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, Imp, Sel};
use objc2::sel;
use objc2_app_kit::{NSView, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

/// `NSScreenSaverWindowLevel`, Electron's `'screen-saver'` level.
const SCREEN_SAVER_LEVEL: isize = 1000;

unsafe extern "C-unwind" fn can_become(this: &NSWindow, _: Sel) -> Bool {
    Bool::new(this.styleMask().contains(NSWindowStyleMask::Titled))
}

fn ns_window(window: &Window) -> Option<Retained<NSWindow>> {
    let RawWindowHandle::AppKit(handle) = window.window_handle().ok()?.as_raw() else {
        return None;
    };
    // SAFETY: winit hands out a live NSView, used on the main thread.
    let view: &NSView = unsafe { handle.ns_view.cast().as_ref() };
    view.window()
}

/// winit's window class answers yes to `canBecomeKeyWindow` always. Replace
/// it so only titled (rehearsal) windows can become key: a borderless output
/// cannot take focus even when clicked. Winit 0.30 has no option for this
/// (decision 002).
pub fn make_nonfocusable(window: &Window) {
    let Some(ns) = ns_window(window) else { return };
    let class = AnyObject::class(&ns);
    // SAFETY: the function matches `- (BOOL)canBecomeKeyWindow` and
    // `- (BOOL)canBecomeMainWindow`.
    unsafe {
        let imp: Imp =
            std::mem::transmute(can_become as unsafe extern "C-unwind" fn(&NSWindow, Sel) -> Bool);
        let types = c"B16@0:8".as_ptr();
        let class = class as *const _ as *mut _;
        objc2::ffi::class_replaceMethod(class, sel!(canBecomeKeyWindow), imp, types);
        objc2::ffi::class_replaceMethod(class, sel!(canBecomeMainWindow), imp, types);
    }
}

/// Full-screen output: above the menu bar, on every Space, no shadow, shown
/// without activating the app.
pub fn place_output(window: &Window) {
    let Some(ns) = ns_window(window) else { return };
    ns.setLevel(SCREEN_SAVER_LEVEL);
    ns.setExcludedFromWindowsMenu(true);
    ns.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    ns.setHasShadow(false);
    ns.orderFrontRegardless();
}

/// A rehearsal window is shown without taking focus from the controls.
pub fn show_without_focus(window: &Window) {
    if let Some(ns) = ns_window(window) {
        ns.orderFrontRegardless();
    }
}

/// Mapped WebKit source, behind the visible outputs and operator windows. A
/// hidden NSWindow suspends WKWebView's animation frames even with timer
/// throttling disabled. It never takes input or appears in window cycling.
#[allow(dead_code)] // Also compiled by the binary for its display window helpers.
pub fn place_web_source(window: &Window) {
    if let Some(ns) = ns_window(window) {
        ns.setIgnoresMouseEvents(true);
        ns.setLevel(-1);
        ns.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::IgnoresCycle,
        );
        ns.orderBack(None);
    }
}

/// AppKit's topology notification arrives before the next polling pass.
#[allow(dead_code)]
pub struct DisplayObserver(
    Retained<objc2::runtime::ProtocolObject<dyn objc2_foundation::NSObjectProtocol>>,
);
#[allow(dead_code)]
impl DisplayObserver {
    pub fn new(callback: impl Fn() + Send + Sync + 'static) -> Self {
        use objc2_foundation::{NSNotificationCenter, NSOperationQueue, ns_string};
        let block = block2::RcBlock::new(
            move |_: std::ptr::NonNull<objc2_foundation::NSNotification>| callback(),
        );
        // SAFETY: no UI references cross threads; the block only posts an event.
        let token = unsafe {
            NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                Some(ns_string!(
                    "NSApplicationDidChangeScreenParametersNotification"
                )),
                None,
                Some(&NSOperationQueue::mainQueue()),
                &block,
            )
        };
        Self(token)
    }
}
impl Drop for DisplayObserver {
    fn drop(&mut self) {
        let observer: &AnyObject = (*self.0).as_ref();
        unsafe {
            objc2_foundation::NSNotificationCenter::defaultCenter().removeObserver(observer);
        }
    }
}
