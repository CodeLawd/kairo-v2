//! Denying device permissions to the Slido page.
//!
//! Electron answered every permission request and check with "no". Wry has no
//! permission API, so this is done twice: a script that makes the page's own
//! calls fail in every frame, and, where the platform lets us reach it, the
//! web view's native permission delegate.
//!
//! What each platform does (see DECISIONS.md, 047):
//! - macOS: Wry's UI delegate grants camera and microphone requests. The
//!   delegate of each Slido web view is switched to a subclass that denies
//!   them ([`deny_media_capture`]). Geolocation needs the app to hold
//!   location access, which Kairo does not request.
//! - Windows: WebView2 `PermissionRequested` is not wired (not written).
//! - Linux: WebKitGTK `permission-request` is not wired (not written).

/// Runs in every frame, before the page's own scripts.
pub const GUARD_JS: &str = r#"(function () {
  const denied = name => { const e = new Error('Permission denied'); e.name = name; return e; };
  const define = (target, key, value) => { try { Object.defineProperty(target, key, { value, configurable: false, writable: false }); } catch (_) {} };
  const media = navigator.mediaDevices;
  if (media) {
    define(media, 'getUserMedia', () => Promise.reject(denied('NotAllowedError')));
    define(media, 'getDisplayMedia', () => Promise.reject(denied('NotAllowedError')));
  }
  define(navigator, 'webkitGetUserMedia', (_c, _ok, fail) => { if (fail) fail(denied('NotAllowedError')); });
  const geo = navigator.geolocation;
  if (geo) {
    const fail = (_ok, error) => { if (typeof error === 'function') error({ code: 1, message: 'User denied Geolocation', PERMISSION_DENIED: 1, POSITION_UNAVAILABLE: 2, TIMEOUT: 3 }); };
    define(geo, 'getCurrentPosition', fail);
    define(geo, 'watchPosition', (ok, error) => { fail(ok, error); return 0; });
  }
  if (window.Notification) {
    define(window.Notification, 'requestPermission', callback => { if (typeof callback === 'function') callback('denied'); return Promise.resolve('denied'); });
    define(window.Notification, 'permission', 'denied');
  }
})();"#;

/// Makes a web view's UI delegate deny camera and microphone requests.
///
/// Wry's delegate answers `Grant`. The delegate object is moved to a
/// subclass that replaces only that one method, so other Wry web views in the
/// process keep their behaviour. Returns `false` if the delegate is missing.
#[cfg(target_os = "macos")]
pub fn deny_media_capture(webview: &wry::WebView) -> bool {
    use objc2::runtime::AnyObject;
    use wry::WebViewExtMacOS;

    let wk = webview.webview();
    // SAFETY: `UIDelegate` is a plain getter on a live WKWebView.
    let delegate: *mut AnyObject = unsafe { objc2::msg_send![&*wk, UIDelegate] };
    // SAFETY: the delegate is live; see `deny_in`.
    !delegate.is_null() && unsafe { deny_in(delegate) }
}

