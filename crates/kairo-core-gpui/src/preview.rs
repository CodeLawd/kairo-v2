//! The live output preview (prompt 066): an output's picture, as the engine
//! drew it, in a GPUI view.
//!
//! The view holds a [`PreviewSubscription`] only while it is painted. Each
//! new frame becomes a `RenderImage`; the one it replaces is evicted from
//! the window's sprite atlas with `drop_image` once it can no longer be on
//! screen (spike 002: without that the atlas grew from 60 MB to 3 GB in a
//! minute). A view that has not been painted for [`HIDDEN_AFTER`] (another
//! page is showing, the window is minimised or covered) lets go of its
//! subscription, so the engine converts nothing for it, and takes it up
//! again the next time it is drawn.

use gpui_kit::*;
use image::{Frame, RgbaImage};
use kairo_core::preview::{DEFAULT_FPS, PreviewHub, PreviewSubscription};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Not painted for this long: the view is not on screen.
pub const HIDDEN_AFTER: Duration = Duration::from_millis(1000);
/// While shown but still, the view repaints this often, which is how it
/// knows it is still on screen.
const HEARTBEAT: Duration = Duration::from_millis(250);
/// The size asked of the engine moves in steps, so resizing a panel does
/// not ask for a new size every pixel.
const SIZE_STEP: u32 = 32;
/// Size asked for before the view is first laid out.
const FIRST_SIZE: (u32, u32) = (640, 360);

/// The image on screen and the one it replaced. The replaced one may still
/// be drawn until the next paint, so it is evicted one frame later.
#[derive(Default)]
pub struct FrameSwap {
    current: Option<Arc<RenderImage>>,
    retired: Option<Arc<RenderImage>>,
}

impl FrameSwap {
    /// Shows `next`. Returns the image that is now safe to evict.
    pub fn push(&mut self, next: Arc<RenderImage>) -> Option<Arc<RenderImage>> {
        let evict = self.retired.take();
        self.retired = self.current.replace(next);
        evict
    }

    pub fn current(&self) -> Option<&Arc<RenderImage>> {
        self.current.as_ref()
    }

    /// Every image held, to evict when the view stops.
    pub fn clear(&mut self) -> Vec<Arc<RenderImage>> {
        self.current
            .take()
            .into_iter()
            .chain(self.retired.take())
            .collect()
    }
}

/// The size to ask the engine for, from the painted size in device pixels.
pub fn wanted_size(device: (f32, f32)) -> (u32, u32) {
    let step = |v: f32| ((v.max(1.0).ceil() as u32).div_ceil(SIZE_STEP) * SIZE_STEP).max(SIZE_STEP);
    (step(device.0), step(device.1))
}

/// Counters for tests and the soak.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PreviewStats {
    pub frames_shown: u64,
    pub images_dropped: u64,
    pub subscribes: u64,
    pub unsubscribes: u64,
}

/// What the view paints, in device pixels, and when.
#[derive(Default)]
struct Painted {
    at: Cell<Option<Instant>>,
    size: Cell<(f32, f32)>,
}

pub struct LivePreview {
    hub: PreviewHub,
    output: Option<String>,
    fps: u32,
    backdrop: bool,
    subscription: Option<PreviewSubscription>,
    frames: FrameSwap,
    seen: u64,
    buffer: Vec<u8>,
    painted: Rc<Painted>,
    poll: Option<Task<()>>,
    checker: Option<((u32, u32), Arc<RenderImage>)>,
    stats: PreviewStats,
    _release: Subscription,
}

impl LivePreview {
    pub fn new(hub: PreviewHub, cx: &mut Context<Self>) -> Self {
        // Images still held when the view goes are evicted from every window.
        let release = cx.on_release(|view: &mut Self, cx| {
            for image in view.frames.clear() {
                cx.drop_image(image, None);
            }
            if let Some((_, checker)) = view.checker.take() {
                cx.drop_image(checker, None);
            }
        });
        Self {
            hub,
            output: None,
            fps: DEFAULT_FPS,
            backdrop: true,
            subscription: None,
            frames: FrameSwap::default(),
            seen: 0,
            buffer: Vec::new(),
            painted: Rc::default(),
            poll: None,
            checker: None,
            stats: PreviewStats::default(),
            _release: release,
        }
    }

    /// Which output to show; `None` shows the empty frame.
    pub fn set_output(
        &mut self,
        output: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.output == output {
            return;
        }
        self.output = output;
        self.stop(window);
        cx.notify();
    }

    pub fn set_fps(&mut self, fps: u32) {
        self.fps = fps.clamp(1, 60);
        if let Some(sub) = self.subscription.as_mut() {
            sub.update(wanted_size(self.painted.size.get()), self.fps);
        }
    }

    /// The checkerboard behind see-through parts; off when something is
    /// drawn underneath (`ScaledOverlayPreview`'s `backdrop`).
    pub fn set_backdrop(&mut self, backdrop: bool, cx: &mut Context<Self>) {
        self.backdrop = backdrop;
        cx.notify();
    }

    pub fn stats(&self) -> PreviewStats {
        self.stats
    }

    pub fn subscribed(&self) -> bool {
        self.subscription.is_some()
    }

    /// Lets go of the subscription and the images.
    fn stop(&mut self, window: &mut Window) {
        if self.subscription.take().is_some() {
            self.stats.unsubscribes += 1;
        }
        self.poll = None;
        self.seen = 0;
        for image in self.frames.clear() {
            let _ = window.drop_image(image);
            self.stats.images_dropped += 1;
        }
    }

