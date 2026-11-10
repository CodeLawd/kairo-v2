//! The engine's winit loop: surfaces, their windows, and the client.

use crate::display;
use crate::program::Shell;
use crate::server::{Message, Outbox};
use kairo_compositor::thread::{Compositor, CompositorEvent, Sink};
use kairo_engine_proto::*;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::window::{Window, WindowId};

/// Display changes are noticed this quickly. Prompt 060 adds the 250 ms
/// debounce of `surface-manager.ts` on top; parking happens at once.
const DISPLAY_POLL: Duration = Duration::from_millis(250);
const REPORT_EVERY: Duration = Duration::from_secs(2);
/// Exit status when the GPU device is lost: the supervisor relaunches the
/// engine and replays every surface.
pub const EXIT_DEVICE_LOST: u8 = 75;

struct OutputWindow {
    window: Arc<Window>,
    display: u32,
    sink: DisplaySink,
}

struct EngineSurface {
    state: SurfaceState,
    /// The newest command applied to this surface.
    seq: u64,
    status: Option<SurfaceStatus>,
    window: Option<OutputWindow>,
    stats: SinkStats,
    /// The `overlay.html` shell state of the open window.
    shell: Shell,
}

impl EngineSurface {
    fn output(&self) -> &str {
        &self.state.spec.output
    }
}

pub struct Engine {
    start: Instant,
    outbox: Outbox,
    proxy: EventLoopProxy<Message>,
    grace: Duration,
    compositor: Option<Compositor>,
    gpu_error: Option<String>,
    /// Set when the engine must exit with a failure status.
    pub exit_code: Option<u8>,
    surfaces: Vec<EngineSurface>,
    displays: Vec<DisplayInfo>,
    next_scan: Instant,
    next_report: Instant,
    /// Set while no client is connected, including before the first one.
    orphaned_at: Option<Instant>,
}

impl Engine {
    pub fn new(outbox: Outbox, proxy: EventLoopProxy<Message>, grace: Duration) -> Self {
        let now = Instant::now();
        Self {
            start: now,
            outbox,
            proxy,
            grace,
            compositor: None,
            gpu_error: None,
            exit_code: None,
            surfaces: Vec::new(),
            displays: Vec::new(),
            next_scan: now,
            next_report: now + REPORT_EVERY,
            orphaned_at: Some(now),
        }
    }

    fn send_event(&self, event: Event) {
        self.outbox.send(&FromEngine::Event(event));
    }

    fn error(&self, output: Option<&str>, message: String) {
        tracing::error!(output, "{message}");
        self.send_event(Event::Error {
            output: output.map(str::to_owned),
            message,
        });
    }

    fn surface_mut(&mut self, output: &str) -> Option<&mut EngineSurface> {
        self.surfaces.iter_mut().find(|s| s.output() == output)
    }

    fn command(&mut self, event_loop: &ActiveEventLoop, seq: u64, command: Command) {
        match &command {
            Command::Shutdown => {
                tracing::info!("shutdown requested");
                self.surfaces.clear();
                self.compositor = None;
                event_loop.exit();
                return;
            }
            Command::RetainSurfaces { outputs } => {
                self.surfaces
                    .retain(|s| outputs.iter().any(|o| o == s.output()));
            }
            Command::OpenSurface(spec) => match self.surface_mut(&spec.output) {
                Some(surface) => {
                    surface.state.apply(&command, true);
                    surface.seq = seq;
                }
                None => self.surfaces.push(EngineSurface {
                    state: SurfaceState::new(spec.clone()),
                    seq,
                    status: None,
                    window: None,
                    stats: SinkStats {
                        output: spec.output.clone(),
                        ..SinkStats::default()
                    },
                    shell: Shell::new(&spec.output, frame_height_of(&spec.sink)),
                }),
            },
            Command::CloseSurface {
                output,
                destroy: true,
            } => self.surfaces.retain(|s| s.output() != output),
            _ => {
                let output = command.output().expect("surface command").to_owned();
                let Some(surface) = self.surfaces.iter_mut().find(|s| s.output() == output) else {
                    self.error(
                        Some(&output),
                        format!("command for unknown surface {output}"),
                    );
                    return;
                };
                let before = surface.state.clone();
                surface.state.apply(&command, true);
                surface.seq = seq;
                let ops = surface.shell.command(&command, &before, &surface.state);
                // Every applied command gets a frame, so `FrameCommitted`
                // follows even when no pixels change.
                if surface.window.is_some()
                    && let Some(compositor) = self.compositor.as_ref()
                {
                    compositor.apply(&output, seq, ops);
                }
                if !matches!(
                    command,
                    Command::CloseSurface { .. } | Command::RestoreSurface { .. }
                ) {
                    return;
                }
            }
        }
        self.reconcile(event_loop);
    }

