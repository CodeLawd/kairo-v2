//! The compositor thread. It owns the device and every output's scene,
//! applies the engine's layer operations as they arrive, and draws an output
//! only when it changed or while one of its fades runs. Idle, it blocks on
//! its mailbox and uses no CPU or GPU.

use crate::camera::VideoLayout;
use crate::layers::{Content, Raster, Slot};
use crate::renderer::{FrameTarget, Renderer, Scene};
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
    },
    RemoveSource {
        id: u64,
    },
    Read {
        output: String,
        reply: mpsc::Sender<Option<(u32, u32, Vec<u8>)>>,
    },
    Frames {
        output: String,
        reply: mpsc::Sender<Option<u64>>,
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

    pub fn source_frame(&self, id: u64, frame: Arc<Raster>) {
        self.send(Msg::Source { id, frame });
    }

    pub fn remove_source(&self, id: u64) {
        self.send(Msg::RemoveSource { id });
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
            if !handle(msg, &mut renderer, &adapter, &mut outputs, &events) {
                return;
            }
        }
        let now = Instant::now();
        for (name, output) in &mut outputs {
            output.scene.stack.prune(now);
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
                    window,
                    seq,
                    committed: 0,
                    due: Some(now),
                    last_frame: None,
                    animated: false,
                    frames: 0,
                },
            );
        }
        Msg::Close { output } => {
            outputs.remove(&output);
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
            let height = o.target.height;
            for op in ops {
                apply(&mut o.scene, op, height, now);
            }
            o.seq = o.seq.max(seq);
            o.due = Some(now);
        }
        Msg::Source { id, frame } => {
            renderer.update_source(id, &frame);
            for o in outputs.values_mut() {
                if o.scene.stack.sources().any(|s| s == id) {
                    o.due = Some(o.due.map_or(now, |d| d.min(now)));
                }
            }
        }
        Msg::RemoveSource { id } => renderer.remove_source(id),
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
                // The frame target is still drawn, for offscreen readers.
                renderer.queue().submit([encoder.finish()]);
                output.due = Some(now + PRESENT_RETRY);
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
