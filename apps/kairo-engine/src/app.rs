//! The engine's winit loop: surfaces, their windows, and the client.

use crate::display;
use crate::program::{Planned, Role, Shell};
use crate::server::{Message, Outbox};
use crate::text::{Ordered, TextWork, Timing, Wait, text_slot};
use crate::visuals::{Output, VisualWork};
use kairo_compositor::thread::{Compositor, CompositorEvent, MediaControl, Sink, SourceFeed};
use kairo_engine_proto::*;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::window::{Window, WindowId};

/// Display changes are noticed this quickly. Prompt 060 adds the 250 ms
/// debounce of `surface-manager.ts` on top; parking happens at once.
const DISPLAY_POLL: Duration = Duration::from_millis(50);
const REPORT_EVERY: Duration = Duration::from_secs(2);
/// How often video sound is checked against its picture.
const SOUND_FOLLOW: Duration = Duration::from_millis(50);
/// NDI frames are 1920 × 1080, as Electron's NDI surface painted them.
const NDI_HEIGHT: u32 = 1080;
/// How often countdowns and clocks are read (`setInterval(…, 250)`).
const TICK: Duration = Duration::from_millis(250);
/// Exit status when the GPU device is lost: the supervisor relaunches the
/// engine and replays every surface.
pub const EXIT_DEVICE_LOST: u8 = 75;

struct OutputWindow {
    window: Arc<Window>,
    display: u32,
    sink: DisplaySink,
    target: DisplayInfo,
}

struct EngineSurface {
    preview_open: bool,
    state: SurfaceState,
    /// The newest command applied to this surface.
    seq: u64,
    status: Option<SurfaceStatus>,
    window: Option<OutputWindow>,
    /// The Syphon server an offscreen Syphon surface publishes to.
    syphon: Option<kairo_sinks::syphon::SyphonChannel>,
    stats: SinkStats,
    /// The `overlay.html` shell state of the open window.
    shell: Shell,
    /// Commands for this surface, in order, some waiting for slide text.
    queue: Ordered,
    /// Slides applied and not yet reported committed: for the trace line.
    traces: VecDeque<(u64, Instant, Timing)>,
    /// Page turns waiting for their decode: job to command sequence.
    pages: HashMap<u64, u64>,
    /// Pages being decoded ahead of the click.
    preloads: HashSet<u64>,
    /// Decoded pages kept on the GPU, newest last (`PAGE_CACHE`).
    resident: VecDeque<Arc<kairo_compositor::Raster>>,
    /// The NDI feed an NDI surface sends to (prompt 061).
    ndi: Option<crate::ndi::Feed>,
    /// The video card port this surface plays to (062).
    card: Option<Arc<kairo_sinks::video_devices::Managed>>,
}

/// Pages kept on the GPU per surface: the one shown, the ones either side
/// and one spare, so back-and-forth turns never upload.
const PAGE_CACHE: usize = 4;

impl EngineSurface {
    /// Remembers `raster` as the newest page; the oldest past the bound goes.
    fn keep_page(
        &mut self,
        raster: Arc<kairo_compositor::Raster>,
    ) -> Vec<Arc<kairo_compositor::Raster>> {
        self.resident.retain(|r| !Arc::ptr_eq(r, &raster));
        self.resident.push_back(raster);
        while self.resident.len() > PAGE_CACHE {
            self.resident.pop_front();
        }
        self.resident.iter().cloned().collect()
    }
}

impl EngineSurface {
    fn output(&self) -> &str {
        &self.state.spec.output
    }

    /// Drawn somewhere: a window, an NDI feed or a running Syphon server.
    fn is_open(&self) -> bool {
        self.preview_open
            || self.window.is_some()
            || self.ndi.is_some()
            || self.syphon.as_ref().is_some_and(|c| c.running())
            || self.card.is_some()
    }
}

