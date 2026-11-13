//! Windows.Graphics.Capture on the Wry/WebView2 source window. Runtime and
//! performance qualification are deferred by the owner; no timings are invented.
use super::{Signals, address, observed_scroll, sandbox_response};
use kairo_compositor::{Raster, thread::SourceFeed};
use kairo_engine_proto::WebInput;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};
use webview2_com::{
    CallDevToolsProtocolMethodCompletedHandler, DevToolsProtocolEventReceivedEventHandler,
    Microsoft::Web::WebView2::Win32::*, NavigationCompletedEventHandler,
    PermissionRequestedEventHandler, ProcessFailedEventHandler, take_pwstr,
};
use windows::{
    Graphics::{
        Capture::*,
        DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat},
    },
    Win32::{
        Foundation::{HMODULE, HWND, LPARAM, WPARAM},
        Graphics::{
            Direct3D::D3D_DRIVER_TYPE_HARDWARE,
            Direct3D11::*,
            Dxgi::{Common::DXGI_FORMAT_B8G8R8A8_UNORM, IDXGIDevice},
        },
        System::{
            Performance::{QueryPerformanceCounter, QueryPerformanceFrequency},
            WinRT::{
                Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess},
                Graphics::Capture::IGraphicsCaptureItemInterop,
            },
        },
        UI::WindowsAndMessaging::*,
    },
    core::{BOOL, Interface, PCWSTR, PWSTR, factory, w},
};
use winit::{
    dpi::{PhysicalPosition, PhysicalSize},
    event_loop::ActiveEventLoop,
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::Window,
};
use wry::{WebView, WebViewBuilder, WebViewExtWindows};

const OBSERVE: &str = r#"(()=>{if(self!==top)return;let q=false;addEventListener('scroll',()=>{if(q)return;q=true;requestAnimationFrame(()=>{q=false;window.ipc.postMessage(JSON.stringify({type:'scroll',x:scrollX,y:scrollY}))})},{passive:true})})()"#;

