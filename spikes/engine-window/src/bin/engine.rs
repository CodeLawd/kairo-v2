//! Child process: borderless output windows with winit and wgpu.

use engine_window_spike::*;
use interprocess::local_socket::{GenericNamespaced, ListenerOptions, ToNsName, prelude::*};
use std::collections::{HashMap, HashSet};
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use wgpu::*;
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

const SHADER: &str = r#"
struct U { t: f32 }
@group(0) @binding(0) var<uniform> u: U;
struct V { @builtin(position) p: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vs(@builtin(vertex_index) i: u32) -> V {
  let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
  return V(vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0), uv);
}
@fragment fn fs(v: V) -> @location(0) vec4<f32> {
  let bar = step(abs(fract(v.uv.x - u.t * 0.2) - 0.5), 0.02);
  return vec4<f32>(v.uv.x, v.uv.y, 0.5 + 0.5 * sin(u.t), 1.0) + vec4<f32>(bar);
}
"#;

enum Message {
    Connected,
    Disconnected,
    Command(Command),
}

type Sender = Arc<Mutex<Option<Arc<LocalSocketStream>>>>;

fn send(sender: &Sender, event: &Event) {
    if let Some(stream) = sender.lock().unwrap().as_ref() {
        let _ = write_message(&mut &**stream, event);
    }
}

fn log(start: Instant, line: &str) {
    eprintln!("[engine {:>9.3}s] {line}", start.elapsed().as_secs_f64());
}

fn serve(socket: String, proxy: EventLoopProxy<Message>, sender: Sender) -> std::io::Result<()> {
    let name = socket.to_ns_name::<GenericNamespaced>()?;
    let listener = ListenerOptions::new()
        .name(name)
        .try_overwrite(true)
        .create_sync()?;
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let stream = Arc::new(stream);
        *sender.lock().unwrap() = Some(stream.clone());
        if proxy.send_event(Message::Connected).is_err() {
            return Ok(());
        }
        let mut reader = BufReader::new(&*stream);
        while let Ok(command) = read_message::<Command>(&mut reader) {
            // Answered here so the measurement isolates the transport.
            if let Command::Ping(value) = command {
                send(&sender, &Event::Pong(value));
            } else if proxy.send_event(Message::Command(command)).is_err() {
                return Ok(());
            }
        }
        *sender.lock().unwrap() = None;
        if proxy.send_event(Message::Disconnected).is_err() {
            return Ok(());
        }
    }
    Ok(())
}

struct Output {
    window: Arc<Window>,
    surface: Surface<'static>,
    config: SurfaceConfiguration,
    display: u32,
    reported: bool,
    rehearsal: bool,
}

struct Gpu {
    instance: Instance,
    adapter: Adapter,
    device: Device,
    queue: Queue,
    shader: ShaderModule,
    layout: PipelineLayout,
    bind_group: BindGroup,
    uniform: Buffer,
    pipelines: HashMap<TextureFormat, RenderPipeline>,
    preview: Texture,
    readback: Buffer,
}

