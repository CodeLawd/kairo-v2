//! Native window policy shared by program, stage and transient diagnostics.
use kairo_engine_proto::{DisplayInfo, Rect};
use std::sync::Arc;
use winit::{
    dpi::{PhysicalPosition, PhysicalSize},
    event_loop::ActiveEventLoop,
    window::{Window, WindowButtons, WindowLevel},
};

pub fn open(
    el: &ActiveEventLoop,
    title: &str,
    bounds: &Rect,
    rehearsal: bool,
    transparent: bool,
) -> Result<Arc<Window>, String> {
    #[allow(unused_mut)]
    let mut attributes = Window::default_attributes()
        .with_title(title)
        .with_visible(false)
        .with_active(false)
        .with_decorations(rehearsal)
        .with_resizable(rehearsal)
        .with_enabled_buttons(WindowButtons::MINIMIZE | WindowButtons::MAXIMIZE)
        .with_transparent(transparent)
        .with_inner_size(PhysicalSize::new(bounds.width.max(1), bounds.height.max(1)))
        .with_position(PhysicalPosition::new(bounds.x, bounds.y));
    if !rehearsal {
        attributes = attributes.with_window_level(WindowLevel::AlwaysOnTop);
    }
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::WindowAttributesExtMacOS;
        attributes = attributes.with_has_shadow(rehearsal);
    }
    #[cfg(target_os = "windows")]
    {
        use winit::platform::windows::WindowAttributesExtWindows;
        attributes = attributes
            .with_skip_taskbar(!rehearsal)
            .with_drag_and_drop(false);
    }
    let window = Arc::new(el.create_window(attributes).map_err(|e| e.to_string())?);
    #[cfg(target_os = "macos")]
    {
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(handle) = window.window_handle()
            && let RawWindowHandle::AppKit(handle) = handle.as_raw()
        {
            let view: &objc2_app_kit::NSView = unsafe { handle.ns_view.cast().as_ref() };
            if let Some(ns) = view.window() {
                let color = if transparent {
                    objc2_app_kit::NSColor::clearColor()
                } else {
                    objc2_app_kit::NSColor::blackColor()
                };
                ns.setBackgroundColor(Some(&color));
                ns.setOpaque(!transparent);
            }
        }
        crate::mac::make_nonfocusable(&window);
    }
    #[cfg(target_os = "windows")]
    if !rehearsal {
        use windows::Win32::{Foundation::HWND, UI::WindowsAndMessaging::*};
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(handle) = window.window_handle()
            && let RawWindowHandle::Win32(handle) = handle.as_raw()
        {
            let hwnd = HWND(handle.hwnd.get() as _);
            unsafe {
                let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
                SetWindowLongPtrW(
                    hwnd,
                    GWL_EXSTYLE,
                    ((style | WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0) & !WS_EX_APPWINDOW.0)
                        as isize,
                );
            }
        }
    }
    Ok(window)
}

pub fn show(window: &Window, rehearsal: bool) {
    #[cfg(target_os = "macos")]
    if rehearsal {
        crate::mac::show_without_focus(window);
    } else {
        crate::mac::place_output(window);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = rehearsal;
        window.set_visible(true);
    }
    if !rehearsal {
        window.set_cursor_visible(false);
    }
}

pub fn rehearsal_bounds(display: &DisplayInfo, frame_height: u32) -> Rect {
    let scale = display.scale_factor;
    let (width, height) = display.logical_size();
    let logical = Rect {
        x: (display.bounds.x as f64 / scale).round() as i32,
        y: (display.bounds.y as f64 / scale).round() as i32,
        width,
        height,
    };
    let bounds = crate::display::rehearsal_bounds(&logical, frame_height);
    Rect {
        x: (bounds.x as f64 * scale).round() as i32,
        y: (bounds.y as f64 * scale).round() as i32,
        width: (bounds.width as f64 * scale).round() as u32,
        height: (bounds.height as f64 * scale).round() as u32,
    }
}

pub fn native_id(window: &Window) -> u64 {
    use winit::raw_window_handle::HasWindowHandle;
    let Ok(handle) = window.window_handle() else {
        return 0;
    };
    #[cfg(target_os = "macos")]
    if let winit::raw_window_handle::RawWindowHandle::AppKit(handle) = handle.as_raw() {
        let view: &objc2_app_kit::NSView = unsafe { handle.ns_view.cast().as_ref() };
        return view.window().map_or(0, |w| w.windowNumber() as u64);
    }
    #[cfg(target_os = "windows")]
    if let winit::raw_window_handle::RawWindowHandle::Win32(handle) = handle.as_raw() {
        return handle.hwnd.get() as u64;
    }
    // No native window number elsewhere.
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = handle;
    0
}

/// Electron's window width/height include chrome; winit accepts content size.
pub fn set_bounds(window: &Window, bounds: &Rect) {
    let outer = window.outer_size();
    let inner = window.inner_size();
    let dx = outer.width.saturating_sub(inner.width);
    let dy = outer.height.saturating_sub(inner.height);
    window.set_outer_position(PhysicalPosition::new(bounds.x, bounds.y));
    let _ = window.request_inner_size(PhysicalSize::new(
        bounds.width.saturating_sub(dx).max(1),
        bounds.height.saturating_sub(dy).max(1),
    ));
}
