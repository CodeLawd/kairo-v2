//! The compositor thread. It owns the device and every output's scene,
//! applies the engine's layer operations as they arrive, and draws an output
//! only when it changed or while one of its fades runs. Idle, it blocks on
//! its mailbox and uses no CPU or GPU.

use crate::camera::VideoLayout;
use crate::layers::{Content, MediaUse, Raster, Slot};
use crate::renderer::{FrameTarget, Renderer, Scene};
use crate::yuv::YuvFrame;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use wgpu::*;

/// While something animates on an offscreen output, frames this far apart.
const OFFSCREEN_FRAME: Duration = Duration::from_micros(16_667);
/// A window frame that could not be presented is tried again this soon.
const PRESENT_RETRY: Duration = Duration::from_millis(100);

/// One change to an output's program shell.
#[derive(Clone, Debug)]
pub enum Op {
    /// `swapLayer`.
    Swap {
        slot: Slot,
        content: Option<Content>,
        ms: u32,
    },
    /// `setBackground` / `setElements`: kept when the key matches.
    Keyed {
        slot: Slot,
        content: Option<(Content, String)>,
        ms: u32,
    },
    /// The pixels of a keyed node that was made before they were ready
    /// (an image decoded off-thread). Ignored when the key is no longer the
    /// slot's current one.
    Update {
        slot: Slot,
        key: String,
        content: Content,
    },
    /// `program.style.background`.
    ProgramBackground(Option<[f32; 4]>),
    /// `config.video`, normalised once.
    Layout(VideoLayout),
    /// `__setVideoPaused` / `__seekVideo`: addressed to whatever video is the
    /// current background when the operation is applied, in order with the
    /// slides around it. Nothing happens when there is none.
    Media(MediaControl),
    /// Use an aspect-matched canvas for a stage script, or restore the program canvas.
    StageCanvas(Option<(u32, u32)>),
}

/// The operator's transport on a background video.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MediaControl {
    Pause(bool),
    Seek(f64),
}