/// Moves `delegate` to a subclass that denies media capture.
///
/// # Safety
/// `delegate` must point to a live Objective-C object.
#[cfg(target_os = "macos")]
unsafe fn deny_in(delegate: *mut objc2::runtime::AnyObject) -> bool {
    use block2::Block;
    use objc2::ffi;
    use objc2::runtime::{AnyClass, AnyObject, Sel};
    use objc2::sel;
    use std::collections::HashMap;
    use std::ffi::{CString, c_char};
    use std::sync::Mutex;

    // WKPermissionDecision: prompt 0, grant 1, deny 2.
    const DENY: isize = 2;
    // webView:requestMediaCapturePermissionForOrigin:initiatedByFrame:type:decisionHandler:
    unsafe extern "C-unwind" fn deny(
        _this: *mut AnyObject,
        _cmd: Sel,
        _webview: *mut AnyObject,
        _origin: *mut AnyObject,
        _frame: *mut AnyObject,
        _kind: isize,
        decision: *mut Block<dyn Fn(isize)>,
    ) {
        // SAFETY: WebKit passes a live block for the duration of the call.
        if let Some(block) = unsafe { decision.as_ref() } {
            block.call((DENY,));
        }
    }

    // One subclass per delegate class, made once.
    static SUBCLASSES: Mutex<Option<HashMap<usize, usize>>> = Mutex::new(None);

    // SAFETY (whole body): the runtime calls only touch class metadata and the
    // replacement has the method's exact signature.
    unsafe {
        let class: *mut AnyClass = objc2::msg_send![delegate, class];
        let mut cache = SUBCLASSES.lock().unwrap_or_else(|e| e.into_inner());
        let known = cache.get_or_insert_with(HashMap::new);
        let subclass = match known.get(&(class as usize)) {
            Some(&sub) => sub as *mut AnyClass,
            None => {
                let name = CString::new(format!("KairoDeniedMediaCapture{}", known.len()))
                    .unwrap_or_default();
                let sub = ffi::objc_allocateClassPair(class, name.as_ptr() as *const c_char, 0);
                if sub.is_null() {
                    return false;
                }
                let imp: unsafe extern "C-unwind" fn() = std::mem::transmute(deny as *const ());
                let selector = sel!(webView:requestMediaCapturePermissionForOrigin:initiatedByFrame:type:decisionHandler:);
                if !ffi::class_addMethod(sub, selector, imp, c"v@:@@@q@?".as_ptr()).as_bool() {
                    return false;
                }
                ffi::objc_registerClassPair(sub);
                known.insert(class as usize, sub as usize);
                sub
            }
        };
        ffi::object_setClass(delegate, subclass);
    }
    true
}

#[cfg(not(target_os = "macos"))]
pub fn deny_media_capture(_webview: &wry::WebView) -> bool {
    false
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use block2::RcBlock;
    use objc2::runtime::{AnyObject, NSObject};
    use objc2::{ClassType, define_class, msg_send};
    use std::cell::Cell;
    use std::rc::Rc;

    define_class!(
        // Stands in for Wry's delegate, which grants every request.
        #[unsafe(super(NSObject))]
        #[name = "KairoTestGrantingDelegate"]
        struct Granting;

        impl Granting {
            #[unsafe(method(webView:requestMediaCapturePermissionForOrigin:initiatedByFrame:type:decisionHandler:))]
            fn request(
                &self,
                _webview: *mut AnyObject,
                _origin: *mut AnyObject,
                _frame: *mut AnyObject,
                _kind: isize,
                decision: &block2::Block<dyn Fn(isize)>,
            ) {
                decision.call((1,));
            }
        }
    );

    fn ask(delegate: *mut AnyObject) -> isize {
        let answer = Rc::new(Cell::new(-1));
        let seen = answer.clone();
        let block = RcBlock::new(move |decision: isize| seen.set(decision));
        let null = std::ptr::null_mut::<AnyObject>();
        // SAFETY: `delegate` is a live object that implements this selector.
        unsafe {
            let _: () = msg_send![
                delegate,
                webView: null,
                requestMediaCapturePermissionForOrigin: null,
                initiatedByFrame: null,
                type: 0isize,
                decisionHandler: &*block
            ];
        }
        answer.get()
    }

    #[test]
    fn the_subclass_denies_and_leaves_other_delegates_alone() {
        // SAFETY: plain allocation of a test object.
        let (a, b): (*mut AnyObject, *mut AnyObject) = unsafe {
            (
                msg_send![Granting::class(), new],
                msg_send![Granting::class(), new],
            )
        };
        assert_eq!(ask(a), 1, "granted before");
        assert_eq!(ask(b), 1);
        // SAFETY: `a` is live.
        assert!(unsafe { deny_in(a) });
        assert_eq!(ask(a), 2, "denied after");
        assert_eq!(ask(b), 1, "another delegate of the same class still grants");
        // A second delegate reuses the subclass.
        // SAFETY: `b` is live.
        assert!(unsafe { deny_in(b) });
        assert_eq!(ask(b), 2);
    }
}