    fn query(&self, id: u64, query: Query) {
        let has = |output: &str| self.surfaces.iter().find(|s| s.output() == output);
        let reply = match query {
            // No background video plays in this engine yet (prompt 055), so
            // there is none to be still running.
            Query::VideoEnded { output } => match has(&output) {
                Some(_) => Reply::VideoEnded(true),
                None => Reply::UnknownSurface,
            },
            // Document video playback arrives with prompt 058.
            Query::DocumentPlayback { output } => match has(&output) {
                Some(_) => Reply::DocumentPlayback(None),
                None => Reply::UnknownSurface,
            },
            Query::State => Reply::State(self.surfaces.iter().map(|s| s.state.clone()).collect()),
        };
        self.outbox.send(&FromEngine::Reply { id, reply });
    }

    /// Places each surface: a window on its display, parked when the
    /// display is gone, unavailable when its sink cannot run here.
    fn reconcile(&mut self, event_loop: &ActiveEventLoop) {
        let mut claimed = HashSet::new();
        for index in 0..self.surfaces.len() {
            let spec = self.surfaces[index].state.spec.clone();
            let status = match &spec.sink {
                SinkSpec::Display(sink) => {
                    if self.surfaces[index].state.parked {
                        self.close_window(index);
                        continue;
                    }
                    let resolved =
                        display::resolve(&sink.binding, &self.displays, &claimed).cloned();
                    match resolved {
                        None => {
                            // Never placed on whatever display is left.
                            self.close_window(index);
                            SurfaceStatus::Parked {
                                reason: "Display not connected".into(),
                            }
                        }
                        Some(target) => {
                            claimed.insert(target.id);
                            let current = self.surfaces[index].window.as_ref();
                            if current.is_some_and(|w| w.display == target.id && &w.sink == sink) {
                                continue;
                            }
                            self.close_window(index);
                            match self.open_window(event_loop, index, sink, &target) {
                                Ok(()) => {
                                    tracing::info!(
                                        output = spec.output,
                                        "window on {} (rehearsal {})",
                                        target.label,
                                        sink.rehearsal
                                    );
                                    SurfaceStatus::Ready
                                }
                                Err(reason) => {
                                    self.error(
                                        Some(&spec.output),
                                        format!("output window failed: {reason}"),
                                    );
                                    SurfaceStatus::Unavailable { reason }
                                }
                            }
                        }
                    }
                }
                SinkSpec::Ndi { .. } => unavailable("NDI output"),
                SinkSpec::Syphon { .. } => unavailable("Syphon output"),
                SinkSpec::Device { .. } => unavailable("Video card output"),
            };
            let surface = &mut self.surfaces[index];
            if surface.status.as_ref() != Some(&status) {
                surface.status = Some(status.clone());
                let output = surface.output().to_owned();
                self.send_event(Event::SurfaceStatus { output, status });
            }
        }
    }

    /// The compositor, started on first use so an engine without display
    /// windows needs no GPU.
    fn compositor(&mut self) -> Result<&Compositor, String> {
        if self.compositor.is_none() {
            if let Some(error) = &self.gpu_error {
                return Err(error.clone());
            }
            let proxy = self.proxy.clone();
            match Compositor::start(move |event| {
                let _ = proxy.send_event(Message::Compositor(event));
            }) {
                Ok(compositor) => self.compositor = Some(compositor),
                Err(error) => {
                    self.gpu_error = Some(error.clone());
                    return Err(error);
                }
            }
        }
        Ok(self.compositor.as_ref().expect("compositor"))
    }