pub struct Page {
    view: WebView,
    capture: Capture,
    _window: Window,
    hwnd: HWND,
    signals: Rc<RefCell<Signals>>,
    tokens: [i64; 3],
    last_error: Option<String>,
    sandbox: ICoreWebView2DevToolsProtocolEventReceiver,
    sandbox_token: i64,
    initial_url: String,
    retry_at: Instant,
}
impl Page {
    pub fn new(
        el: &ActiveEventLoop,
        url: &str,
        restore: Option<(f64, f64)>,
    ) -> Result<Self, String> {
        Self::open(el, url, restore).map_err(|e| e.to_string())
    }
    fn open(
        el: &ActiveEventLoop,
        url: &str,
        restore: Option<(f64, f64)>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let window = el.create_window(
            Window::default_attributes()
                .with_title("Kairo live web source")
                .with_inner_size(PhysicalSize::new(1920, 1080))
                .with_decorations(false)
                .with_resizable(false)
                .with_active(false),
        )?;
        let RawWindowHandle::Win32(raw) = window.window_handle()?.as_raw() else {
            return Err("no Win32 handle".into());
        };
        let hwnd = HWND(raw.hwnd.get() as *mut _);
        unsafe {
            SetWindowPos(hwnd, Some(HWND_BOTTOM), 0, 0, 1920, 1080, SWP_NOACTIVATE)?;
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(
                hwnd,
                GWL_EXSTYLE,
                ex | WS_EX_NOACTIVATE.0 as isize
                    | WS_EX_TOOLWINDOW.0 as isize
                    | WS_EX_TRANSPARENT.0 as isize,
            );
        }
        let signals = Rc::new(RefCell::new(Signals::default()));
        let observe = signals.clone();
        let view = WebViewBuilder::new()
            .with_bounds(wry::Rect {
                position: PhysicalPosition::new(0, 0).into(),
                size: PhysicalSize::new(1920, 1080).into(),
            })
            .with_background_color((255, 255, 255, 255))
            .with_autoplay(true)
            .with_initialization_script(OBSERVE)
            .with_ipc_handler(move |r| {
                if let Some(p) = observed_scroll(r.body()) {
                    observe.borrow_mut().scroll = Some(p);
                }
            })
            .with_navigation_handler(|u| address(&u).is_some() || u == "about:blank")
            .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
            .with_download_started_handler(|_, _| false)
            .build_as_child(&window)?;
        let core = view.webview();
        let controller = view.controller();
        unsafe {
            // One CSS pixel per captured pixel, independent of monitor DPI.
            let scale: ICoreWebView2Controller3 = controller.cast()?;
            scale.SetShouldDetectMonitorScaleChanges(false)?;
            scale.SetRasterizationScale(1.)?;
            let settings3: ICoreWebView2Settings3 = core.Settings()?.cast()?;
            settings3.SetAreBrowserAcceleratorKeysEnabled(false)?;
            let settings = core.Settings()?;
            settings.SetAreHostObjectsAllowed(false)?;
            settings.SetAreDevToolsEnabled(false)?;
            settings.SetAreDefaultContextMenusEnabled(false)?;
        }
        // Pause every HTTP(S) document at response headers and inject a CSP
        // sandbox before any untrusted script can execute. URL-only navigation
        // callbacks cannot distinguish an iframe navigating its ancestor.
        let sandbox = unsafe { core.GetDevToolsProtocolEventReceiver(w!("Fetch.requestPaused"))? };
        let mut sandbox_token = 0;
        let browser = core.clone();
        let out = signals.clone();
        unsafe {
            sandbox.add_DevToolsProtocolEventReceived(
                &DevToolsProtocolEventReceivedEventHandler::create(Box::new(move |_, args| {
                    if let Some(args) = args {
                        let mut value = PWSTR::null();
                        args.ParameterObjectAsJson(&mut value)?;
                        match sandbox_response(&take_pwstr(value)) {
                            Ok((method, params)) => {
                                let name: Vec<u16> = method.encode_utf16().chain(Some(0)).collect();
                                let params: Vec<u16> =
                                    params.encode_utf16().chain(Some(0)).collect();
                                let out = out.clone();
                                let stop = browser.clone();
                                browser.CallDevToolsProtocolMethod(
                                    PCWSTR(name.as_ptr()),
                                    PCWSTR(params.as_ptr()),
                                    &CallDevToolsProtocolMethodCompletedHandler::create(Box::new(
                                        move |result, _| {
                                            if let Err(error) = result {
                                                out.borrow_mut().error = Some(format!(
                                                    "Cannot enforce web sandbox: {error}"
                                                ));
                                                let _ = stop.Stop();
                                            }
                                            Ok(())
                                        },
                                    )),
                                )?;
                            }
                            Err(error) => {
                                out.borrow_mut().error = Some(error.into());
                                browser.Stop()?;
                            }
                        }
                    }
                    Ok(())
                })),
                &mut sandbox_token,
            )?;
        }
        let mut tokens = [0; 3];
        unsafe {
            core.add_PermissionRequested(
                &PermissionRequestedEventHandler::create(Box::new(|_, a| {
                    if let Some(a) = a {
                        let mut kind = COREWEBVIEW2_PERMISSION_KIND::default();
                        a.PermissionKind(&mut kind)?;
                        // Deny device, location, clipboard and other permission prompts.
                        // EME uses the installed browser engine, not this permission enum.
                        a.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY)?;
                    }
                    Ok(())
                })),
                &mut tokens[0],
            )?;
            let out = signals.clone();
            let crashes = Rc::new(RefCell::new((Instant::now(), 0u32)));
            core.add_ProcessFailed(
                &ProcessFailedEventHandler::create(Box::new(move |view, _| {
                    let mut c = crashes.borrow_mut();
                    if c.0.elapsed() > Duration::from_secs(60) {
                        *c = (Instant::now(), 0);
                    }
                    c.1 += 1;
                    let mut s = out.borrow_mut();
                    s.crashed = true;
                    s.error = Some(
                        "WebView2 renderer failed; reloading (up to three times per minute)".into(),
                    );
                    drop(s);
                    if c.1 <= 3
                        && let Some(view) = view
                    {
                        let mut value = PWSTR::null();
                        view.Source(&mut value)?;
                        let url = take_pwstr(value);
                        enable_sandbox(&view, &url, &out)?;
                    }
                    Ok(())
                })),
                &mut tokens[1],
            )?;
            let out = signals.clone();
            core.add_NavigationCompleted(
                &NavigationCompletedEventHandler::create(Box::new(move |_, args| {
                    if let Some(args) = args {
                        let mut ok = BOOL::default();
                        args.IsSuccess(&mut ok)?;
                        let mut s = out.borrow_mut();
                        s.loaded = ok.as_bool();
                        if !ok.as_bool() {
                            s.error = Some("web navigation failed".into());
                        }
                    }
                    Ok(())
                })),
                &mut tokens[2],
            )?;
        }
        let capture = Capture::new(hwnd)?;
        enable_sandbox(&core, url, &signals)?;
        let page = Self {
            view,
            capture,
            _window: window,
            hwnd,
            signals,
            tokens,
            last_error: None,
            sandbox,
            sandbox_token,
            initial_url: url.into(),
            retry_at: Instant::now(),
        };
        if let Some((x, y)) = restore {
            page.scroll(x, y);
        }
        Ok(page)
    }
    pub fn reload(&mut self) {
        if self.last_error.is_some() {
            match Capture::new(self.hwnd) {
                Ok(c) => self.capture = c,
                Err(e) => self.signals.borrow_mut().error = Some(e.to_string()),
            }
        }
        let url = self.location().unwrap_or_else(|| self.initial_url.clone());
        if let Err(e) = enable_sandbox(&self.view.webview(), &url, &self.signals) {
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
        let mut target = HWND::default();
        // Windowed WebView2 receives native Win32 input at its render widget.
        // If the installed runtime changes this HWND hierarchy, report failure
        // instead of sending input to some unrelated window or the desktop.
        unsafe extern "system" fn find(hwnd: HWND, data: LPARAM) -> BOOL {
            let mut name = [0u16; 128];
            let n = unsafe { GetClassNameW(hwnd, &mut name) };
            if String::from_utf16_lossy(&name[..n.max(0) as usize]) == "Chrome_RenderWidgetHostHWND"
            {
                unsafe { *(data.0 as *mut HWND) = hwnd };
                return false.into();
            }
            true.into()
        }
        unsafe {
            let _ = EnumChildWindows(
                Some(self.hwnd),
                Some(find),
                LPARAM((&mut target as *mut HWND) as isize),
            );
        }
        if target.is_invalid() {
            self.signals.borrow_mut().error =
                Some("WebView2 input target is unavailable in this runtime".into());
            return;
        }
        unsafe {
            match input {
                WebInput::Move { x, y } | WebInput::Click { x, y } => {
                    let xy = LPARAM(((*y as isize) << 16) | (*x as isize & 0xffff));
                    SendMessageW(target, WM_MOUSEMOVE, Some(WPARAM(0)), Some(xy));
                    if matches!(input, WebInput::Click { .. }) {
                        SendMessageW(target, WM_LBUTTONDOWN, Some(WPARAM(1)), Some(xy));
                        SendMessageW(target, WM_LBUTTONUP, Some(WPARAM(0)), Some(xy));
                    }
                }
                WebInput::Text { text } => {
                    for ch in text.encode_utf16() {
                        SendMessageW(target, WM_CHAR, Some(WPARAM(ch as usize)), Some(LPARAM(1)));
                    }
                }
                WebInput::Key { key } => {
                    let key = match key.as_str() {
                        "Enter" => 13,
                        "Tab" => 9,
                        "Backspace" => 8,
                        "Delete" => 46,
                        "Escape" => 27,
                        "ArrowLeft" => 37,
                        "ArrowUp" => 38,
                        "ArrowRight" => 39,
                        "ArrowDown" => 40,
                        "Home" => 36,
                        "End" => 35,
                        "PageUp" => 33,
                        "PageDown" => 34,
                        _ => return,
                    };
                    SendMessageW(target, WM_KEYDOWN, Some(WPARAM(key)), Some(LPARAM(1)));
                    SendMessageW(
                        target,
                        WM_KEYUP,
                        Some(WPARAM(key)),
                        Some(LPARAM(0xc0000001u32 as isize)),
                    );
                }
            }
        }
    }
    pub fn capture(&mut self, now: Instant, feed: &SourceFeed, ids: Vec<u64>) {
        if self.last_error.is_some() && now >= self.retry_at {
            self.retry_at = now + Duration::from_secs(1);
            if let Ok(capture) = Capture::new(self.hwnd) {
                self.capture = capture;
            }
        }
        match self.capture.frame(now) {
            Ok(Some((pixels, started))) => {
                let ready = Instant::now();
                let frame = Raster::new(1920, 1080, pixels);
                for id in ids {
                    feed.rgba_frame(id, frame.clone(), started, ready);
                }
                self.last_error = None;
            }
            Ok(None) => {}
            Err(e) => {
                let e = e.to_string();
                if self.last_error.as_ref() != Some(&e) {
                    self.signals.borrow_mut().error = Some(e.clone());
                    self.last_error = Some(e);
                }
            }
        }
    }
    pub fn location(&self) -> Option<String> {
        self.view.url().ok().as_deref().and_then(address)
    }
    pub fn drain(&mut self) -> (Signals, Option<String>) {
        let signals = std::mem::take(&mut *self.signals.borrow_mut());
        (signals, self.location())
    }
}
impl Drop for Page {
    fn drop(&mut self) {
        unsafe {
            let core = self.view.webview();
            let _ = core.remove_PermissionRequested(self.tokens[0]);
            let _ = core.remove_ProcessFailed(self.tokens[1]);
            let _ = core.remove_NavigationCompleted(self.tokens[2]);
            let _ = self
                .sandbox
                .remove_DevToolsProtocolEventReceived(self.sandbox_token);
            let _ = core.Stop();
        }
    }
}

