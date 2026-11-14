//! The Dock icon (`app.dock.setIcon` in `createWindow`). A packaged app gets
//! it from its bundle; a development run would otherwise show a blank icon.
//! GPUI has no call for it, so this goes to AppKit directly. Other platforms
//! take the icon from the executable and do nothing here.

#[cfg(target_os = "macos")]
pub fn set_icon(png: &'static [u8]) {
    use objc2::AllocAnyThread;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    // Called from GPUI's run callback, which is on the main thread.
    let Some(mtm) = MainThreadMarker::new() else {
        tracing::warn!("[Dock] not on the main thread; icon unchanged");
        return;
    };
    let data = NSData::with_bytes(png);
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        tracing::warn!("[Dock] icon could not be decoded; icon unchanged");
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    // SAFETY: a valid NSImage on the main thread, as AppKit requires.
    unsafe { app.setApplicationIconImage(Some(&image)) };
}

#[cfg(not(target_os = "macos"))]
pub fn set_icon(_png: &'static [u8]) {}