pub struct Engine {
    start: Instant,
    outbox: Outbox,
    proxy: EventLoopProxy<Message>,
    grace: Duration,
    /// Background and element videos, fed to the compositor.
    videos: Option<kairo_video::Videos>,
    /// Live cameras, fed to the compositor.
    cameras: Option<kairo_camera::Cameras>,
    web: crate::web::Webs,
    next_web: Instant,
    web_timings: Vec<(u64, u64)>,
    shutting_down: bool,
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
    control_native: Option<Option<u64>>,
    display_debounce: display::Debounce,
    diagnostics: crate::diagnostics::Diagnostics,
    power: crate::power::Power,
    #[cfg(target_os = "macos")]
    display_observer: Option<crate::mac::DisplayObserver>,
    next_scan: Instant,
    next_report: Instant,
    /// Live → Start Capture (prompt 064).
    capture: crate::capture::Capture,
    /// Operator preview maps by output (prompt 066).
    previews: HashMap<String, crate::preview::Feed>,
    next_capture: Instant,
    next_tick: Instant,
    /// Program sound; made with the first surface.
    sound: Option<crate::sound::Sound>,
    /// The NDI runtime and feeds; the primary sender starts at launch.
    ndi: crate::ndi::Ndi,
    /// Video cards; started with the first card surface or query.
    devices: Option<kairo_sinks::video_devices::VideoDevices>,
    /// The card bindings last sent, so an unchanged reconcile sends none.
    device_bindings: Vec<kairo_sinks::video_devices::Binding>,
    next_sound: Instant,
    /// Sound glitch frames already reported.
    glitches: u64,
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
            videos: None,
            cameras: None,
            web: crate::web::Webs::default(),
            next_web: now,
            web_timings: Vec::new(),
            shutting_down: false,
            compositor: None,
            gpu_error: None,
            exit_code: None,
            surfaces: Vec::new(),
            text: TextWork::new(),
            visuals: VisualWork::new(),
            next_job: 0,
            displays: Vec::new(),
            control_native: None,
            display_debounce: display::Debounce::default(),
            diagnostics: crate::diagnostics::Diagnostics::default(),
            power: crate::power::Power::default(),
            #[cfg(target_os = "macos")]
            display_observer: None,
            next_scan: now,
            next_report: now + REPORT_EVERY,
            capture: crate::capture::Capture::new(Arc::new(crate::capture::System)),
            previews: HashMap::new(),
            next_capture: now,
            next_tick: now,
            sound: None,
            ndi: crate::ndi::Ndi::launch(),
            devices: None,
            device_bindings: Vec::new(),
            next_sound: now,
            glitches: 0,
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
        if self.shutting_down {
            return;
        }
        if let Command::SetWeb {
            output,
            web: Some(w),
        } = &command
            && (crate::web::address(&w.url).is_none() || !w.reload.is_finite())
        {
            self.error(
                Some(output),
                "Invalid live web address or reload value".into(),
            );
            return;
        }
        if let Command::WebInput { output, input } = &command
            && !crate::web::input_valid(input)
        {
            self.error(Some(output), "Invalid web input".into());
            return;
        }
        if let Command::SetStage { output, view } = &command
            && view.decode::<kairo_domain::program::StageView>().is_err()
        {
            self.error(Some(output), "Invalid stage view".into());
            return;
        }
        if let Command::OpenSurface(spec) = &command
            && let SinkSpec::Display(sink) = &spec.sink
            && !(360..=4320).contains(&sink.frame_height)
        {
            self.error(Some(&spec.output), "Invalid display frame height".into());
            return;
        }
        if matches!(command, Command::Shutdown) {
            self.web = crate::web::Webs::default();
        }
        self.command_inner(event_loop, seq, command.clone());
        self.sync_web(event_loop);
        match &command {
            Command::WebScroll { output, x, y } => self.web.scroll(output, *x, *y),
            Command::WebInput { output, input } => self.web.input(output, input),
            Command::WebPointer {
                output,
                point: Some((x, y)),
            } if x.is_finite() && y.is_finite() => self.web.input(
                output,
                &WebInput::Move {
                    x: x.clamp(0., 1920.),
                    y: y.clamp(0., 1080.),
                },
            ),
            _ => {}
        }
    }

    fn sync_web(&mut self, event_loop: &ActiveEventLoop) {
        let Some(compositor) = self.compositor.as_ref() else {
            return;
        };
        let wanted = self
            .surfaces
            .iter()
            .filter(|s| s.is_open() && !s.state.parked && s.state.config.web_enabled)
            .filter_map(|s| {
                s.state
                    .layers
                    .web
                    .clone()
                    .map(|w| (s.output().to_owned(), w))
            })
            .collect();
        let notices = self.web.sync(event_loop, wanted, &compositor.feed());
        self.web_notices(notices);
    }

    fn web_notices(&mut self, notices: Vec<crate::web::Notice>) {
        for notice in notices {
            match notice {
                crate::web::Notice::Navigated { output, url } => {
                    let planned = self.surface_mut(&output).and_then(|surface| {
                        if surface
                            .state
                            .layers
                            .web
                            .as_ref()
                            .is_none_or(|w| w.url == url)
                        {
                            return None;
                        }
                        let before = surface.state.clone();
                        surface.state.layers.web.as_mut()?.url = url.clone();
                        let command = Command::SetWeb {
                            output: output.clone(),
                            web: surface.state.layers.web.clone(),
                        };
                        Some(surface.shell.command(&command, &before, &surface.state))
                    });
                    if let Some(planned) = planned {
                        self.dispatch(&output, 0, planned);
                    }
                    self.send_event(Event::WebNavigated { output, url });
                }
                crate::web::Notice::Error(error) => self.error(None, error),
            }
        }
    }

    fn command_inner(&mut self, event_loop: &ActiveEventLoop, seq: u64, command: Command) {
        match &command {
            Command::ControlDisplay { native_id } => {
                self.control_native = Some(*native_id);
                self.scan_displays(event_loop);
                return;
            }
            Command::RefreshVideoDevices => {
                self.devices().refresh();
                return;
            }
            Command::IdentifyDisplays => {
                let displays = self.current_displays(event_loop);
                for error in self.diagnostics.identify(event_loop, &displays) {
                    self.error(None, error);
                }
                self.update_power();
                return;
            }
            Command::DisplayTestPattern {
                display,
                name,
                shape,
                frame_height,
            } => {
                if !(360..=4320).contains(frame_height) || name.len() > 1024 || shape.len() > 256 {
                    self.error(None, "Invalid display test pattern".into());
                    return;
                }
                let target = self
                    .current_displays(event_loop)
                    .into_iter()
                    .find(|d| d.id == *display);
                match target {
                    Some(target) => {
                        if let Err(error) = self.diagnostics.pattern(
                            event_loop,
                            &target,
                            name,
                            shape,
                            *frame_height,
                        ) {
                            self.error(None, error);
                        }
                    }
                    None => self.error(None, "Display not connected".into()),
                }
                self.update_power();
                return;
            }
            Command::StartCapture {
                output,
                source_name,
                settings,
            } => {
                let settings = settings
                    .decode::<kairo_domain::capture::CaptureSettings>()
                    .unwrap_or_default();
                let aspect = self.capture_aspect(output);
                self.capture.start(output, source_name, &settings, aspect);
                self.attach_capture(output);
                self.next_capture = Instant::now() + crate::capture::POLL;
                self.send_capture();
                return;
            }
            Command::StopCapture => {
                if let Some(output) = self.capture.output().map(str::to_owned)
                    && let Some(compositor) = self.compositor.as_ref()
                {
                    compositor.tap(&output, kairo_compositor::thread::Tap::Record, None);
                }
                self.capture.stop();
                self.send_capture();
                return;
            }
            Command::Shutdown => {
                // `before-quit`: the recording's file is finished first.
                if self.capture.recording() {
                    tracing::info!("finishing the recording before quitting");
                    self.capture.finish_before_quit();
                }
                self.diagnostics = crate::diagnostics::Diagnostics::default();
                self.power.set_active(false);
                tracing::info!("shutdown requested");
                for index in 0..self.surfaces.len() {
                    self.close_window(index);
                }
                self.surfaces.clear();
                // Quit releases the cards first, bounded at 2 s.
                if let Some(devices) = self.devices.take()
                    && !devices.close(Duration::from_secs(2))
                {
                    tracing::warn!("video card release timed out, quitting anyway");
                }
                self.sound = None;
                self.cameras = None;
                if let Some(mut videos) = self.videos.take() {
                    videos.shutdown();
                }
                self.shutting_down = true;
                if let Some(compositor) = self.compositor.as_ref() {
                    compositor.begin_shutdown();
                } else {
                    event_loop.exit();
                }
                return;
            }
            Command::RetainSurfaces { outputs } => {
                if let Some(cameras) = self.cameras.as_ref() {
                    for surface in &self.surfaces {
                        if !outputs.iter().any(|o| o == surface.output()) {
                            cameras.want(surface.output(), None);
                        }
                    }
                }
                for index in 0..self.surfaces.len() {
                    if !outputs.iter().any(|o| o == self.surfaces[index].output()) {
                        self.close_window(index);
                    }
                }
                self.surfaces
                    .retain(|s| outputs.iter().any(|o| o == s.output()));
                self.previews
                    .retain(|o, _| outputs.iter().any(|kept| kept == o));
                if let Some(sound) = self.sound.as_mut() {
                    sound.retain(outputs);
                }
            }
            Command::OpenSurface(spec) => match self.surface_mut(&spec.output) {
                Some(surface) => {
                    surface.state.apply(&command, true);
                    surface.seq = seq;
                }
                None => self.surfaces.push(EngineSurface {
                    preview_open: false,
                    state: SurfaceState::new(spec.clone()),
                    seq,
                    status: None,
                    window: None,
                    syphon: None,
                    stats: SinkStats {
                        output: spec.output.clone(),
                        ..SinkStats::default()
                    },
                    shell: Shell::new(&spec.output, frame_height_of(&spec.sink)),
                    queue: Ordered::default(),
                    traces: VecDeque::new(),
                    pages: HashMap::new(),
                    preloads: HashSet::new(),
                    resident: VecDeque::new(),
                    ndi: None,
                    card: None,
                }),
            },
            Command::CloseSurface {
                output,
                destroy: true,
            } => {
                if let Some(index) = self.surfaces.iter().position(|s| s.output() == output) {
                    self.close_window(index);
                }
                self.surfaces.retain(|s| s.output() != output);
                self.previews.remove(output.as_str());
                if let Some(sound) = self.sound.as_mut() {
                    sound.forget(output);
                }
                if let Some(cameras) = self.cameras.as_ref() {
                    cameras.want(output, None);
                }
            }
            // Not a picture change: no frame, no commit.
            Command::SetPreview { output, .. } => {
                let Some(surface) = self.surfaces.iter_mut().find(|s| s.output() == output) else {
                    self.error(
                        Some(output),
                        format!("command for unknown surface {output}"),
                    );
                    return;
                };
                surface.state.apply(&command, true);
                surface.seq = seq;
                let output = output.clone();
                self.attach_preview(&output);
                return;
            }
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
                let applied = surface.state.apply(&command, true);
                if let Command::SetCamera { camera, .. } = &command {
                    self.set_camera(&output, camera.as_ref());
                }
                let Some(surface) = self.surfaces.iter_mut().find(|s| s.output() == output) else {
                    return;
                };
                if let Command::Configure { config, .. } = &command
                    && before.config != surface.state.config
                {
                    self.sound
                        .get_or_insert_with(|| new_sound(&self.ndi))
                        .configure(&output, config);
                }
                let Some(surface) = self.surfaces.iter_mut().find(|s| s.output() == output) else {
                    return;
                };
                surface.seq = seq;
                let planned = surface.shell.command(&command, &before, &surface.state);
                // A turn the reducer refused, or one with no page to decode
                // (no window, not a picture), is answered at once.
                if matches!(command, Command::SwapPage { .. })
                    && (!applied
                        || !surface.is_open()
                        || !planned.visuals.iter().any(|v| v.role == Role::Page))
                {
                    self.send_event(Event::PageSwapped {
                        output: output.clone(),
                        seq,
                        swapped: false,
                    });
                }
                let Some(surface) = self.surfaces.iter_mut().find(|s| s.output() == output) else {
                    return;
                };
                // Every applied command gets a frame, so `FrameCommitted`
                // follows even when no pixels change.
                if surface.is_open() {
                    self.dispatch(&output, seq, planned);
                }
                if let Command::ControlDocumentVideo {
                    video,
                    command: control,
                    ..
                } = &command
                    && applied
                {
                    self.control_document_video(&output, *video as usize, control);
                }
                if !matches!(
                    command,
                    Command::CloseSurface { .. }
                        | Command::RestoreSurface { .. }
                        | Command::SetStage { .. }
                ) {
                    return;
                }
            }
        }
        self.reconcile(event_loop);
    }

    /// `__controlDocumentVideo` on `output`'s video `index`.
    fn control_document_video(&mut self, output: &str, index: usize, command: &Json) {
        let Ok(command) = command.decode::<kairo_domain::documents::DocumentVideoCommand>() else {
            tracing::warn!(output, "document video command could not be read");
            return;
        };
        let status = |this: &Self, id| this.videos.as_ref().and_then(|v| v.status(id));
        let Some(surface) = self.surfaces.iter().find(|s| s.output() == output) else {
            return;
        };
        let Some(videos) = surface.shell.doc_videos.as_ref() else {
            return;
        };
        let Some(&id) = videos.sources.get(index) else {
            return;
        };
        let range = videos.range(index).unwrap_or((0.0, None));
        let plan = crate::documents::control(&command, range, status(self, id).as_ref());
        for transport in plan.transport {
            self.document_transport(id, transport);
        }
        let Some(surface) = self.surfaces.iter_mut().find(|s| s.output() == output) else {
            return;
        };
        if let Some(videos) = surface.shell.doc_videos.as_mut()
            && let Some(slot) = videos.play_error.get_mut(index)
        {
            match plan.play_error {
                crate::documents::PlayError::Clear => *slot = None,
                crate::documents::PlayError::Set => {
                    *slot = Some(crate::documents::PLAY_FAILED.into());
                }
                crate::documents::PlayError::Keep => {}
            }
        }
        if let Some(visible) = plan.visible
            && let Some(op) = surface.shell.document_video_visible(index, visible)
        {
            surface
                .queue
                .push_waiting(0, vec![op], Vec::new(), Instant::now());
            self.flush(output);
        }
    }

    /// Picture and sound of document video `id` take `transport`.
    fn document_transport(&mut self, id: u64, transport: crate::documents::Transport) {
        use crate::documents::Transport;
        if let Some(videos) = self.videos.as_ref() {
            videos.control(
                id,
                match transport {
                    Transport::Pause => kairo_video::Control::Pause(true),
                    Transport::Play => kairo_video::Control::Pause(false),
                    Transport::Seek(s) => kairo_video::Control::Seek(s),
                },
            );
        }
        if let Some(sound) = self.sound.as_mut() {
            sound.control(
                id,
                match transport {
                    Transport::Pause => kairo_audio::ClipControl::Pause,
                    Transport::Play => kairo_audio::ClipControl::Play,
                    Transport::Seek(s) => kairo_audio::ClipControl::Seek(s),
                },
            );
        }
    }

    /// `__setCamera` for `output`: the capture (shared by every output
    /// that wants the same input) and the camera's sound.
    fn set_camera(&mut self, output: &str, camera: Option<&CameraLayer>) {
        if let Err(error) = self.compositor() {
            self.error(Some(output), format!("camera not shown: {error}"));
            return;
        }
        let Some(cameras) = self.cameras.as_ref() else {
            return;
        };
        let label = camera.map(|c| c.label.as_str());
        cameras.want(output, label);
        let live = label.and_then(|l| cameras.live_id(l).map(|id| (l.to_owned(), id)));
        if let (Some((label, id)), Some(surface)) = (live, self.surface_mut(output)) {
            surface.shell.know_camera(&label, id);
        }
        self.sound
            .get_or_insert_with(|| new_sound(&self.ndi))
            .camera(output, camera.and_then(|c| c.audio_label.as_deref()));
    }

    /// A camera's capture changed state.
    fn camera_event(&mut self, event: kairo_camera::CameraEvent) {
        use kairo_camera::CameraEvent;
        match event {
            CameraEvent::Live { label, id, .. } => {
                let mut ready = Vec::new();
                for surface in &mut self.surfaces {
                    if let Some(op) = surface.shell.camera_live(&label, id, &surface.state) {
                        surface
                            .queue
                            .push_waiting(0, vec![op], Vec::new(), Instant::now());
                        ready.push(surface.output().to_owned());
                    }
                }
                for output in ready {
                    self.flush(&output);
                }
            }
            CameraEvent::Failed { label, error } => {
                self.error(None, format!("camera {label}: {error}"));
            }
            CameraEvent::Lost { label, error } => {
                self.error(
                    None,
                    format!("camera {label}: {error}; showing its last frame"),
                );
            }
            CameraEvent::Restored { label } => {
                tracing::info!(%label, "camera running again");
            }
        }
    }

    /// Hand a command's operations to the compositor, after the words of any
    /// slide in it have been drawn and every earlier command for the surface
    /// has been applied.
    fn dispatch(&mut self, output: &str, seq: u64, planned: Planned) {
        let received = Instant::now();
        let mut waits = Vec::new();
        let mut roles = Vec::new();
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
            roles.push((job, request.role));
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
            for (job, role) in roles {
                match role {
                    Role::Page => {
                        surface.pages.insert(job, seq);
                    }
                    Role::Preload => {
                        surface.preloads.insert(job);
                    }
                    Role::Node => {}
                }
            }
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
        if let Some(seq) = surface.pages.remove(&job) {
            // `__swapDocumentPage`: shown only if decoded and still the
            // newest request for the page node.
            let page = match result {
                Output::Image(Some(content)) if surface.shell.is_current(slot, generation) => {
                    Some(content)
                }
                _ => None,
            };
            let swapped = page.is_some();
            let mut keep = None;
            match page {
                Some(content) => {
                    surface.shell.page_shown(generation);
                    if let kairo_compositor::Fill::Image(raster) = &content.fill {
                        keep = Some(surface.keep_page(raster.clone()));
                    }
                    surface.queue.complete_visual(job, content);
                }
                None => {
                    surface.queue.cancel_visual(job);
                }
            }
            if let (Some(rasters), Some(compositor)) = (keep, self.compositor.as_ref()) {
                compositor.keep_resident(output, rasters);
            }
            self.send_event(Event::PageSwapped {
                output: output.to_owned(),
                seq,
                swapped,
            });
            self.flush(output);
            return;
        }
        if surface.preloads.remove(&job) {
            // A missed preload only costs the next turn its head start.
            if let Output::Image(Some(content)) = result
                && let kairo_compositor::Fill::Image(raster) = &content.fill
            {
                let rasters = surface.keep_page(raster.clone());
                if let Some(compositor) = self.compositor.as_ref() {
                    compositor.keep_resident(output, rasters);
                }
            }
            return;
        }
        match result {
            // A document page the frame waits for (`__markDocument`).
            Output::Image(Some(content)) if surface.queue.complete_visual(job, content.clone()) => {
                if let kairo_compositor::Fill::Image(raster) = &content.fill {
                    let rasters = surface.keep_page(raster.clone());
                    if let Some(compositor) = self.compositor.as_ref() {
                        compositor.keep_resident(output, rasters);
                    }
                }
            }
            // An unreadable page shows nothing on the black frame.
            Output::Image(None)
                if surface.queue.complete_visual(
                    job,
                    kairo_compositor::Content::new(kairo_compositor::Fill::Pending),
                ) =>
            {
                tracing::warn!(output, "a document page could not be shown");
            }
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
            Query::DisplayState => {
                let mut windows = self.diagnostics.info();
                windows.extend(self.surfaces.iter().filter_map(|s| {
                    let w = s.window.as_ref()?;
                    let position = w.window.outer_position().ok()?;
                    let size = w.window.outer_size();
                    Some(WindowInfo {
                        output: s.output().into(),
                        display: w.display,
                        kind: if s.state.stage.is_some() {
                            WindowKind::Stage
                        } else {
                            WindowKind::Program
                        },
                        rehearsal: w.sink.rehearsal,
                        bounds: Rect {
                            x: position.x,
                            y: position.y,
                            width: size.width,
                            height: size.height,
                        },
                        native_window: crate::window_policy::native_id(&w.window),
                    })
                }));
                Reply::DisplayState {
                    windows,
                    sleep_blocked: self.power.active(),
                }
            }
            // No video (or no compositor yet) counts as ended.
            Query::VideoEnded { output } => match has(&output) {
                Some(_) => Reply::VideoEnded(self.videos.as_ref().is_none_or(|v| v.ended(&output))),
                None => Reply::UnknownSurface,
            },
            Query::VideoTime { output } => match has(&output) {
                Some(_) => Reply::VideoTime(
                    self.videos
                        .as_ref()
                        .and_then(|v| v.background_status(&output))
                        .map(|status| video_time(&status)),
                ),
                None => Reply::UnknownSurface,
            },
            Query::DocumentPlayback { output } => match has(&output) {
                Some(surface) => Reply::DocumentPlayback(
                    crate::documents::playback(
                        surface.state.document_mode,
                        &surface.shell,
                        self.videos.as_ref(),
                    )
                    .map(|status| Json::encode(&status)),
                ),
                None => Reply::UnknownSurface,
            },
            // Empty until a card surface or `RefreshVideoDevices` starts the service.
            Query::VideoDevices => {
                Reply::VideoDevices(Json::encode(&self.devices.as_ref().map_or_else(
                    || kairo_domain::video_devices::VideoDeviceSnapshot {
                        ports: Vec::new(),
                        providers: Vec::new(),
                        outputs: Default::default(),
                    },
                    |d| d.snapshot(),
                )))
            }
            Query::State => Reply::State(self.surfaces.iter().map(|s| s.state.clone()).collect()),
            // Every sender of the native runtime sends audio; without the
            // runtime there is no sender, as Electron's missing binding.
            Query::NdiAudioSupported => Reply::NdiAudioSupported(self.ndi.available()),
        };
        self.outbox.send(&FromEngine::Reply { id, reply });
    }

    /// Places each surface: a window on its display, parked when the
    /// display is gone, unavailable when its sink cannot run here.
    fn reconcile(&mut self, event_loop: &ActiveEventLoop) {
        self.sync_cards();
        let mut claimed = HashSet::new();
        let mut order: Vec<_> = (0..self.surfaces.len()).collect();
        // Program screens claim displays first; stage displays never cover one.
        order.sort_by_key(|index| self.surfaces[*index].state.stage.is_some());
        for index in order {
            let spec = self.surfaces[index].state.spec.clone();
            let status = match &spec.sink {
                SinkSpec::Display(requested) => {
                    if self.surfaces[index].state.parked || self.control_native == Some(None) {
                        self.close_window(index);
                        SurfaceStatus::Parked {
                            reason: "Output is closed".into(),
                        }
                    } else {
                        let resolved =
                            display::resolve(&requested.binding, &self.displays, &claimed).cloned();
                        match resolved {
                            None => {
                                self.close_window(index);
                                let connected = display::resolve(
                                    &requested.binding,
                                    &self.displays,
                                    &HashSet::new(),
                                )
                                .is_some();
                                SurfaceStatus::Parked {
                                    reason: if requested.binding.display_id.is_none() {
                                        "No display chosen"
                                    } else if connected {
                                        "Display in use by another screen"
                                    } else {
                                        "Display not connected"
                                    }
                                    .into(),
                                }
                            }
                            Some(target) => {
                                claimed.insert(target.id);
                                let mut sink = requested.clone();
                                if let Some(Some(control)) = self.control_native {
                                    sink.rehearsal = control == target.native_id;
                                }
                                if self.surfaces[index].state.stage.is_some() {
                                    sink.frame_height = 1080;
                                }
                                if let Some(old_id) =
                                    sink.binding.display_id.filter(|id| *id != target.id)
                                {
                                    sink.binding.display_id = Some(target.id);
                                    if let SinkSpec::Display(retained) =
                                        &mut self.surfaces[index].state.spec.sink
                                    {
                                        retained.binding.display_id = Some(target.id);
                                    }
                                    self.send_event(Event::DisplayRebound {
                                        output: spec.output.clone(),
                                        old_id,
                                        new_id: target.id,
                                        stage: self.surfaces[index].state.stage.is_some(),
                                    });
                                }
                                let current = self.surfaces[index].window.as_ref();
                                if current.is_some_and(|w| {
                                    w.display == target.id
                                        && w.sink.rehearsal == sink.rehearsal
                                        && w.sink.frame_height == sink.frame_height
                                }) {
                                    let w = self.surfaces[index].window.as_mut().unwrap();
                                    if w.target.bounds != target.bounds
                                        || w.target.scale_factor != target.scale_factor
                                    {
                                        let bounds = if sink.rehearsal {
                                            crate::window_policy::rehearsal_bounds(
                                                &target,
                                                sink.frame_height,
                                            )
                                        } else {
                                            target.bounds.clone()
                                        };
                                        crate::window_policy::set_bounds(&w.window, &bounds);
                                        crate::window_policy::show(&w.window, sink.rehearsal);
                                    }
                                    w.target = target;
                                    w.sink = sink;
                                    SurfaceStatus::Ready
                                } else {
                                    self.close_window(index);
                                    match self.open_window(event_loop, index, &sink, &target) {
                                        Ok(()) => SurfaceStatus::Ready,
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
                    }
                }
                SinkSpec::Ndi { name } => {
                    if self.surfaces[index].state.parked {
                        self.close_window(index);
                        continue;
                    }
                    if self.surfaces[index]
                        .ndi
                        .as_ref()
                        .is_some_and(|f| &f.name == name)
                    {
                        continue;
                    }
                    // A new name is a new sender.
                    self.close_window(index);
                    match self.open_ndi(index, name) {
                        Ok(()) => SurfaceStatus::Ready,
                        Err(reason) => SurfaceStatus::Unavailable { reason },
                    }
                }
                SinkSpec::Syphon { name } => match self.place_syphon(index, name) {
                    Some(status) => status,
                    None => continue,
                },
                SinkSpec::Device { .. } => match self.place_card(index) {
                    Some(status) => status,
                    None => continue,
                },
                SinkSpec::Preview => match self.place_preview(index) {
                    Some(status) => status,
                    None => continue,
                },
            };
            let surface = &mut self.surfaces[index];
            if surface.status.as_ref() != Some(&status) {
                surface.status = Some(status.clone());
                let output = surface.output().to_owned();
                self.send_event(Event::SurfaceStatus { output, status });
            }
        }
        self.update_power();
    }

    fn current_displays(&self, el: &ActiveEventLoop) -> Vec<DisplayInfo> {
        let mut displays = display::scan(el);
        for display in &mut displays {
            display.hosts_main_window = self.control_native.flatten() == Some(display.native_id);
        }
        displays
    }

    fn scan_displays(&mut self, el: &ActiveEventLoop) {
        let observed = self.current_displays(el);
        self.diagnostics.topology(&observed);
        // macOS can move an unplugged window onto another display before the
        // debounce expires. Hide it immediately, before asynchronous GPU close.
        for index in 0..self.surfaces.len() {
            let unsafe_window = self.surfaces[index].window.as_ref().is_some_and(|w| {
                !observed.iter().any(|d| d.id == w.display)
                    || self.control_native == Some(None)
                    || (!w.sink.rehearsal
                        && self.control_native.flatten() == Some(w.target.native_id))
            });
            if unsafe_window {
                self.close_window(index);
            }
        }
        self.display_debounce
            .observe(Instant::now(), &self.displays, observed);
        self.update_power();
    }

    fn update_power(&mut self) {
        let active = self.surfaces.iter().any(|s| s.window.is_some()) || self.diagnostics.active();
        if let Some(error) = self.power.set_active(active) {
            self.error(None, error);
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
                Ok(compositor) => {
                    let proxy = self.proxy.clone();
                    self.videos = Some(kairo_video::Videos::new(
                        Arc::new(Feed(compositor.feed())),
                        move |_, path, message| {
                            let _ = proxy.send_event(Message::VideoFailed {
                                path: path.to_owned(),
                                message: message.to_owned(),
                            });
                        },
                    ));
                    let proxy = self.proxy.clone();
                    self.cameras = Some(kairo_camera::Cameras::new(
                        kairo_camera::system(),
                        Arc::new(Feed(compositor.feed())),
                        move |event| {
                            let _ = proxy.send_event(Message::Camera(event));
                        },
                    ));
                    self.compositor = Some(compositor);
                }
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
        if let Some(window) = &surface.window {
            window.window.set_visible(false);
        }
        let feed = surface.ndi.take();
        // Clients see the Syphon server leave.
        let syphon = surface.syphon.take().map(|mut c| c.stop()).is_some();
        let card = surface.card.take().is_some();
        let preview = std::mem::take(&mut surface.preview_open);
        if (surface.window.take().is_some() || feed.is_some() || syphon || card || preview)
            && let Some(compositor) = self.compositor.as_ref()
        {
            compositor.capture(&surface.state.spec.output, None);
            compositor.close(&surface.state.spec.output);
        }
        if let Some(feed) = feed {
            self.ndi.close(&surface.state.spec.output, feed);
        }
    }

    /// An NDI surface: an offscreen 1920 × 1080 output whose frames go to
    /// its feed as UYVA.
    fn open_ndi(&mut self, index: usize, name: &str) -> Result<(), String> {
        let output = self.surfaces[index].state.spec.output.clone();
        let feed = self.ndi.open(&output, name)?;
        let sink = feed.output.clone();
        let compositor = match self.compositor() {
            Ok(c) => c,
            Err(error) => {
                self.ndi.close(&output, feed);
                return Err(error);
            }
        };
        compositor.open(&output, NDI_HEIGHT, Sink::Offscreen);
        compositor.capture(&output, Some(Arc::new(move |frame| sink.frame(frame))));
        tracing::info!(output, name, "NDI output open");
        let surface = &mut self.surfaces[index];
        surface.ndi = Some(feed);
        surface.shell = Shell::new(&output, NDI_HEIGHT);
        if let (Some(cameras), Some(camera)) = (self.cameras.as_ref(), &surface.state.layers.camera)
            && let Some(id) = cameras.live_id(&camera.label)
        {
            surface.shell.know_camera(&camera.label, id);
        }
        surface.queue.clear();
        surface.traces.clear();
        let planned = surface.shell.rebuild(&surface.state);
        let seq = surface.seq;
        self.dispatch(&output, seq, planned);
        // A recording and a preview follow their output through rebuilds.
        self.attach_capture(&output);
        self.attach_preview(&output);
        Ok(())
    }

    /// The video card service, started on first use. The provider is the
    /// DeckLink helper on macOS unless `KAIRO_ENABLE_DECKLINK=0`.
    fn devices(&mut self) -> &kairo_sinks::video_devices::VideoDevices {
        if self.devices.is_none() {
            let proxy = self.proxy.clone();
            self.devices = Some(kairo_sinks::video_devices::VideoDevices::start(
                crate::cards::providers(),
                Duration::from_millis(250),
                move || {
                    let _ = proxy.send_event(Message::VideoDevices);
                },
            ));
        }
        self.devices.as_ref().expect("devices")
    }

    /// Sends the card bindings to the service when they changed: a reconcile
    /// with no change does no card work. A parked card surface lets go of
    /// its port.
    fn sync_cards(&mut self) {
        let bindings: Vec<_> = self
            .surfaces
            .iter()
            .filter_map(|s| match &s.state.spec.sink {
                SinkSpec::Device {
                    port_id,
                    mode_id,
                    key_fill,
                } => Some(kairo_sinks::video_devices::Binding {
                    id: s.output().to_owned(),
                    enabled: !s.state.parked,
                    port_id: port_id.clone(),
                    mode_id: mode_id.clone(),
                    key_fill: *key_fill,
                }),
                _ => None,
            })
            .collect();
        if bindings == self.device_bindings || (bindings.is_empty() && self.devices.is_none()) {
            return;
        }
        self.device_bindings = bindings.clone();
        self.devices().reconcile(bindings);
    }

    /// A card surface: drawn offscreen at the card mode's shape and sent to
    /// the port the service holds for it, as UYVY in the frame ring or BGRA
    /// down the helper's stdin. `None`: unchanged.
    fn place_card(&mut self, index: usize) -> Option<SurfaceStatus> {
        let output = self.surfaces[index].state.spec.output.clone();
        let (state, sink) = match self.devices.as_ref() {
            Some(devices) => (devices.output_status(&output), devices.sink_for(&output)),
            None => {
                self.compose_without_sink(index);
                return Some(SurfaceStatus::Unavailable {
                    reason: "Output unavailable".into(),
                });
            }
        };
        let Some(sink) = sink.filter(|s| state.ready && s.active()) else {
            if self.surfaces[index].card.is_some() {
                self.close_window(index);
            }
            self.compose_without_sink(index);
            let reason = if state.reason.is_empty() {
                "Output unavailable".into()
            } else {
                state.reason
            };
            return Some(SurfaceStatus::Unavailable { reason });
        };
        if self.surfaces[index]
            .card
            .as_ref()
            .is_some_and(|c| Arc::ptr_eq(c, &sink))
        {
            return None;
        }
        self.close_window(index);
        let frame_height = crate::cards::frame_height(sink.raster);
        let compositor = match self.compositor() {
            Ok(compositor) => compositor,
            Err(reason) => return Some(SurfaceStatus::Unavailable { reason }),
        };
        compositor.open(&output, frame_height, Sink::Offscreen);
        crate::cards::attach(compositor, &output, &sink);
        tracing::info!(
            output,
            raster = ?sink.raster,
            ring = sink.frame_ring().is_some(),
            "video card output open"
        );
        self.surfaces[index].card = Some(sink);
        self.show_state(index, frame_height);
        Some(SurfaceStatus::Ready)
    }

    /// Keeps composing an output whose sink is missing (no card), offscreen
    /// like the Screens sample, so the operator's monitor still shows what
    /// the output would. The status stays Unavailable.
    fn compose_without_sink(&mut self, index: usize) {
        if self.surfaces[index].preview_open {
            return;
        }
        let output = self.surfaces[index].output().to_owned();
        let Ok(compositor) = self.compositor() else {
            return;
        };
        compositor.open(&output, 1080, Sink::Offscreen);
        self.surfaces[index].preview_open = true;
        self.show_state(index, 1080);
    }

    /// The Screens sample uses exactly the program renderer, but never
    /// acquires an external sink or routes sound to an output device.
    fn place_preview(&mut self, index: usize) -> Option<SurfaceStatus> {
        if self.surfaces[index].preview_open {
            return None;
        }
        self.close_window(index);
        let output = self.surfaces[index].output().to_owned();
        let compositor = match self.compositor() {
            Ok(compositor) => compositor,
            Err(reason) => return Some(SurfaceStatus::Unavailable { reason }),
        };
        compositor.open(&output, 1080, Sink::Offscreen);
        self.surfaces[index].preview_open = true;
        self.show_state(index, 1080);
        Some(SurfaceStatus::Ready)
    }

    /// A Syphon surface: a new name restarts the server; None means unchanged.
    fn place_syphon(&mut self, index: usize, name: &str) -> Option<SurfaceStatus> {
        if self.surfaces[index]
            .syphon
            .as_ref()
            .is_some_and(|c| c.server_name() == name && c.running())
        {
            return None;
        }
        self.close_window(index);
        let output = self.surfaces[index].state.spec.output.clone();
        let frame_height = frame_height_of(&self.surfaces[index].state.spec.sink);
        let mut channel = kairo_sinks::syphon::SyphonChannel::new(name);
        if let Err(reason) = channel.status() {
            return Some(SurfaceStatus::Unavailable { reason });
        }
        let proxy = self.proxy.clone();
        let owner = output.clone();
        let report: kairo_sinks::syphon::Report = Arc::new(move |message| {
            let _ = proxy.send_event(Message::Compositor(CompositorEvent::Error {
                output: Some(owner.clone()),
                message,
            }));
        });
        let Some(sink) = channel.sink(report) else {
            let reason = channel
                .status()
                .err()
                .unwrap_or_else(|| "Syphon unavailable".into());
            return Some(SurfaceStatus::Unavailable { reason });
        };
        let compositor = match self.compositor() {
            Ok(compositor) => compositor,
            Err(reason) => return Some(SurfaceStatus::Unavailable { reason }),
        };
        compositor.open(&output, frame_height, Sink::Offscreen);
        compositor.attach(&output, Some(sink));
        tracing::info!(output, name, "Syphon server publishing");
        self.surfaces[index].syphon = Some(channel);
        self.show_state(index, frame_height);
        Some(SurfaceStatus::Ready)
    }

    /// A freshly opened compositor output shows the surface's state at once.
    fn show_state(&mut self, index: usize, frame_height: u32) {
        let output = self.surfaces[index].state.spec.output.clone();
        let surface = &mut self.surfaces[index];
        surface.shell = Shell::new(&output, frame_height);
        if let (Some(cameras), Some(camera)) = (self.cameras.as_ref(), &surface.state.layers.camera)
            && let Some(id) = cameras.live_id(&camera.label)
        {
            surface.shell.know_camera(&camera.label, id);
        }
        surface.queue.clear();
        surface.traces.clear();
        let planned = surface.shell.rebuild(&surface.state);
        let seq = surface.seq;
        self.dispatch(&output, seq, planned);
        // A recording and a preview follow their output through rebuilds.
        self.attach_capture(&output);
        self.attach_preview(&output);
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
            crate::window_policy::rehearsal_bounds(target, sink.frame_height)
        } else {
            target.bounds.clone()
        };
        let title = if sink.rehearsal {
            format!("{label} (rehearsal)")
        } else {
            label
        };
        let window =
            crate::window_policy::open(event_loop, &title, &bounds, sink.rehearsal, false)?;
        crate::window_policy::set_bounds(&window, &bounds);
        #[cfg(not(target_os = "macos"))]
        if !sink.rehearsal {
            // Use true borderless fullscreen on Windows so the taskbar cannot
            // draw over the output. macOS uses screen-saver level, no Space.
            let monitor = event_loop
                .available_monitors()
                .find(|m| m.position().x == target.bounds.x && m.position().y == target.bounds.y);
            window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(monitor)));
        }
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
        self.show_state(index, sink.frame_height);
        crate::window_policy::show(&window, sink.rehearsal);
        self.surfaces[index].window = Some(OutputWindow {
            window,
            display: target.id,
            sink: sink.clone(),
            target: target.clone(),
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
            CompositorEvent::Media { output, uses } => {
                self.sound
                    .get_or_insert_with(|| new_sound(&self.ndi))
                    .media(&output, &uses);
                if let Some(videos) = self.videos.as_mut() {
                    videos.sync(&output, uses.clone());
                }
                // Embedded document videos wait, paused at the clip start.
                if let Some(surface) = self.surfaces.iter_mut().find(|s| s.output() == output) {
                    let ids: Vec<u64> = uses.iter().map(|u| u.id).collect();
                    let primed = crate::documents::prime(&mut surface.shell, &ids);
                    for (id, start) in primed {
                        self.document_transport(id, crate::documents::Transport::Pause);
                        self.document_transport(id, crate::documents::Transport::Seek(start));
                    }
                }
            }
            CompositorEvent::Transport { id, control, .. } => {
                if let Some(sound) = self.sound.as_mut() {
                    sound.control(
                        id,
                        match control {
                            MediaControl::Pause(true) => kairo_audio::ClipControl::Pause,
                            MediaControl::Pause(false) => kairo_audio::ClipControl::Play,
                            MediaControl::Seek(s) => kairo_audio::ClipControl::Seek(s),
                        },
                    );
                }
                if let Some(videos) = self.videos.as_ref() {
                    videos.control(
                        id,
                        match control {
                            MediaControl::Pause(p) => kairo_video::Control::Pause(p),
                            MediaControl::Seek(s) => kairo_video::Control::Seek(s),
                        },
                    );
                }
            }
            CompositorEvent::WebPresented {
                output,
                capture_us,
                compose_us,
                frames,
            } => {
                if let Some(surface) = self.surface_mut(&output) {
                    surface.stats.produced = frames;
                }
                if self.web_timings.len() < 10_000 {
                    self.web_timings.push((capture_us, compose_us));
                }
            }
            CompositorEvent::DeviceLost(why) => {
                self.error(None, format!("GPU device lost: {why}"));
                self.exit_code = Some(EXIT_DEVICE_LOST);
                self.command_inner(event_loop, 0, Command::Shutdown);
            }
        }
    }

    fn report(&mut self) {
        if !self.web_timings.is_empty() {
            let n = self.web_timings.len();
            let mut capture: Vec<_> = self.web_timings.iter().map(|(c, _)| *c).collect();
            let mut compose: Vec<_> = self.web_timings.iter().map(|(_, c)| *c).collect();
            let mut total: Vec<_> = self.web_timings.iter().map(|(c, p)| c + p).collect();
            capture.sort_unstable();
            compose.sort_unstable();
            total.sort_unstable();
            let p99 = (n * 99).div_ceil(100) - 1;
            tracing::info!(
                samples = n,
                capture_p99_ms = capture[p99] as f64 / 1000.,
                compose_p99_ms = compose[p99] as f64 / 1000.,
                request_to_present_p50_ms = total[n / 2] as f64 / 1000.,
                request_to_present_p99_ms = total[p99] as f64 / 1000.,
                "web display latency (software, excludes capture sampling and scanout)"
            );
            self.web_timings.clear();
        }
        if self.web.active()
            && let Some(dir) = std::env::var_os("KAIRO_WEB_EVIDENCE_DIR")
            && let Some(compositor) = self.compositor.as_ref()
            && let Some(surface) = self.surfaces.iter().find(|s| {
                s.window.is_some() && s.state.config.web_enabled && s.state.layers.web.is_some()
            })
        {
            let feed = compositor.feed();
            let output = surface.output().to_owned();
            std::thread::spawn(move || {
                if let Some((w, h, pixels)) = feed.read(&output) {
                    let dir = std::path::PathBuf::from(dir);
                    let _ = std::fs::create_dir_all(&dir);
                    if let Ok(file) = std::fs::File::create(dir.join("web-output.png")) {
                        let mut encoder = png::Encoder::new(file, w, h);
                        encoder.set_color(png::ColorType::Rgba);
                        encoder.set_depth(png::BitDepth::Eight);
                        if let Ok(mut writer) = encoder.write_header() {
                            let _ = writer.write_image_data(&pixels);
                        }
                    }
                }
            });
        }
        if let Some(sound) = self.sound.as_ref()
            && sound.active()
        {
            let tapping: Vec<&str> = self
                .surfaces
                .iter()
                .map(|s| s.output())
                .filter(|o| sound.tapping(o))
                .collect();
            let glitches = sound.glitches();
            if glitches > self.glitches {
                tracing::warn!(frames = glitches - self.glitches, "program sound glitched");
                self.glitches = glitches;
            }
            tracing::debug!(
                device = ?sound.device(),
                glitches,
                ?tapping,
                ndi_blocks = sound.unsent.load(std::sync::atomic::Ordering::Relaxed),
                "program sound"
            );
        }
        // Frames drawn, fades included, as the compositor counts them.
        if let Some(compositor) = self.compositor.as_ref() {
            for surface in &mut self.surfaces {
                if surface.is_open()
                    && !(surface.state.config.web_enabled && surface.state.layers.web.is_some())
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

impl kairo_camera::Feed for Feed {
    fn frame(&self, id: u64, frame: Arc<kairo_compositor::YuvFrame>) {
        self.0.video_frame(id, frame);
    }

    fn remove(&self, id: u64) {
        self.0.remove(id);
    }
}

/// The compositor as the video players' frame destination.
struct Feed(SourceFeed);

impl kairo_video::Feed for Feed {
    fn frame(&self, id: u64, frame: Arc<kairo_compositor::YuvFrame>) {
        self.0.video_frame(id, frame);
    }

    fn remove(&self, id: u64) {
        self.0.remove(id);
    }
}

fn frame_height_of(sink: &SinkSpec) -> u32 {
    match sink {
        SinkSpec::Display(display) => display.frame_height,
        // Offscreen surfaces paint 1920 × 1080 (`HEIGHT`).
        _ => 1080,
    }
}

impl ApplicationHandler<Message> for Engine {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        #[cfg(target_os = "macos")]
        if self.display_observer.is_none() {
            let proxy = self.proxy.clone();
            self.display_observer = Some(crate::mac::DisplayObserver::new(move || {
                let _ = proxy.send_event(Message::DisplaysChanged);
            }));
        }
        if self.displays.is_empty() {
            self.displays = self.current_displays(event_loop);
            tracing::info!("{} display(s)", self.displays.len());
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, message: Message) {
        match message {
            Message::DisplaysChanged => self.scan_displays(event_loop),
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
            Message::Camera(event) => self.camera_event(event),
            Message::VideoDevices => {
                if let Some(devices) = self.devices.as_ref() {
                    let snapshot = Json::encode(&devices.snapshot());
                    self.send_event(Event::VideoDevices(snapshot));
                }
                self.reconcile(event_loop);
            }
            // The node stays and draws nothing; the video counts as ended.
            Message::VideoFailed { path, message } => {
                self.error(None, format!("video {path}: {message}"))
            }
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
        if self.shutting_down {
            if self.compositor.as_ref().is_none_or(|c| c.finished()) {
                self.compositor = None;
                event_loop.exit();
            } else {
                event_loop.set_control_flow(ControlFlow::WaitUntil(now + Duration::from_millis(5)));
            }
            return;
        }
        if let Some(at) = self.orphaned_at
            && now.duration_since(at) >= self.grace
        {
            tracing::warn!("no client for {} ms; exiting", self.grace.as_millis());
            self.command_inner(event_loop, 0, Command::Shutdown);
            return;
        }
        self.diagnostics.expire(now);
        self.update_power();
        if now >= self.next_capture {
            self.next_capture = now + crate::capture::POLL;
            let aspect = self
                .capture
                .output()
                .map(str::to_owned)
                .and_then(|o| self.capture_aspect(&o));
            if self.capture.check(aspect) {
                self.send_capture();
            }
        }
        if now >= self.next_scan {
            self.next_scan = now + DISPLAY_POLL;
            self.scan_displays(event_loop);
        }
        if let Some(displays) = self.display_debounce.take_ready(now) {
            self.displays = displays;
            self.send_event(Event::Displays(self.displays.clone()));
            self.reconcile(event_loop);
            self.sync_web(event_loop);
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
        // The recording's once-a-second checks, and a file being finished.
        if self.capture.active() {
            wake = wake.min(self.next_capture);
        }
        if let Some(deadline) = self.display_debounce.deadline() {
            wake = wake.min(deadline);
        }
        if let Some(deadline) = self.diagnostics.deadline() {
            wake = wake.min(deadline);
        }
        if ticking {
            wake = wake.min(self.next_tick);
        }
        if self.web.active() {
            if now >= self.next_web {
                if now.duration_since(self.next_web) > Duration::from_secs(1) {
                    self.next_web = now;
                }
                while self.next_web <= now {
                    self.next_web += crate::web::PERIOD;
                }
                if let Some(compositor) = self.compositor.as_ref() {
                    let notices = self.web.tick(now, &compositor.feed());
                    self.web_notices(notices);
                    self.sync_web(event_loop);
                }
            }
            wake = wake.min(self.next_web);
        }
        // Video sound follows its picture, checked every 50 ms; camera
        // inputs that stopped are retried.
        if let Some(sound) = self.sound.as_mut()
            && (sound.active() || sound.has_cameras())
        {
            if now >= self.next_sound {
                self.next_sound = now + SOUND_FOLLOW;
                sound.check_cameras();
                let videos = self.videos.as_ref();
                sound.follow(|id| {
                    let status = videos?.status(id)?;
                    Some(crate::sound::PictureClock {
                        position: status.position,
                        playing: status.shown && !status.paused && !status.ended,
                    })
                });
            }
            wake = wake.min(self.next_sound);
        }
        if let Some(at) = self.orphaned_at {
            wake = wake.min(at + self.grace);
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(wake));
    }
}

impl Engine {
    /// The source's picture shape while it is open (`sourceAspect`).
    fn capture_aspect(&self, output: &str) -> crate::capture::Aspect {
        let surface = self.surfaces.iter().find(|s| s.output() == output)?;
        if !surface.is_open() || surface.state.parked {
            return None;
        }
        let height = match &surface.state.spec.sink {
            SinkSpec::Ndi { .. } => NDI_HEIGHT,
            sink => frame_height_of(sink),
        };
        Some(f64::from(kairo_compositor::FRAME_WIDTH) / f64::from(height.max(1)))
    }

    /// Hands `output`'s frames to the recording, if it records that output.
    fn attach_capture(&self, output: &str) {
        if self.capture.output() != Some(output) {
            return;
        }
        if let (Some(compositor), Some(sink)) = (self.compositor.as_ref(), self.capture.sink()) {
            compositor.tap(output, kairo_compositor::thread::Tap::Record, Some(sink));
        }
    }

    /// Feeds `output`'s operator preview, or stops it, as its state says.
    fn attach_preview(&mut self, output: &str) {
        let Some(surface) = self.surfaces.iter().find(|s| s.output() == output) else {
            self.stop_preview(output);
            return;
        };
        let target = surface.state.preview.clone();
        let height = match &surface.state.spec.sink {
            SinkSpec::Ndi { .. } => NDI_HEIGHT,
            sink => frame_height_of(sink),
        };
        let Some(target) = target else {
            self.stop_preview(output);
            return;
        };
        let feed = match self.previews.get(output) {
            Some(feed) if feed.path == target.path => feed.clone(),
            _ => match crate::preview::Feed::open(&target.path) {
                Ok(feed) => {
                    self.previews.insert(output.to_owned(), feed.clone());
                    feed
                }
                Err(error) => {
                    self.previews.remove(output);
                    self.error(Some(output), format!("preview map: {error}"));
                    return;
                }
            },
        };
        let Some(compositor) = self.compositor.as_ref() else {
            // Attached when the output's window or sink is built.
            return;
        };
        let size = crate::preview::fit(
            (kairo_compositor::FRAME_WIDTH, height),
            (target.width, target.height),
            feed.capacity(),
        );
        compositor.preview(output, size, target.fps, feed.sink(output));
    }

    /// Stops `output`'s preview and lets go of its map.
    fn stop_preview(&mut self, output: &str) {
        if self.previews.remove(output).is_some()
            && let Some(compositor) = self.compositor.as_ref()
        {
            compositor.stop_preview(output);
        }
    }

    fn send_capture(&self) {
        self.send_event(Event::Capture(Json::from_value(
            &serde_json::to_value(self.capture.status()).unwrap_or_default(),
        )));
    }
}

/// Program sound whose NDI taps feed the NDI senders.
fn new_sound(ndi: &crate::ndi::Ndi) -> crate::sound::Sound {
    let mut sound = crate::sound::Sound::new();
    sound.send_ndi_audio(ndi.audio_route());
    sound
}

/// `OverlayVideoTime` from a transport status: the duration is the range's
/// end once known, 0 before.
fn video_time(status: &kairo_video::transport::Status) -> kairo_engine_proto::VideoTime {
    kairo_engine_proto::VideoTime {
        current_time: status.position,
        duration: if status.end.is_finite() {
            status.end
        } else {
            0.0
        },
        ended: status.video_ended(),
    }
}