impl Gpu {
    fn new() -> Result<Self, String> {
        let instance = Instance::new(InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = pollster::block_on(instance.request_adapter(&RequestAdapterOptions {
            power_preference: PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(|e| e.to_string())?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&DeviceDescriptor::default()))
                .map_err(|e| e.to_string())?;
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: None,
            source: ShaderSource::Wgsl(SHADER.into()),
        });
        let uniform = device.create_buffer(&BufferDescriptor {
            label: None,
            size: 16,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: None,
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: None,
            layout: &group_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&group_layout)],
            immediate_size: 0,
        });
        let preview = device.create_texture(&TextureDescriptor {
            label: None,
            size: Extent3d {
                width: PREVIEW_WIDTH,
                height: PREVIEW_HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Bgra8Unorm,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let readback = device.create_buffer(&BufferDescriptor {
            label: None,
            size: u64::from(PREVIEW_WIDTH * PREVIEW_HEIGHT * 4),
            usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            shader,
            layout,
            bind_group,
            uniform,
            pipelines: HashMap::new(),
            preview,
            readback,
        })
    }

    fn pipeline(&mut self, format: TextureFormat) {
        if self.pipelines.contains_key(&format) {
            return;
        }
        let pipeline = self
            .device
            .create_render_pipeline(&RenderPipelineDescriptor {
                label: None,
                layout: Some(&self.layout),
                vertex: VertexState {
                    module: &self.shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(FragmentState {
                    module: &self.shader,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(format.into())],
                }),
                multiview_mask: None,
                cache: None,
            });
        self.pipelines.insert(format, pipeline);
    }

    fn draw(&self, encoder: &mut CommandEncoder, view: &TextureView, format: TextureFormat) {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(Color::BLACK),
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipelines[&format]);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    fn capture_preview(&mut self, shm: &PreviewShm) {
        let format = TextureFormat::Bgra8Unorm;
        self.pipeline(format);
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let view = self.preview.create_view(&Default::default());
        self.draw(&mut encoder, &view, format);
        encoder.copy_texture_to_buffer(
            self.preview.as_image_copy(),
            TexelCopyBufferInfo {
                buffer: &self.readback,
                layout: TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(PREVIEW_WIDTH * 4),
                    rows_per_image: None,
                },
            },
            Extent3d {
                width: PREVIEW_WIDTH,
                height: PREVIEW_HEIGHT,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        let slice = self.readback.slice(..);
        slice.map_async(MapMode::Read, |_| {});
        if self.device.poll(PollType::wait_indefinitely()).is_ok()
            && let Ok(pixels) = slice.get_mapped_range()
        {
            shm.publish(&pixels);
        }
        self.readback.unmap();
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, Bool, Imp, Sel};
    use objc2::{msg_send, sel};
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSView, NSWindow, NSWindowCollectionBehavior,
        NSWindowStyleMask,
    };
    use objc2_foundation::MainThreadMarker;
    use std::sync::Arc;
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use winit::window::Window;

    /// NSScreenSaverWindowLevel, matching Electron's 'screen-saver' level.
    const SCREEN_SAVER_LEVEL: isize = 1000;

    unsafe extern "C-unwind" fn can_become(this: &NSWindow, _: Sel) -> Bool {
        Bool::new(this.styleMask().contains(NSWindowStyleMask::Titled))
    }

    fn ns_window(window: &Window) -> Option<Retained<NSWindow>> {
        let RawWindowHandle::AppKit(handle) = window.window_handle().ok()?.as_raw() else {
            return None;
        };
        // SAFETY: winit guarantees the handle is a live NSView on this thread.
        let view: &NSView = unsafe { handle.ns_view.cast().as_ref() };
        view.window()
    }

    /// Replaces winit's always-true key and main window answers, so that a
    /// borderless output cannot take focus even when clicked.
    pub fn make_nonfocusable(window: &Window) {
        let Some(ns) = ns_window(window) else { return };
        let class = AnyObject::class(&ns);
        // SAFETY: the signature matches `- (BOOL)canBecomeKeyWindow`.
        unsafe {
            let imp: Imp = std::mem::transmute(
                can_become as unsafe extern "C-unwind" fn(&NSWindow, Sel) -> Bool,
            );
            let types = c"B16@0:8".as_ptr();
            let class = class as *const _ as *mut _;
            objc2::ffi::class_replaceMethod(class, sel!(canBecomeKeyWindow), imp, types);
            objc2::ffi::class_replaceMethod(class, sel!(canBecomeMainWindow), imp, types);
        }
    }

    pub fn place_output(window: &Arc<Window>) {
        let Some(ns) = ns_window(window) else { return };
        ns.setLevel(SCREEN_SAVER_LEVEL);
        ns.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Stationary,
        );
        ns.setHasShadow(false);
        ns.orderFrontRegardless();
    }

    pub fn describe(window: &Window) -> String {
        let Some(ns) = ns_window(window) else {
            return "no NSWindow".into();
        };
        let marker = MainThreadMarker::new().expect("main thread");
        let app = NSApplication::sharedApplication(marker);
        let frame = ns.frame();
        let can_key: bool = unsafe { msg_send![&*ns, canBecomeKeyWindow] };
        format!(
            "policy_accessory={} app_active={} isKey={} canBecomeKey={can_key} level={} frame=({},{},{},{}) visible={}",
            app.activationPolicy() == NSApplicationActivationPolicy::Accessory,
            app.isActive(),
            ns.isKeyWindow(),
            ns.level(),
            frame.origin.x,
            frame.origin.y,
            frame.size.width,
            frame.size.height,
            ns.isVisible(),
        )
    }
}

struct Engine {
    start: Instant,
    gpu: Option<Gpu>,
    sender: Sender,
    outputs: HashMap<String, Output>,
    specs: Vec<OutputSpec>,
    parked: HashSet<String>,
    displays: Vec<DisplayInfo>,
    last_scan: Instant,
    shm: Option<PreviewShm>,
    preview_interval: Option<Duration>,
    next_preview: Instant,
    grace: Duration,
    orphaned_at: Option<Instant>,
    ready_pending: bool,
}