    fn close_window(&mut self, index: usize) {
        let surface = &mut self.surfaces[index];
        if surface.window.take().is_some()
            && let Some(compositor) = self.compositor.as_ref()
        {
            compositor.close(&surface.state.spec.output);
        }
    }

    /// `createDisplayWindow`: borderless over the whole display, or a
    /// half-size titled window when the display also holds the controls.
    /// The new window shows the surface's state at once (`ready`).
    fn open_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        index: usize,
        sink: &DisplaySink,
        target: &DisplayInfo,
    ) -> Result<(), String> {
        let label = self.surfaces[index].state.spec.label.clone();
        let output = self.surfaces[index].state.spec.output.clone();
        let bounds = if sink.rehearsal {
            display::rehearsal_bounds(&target.bounds, sink.frame_height)
        } else {
            target.bounds.clone()
        };
        let title = if sink.rehearsal {
            format!("{label} (rehearsal)")
        } else {
            label
        };
        // On macOS borderless windows are ordered front by `place_output`,
        // which does not activate the engine.
        let visible = sink.rehearsal || cfg!(not(target_os = "macos"));
        #[allow(unused_mut)]
        let mut attributes = Window::default_attributes()
            .with_title(title)
            .with_visible(visible)
            .with_active(false)
            .with_decorations(sink.rehearsal)
            .with_resizable(sink.rehearsal)
            .with_transparent(false)
            .with_inner_size(PhysicalSize::new(bounds.width, bounds.height))
            .with_position(PhysicalPosition::new(bounds.x, bounds.y));
        #[cfg(target_os = "macos")]
        {
            use winit::platform::macos::WindowAttributesExtMacOS;
            attributes = attributes.with_has_shadow(sink.rehearsal);
        }
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|e| e.to_string())?,
        );
        #[cfg(target_os = "macos")]
        crate::mac::make_nonfocusable(&window);
        let compositor = self.compositor()?;
        // The surface is made here, on the window's thread; the compositor
        // thread configures and presents it.
        let surface = compositor
            .instance()
            .create_surface(window.clone())
            .map_err(|e| e.to_string())?;
        let size = window.inner_size();
        compositor.open(
            &output,
            sink.frame_height,
            Sink::Window {
                surface,
                size: (size.width, size.height),
            },
        );
        let surface = &mut self.surfaces[index];
        surface.shell = Shell::new(&output, sink.frame_height);
        let ops = surface.shell.rebuild(&surface.state);
        let seq = surface.seq;
        self.compositor
            .as_ref()
            .expect("started above")
            .apply(&output, seq, ops);
        if !sink.rehearsal {
            window.set_cursor_visible(false);
            #[cfg(target_os = "macos")]
            crate::mac::place_output(&window);
        } else {
            #[cfg(target_os = "macos")]
            crate::mac::show_without_focus(&window);
        }
        self.surfaces[index].window = Some(OutputWindow {
            window,
            display: target.id,
            sink: sink.clone(),
        });
        Ok(())
    }

    fn output_of(&self, id: WindowId) -> Option<String> {
        self.surfaces
            .iter()
            .find(|s| s.window.as_ref().is_some_and(|w| w.window.id() == id))
            .map(|s| s.output().to_owned())
    }

    fn compositor_event(&mut self, event_loop: &ActiveEventLoop, event: CompositorEvent) {
        match event {
            CompositorEvent::Committed { output, seq } => {
                self.send_event(Event::FrameCommitted { output, seq });
            }
            CompositorEvent::Dropped { output, why } => {
                tracing::debug!(output, "frame not presented: {why}");
                if let Some(surface) = self.surface_mut(&output) {
                    surface.stats.dropped += 1;
                }
            }
            CompositorEvent::Error { output, message } => self.error(output.as_deref(), message),
            CompositorEvent::DeviceLost(why) => {
                self.error(None, format!("GPU device lost: {why}"));
                self.exit_code = Some(EXIT_DEVICE_LOST);
                event_loop.exit();
            }
        }
    }

    fn report(&mut self) {
        // Frames drawn, fades included, as the compositor counts them.
        if let Some(compositor) = self.compositor.as_ref() {
            for surface in &mut self.surfaces {
                if surface.window.is_some()
                    && let Some(frames) = compositor.frames(surface.state.spec.output.as_str())
                {
                    surface.stats.produced = frames;
                }
            }
        }
        let windows = self.surfaces.iter().filter(|s| s.window.is_some()).count();
        self.send_event(Event::Health(Health {
            uptime_ms: self.start.elapsed().as_millis() as u64,
            surfaces: self.surfaces.len() as u32,
            windows: windows as u32,
            gpu: self.compositor.is_some(),
        }));
        self.send_event(Event::Stats(
            self.surfaces.iter().map(|s| s.stats.clone()).collect(),
        ));
    }
}