    /// Takes the subscription up if the view is being drawn without one.
    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(output) = self.output.clone() else {
            return;
        };
        if self.subscription.is_some() {
            return;
        }
        let size = match self.painted.size.get() {
            (w, h) if w > 0.0 && h > 0.0 => wanted_size((w, h)),
            _ => FIRST_SIZE,
        };
        match self.hub.subscribe(&output, size, self.fps) {
            Ok(sub) => {
                self.subscription = Some(sub);
                self.stats.subscribes += 1;
            }
            Err(error) => {
                tracing::warn!(output, %error, "preview could not start");
                return;
            }
        }
        // Painting counts from now, so the first look is not "hidden".
        self.painted.at.set(Some(Instant::now()));
        let interval = Duration::from_secs(1) / (self.fps * 2).max(2);
        self.poll = Some(cx.spawn_in(window, async move |view, cx| {
            loop {
                cx.background_executor().timer(interval).await;
                let go_on = view
                    .update_in(cx, |view, window, cx| view.tick(window, cx))
                    .unwrap_or(false);
                if !go_on {
                    break;
                }
            }
        }));
    }

    /// One look at the map. False when the view stopped.
    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let now = Instant::now();
        let shown = self
            .painted
            .at
            .get()
            .is_some_and(|at| now.duration_since(at) < HIDDEN_AFTER);
        if !shown {
            self.stop(window);
            return false;
        }
        let Some(sub) = self.subscription.as_mut() else {
            return false;
        };
        sub.ensure();
        let wanted = wanted_size(self.painted.size.get());
        sub.update(wanted, self.fps);
        if let Some((number, width, height)) = sub.read_into(self.seen, &mut self.buffer) {
            self.seen = number;
            let pixels = std::mem::take(&mut self.buffer);
            if let Some(buffer) = RgbaImage::from_raw(width, height, pixels) {
                // GPUI reads these bytes as BGRA, which is what the engine sent.
                let image = Arc::new(RenderImage::new([Frame::new(buffer)]));
                if let Some(old) = self.frames.push(image) {
                    let _ = window.drop_image(old);
                    self.stats.images_dropped += 1;
                }
                self.stats.frames_shown += 1;
                cx.notify();
                return true;
            }
        }
        // Still: repaint now and then to learn whether the view is shown.
        if self
            .painted
            .at
            .get()
            .is_some_and(|at| now.duration_since(at) >= HEARTBEAT)
        {
            cx.notify();
        }
        true
    }

    /// A checkerboard of 12-pixel squares at `size`, made once per size.
    fn checker(&mut self, size: (u32, u32), window: &mut Window) -> Arc<RenderImage> {
        if let Some((at, image)) = &self.checker
            && *at == size
        {
            return image.clone();
        }
        if let Some((_, old)) = self.checker.take() {
            let _ = window.drop_image(old);
        }
        let (w, h) = (size.0.max(1), size.1.max(1));
        let square = (12.0 * window.scale_factor()).round().max(1.0) as u32;
        let mut pixels = Vec::with_capacity(w as usize * h as usize * 4);
        for y in 0..h {
            for x in 0..w {
                let v = if ((x / square) + (y / square)).is_multiple_of(2) {
                    0x1a
                } else {
                    0x0d
                };
                pixels.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let image = Arc::new(RenderImage::new([Frame::new(
            RgbaImage::from_raw(w, h, pixels).expect("checker size"),
        )]));
        self.checker = Some((size, image.clone()));
        image
    }
}

impl Render for LivePreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.start(window, cx);
        let painted = self.painted.clone();
        let scale = window.scale_factor();
        let (w, h) = painted.size.get();
        let backdrop = (self.backdrop && w >= 1.0 && h >= 1.0)
            .then(|| self.checker((w as u32, h as u32), window));
        let frame = self.frames.current().cloned();
        div()
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(black())
            .children(backdrop.map(|image| img(image).absolute().size_full()))
            .children(frame.map(|image| {
                img(image)
                    .absolute()
                    .size_full()
                    .object_fit(ObjectFit::Contain)
            }))
            .child(
                canvas(
                    move |bounds, _, _| {
                        painted.size.set((
                            f32::from(bounds.size.width) * scale,
                            f32::from(bounds.size.height) * scale,
                        ));
                        painted
                    },
                    |_, painted, _, _| painted.at.set(Some(Instant::now())),
                )
                .absolute()
                .size_full(),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{FrameSwap, wanted_size};
    use gpui_kit::RenderImage;
    use image::{Frame, RgbaImage};
    use std::sync::Arc;

    fn image() -> Arc<RenderImage> {
        Arc::new(RenderImage::new([Frame::new(
            RgbaImage::from_raw(1, 1, vec![0; 4]).unwrap(),
        )]))
    }

    #[test]
    fn a_replaced_frame_is_evicted_one_frame_later() {
        let mut swap = FrameSwap::default();
        let (a, b, c) = (image(), image(), image());
        assert!(swap.push(a.clone()).is_none());
        assert!(swap.push(b.clone()).is_none(), "a may still be on screen");
        let evicted = swap.push(c.clone()).unwrap();
        assert_eq!(evicted.id, a.id);
        assert_eq!(swap.current().unwrap().id, c.id);
        let rest: Vec<_> = swap.clear().into_iter().map(|i| i.id).collect();
        assert_eq!(rest, [c.id, b.id]);
        assert!(swap.current().is_none());
    }

    #[test]
    fn the_size_asked_for_moves_in_steps() {
        assert_eq!(wanted_size((640.0, 360.0)), (640, 384));
        assert_eq!(wanted_size((641.0, 360.0)), (672, 384));
        assert_eq!(wanted_size((0.0, 0.0)), (32, 32));
        // A retina panel 400 points wide.
        assert_eq!(wanted_size((800.0, 450.0)), (800, 480));
    }
}