fn scan(event_loop: &ActiveEventLoop) -> Vec<DisplayInfo> {
    event_loop
        .available_monitors()
        .map(|monitor| {
            #[cfg(target_os = "macos")]
            let id = winit::platform::macos::MonitorHandleExtMacOS::native_id(&monitor);
            #[cfg(not(target_os = "macos"))]
            let id = {
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                monitor.name().hash(&mut hasher);
                hasher.finish() as u32
            };
            let position = monitor.position();
            let size = monitor.size();
            DisplayInfo {
                id,
                label: monitor.name().unwrap_or_default(),
                bounds: Rect {
                    x: position.x,
                    y: position.y,
                    width: size.width,
                    height: size.height,
                },
                scale_factor: monitor.scale_factor(),
            }
        })
        .collect()
}

impl Engine {
    fn build(
        &mut self,
        event_loop: &ActiveEventLoop,
        spec: &OutputSpec,
        display: &DisplayInfo,
    ) -> Result<Output, String> {
        let bounds = if spec.rehearsal {
            rehearsal_bounds(&display.bounds, spec.frame_height)
        } else {
            display.bounds.clone()
        };
        let mut attributes = Window::default_attributes()
            .with_title(if spec.rehearsal {
                format!("{} (rehearsal)", spec.name)
            } else {
                spec.name.clone()
            })
            .with_visible(spec.rehearsal)
            .with_active(false)
            .with_decorations(spec.rehearsal)
            .with_resizable(spec.rehearsal)
            .with_inner_size(PhysicalSize::new(bounds.width, bounds.height))
            .with_position(PhysicalPosition::new(bounds.x, bounds.y));
        #[cfg(target_os = "macos")]
        {
            use winit::platform::macos::WindowAttributesExtMacOS;
            attributes = attributes.with_has_shadow(spec.rehearsal);
        }
        attributes = attributes.with_transparent(false);
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|e| e.to_string())?,
        );
        #[cfg(target_os = "macos")]
        mac::make_nonfocusable(&window);
        let gpu = self.gpu.as_mut().ok_or("no GPU")?;
        let surface = gpu
            .instance
            .create_surface(window.clone())
            .map_err(|e| e.to_string())?;
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&gpu.adapter, size.width.max(1), size.height.max(1))
            .ok_or("surface unsupported by adapter")?;
        config.present_mode = PresentMode::AutoVsync;
        surface.configure(&gpu.device, &config);
        gpu.pipeline(config.format);
        if !spec.rehearsal {
            window.set_cursor_visible(false);
            #[cfg(target_os = "macos")]
            mac::place_output(&window);
        }
        Ok(Output {
            window,
            surface,
            config,
            display: display.id,
            reported: false,
            rehearsal: spec.rehearsal,
        })
    }

    /// Parks outputs whose display left and restores those whose display returned.
    fn reconcile(&mut self, event_loop: &ActiveEventLoop) {
        let mut claimed = HashSet::new();
        for spec in self.specs.clone() {
            let resolved = resolve_display(&spec.binding, &self.displays, &claimed).cloned();
            match resolved {
                Some(display) => {
                    claimed.insert(display.id);
                    if self
                        .outputs
                        .get(&spec.name)
                        .is_some_and(|o| o.display == display.id)
                    {
                        continue;
                    }
                    match self.build(event_loop, &spec, &display) {
                        Ok(output) => {
                            self.outputs.insert(spec.name.clone(), output);
                            if self.parked.remove(&spec.name) {
                                send(&self.sender, &Event::Restored(spec.name.clone()));
                            }
                            log(
                                self.start,
                                &format!("output {} on {}", spec.name, display.label),
                            );
                        }
                        Err(error) => {
                            log(self.start, &format!("build {} failed: {error}", spec.name))
                        }
                    }
                }
                None => {
                    self.outputs.remove(&spec.name);
                    if self.parked.insert(spec.name.clone()) {
                        send(&self.sender, &Event::Parked(spec.name.clone()));
                        log(self.start, &format!("output {} parked", spec.name));
                    }
                }
            }
        }
    }

    fn render(&mut self) {
        let Some(gpu) = self.gpu.as_mut() else { return };
        let time = self.start.elapsed().as_secs_f32();
        gpu.queue.write_buffer(&gpu.uniform, 0, &time.to_le_bytes());
        for output in self.outputs.values_mut() {
            let frame = match output.surface.get_current_texture() {
                CurrentSurfaceTexture::Success(frame)
                | CurrentSurfaceTexture::Suboptimal(frame) => frame,
                CurrentSurfaceTexture::Outdated | CurrentSurfaceTexture::Lost => {
                    let size = output.window.inner_size();
                    output.config.width = size.width.max(1);
                    output.config.height = size.height.max(1);
                    output.surface.configure(&gpu.device, &output.config);
                    continue;
                }
                _ => continue,
            };
            let view = frame.texture.create_view(&Default::default());
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            gpu.draw(&mut encoder, &view, output.config.format);
            gpu.queue.submit([encoder.finish()]);
            gpu.queue.present(frame);
            if !output.reported {
                output.reported = true;
                #[cfg(target_os = "macos")]
                send(
                    &self.sender,
                    &Event::Report(format!(
                        "rehearsal={} {}",
                        output.rehearsal,
                        mac::describe(&output.window)
                    )),
                );
            }
        }
        if let (Some(interval), Some(shm)) = (self.preview_interval, self.shm.as_ref())
            && Instant::now() >= self.next_preview
        {
            gpu.capture_preview(shm);
            self.next_preview += interval;
            if self.next_preview < Instant::now() {
                self.next_preview = Instant::now() + interval;
            }
        }
        if self.ready_pending {
            self.ready_pending = false;
            send(&self.sender, &Event::Ready);
        }
    }
}