fn enable_sandbox(
    core: &ICoreWebView2,
    url: &str,
    signals: &Rc<RefCell<Signals>>,
) -> windows::core::Result<()> {
    let browser = core.clone();
    let out = signals.clone();
    let url = url.to_owned();
    let params = r#"{"patterns":[{"urlPattern":"http*","resourceType":"Document","requestStage":"Response"}]}"#;
    let params: Vec<u16> = params.encode_utf16().chain(Some(0)).collect();
    unsafe {
        core.CallDevToolsProtocolMethod(
            w!("Fetch.enable"),
            PCWSTR(params.as_ptr()),
            &CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |result, _| {
                if let Err(error) = result {
                    out.borrow_mut().error = Some(format!(
                        "Web sandbox unavailable; refusing to load the page: {error}"
                    ));
                    return Ok(());
                }
                if let Some(url) = address(&url) {
                    let url: Vec<u16> = url.encode_utf16().chain(Some(0)).collect();
                    browser.Navigate(PCWSTR(url.as_ptr()))?;
                }
                Ok(())
            })),
        )
    }
}

struct Capture {
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    context: ID3D11DeviceContext,
    staging: ID3D11Texture2D,
    last: Instant,
    stamp: i64,
}
impl Capture {
    fn new(hwnd: HWND) -> windows::core::Result<Self> {
        if !GraphicsCaptureSession::IsSupported()? {
            return Err(windows::core::Error::new(
                windows::core::HRESULT(0x80004001u32 as i32),
                "Windows.Graphics.Capture is unavailable",
            ));
        }
        unsafe {
            let mut device = None;
            let mut context = None;
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )?;
            let device = device.ok_or_else(|| {
                windows::core::Error::new(
                    windows::core::HRESULT(0x80004005u32 as i32),
                    "D3D11 returned no resource",
                )
            })?;
            let context = context.ok_or_else(|| {
                windows::core::Error::new(
                    windows::core::HRESULT(0x80004005u32 as i32),
                    "D3D11 returned no resource",
                )
            })?;
            let dxgi: IDXGIDevice = device.cast()?;
            let direct: IDirect3DDevice = CreateDirect3D11DeviceFromDXGIDevice(&dxgi)?.cast()?;
            let interop: IGraphicsCaptureItemInterop =
                factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
            let item: GraphicsCaptureItem = interop.CreateForWindow(hwnd)?;
            let size = item.Size()?;
            if size.Width != 1920 || size.Height != 1080 {
                return Err(windows::core::Error::new(
                    windows::core::HRESULT(0x80070057u32 as i32),
                    "capture source must be exactly 1920x1080",
                ));
            }
            let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
                &direct,
                DirectXPixelFormat::B8G8R8A8UIntNormalized,
                2,
                size,
            )?;
            let session = pool.CreateCaptureSession(&item)?;
            session.SetIsCursorCaptureEnabled(false)?;
            let _ =
                session.SetMinUpdateInterval(windows::Foundation::TimeSpan { Duration: 333333 });
            let desc = D3D11_TEXTURE2D_DESC {
                Width: 1920,
                Height: 1080,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_STAGING,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                ..Default::default()
            };
            let mut staging = None;
            device.CreateTexture2D(&desc, None, Some(&mut staging))?;
            session.StartCapture()?;
            Ok(Self {
                pool,
                session,
                context,
                staging: staging.ok_or_else(|| {
                    windows::core::Error::new(
                        windows::core::HRESULT(0x80004005u32 as i32),
                        "D3D11 returned no resource",
                    )
                })?,
                last: Instant::now(),
                stamp: 0,
            })
        }
    }
    fn frame(&mut self, now: Instant) -> windows::core::Result<Option<(Vec<u8>, Instant)>> {
        let mut latest = None;
        for _ in 0..2 {
            match self.pool.TryGetNextFrame() {
                Ok(f) => {
                    if let Some(old) = latest.replace(f) {
                        let _ = old.Close();
                    }
                }
                // A null result is the documented empty-pool case. Device
                // loss/closed-session errors must be reported and retried.
                Err(error) if error.code().0 as u32 == 0x80004003 => break,
                Err(error) => return Err(error),
            }
        }
        let Some(frame) = latest else {
            if self.stamp == 0 && self.last.elapsed() > Duration::from_secs(2) {
                return Err(windows::core::Error::new(
                    windows::core::HRESULT(0x800705B4u32 as i32),
                    "no capture frames for two seconds; ensure the source is not minimized",
                ));
            }
            return Ok(None);
        };
        let result = (|| unsafe {
            let stamp = frame.SystemRelativeTime()?.Duration;
            if stamp == self.stamp {
                return Ok(None);
            }
            self.stamp = stamp;
            let size = frame.ContentSize()?;
            if size.Width != 1920 || size.Height != 1080 {
                return Err(windows::core::Error::new(
                    windows::core::HRESULT(0x80070057u32 as i32),
                    "capture dimensions changed",
                ));
            }
            let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
            let texture: ID3D11Texture2D = access.GetInterface()?;
            self.context.CopyResource(&self.staging, &texture);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context
                .Map(&self.staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
            if mapped.pData.is_null() || mapped.RowPitch < 1920 * 4 {
                self.context.Unmap(&self.staging, 0);
                return Err(windows::core::Error::new(
                    windows::core::HRESULT(0x80004005u32 as i32),
                    "invalid mapped capture pixels",
                ));
            }
            let mut pixels = vec![0; 1920 * 1080 * 4];
            for y in 0..1080usize {
                let row = std::slice::from_raw_parts(
                    (mapped.pData as *const u8).add(y * mapped.RowPitch as usize),
                    1920 * 4,
                );
                for (x, p) in row.as_chunks::<4>().0.iter().enumerate() {
                    let dst = (y * 1920 + x) * 4;
                    pixels[dst..dst + 4].copy_from_slice(&[p[2], p[1], p[0], 255]);
                }
            }
            self.context.Unmap(&self.staging, 0);
            self.last = now;
            let mut qpc = 0;
            let mut freq = 0;
            QueryPerformanceCounter(&mut qpc)?;
            QueryPerformanceFrequency(&mut freq)?;
            let age = (qpc as f64 / freq as f64 - stamp as f64 / 10_000_000.).clamp(0., 60.);
            Ok(Some((
                pixels,
                Instant::now()
                    .checked_sub(Duration::from_secs_f64(age))
                    .unwrap_or(now),
            )))
        })();
        let _ = frame.Close();
        result
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}