/// Where an output's frames go.
pub enum Sink {
    /// A window's surface, created by the caller on the window's thread.
    Window {
        surface: Surface<'static>,
        size: (u32, u32),
    },
    /// No window: the frame stays on the GPU for readers (preview, NDI).
    Offscreen,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CompositorEvent {
    /// The output's frame carrying `seq` is complete on the GPU and its
    /// sink has it.
    Committed { output: String, seq: u64 },
    /// A frame could not be presented (occluded, timed out).
    Dropped { output: String, why: &'static str },
    Error {
        output: Option<String>,
        message: String,
    },
    /// The GPU device is gone; nothing more will draw.
    DeviceLost(String),
    /// Web capture request/CPU pixels to GPU-complete display submission.
    WebPresented {
        output: String,
        capture_us: u64,
        compose_us: u64,
        frames: u64,
    },
    /// The output's video nodes changed (made, kept with new settings,
    /// retired or removed). Empty once the output is closed.
    Media { output: String, uses: Vec<MediaUse> },
    /// A transport operation reached the output's current background video,
    /// source `id`. Always follows the `Media` report it depends on.
    Transport {
        output: String,
        id: u64,
        control: MediaControl,
    },
}

enum Msg {
    Open {
        output: String,
        frame_height: u32,
        sink: Sink,
    },
    Close {
        output: String,
    },
    Resize {
        output: String,
        size: (u32, u32),
    },
    Invalidate {
        output: String,
    },
    Ops {
        output: String,
        seq: u64,
        ops: Vec<Op>,
    },
    Source {
        id: u64,
        frame: Arc<Raster>,
        timing: Option<(Instant, Instant)>,
    },
    RemoveSource {
        id: u64,
    },
    Resident {
        output: String,
        rasters: Vec<std::sync::Arc<crate::Raster>>,
    },
    SourceYuv {
        id: u64,
        frame: Arc<YuvFrame>,
    },
    Read {
        output: String,
        reply: mpsc::Sender<Option<(u32, u32, Vec<u8>)>>,
    },
    Frames {
        output: String,
        reply: mpsc::Sender<Option<u64>>,
    },
    Attach {
        output: String,
        sink: Option<Box<dyn FrameSink>>,
    },
    Capture {
        output: String,
        tap: Tap,
        sink: Option<CaptureSink>,
        /// The converted size; `None`: the frame's own.
        raster: Option<(u32, u32)>,
        alpha: bool,
        /// At most one converted frame per interval (the preview's rate).
        interval: Option<Duration>,
    },
    Shutdown,
}

struct Window {
    surface: Surface<'static>,
    config: SurfaceConfiguration,
}

struct Output {
    scene: Scene,
    target: FrameTarget,
    frame_height: u32,
    window: Option<Window>,
    seq: u64,
    committed: u64,
    /// A frame is due at this time.
    due: Option<Instant>,
    last_frame: Option<Instant>,
    /// A fade was running when the loop last looked.
    animated: bool,
    /// Frames drawn since the output opened.
    frames: u64,
    /// The video nodes last reported.
    media: Vec<MediaUse>,
    web_pending: Option<(Instant, Instant)>,
    /// Takes every frame drawn (Syphon).
    frame_sink: Option<Box<dyn FrameSink>>,
    /// Frame taps: every drawn frame converted for the NDI sender (UYVA,
    /// prompt 061) or the recorder (BGRA, prompt 064), each on its own
    /// read-back ring, so a slow one never holds up another.
    taps: Vec<(Tap, Capture)>,
}

/// Takes each frame an output draws, on the compositor thread (Syphon).
pub trait FrameSink: Send {
    /// While the frame's GPU work is recorded: copy it on the GPU.
    fn encode(&mut self, frame: &mut SinkFrame<'_>);
    /// Once the GPU has finished the frame.
    fn finished(&mut self, frame: &FinishedFrame<'_>);
}

/// A frame being recorded, for [`FrameSink::encode`].
pub struct SinkFrame<'a> {
    renderer: &'a mut Renderer,
    encoder: &'a mut CommandEncoder,
    target: &'a FrameTarget,
}

impl SinkFrame<'_> {
    pub fn device(&self) -> &Device {
        self.renderer.device()
    }

    /// The frame's size: 1920 × frame height.
    pub fn size(&self) -> (u32, u32) {
        (self.target.width, self.target.height)
    }

    /// Records a copy of the frame, transparency kept, into `view` (a
    /// `format` texture of [`SinkFrame::size`]): no read-back.
    pub fn copy_to(&mut self, view: &TextureView, format: TextureFormat) {
        self.renderer
            .copy(self.encoder, self.target, view, format, self.size());
    }
}

/// A frame the GPU finished, for [`FrameSink::finished`].
pub struct FinishedFrame<'a> {
    renderer: &'a Renderer,
    target: &'a FrameTarget,
}

impl FinishedFrame<'_> {
    pub fn size(&self) -> (u32, u32) {
        (self.target.width, self.target.height)
    }

    /// The frame read back as tight premultiplied RGBA rows: the slow
    /// path, for sinks that cannot take it on the GPU.
    pub fn read_rgba(&self) -> Vec<u8> {
        self.renderer.read_frame(self.target)
    }
}

/// Which tap of an output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tap {
    /// UYVA (NDI, prompt 061) or UYVY at a card's mode (DeckLink, 062):
    /// an output is one or the other.
    Video,
    /// Premultiplied BGRA, for the recorder.
    Record,
    /// Straight-alpha BGRA, box-filtered down, at a capped rate: the
    /// operator preview (prompt 066).
    Preview,
}

/// Where a tap's frames go.
pub type CaptureSink = Arc<dyn Fn(crate::uyva::UyvaFrame) + Send + Sync>;

struct Capture {
    converter: crate::uyva::UyvaConverter,
    sink: CaptureSink,
    interval: Option<Duration>,
    /// When a frame was last handed to the converter.
    last: Option<Instant>,
}

/// Feeds live sources from other threads (the video players).
#[derive(Clone)]
pub struct SourceFeed {
    tx: mpsc::Sender<Msg>,
}

