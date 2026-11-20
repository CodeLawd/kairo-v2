//! Short-lived local-only identify and physical shape windows. They never
//! navigate, execute JavaScript or join the shared remote-page registry.
use kairo_engine_proto::{DisplayInfo, Rect, WindowInfo, WindowKind};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use winit::{event_loop::ActiveEventLoop, window::Window};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use wry::{WebView, WebViewBuilder};

#[derive(Default)]
pub struct Diagnostics {
    windows: Vec<Transient>,
}
struct Transient {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    _view: WebView,
    window: Arc<Window>,
    alive: Arc<AtomicBool>,
    target: DisplayInfo,
    info: WindowInfo,
    until: Instant,
}
impl Drop for Transient {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Release);
        self.window.set_visible(false);
    }
}
impl Diagnostics {
    pub fn active(&self) -> bool {
        !self.windows.is_empty()
    }
    pub fn deadline(&self) -> Option<Instant> {
        self.windows.iter().map(|w| w.until).min()
    }
    pub fn info(&self) -> Vec<WindowInfo> {
        self.windows.iter().map(|w| w.info.clone()).collect()
    }
    pub fn expire(&mut self, now: Instant) {
        self.windows.retain(|w| now < w.until);
    }
    pub fn topology(&mut self, displays: &[DisplayInfo]) {
        self.windows
            .retain(|w| displays.iter().any(|d| d == &w.target));
    }
    pub fn identify(&mut self, el: &ActiveEventLoop, displays: &[DisplayInfo]) -> Vec<String> {
        let mut errors = Vec::new();
        for (index, display) in displays.iter().enumerate() {
            let scale = display.scale_factor;
            let width = (480. * scale).round() as u32;
            let height = (240. * scale).round() as u32;
            let bounds = Rect {
                x: display.bounds.x + (display.bounds.width as f64 / 2.).round() as i32
                    - (240. * scale).round() as i32,
                y: display.bounds.y + (display.bounds.height as f64 / 2.).round() as i32
                    - (120. * scale).round() as i32,
                width,
                height,
            };
            let html = identify_html(index + 1, &display.description(), display.hosts_main_window);
            if let Err(error) = self.open(
                el,
                display,
                WindowKind::Identify,
                bounds,
                false,
                html,
                Duration::from_secs(3),
            ) {
                errors.push(error);
            }
        }
        errors
    }
    pub fn pattern(
        &mut self,
        el: &ActiveEventLoop,
        display: &DisplayInfo,
        name: &str,
        shape: &str,
        frame_height: u32,
    ) -> Result<(), String> {
        let bounds = if display.hosts_main_window {
            crate::window_policy::rehearsal_bounds(display, frame_height)
        } else {
            display.bounds.clone()
        };
        let scale = (bounds.width as f64 / 1920.).min(bounds.height as f64 / frame_height as f64);
        let frame = (
            1920. * scale / bounds.width as f64,
            frame_height as f64 * scale / bounds.height as f64,
        );
        let html = pattern_html(name, shape, &format!("1920 × {frame_height}"), frame);
        self.open(
            el,
            display,
            WindowKind::Pattern,
            bounds,
            display.hosts_main_window,
            html,
            Duration::from_secs(5),
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn open(
        &mut self,
        el: &ActiveEventLoop,
        display: &DisplayInfo,
        kind: WindowKind,
        bounds: Rect,
        rehearsal: bool,
        html: String,
        duration: Duration,
    ) -> Result<(), String> {
        // Remove the old request before opening. There is no detached timer:
        // only this entry's deadline can remove this entry.
        self.windows
            .retain(|w| w.info.display != display.id || w.info.kind != kind);
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = (el, display, bounds, rehearsal, html, duration);
            Err("Native display diagnostics are unavailable on this platform".into())
        }
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            let transparent = kind == WindowKind::Identify;
            // Diagnostic windows are always borderless/nonactivating, including
            // a half-sized shape card on the control display.
            let window = crate::window_policy::open(
                el,
                "Kairo display check",
                &bounds,
                display.scale_factor,
                false,
                transparent,
            )?;
            let alive = Arc::new(AtomicBool::new(true));
            let ready = alive.clone();
            let shown = window.clone();
            let view = WebViewBuilder::new()
                .with_html(html)
                .with_javascript_disabled()
                .with_transparent(transparent)
                .with_background_color(if transparent {
                    (0, 0, 0, 0)
                } else {
                    (0, 0, 0, 255)
                })
                .with_bounds(wry::Rect {
                    position: winit::dpi::LogicalPosition::new(0., 0.).into(),
                    size: winit::dpi::LogicalSize::new(
                        bounds.width as f64 / display.scale_factor,
                        bounds.height as f64 / display.scale_factor,
                    )
                    .into(),
                })
                .with_navigation_handler(|url| url == "about:blank")
                .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
                .with_download_started_handler(|_, _| false)
                .with_on_page_load_handler(move |event, _| {
                    if matches!(event, wry::PageLoadEvent::Finished)
                        && ready.load(Ordering::Acquire)
                    {
                        crate::window_policy::show(&shown, false);
                    }
                })
                .build_as_child(&*window)
                .map_err(|e| e.to_string())?;
            let native_window = crate::window_policy::native_id(&window);
            self.windows.push(Transient {
                _view: view,
                window,
                alive,
                target: display.clone(),
                info: WindowInfo {
                    output: String::new(),
                    display: display.id,
                    kind,
                    rehearsal,
                    bounds,
                    native_window,
                },
                until: Instant::now() + duration,
            });
            Ok(())
        }
    }
}
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
const CSP: &str = "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'\">";
fn identify_html(number: usize, description: &str, control: bool) -> String {
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8">{CSP}</head><body style="margin:0;height:100vh;display:flex;align-items:center;justify-content:center;background:transparent;font-family:-apple-system,'Segoe UI',sans-serif;cursor:default">
<div style="background:rgba(12,17,29,.9);color:#F9FAFB;border:2px solid #F9FAFB;border-radius:20px;padding:24px 36px;text-align:center">
<div style="font-size:88px;font-weight:700;line-height:1">{number}</div>
<div style="margin-top:10px;font-size:18px;opacity:.85">{description}</div>{control}</div></body></html>"#,
        description = esc(description),
        control = if control {
            r#"<div style="margin-top:6px;font-size:13px;color:#A29F96">Kairo’s controls are here</div>"#
        } else {
            ""
        }
    )
}
fn pattern_html(name: &str, shape: &str, size: &str, frame: (f64, f64)) -> String {
    let w = format!("{:.3}", frame.0 * 100.);
    let h = format!("{:.3}", frame.1 * 100.);
    let left = (100. - w.parse::<f64>().unwrap()) / 2.;
    let top = (100. - h.parse::<f64>().unwrap()) / 2.;
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8">{CSP}</head><body style="margin:0;height:100vh;background:#000;overflow:hidden;font-family:-apple-system,'Segoe UI',sans-serif;cursor:default">
<div style="position:absolute;left:{left}%;top:{top}%;width:{w}%;height:{h}%;box-sizing:border-box;background:#F5C518;border:6px solid #11120D;background-image:linear-gradient(rgba(17,18,13,.18) 2px,transparent 2px),linear-gradient(90deg,rgba(17,18,13,.18) 2px,transparent 2px);background-size:10% 10%;display:flex;flex-direction:column;align-items:center;justify-content:center;color:#11120D;text-align:center">
<div style="font-size:min(7vw,7vh);font-weight:800;letter-spacing:-.02em">{name}</div>
<div style="margin-top:1.5vh;font-size:min(4vw,4vh);font-weight:600">{shape}</div>
<div style="margin-top:1vh;font-size:min(2.4vw,2.4vh);opacity:.7">{size} frame · black bars are outside the frame</div></div></body></html>"#,
        name = esc(if name.is_empty() { "Screen" } else { name }),
        shape = esc(shape),
        size = esc(size)
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identify_content_is_numbered_escaped_and_marks_only_the_control_display() {
        let card = identify_html(2, "<Display>& · 3840×2160", true);
        assert!(card.contains(">2</div>"));
        assert!(card.contains("&lt;Display&gt;&amp;"));
        assert!(card.contains("Kairo’s controls are here"));
        assert!(!identify_html(1, "A", false).contains("Kairo’s controls are here"));
    }
    #[test]
    fn pattern_has_the_reference_grid_and_three_lines_with_rounded_letterbox_geometry() {
        let card = pattern_html("<script>", "4:3", "1920 × 1440", (0.75, 1.));
        assert!(card.contains("left:12.5%"));
        assert!(card.contains("width:75.000%"));
        assert!(card.contains("background:#F5C518"));
        assert!(card.contains("background-size:10% 10%"));
        assert!(card.contains("&lt;script&gt;"));
        assert!(card.contains("1920 × 1440 frame · black bars are outside the frame"));
    }
}