impl ApplicationHandler<Message> for Engine {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_some() {
            return;
        }
        match Gpu::new() {
            Ok(gpu) => self.gpu = Some(gpu),
            Err(error) => {
                log(self.start, &format!("GPU unavailable: {error}"));
                event_loop.exit();
                return;
            }
        }
        self.displays = scan(event_loop);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, message: Message) {
        match message {
            Message::Connected => {
                if self.orphaned_at.take().is_some() {
                    log(self.start, "controller reconnected; grace cancelled");
                }
                send(&self.sender, &Event::Displays(self.displays.clone()));
            }
            Message::Disconnected => {
                log(self.start, "controller gone; holding picture");
                self.orphaned_at = Some(Instant::now());
            }
            Message::Command(Command::Configure {
                outputs,
                preview_fps,
            }) => {
                self.specs = outputs;
                self.preview_interval = (preview_fps > 0)
                    .then(|| Duration::from_secs_f64(1.0 / f64::from(preview_fps)));
                self.next_preview = Instant::now();
                self.ready_pending = true;
                self.reconcile(event_loop);
            }
            Message::Command(Command::Shutdown) => event_loop.exit(),
            Message::Command(Command::Ping(_)) => {}
        }
    }

    fn window_event(&mut self, _: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        // CloseRequested is ignored: as in Electron, only turning the output
        // off closes it, so the window and the settings cannot disagree.
        if let WindowEvent::Resized(size) = event
            && let Some(gpu) = self.gpu.as_ref()
            && let Some(output) = self.outputs.values_mut().find(|o| o.window.id() == id)
            && size.width > 0
            && size.height > 0
        {
            output.config.width = size.width;
            output.config.height = size.height;
            output.surface.configure(&gpu.device, &output.config);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self
            .orphaned_at
            .is_some_and(|at| at.elapsed() >= self.grace)
        {
            log(self.start, "grace period expired; exiting");
            event_loop.exit();
            return;
        }
        if self.last_scan.elapsed() >= Duration::from_secs(1) {
            self.last_scan = Instant::now();
            let displays = scan(event_loop);
            if displays != self.displays {
                log(
                    self.start,
                    &format!("displays changed: {} now", displays.len()),
                );
                self.displays = displays;
                send(&self.sender, &Event::Displays(self.displays.clone()));
                self.reconcile(event_loop);
            }
        }
        self.render();
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(16),
        ));
    }
}

fn argument(name: &str) -> Option<String> {
    let mut arguments = std::env::args();
    arguments.find(|a| a == name)?;
    arguments.next()
}

fn main() {
    let start = Instant::now();
    let socket = argument("--socket").expect("--socket is required");
    let grace = argument("--grace-ms")
        .and_then(|v| v.parse().ok())
        .unwrap_or(10_000);
    let mut builder = EventLoop::<Message>::with_user_event();
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        builder.with_activation_policy(ActivationPolicy::Accessory);
        builder.with_activate_ignoring_other_apps(false);
    }
    let event_loop = builder.build().expect("event loop");
    let sender: Sender = Arc::new(Mutex::new(None));
    let proxy = event_loop.create_proxy();
    let thread_sender = sender.clone();
    std::thread::spawn(move || {
        if let Err(error) = serve(socket, proxy, thread_sender) {
            eprintln!("[engine] socket failed: {error}");
            std::process::exit(2);
        }
    });
    let shm = argument("--shm")
        .map(PathBuf::from)
        .and_then(|path| match PreviewShm::open(&path) {
            Ok(shm) => Some(shm),
            Err(error) => {
                log(start, &format!("preview disabled: {error}"));
                None
            }
        });
    let mut engine = Engine {
        start,
        gpu: None,
        sender,
        outputs: HashMap::new(),
        specs: Vec::new(),
        parked: HashSet::new(),
        displays: Vec::new(),
        last_scan: Instant::now(),
        shm,
        preview_interval: None,
        next_preview: Instant::now(),
        grace: Duration::from_millis(grace),
        orphaned_at: None,
        ready_pending: false,
    };
    event_loop.run_app(&mut engine).expect("event loop run");
}