impl SourceFeed {
    /// Diagnostic readback from a worker, without blocking the window thread.
    pub fn read(&self, output: &str) -> Option<(u32, u32, Vec<u8>)> {
        let (tx, rx) = mpsc::channel();
        self.tx
            .send(Msg::Read {
                output: output.into(),
                reply: tx,
            })
            .ok()?;
        rx.recv_timeout(Duration::from_secs(2)).ok().flatten()
    }

    /// Premultiplied RGBA from a captured web page.
    pub fn rgba_frame(&self, id: u64, frame: Arc<Raster>, started: Instant, ready: Instant) {
        let _ = self.tx.send(Msg::Source {
            id,
            frame,
            timing: Some((started, ready)),
        });
    }

    /// The newest decoded frame of video source `id`.
    pub fn video_frame(&self, id: u64, frame: Arc<YuvFrame>) {
        let _ = self.tx.send(Msg::SourceYuv { id, frame });
    }

    /// The source's texture is no longer needed.
    pub fn remove(&self, id: u64) {
        let _ = self.tx.send(Msg::RemoveSource { id });
    }
}

/// The engine's handle on the compositor thread.
pub struct Compositor {
    tx: mpsc::Sender<Msg>,
    thread: Option<JoinHandle<()>>,
    instance: wgpu::Instance,
}

