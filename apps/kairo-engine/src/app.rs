//! The engine's winit loop: surfaces, their windows, and the client.

use crate::display;
use crate::program::{Planned, Shell};
use crate::server::{Message, Outbox};
use crate::text::{Ordered, TextWork, Timing, Wait, text_slot};
use crate::visuals::{Output, VisualWork};
use kairo_compositor::thread::{Compositor, CompositorEvent, Sink};
use kairo_engine_proto::*;
use std::collections::{HashSet, VecDeque};
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
/// How often countdowns and clocks are read (`setInterval(…, 250)`).
const TICK: Duration = Duration::from_millis(250);
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
    /// Commands for this surface, in order, some waiting for slide text.
    queue: Ordered,
    /// Slides applied and not yet reported committed: for the trace line.
    traces: VecDeque<(u64, Instant, Timing)>,
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
    /// Draws slide text off the winit and compositor threads.
    text: TextWork,
    /// Reads pictures and draws vector elements off the winit thread.
    visuals: VisualWork,
    next_job: u64,
    displays: Vec<DisplayInfo>,
    next_scan: Instant,
    next_report: Instant,
    next_tick: Instant,
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
            text: TextWork::new(),
            visuals: VisualWork::new(),
            next_job: 0,
            displays: Vec::new(),
            next_scan: now,
            next_report: now + REPORT_EVERY,
            next_tick: now,
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
                    queue: Ordered::default(),
                    traces: VecDeque::new(),
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
                let planned = surface.shell.command(&command, &before, &surface.state);
                // Every applied command gets a frame, so `FrameCommitted`
                // follows even when no pixels change.
                if surface.window.is_some() {
                    self.dispatch(&output, seq, planned);
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

    /// Hand a command's operations to the compositor, after the words of any
    /// slide in it have been drawn and every earlier command for the surface
    /// has been applied.
    fn dispatch(&mut self, output: &str, seq: u64, planned: Planned) {
        let received = Instant::now();
        let mut waits = Vec::new();
        if let Some(request) = planned.text
            && let Some(slot) = text_slot(&planned.ops)
        {
            let job = self.next_job;
            self.next_job += 1;
            let proxy = self.proxy.clone();
            let owner = output.to_owned();
            self.text.submit(request, move |done| {
                let _ = proxy.send_event(Message::Text {
                    output: owner,
                    job,
                    done,
                });
            });
            waits.push(Wait::Text(job, slot));
        }
        for request in planned.visuals {
            let job = self.next_job;
            self.next_job += 1;
            let send = |proxy: winit::event_loop::EventLoopProxy<Message>,
                        owner: String,
                        slot,
                        key: String,
                        generation| {
                move |o: Output| {
                    let _ = proxy.send_event(Message::Visual {
                        output: owner,
                        job,
                        slot,
                        key,
                        generation,
                        output_kind: o,
                    });
                }
            };
            let make = |this: &Self| {
                send(
                    this.proxy.clone(),
                    output.to_owned(),
                    request.slot,
                    request.key.clone(),
                    request.generation,
                )
            };
            if request.blocking {
                waits.push(Wait::Visual(job, request.op));
                self.visuals.submit(
                    request.job,
                    Some(Box::new(make(self))),
                    Box::new(make(self)),
                );
            } else {
                self.visuals.submit(request.job, None, Box::new(make(self)));
            }
        }
        if let Some(surface) = self.surface_mut(output) {
            surface
                .queue
                .push_waiting(seq, planned.ops, waits, received);
        }
        self.flush(output);
    }

    /// A worker finished a layer's pixels.
    fn visual_done(
        &mut self,
        output: &str,
        job: u64,
        slot: kairo_compositor::Slot,
        key: String,
        generation: u64,
        result: Output,
    ) {
        let Some(surface) = self.surfaces.iter_mut().find(|s| s.output() == output) else {
            return;
        };
        match result {
            // The first stage of an elements layer, which the slide waits for.
            Output::Fast(content) if surface.queue.complete_visual(job, content.clone()) => {}
            Output::Fast(_) => {}
            // Pictures: an update to the node, unless a newer request replaced it.
            Output::Full(content) | Output::Image(Some(content)) => {
                if surface.shell.is_current(slot, generation) {
                    surface.queue.push_waiting(
                        0,
                        vec![kairo_compositor::thread::Op::Update { slot, key, content }],
                        Vec::new(),
                        Instant::now(),
                    );
                }
            }
            Output::Image(None) => {
                tracing::warn!(output, "a background picture could not be shown");
            }
            Output::Layer(out) => {
                let content = surface.shell.layer_done(slot, &key, generation, out);
                // The first result fills the command waiting for it; a later
                // one (with its pictures) updates the node.
                if !surface.queue.complete_visual(job, content.clone())
                    && surface.shell.is_current(slot, generation)
                {
                    surface.queue.push_waiting(
                        0,
                        vec![kairo_compositor::thread::Op::Update { slot, key, content }],
                        Vec::new(),
                        Instant::now(),
                    );
                }
            }
            Output::Live(out) => {
                if let Some(op) = surface.shell.live_done(slot, generation, *out) {
                    surface
                        .queue
                        .push_waiting(0, vec![op], Vec::new(), Instant::now());
                }
            }
        }
        self.flush(output);
    }

    /// Apply what is ready, in order.
    fn flush(&mut self, output: &str) {
        let Some(compositor) = self.compositor.as_ref() else {
            return;
        };
        let Some(surface) = self.surfaces.iter_mut().find(|s| s.output() == output) else {
            return;
        };
        for ready in surface.queue.ready() {
            if let Some(timing) = ready.text {
                if timing.failed {
                    tracing::error!(
                        output,
                        seq = ready.seq,
                        "slide text could not be built; showing none"
                    );
                }
                surface
                    .traces
                    .push_back((ready.seq, ready.received, timing));
            }
            compositor.apply(output, ready.seq, ready.ops);
        }
    }

    /// Countdowns and clocks: every 250 ms, each readout whose text,
    /// colour or dimming changed is redrawn on a worker, alone.
    fn tick(&mut self) {
        let now = crate::layers::now();
        let mut jobs = Vec::new();
        for surface in &mut self.surfaces {
            for job in surface.shell.tick(&now) {
                jobs.push((surface.output().to_owned(), job));
            }
        }
        for (output, (slot, key, generation, live)) in jobs {
            let proxy = self.proxy.clone();
            let job = self.next_job;
            self.next_job += 1;
            self.visuals.submit(
                crate::visuals::Job::Live(live),
                None,
                Box::new(move |o| {
                    let _ = proxy.send_event(Message::Visual {
                        output,
                        job,
                        slot,
                        key,
                        generation,
                        output_kind: o,
                    });
                }),
            );
        }
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
        surface.queue.clear();
        surface.traces.clear();
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
        surface.queue.clear();
        surface.traces.clear();
        let planned = surface.shell.rebuild(&surface.state);
        let seq = surface.seq;
        self.dispatch(&output, seq, planned);
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
                if let Some(surface) = self.surface_mut(&output) {
                    while let Some(&(slide, received, t)) = surface.traces.front()
                        && slide <= seq
                    {
                        surface.traces.pop_front();
                        // The measure of push to commit: from the command
                        // reaching the engine to its frame being presented.
                        tracing::info!(
                            output,
                            seq = slide,
                            push_to_commit_ms =
                                format!("{:.2}", received.elapsed().as_secs_f64() * 1000.0),
                            build_ms = format!("{:.2}", t.build_ms),
                            raster_ms = format!("{:.2}", t.raster_ms),
                            cached = t.cached,
                            "slide text committed"
                        );
                    }
                }
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
            Message::Visual {
                output,
                job,
                slot,
                key,
                generation,
                output_kind,
            } => self.visual_done(&output, job, slot, key, generation, output_kind),
            Message::Text { output, job, done } => {
                if let Some(surface) = self.surface_mut(&output) {
                    surface.queue.complete(job, &done);
                }
                self.flush(&output);
            }
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
        let ticking = self.surfaces.iter().any(|s| !s.shell.live.is_empty());
        if ticking && now >= self.next_tick {
            self.next_tick = now + TICK;
            self.tick();
        }
        let mut wake = self.next_scan.min(self.next_report);
        if ticking {
            wake = wake.min(self.next_tick);
        }
        if let Some(at) = self.orphaned_at {
            wake = wake.min(at + self.grace);
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(wake));
    }
}