fn frame_height_of(sink: &SinkSpec) -> u32 {
    match sink {
        SinkSpec::Display(display) => display.frame_height,
        // Offscreen surfaces paint 1920 × 1080 (`HEIGHT`).
        _ => 1080,
    }
}

fn unavailable(what: &str) -> SurfaceStatus {
    // Sinks other than displays arrive with prompts 061 to 064.
    SurfaceStatus::Unavailable {
        reason: format!("{what} is not available in this engine build"),
    }
}

impl ApplicationHandler<Message> for Engine {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.displays.is_empty() {
            self.displays = display::scan(event_loop);
            tracing::info!("{} display(s)", self.displays.len());
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, message: Message) {
        match message {
            Message::Connected(id) => {
                if self.orphaned_at.take().is_some()
                    && self.start.elapsed() > Duration::from_secs(1)
                {
                    tracing::info!("client {id} connected; grace cancelled");
                }
                self.send_event(Event::Displays(self.displays.clone()));
                for surface in &self.surfaces {
                    if let Some(status) = surface.status.clone() {
                        self.send_event(Event::SurfaceStatus {
                            output: surface.output().to_owned(),
                            status,
                        });
                    }
                }
            }
            Message::Disconnected(id) => {
                tracing::warn!(
                    "client {id} gone; holding the picture for {} ms",
                    self.grace.as_millis()
                );
                self.orphaned_at = Some(Instant::now());
            }
            Message::Ping { nonce } => self.outbox.send(&FromEngine::Pong { nonce }),
            Message::Query { id, query } => self.query(id, query),
            Message::Command { seq, command } => self.command(event_loop, seq, command),
            Message::Compositor(event) => self.compositor_event(event_loop, event),
        }
    }

    fn window_event(&mut self, _: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(output) = self.output_of(id) else {
            return;
        };
        let Some(compositor) = self.compositor.as_ref() else {
            return;
        };
        match event {
            // Ignored: as in Electron, only turning the output off closes
            // it, so the window and the settings cannot disagree.
            WindowEvent::CloseRequested => {}
            WindowEvent::Resized(size) => compositor.resize(&output, (size.width, size.height)),
            WindowEvent::RedrawRequested | WindowEvent::Occluded(false) => {
                compositor.invalidate(&output);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if let Some(at) = self.orphaned_at
            && now.duration_since(at) >= self.grace
        {
            tracing::warn!("no client for {} ms; exiting", self.grace.as_millis());
            event_loop.exit();
            return;
        }
        if now >= self.next_scan {
            self.next_scan = now + DISPLAY_POLL;
            let displays = display::scan(event_loop);
            if displays != self.displays {
                tracing::info!("displays changed: {} now", displays.len());
                self.displays = displays;
                self.send_event(Event::Displays(self.displays.clone()));
                self.reconcile(event_loop);
            }
        }
        if now >= self.next_report {
            self.next_report = now + REPORT_EVERY;
            if self.outbox.connected() {
                self.report();
            }
        }
        let mut wake = self.next_scan.min(self.next_report);
        if let Some(at) = self.orphaned_at {
            wake = wake.min(at + self.grace);
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(wake));
    }
}