impl Compositor {
    /// Opens the GPU and starts the thread. `events` is called on the
    /// compositor thread.
    pub fn start(events: impl Fn(CompositorEvent) + Send + Sync + 'static) -> Result<Self, String> {
        let instance =
            wgpu::Instance::new(InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = pollster::block_on(instance.request_adapter(&RequestAdapterOptions {
            power_preference: PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(|e| format!("no GPU adapter: {e}"))?;
        let renderer = Renderer::new(&adapter)?;
        tracing::info!("compositor GPU: {}", renderer.adapter_info().name);
        let events = Arc::new(events);
        let lost = events.clone();
        renderer
            .device()
            .set_device_lost_callback(move |reason, message| {
                lost(CompositorEvent::DeviceLost(format!(
                    "{reason:?}: {message}"
                )));
            });
        let (tx, rx) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("compositor".into())
            .spawn(move || run(renderer, adapter, rx, events))
            .map_err(|e| format!("cannot start the compositor thread: {e}"))?;
        Ok(Self {
            tx,
            thread: Some(thread),
            instance,
        })
    }

    /// For creating window surfaces on the window's own thread.
    pub fn instance(&self) -> &wgpu::Instance {
        &self.instance
    }

    fn send(&self, msg: Msg) {
        // The thread only stops on `Shutdown` or a panic; either way the
        // engine is going down and there is nothing left to tell.
        let _ = self.tx.send(msg);
    }

    pub fn open(&self, output: &str, frame_height: u32, sink: Sink) {
        self.send(Msg::Open {
            output: output.into(),
            frame_height,
            sink,
        });
    }

    pub fn close(&self, output: &str) {
        self.send(Msg::Close {
            output: output.into(),
        });
    }

    pub fn resize(&self, output: &str, size: (u32, u32)) {
        self.send(Msg::Resize {
            output: output.into(),
            size,
        });
    }

    /// Draws the output again (exposed, no longer occluded).
    pub fn invalidate(&self, output: &str) {
        self.send(Msg::Invalidate {
            output: output.into(),
        });
    }

    /// Applies `ops` in order at one instant; the next frame carries `seq`.
    pub fn apply(&self, output: &str, seq: u64, ops: Vec<Op>) {
        self.send(Msg::Ops {
            output: output.into(),
            seq,
            ops,
        });
    }

    /// Stop asynchronously so the window thread can continue servicing native
    /// destruction while the GPU thread releases its window surfaces.
    pub fn begin_shutdown(&self) {
        self.send(Msg::Shutdown);
    }
    pub fn finished(&self) -> bool {
        self.thread.as_ref().is_none_or(|t| t.is_finished())
    }

    pub fn source_frame(&self, id: u64, frame: Arc<Raster>) {
        self.send(Msg::Source {
            id,
            frame,
            timing: None,
        });
    }

    pub fn remove_source(&self, id: u64) {
        self.send(Msg::RemoveSource { id });
    }

    /// Uploads `rasters` and keeps their textures for `output` until the
    /// next call, so showing one later is a texture change, not an upload.
    pub fn keep_resident(&self, output: &str, rasters: Vec<std::sync::Arc<crate::Raster>>) {
        self.send(Msg::Resident {
            output: output.into(),
            rasters,
        });
    }

    /// Hands every frame `output` draws to `sink`, or stops (`None`). The
    /// output draws at once so the sink starts with the current picture.
    pub fn attach(&self, output: &str, sink: Option<Box<dyn FrameSink>>) {
        self.send(Msg::Attach {
            output: output.into(),
            sink,
        });
    }

    /// A handle that feeds sources from any thread.
    pub fn feed(&self) -> SourceFeed {
        SourceFeed {
            tx: self.tx.clone(),
        }
    }

    /// The output's current frame as premultiplied RGBA, after any pending
    /// work. Blocks until the compositor answers.
    pub fn read(&self, output: &str) -> Option<(u32, u32, Vec<u8>)> {
        let (reply, rx) = mpsc::channel();
        self.send(Msg::Read {
            output: output.into(),
            reply,
        });
        rx.recv().ok().flatten()
    }

    /// How many frames the output has drawn since it opened.
    /// Converts every frame `output` draws to UYVA (with alpha) for
    /// `sink`, or stops. A frame whose read-back slot is busy is dropped.
    pub fn capture(&self, output: &str, sink: Option<CaptureSink>) {
        self.tap(output, Tap::Video, sink);
    }

    /// Sets or removes one of `output`'s frame taps. The output must be
    /// open; a tap goes when the output closes.
    pub fn tap(&self, output: &str, tap: Tap, sink: Option<CaptureSink>) {
        self.send(Msg::Capture {
            output: output.into(),
            tap,
            sink,
            raster: None,
            alpha: true,
            interval: None,
        });
    }

    /// Sends `output`'s frames, downscaled to `size` and at most `fps` a
    /// second, to `sink` (the operator preview). A
    /// change that lands between two preview frames is still sent, once the
    /// interval has passed.
    pub fn preview(&self, output: &str, size: (u32, u32), fps: u32, sink: CaptureSink) {
        self.send(Msg::Capture {
            output: output.into(),
            tap: Tap::Preview,
            sink: Some(sink),
            raster: Some(size),
            alpha: true,
            interval: Some(Duration::from_secs(1) / fps.clamp(1, 60)),
        });
    }

    /// Stops `output`'s preview tap.
    pub fn stop_preview(&self, output: &str) {
        self.send(Msg::Capture {
            output: output.into(),
            tap: Tap::Preview,
            sink: None,
            raster: None,
            alpha: true,
            interval: None,
        });
    }

    /// As [`Compositor::capture`], scaled (nearest neighbour) to `raster`
    /// and with or without the alpha plane: a video card's mode (062).
    pub fn capture_raster(
        &self,
        output: &str,
        raster: (u32, u32),
        alpha: bool,
        sink: Option<CaptureSink>,
    ) {
        self.send(Msg::Capture {
            output: output.into(),
            tap: Tap::Video,
            sink,
            raster: Some(raster),
            alpha,
            interval: None,
        });
    }

    pub fn frames(&self, output: &str) -> Option<u64> {
        let (reply, rx) = mpsc::channel();
        self.send(Msg::Frames {
            output: output.into(),
            reply,
        });
        rx.recv().ok().flatten()
    }
}

impl Drop for Compositor {
    fn drop(&mut self) {
        self.send(Msg::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A swapchain format the compositor writes without conversion: frames are
/// sRGB-encoded values, so an `*Srgb` view would encode them twice.
fn window_format(surface: &Surface<'_>, adapter: &Adapter) -> Option<TextureFormat> {
    let formats = surface.get_capabilities(adapter).formats;
    formats
        .iter()
        .copied()
        .find(|f| matches!(f, TextureFormat::Bgra8Unorm | TextureFormat::Rgba8Unorm))
        .or_else(|| formats.iter().copied().find(|f| !f.is_srgb()))
}

fn run(
    mut renderer: Renderer,
    adapter: Adapter,
    rx: mpsc::Receiver<Msg>,
    events: Arc<dyn Fn(CompositorEvent) + Send + Sync>,
) {
    let mut outputs: HashMap<String, Output> = HashMap::new();
    // Video sources whose frames the GPU cannot take, reported once.
    let mut refused = std::collections::HashSet::new();
    loop {
        let now = Instant::now();
        let wake = outputs
            .values()
            .filter_map(|o| {
                o.due
                    .or_else(|| o.scene.stack.next_change(now))
                    // A fade that ended while a slow frame was drawing still
                    // owes its frame at rest; nothing else would wake for it.
                    .or_else(|| o.animated.then_some(now))
            })
            .min();
        let first = match wake {
            None => match rx.recv() {
                Ok(msg) => Some(msg),
                Err(_) => return,
            },
            Some(at) => match rx.recv_timeout(at.saturating_duration_since(now)) {
                Ok(msg) => Some(msg),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            },
        };
        let mut pending: Vec<Msg> = first.into_iter().collect();
        pending.extend(rx.try_iter());
        for msg in pending {
            if !handle(
                msg,
                &mut renderer,
                &adapter,
                &mut outputs,
                &mut refused,
                &events,
            ) {
                return;
            }
            for (name, output) in &mut outputs {
                report_media(name, output, &events);
            }
        }
        let now = Instant::now();
        for (name, output) in &mut outputs {
            output.scene.stack.prune(now);
            report_media(name, output, &events);
            // Both run every pass: a changed frame must be seen even while a fade runs.
            let fading = output.scene.stack.animating(now);
            let frames = output.scene.stack.frames_changed(now);
            let animating = fading || frames;
            if output.due.is_none() {
                output.due = next_frame(
                    animating,
                    output.animated,
                    output.window.is_some(),
                    output.last_frame,
                    now,
                );
            }
            output.animated = animating;
            if output.due.is_some_and(|at| at <= now) {
                output.due = None;
                draw(&mut renderer, name, output, now, &events);
            }
        }
    }
}

/// Tells the engine when an output's video nodes changed.
fn report_media(
    name: &str,
    output: &mut Output,
    events: &Arc<dyn Fn(CompositorEvent) + Send + Sync>,
) {
    let uses = output.scene.stack.media_uses();
    if uses != output.media {
        output.media = uses.clone();
        events(CompositorEvent::Media {
            output: name.to_owned(),
            uses,
        });
    }
}

/// When the next frame is due, if one is, for an output with none pending.
///
/// A running fade wants a frame now: windows pace themselves on vsync,
/// offscreen frames on a timer. The loop after a fade ends wants one more:
/// the last frame inside the fade was drawn up to a vsync before its end,
/// so without it a window keeps the old node faintly on screen.
fn next_frame(
    animating: bool,
    was_animating: bool,
    window: bool,
    last_frame: Option<Instant>,
    now: Instant,
) -> Option<Instant> {
    if !animating && !was_animating {
        return None;
    }
    Some(match (window, last_frame) {
        (false, Some(last)) => (last + OFFSCREEN_FRAME).max(now),
        _ => now,
    })
}

/// Handles one message. False when the thread should stop.
fn handle(
    msg: Msg,
    renderer: &mut Renderer,
    adapter: &Adapter,
    outputs: &mut HashMap<String, Output>,
    refused: &mut std::collections::HashSet<u64>,
    events: &Arc<dyn Fn(CompositorEvent) + Send + Sync>,
) -> bool {
    let now = Instant::now();
    match msg {
        Msg::Shutdown => return false,
        Msg::Open {
            output,
            frame_height,
            sink,
        } => {
            let window = match sink {
                Sink::Offscreen => None,
                Sink::Window { surface, size } => {
                    let Some(format) = window_format(&surface, adapter) else {
                        events(CompositorEvent::Error {
                            output: Some(output),
                            message: "the window has no usable surface format".into(),
                        });
                        return true;
                    };
                    let Some(mut config) =
                        surface.get_default_config(adapter, size.0.max(1), size.1.max(1))
                    else {
                        events(CompositorEvent::Error {
                            output: Some(output),
                            message: "the window surface is not supported by the GPU".into(),
                        });
                        return true;
                    };
                    config.format = format;
                    config.view_formats = vec![];
                    config.present_mode = PresentMode::AutoVsync;
                    surface.configure(renderer.device(), &config);
                    Some(Window { surface, config })
                }
            };
            // A reopened output keeps its scene: what it showed comes back.
            let previous = outputs.remove(&output);
            let (scene, seq) = previous.map_or((Scene::default(), 0), |o| (o.scene, o.seq));
            outputs.insert(
                output,
                Output {
                    scene,
                    target: renderer.target(frame_height),
                    frame_height,
                    window,
                    seq,
                    committed: 0,
                    due: Some(now),
                    last_frame: None,
                    animated: false,
                    frames: 0,
                    media: Vec::new(),
                    web_pending: None,
                    frame_sink: None,
                    taps: Vec::new(),
                },
            );
        }
        Msg::Close { output } => {
            renderer.keep_resident(&output, &[]);
            if let Some(o) = outputs.remove(&output)
                && !o.media.is_empty()
            {
                events(CompositorEvent::Media {
                    output,
                    uses: Vec::new(),
                });
            }
        }
        Msg::Resize { output, size } => {
            if let Some(o) = outputs.get_mut(&output)
                && let Some(window) = o.window.as_mut()
                // Zero-size resizes (minimised, display leaving) are skipped.
                && size.0 > 0
                && size.1 > 0
            {
                window.config.width = size.0;
                window.config.height = size.1;
                window.surface.configure(renderer.device(), &window.config);
                o.due = Some(now);
            }
        }
        Msg::Invalidate { output } => {
            if let Some(o) = outputs.get_mut(&output) {
                o.due = Some(now);
            }
        }
        Msg::Ops { output, seq, ops } => {
            let Some(o) = outputs.get_mut(&output) else {
                events(CompositorEvent::Error {
                    output: Some(output),
                    message: "layer change for an output the compositor does not have".into(),
                });
                return true;
            };
            for op in ops {
                if let Op::StageCanvas(size) = op {
                    if let Some(size) = size {
                        let size = (size.0.clamp(1, 1920), size.1.clamp(1, 1080));
                        if (o.target.width, o.target.height) != size {
                            o.target = renderer.target_size(size);
                        }
                    } else if (o.target.width, o.target.height)
                        != (crate::FRAME_WIDTH, crate::frame_height(o.frame_height))
                    {
                        o.target = renderer.target(o.frame_height);
                    }
                    continue;
                }
                if let Op::Media(control) = op {
                    // The video it addresses may have been made by an
                    // earlier operation in this batch: report that first.
                    report_media(&output, o, events);
                    let current = o.media.iter().find(|u| u.transport).map(|u| u.id);
                    if let Some(id) = current {
                        events(CompositorEvent::Transport {
                            output: output.clone(),
                            id,
                            control,
                        });
                    }
                    continue;
                }
                apply(&mut o.scene, op, o.target.height, now);
            }
            o.seq = o.seq.max(seq);
            o.due = Some(now);
        }
        Msg::Source { id, frame, timing } => {
            renderer.update_source(id, &frame);
            for o in outputs.values_mut() {
                if o.scene.stack.sources().any(|s| s == id) {
                    o.due = Some(o.due.map_or(now, |d| d.min(now)));
                    if timing.is_some() {
                        o.web_pending = timing;
                    }
                }
            }
        }
        Msg::Resident { output, rasters } => {
            renderer.keep_resident(&output, &rasters);
        }
        Msg::RemoveSource { id } => {
            renderer.remove_source(id);
            refused.remove(&id);
        }
        Msg::SourceYuv { id, frame } => match renderer.update_source_yuv(id, &frame) {
            Ok(()) => {
                for o in outputs.values_mut() {
                    if o.scene.stack.sources().any(|s| s == id) {
                        o.due = Some(o.due.map_or(now, |d| d.min(now)));
                    }
                }
            }
            // Said once per source: a stream's frames all share the fault.
            Err(message) => {
                if refused.insert(id) {
                    tracing::warn!(source = id, "video frames refused: {message}");
                    events(CompositorEvent::Error {
                        output: None,
                        message: format!("a video cannot be shown: {message}"),
                    });
                }
            }
        },
        Msg::Read { output, reply } => {
            let frame = outputs.get_mut(&output).map(|o| {
                if o.due.is_some() {
                    // Draw what is pending first, so the reader sees it.
                    let mut encoder = renderer
                        .device()
                        .create_command_encoder(&Default::default());
                    renderer.draw_frame(&mut encoder, &o.scene, &o.target, now);
                    renderer.queue().submit([encoder.finish()]);
                }
                (
                    o.target.width,
                    o.target.height,
                    renderer.read_frame(&o.target),
                )
            });
            let _ = reply.send(frame);
        }
        Msg::Frames { output, reply } => {
            let _ = reply.send(outputs.get(&output).map(|o| o.frames));
        }
        Msg::Attach { output, sink } => match outputs.get_mut(&output) {
            Some(o) => {
                o.frame_sink = sink;
                o.due = Some(now);
            }
            None if sink.is_some() => events(CompositorEvent::Error {
                output: Some(output),
                message: "frame sink for an output the compositor does not have".into(),
            }),
            None => {}
        },
        Msg::Capture {
            output,
            tap,
            sink,
            raster,
            alpha,
            interval,
        } => {
            if let Some(o) = outputs.get_mut(&output) {
                o.taps.retain(|(t, _)| *t != tap);
                if let Some(sink) = sink {
                    let (w, h) = raster.unwrap_or((o.target.width, o.target.height));
                    let made = match tap {
                        Tap::Video => {
                            crate::uyva::UyvaConverter::new(renderer.device(), w, h, alpha)
                        }
                        Tap::Record => crate::uyva::UyvaConverter::bgra(renderer.device(), w, h),
                        Tap::Preview => {
                            crate::uyva::UyvaConverter::preview(renderer.device(), w, h)
                        }
                    };
                    match made {
                        Ok(converter) => o.taps.push((
                            tap,
                            Capture {
                                converter,
                                sink,
                                interval,
                                last: None,
                            },
                        )),
                        Err(message) => events(CompositorEvent::Error {
                            output: Some(output.clone()),
                            message,
                        }),
                    }
                }
                // The receiver gets the current picture at once.
                o.due = Some(now);
            }
        }
    }
    true
}

pub(crate) fn apply(scene: &mut Scene, op: Op, frame_height: u32, now: Instant) {
    match op {
        Op::Swap { slot, content, ms } => {
            scene.stack.swap(slot, content, ms, now);
            if slot == Slot::Camera {
                scene.restyle_camera(frame_height);
            }
        }
        Op::Keyed { slot, content, ms } => {
            scene.stack.set_keyed(slot, content, ms, now);
        }
        Op::Update { slot, key, content } => {
            scene.stack.update_keyed(slot, &key, content, now);
        }
        Op::ProgramBackground(color) => scene.program_background = color,
        Op::Layout(layout) => {
            scene.layout = layout;
            scene.restyle_camera(frame_height);
        }
        // Routed to the playback layer by the thread before it gets here.
        Op::Media(_) | Op::StageCanvas(_) => {}
    }
}

/// Hands the frame just drawn to every tap whose rate allows it. A tap held
/// back by its rate asks for a frame when its interval ends, so the last
/// change before the picture comes to rest is still sent.
fn convert_taps(renderer: &Renderer, name: &str, output: &mut Output, now: Instant) {
    let mut due = None::<Instant>;
    for (tap, capture) in &mut output.taps {
        if let (Some(interval), Some(last)) = (capture.interval, capture.last)
            && now < last + interval
        {
            let at = last + interval;
            due = Some(due.map_or(at, |d| d.min(at)));
            continue;
        }
        capture.last = Some(now);
        let sink = capture.sink.clone();
        let name = name.to_owned();
        let tap = *tap;
        // Completed by the device wait after the frame; a busy converter
        // drops the frame.
        capture.converter.convert(
            renderer.device(),
            renderer.queue(),
            output.target.texture(),
            Box::new(move |frame| match frame {
                Ok(frame) => sink(frame),
                Err(error) => tracing::warn!(output = name, ?tap, %error, "frame read-back failed"),
            }),
        );
    }
    if let Some(at) = due {
        output.due = Some(output.due.map_or(at, |d| d.min(at)));
    }
}

fn draw(
    renderer: &mut Renderer,
    name: &str,
    output: &mut Output,
    now: Instant,
    events: &Arc<dyn Fn(CompositorEvent) + Send + Sync>,
) {
    let mut encoder = renderer
        .device()
        .create_command_encoder(&Default::default());
    renderer.draw_frame(&mut encoder, &output.scene, &output.target, now);
    if let Some(sink) = output.frame_sink.as_mut() {
        sink.encode(&mut SinkFrame {
            renderer,
            encoder: &mut encoder,
            target: &output.target,
        });
    }
    let mut presented = None;
    if let Some(window) = output.window.as_mut() {
        match window.surface.get_current_texture() {
            CurrentSurfaceTexture::Success(frame) | CurrentSurfaceTexture::Suboptimal(frame) => {
                let view = frame.texture.create_view(&Default::default());
                let size = (window.config.width, window.config.height);
                renderer.present(
                    &mut encoder,
                    &output.target,
                    &view,
                    window.config.format,
                    size,
                );
                presented = Some(frame);
            }
            CurrentSurfaceTexture::Outdated | CurrentSurfaceTexture::Lost => {
                window.surface.configure(renderer.device(), &window.config);
                output.due = Some(now);
                renderer.queue().submit([encoder.finish()]);
                return;
            }
            other => {
                let why = match other {
                    CurrentSurfaceTexture::Occluded => "occluded",
                    CurrentSurfaceTexture::Timeout => "timeout",
                    _ => "validation",
                };
                // The frame target is still drawn, for offscreen readers:
                // a rehearsal window behind the operator's still feeds its
                // preview, NDI and recording.
                renderer.queue().submit([encoder.finish()]);
                output.due = Some(now + PRESENT_RETRY);
                convert_taps(renderer, name, output, now);
                if let Err(error) = renderer.device().poll(PollType::wait_indefinitely()) {
                    events(CompositorEvent::Error {
                        output: Some(name.into()),
                        message: format!("GPU wait failed: {error}"),
                    });
                }
                events(CompositorEvent::Dropped {
                    output: name.into(),
                    why,
                });
                return;
            }
        }
    }
    renderer.queue().submit([encoder.finish()]);
    if let Some(frame) = presented {
        renderer.queue().present(frame);
    }
    convert_taps(renderer, name, output, now);
    output.last_frame = Some(now);
    output.frames += 1;
    // GPU work for this frame is done before it is reported.
    if let Err(error) = renderer.device().poll(PollType::wait_indefinitely()) {
        events(CompositorEvent::Error {
            output: Some(name.into()),
            message: format!("GPU wait failed: {error}"),
        });
        return;
    }
    if let Some(sink) = output.frame_sink.as_mut() {
        sink.finished(&FinishedFrame {
            renderer,
            target: &output.target,
        });
    }
    if let Some((started, ready)) = output.web_pending.take() {
        events(CompositorEvent::WebPresented {
            output: name.into(),
            capture_us: ready.duration_since(started).as_micros() as u64,
            compose_us: ready.elapsed().as_micros() as u64,
            frames: output.frames,
        });
    }
    if output.seq > output.committed || output.committed == 0 {
        output.committed = output.seq;
        events(CompositorEvent::Committed {
            output: name.into(),
            seq: output.seq,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fade_that_ends_gets_one_frame_at_rest() {
        let now = Instant::now();
        let last = Some(now - Duration::from_millis(5));
        // Running: a window draws now, offscreen on the frame timer.
        assert_eq!(next_frame(true, true, true, last, now), Some(now));
        assert_eq!(
            next_frame(true, true, false, last, now),
            Some(now - Duration::from_millis(5) + OFFSCREEN_FRAME)
        );
        // Just ended: one more frame, so the fade's end is what shows.
        assert_eq!(next_frame(false, true, true, last, now), Some(now));
        // Still: nothing.
        assert_eq!(next_frame(false, false, true, last, now), None);
        assert_eq!(next_frame(false, false, false, None, now), None);
    }
}
