//! Showing and hiding a GPUI window. GPUI can minimise a window but not hide
//! it, so macOS uses the native window's `orderOut:`.

use gpui_kit::Window;

/// Hides the window without closing it.
pub fn hide(window: &mut Window) {
    #[cfg(target_os = "macos")]
    {
        use objc2::runtime::AnyObject;
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(handle) = window.window_handle()
            && let RawWindowHandle::AppKit(appkit) = handle.as_raw()
        {
            let view = appkit.ns_view.as_ptr() as *mut AnyObject;
            // SAFETY: the view belongs to the live GPUI window on this thread.
            unsafe {
                let native: *mut AnyObject = objc2::msg_send![view, window];
                if !native.is_null() {
                    let _: () = objc2::msg_send![native, orderOut: std::ptr::null::<AnyObject>()];
                    return;
                }
            }
        }
    }
    // Windows and Linux: no hide call is wired, so the window is minimised.
    window.minimize_window();
}

/// Brings the window to the front and gives it focus.
pub fn show(window: &mut Window) {
    window.activate_window();
}
