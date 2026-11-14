//! Slide-card thumbnails (prompt 066): a slide's words drawn by
//! `kairo-scene`, the same code the engine draws them with, scaled to the
//! card.
//!
//! Electron laid every thumbnail out at 1920 × 1080 and scaled it with CSS
//! (`ScaledOverlayPreview`), so a 160-pixel tile never auto-fits as if it
//! were the frame. Here the scene is built and drawn at 1920 × 1080 on the
//! raster pool's worker threads, reduced to the card's size there, and
//! handed back. Until it arrives the card shows a placeholder; a scene that
//! cannot be built stays a placeholder.
//!
//! The thumbnail holds the text layer only. The card draws the theme's
//! background beneath it (a colour, gradient or picture), as Electron's
//! `stillVideos` already reduced a video background to a still.
//!
//! Images are kept in an LRU keyed by the scene's content hash; requests
//! that give the same scene share one image. An image pushed out is evicted
//! from every window's sprite atlas.

use gpui_kit::*;
use image::{Frame, RgbaImage};
use kairo_domain::overlay::OverlayTheme;
use kairo_engine_proto::preview::box_pixel;
use kairo_scene::{ColoredLine, RasterPool, SlideScene, TextRaster};
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How often finished drawings are looked for while some are outstanding.
const DRAIN_EVERY: Duration = Duration::from_millis(16);

/// The frame every thumbnail is laid out in (`OVERLAY_FRAME_WIDTH/HEIGHT`).
pub const FRAME: (u32, u32) = (1920, 1080);

/// One slide to draw. Build it once per slide and keep it: the key is
/// computed here, not at every paint.
#[derive(Clone)]
pub struct ThumbnailRequest {
    key: u64,
    theme: Arc<OverlayTheme>,
    reference: String,
    text: String,
    colored: Vec<ColoredLine>,
}

impl ThumbnailRequest {
    pub fn new(
        theme: Arc<OverlayTheme>,
        reference: impl Into<String>,
        text: impl Into<String>,
        colored: Vec<ColoredLine>,
    ) -> Self {
        let reference = reference.into();
        let text = text.into();
        let mut h = DefaultHasher::new();
        serde_json::to_string(theme.as_ref())
            .unwrap_or_default()
            .hash(&mut h);
        reference.hash(&mut h);
        text.hash(&mut h);
        for line in &colored {
            line.text.hash(&mut h);
            line.color.hash(&mut h);
        }
        Self {
            key: h.finish(),
            theme,
            reference,
            text,
            colored,
        }
    }

    pub fn key(&self) -> u64 {
        self.key
    }
}

/// What a card shows.
#[derive(Clone)]
pub enum Thumbnail {
    Ready(Arc<RenderImage>),
    /// Being drawn: show the placeholder.
    Pending,
    /// The scene could not be built (a theme that does not parse): the
    /// placeholder stays.
    Failed,
}

#[derive(Clone, Copy)]
enum Slot {
    Pending,
    Ready(u64),
    Failed,
}

/// A finished drawing: the request's key, and the scene hash and pixels.
type Finished = (u64, Option<(u64, Arc<Vec<u8>>)>);

/// Counters for tests and diagnostics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ThumbnailStats {
    pub drawn: u64,
    pub shared: u64,
    pub evicted: u64,
    pub failed: u64,
}

pub struct Thumbnails {
    size: (u32, u32),
    capacity: usize,
    pool: Arc<RasterPool<Arc<Vec<u8>>>>,
    slots: HashMap<u64, Slot>,
    /// Least recently used first.
    images: VecDeque<(u64, Arc<RenderImage>)>,
    /// Drawings the workers finished, taken on the UI thread. Workers never
    /// wake a UI task themselves.
    done: Arc<Mutex<Vec<Finished>>>,
    /// Jobs submitted and not yet taken.
    outstanding: usize,
    stats: ThumbnailStats,
    /// Looks for finished drawings while any are outstanding.
    drain: Option<Task<()>>,
}

impl Thumbnails {
    /// Thumbnails of `size` pixels (the card's device pixels), at most
    /// `capacity` kept, drawn on `workers` threads.
    pub fn new(size: (u32, u32), capacity: usize, workers: usize, _cx: &mut Context<Self>) -> Self {
        let size = (size.0.max(1), size.1.max(1));
        // The pool's own cache holds full-size rasters; the thumbnails are
        // cached here, so it keeps no more than the one it just drew.
        let pool = Arc::new(RasterPool::new(workers, 0, move |raster| {
            Arc::new(downscale(raster.as_ref(), FRAME, size))
        }));
        Self {
            size,
            capacity: capacity.max(1),
            pool,
            slots: HashMap::new(),
            images: VecDeque::new(),
            done: Arc::default(),
            outstanding: 0,
            stats: ThumbnailStats::default(),
            drain: None,
        }
    }

    pub fn stats(&self) -> ThumbnailStats {
        self.stats
    }

    /// Images held.
    pub fn len(&self) -> usize {
        self.images.len()
    }

    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }

    /// The thumbnail for `request`, starting it if it is not drawn yet.
    /// The entity is notified when a pending one is ready.
    pub fn get(&mut self, request: &ThumbnailRequest, cx: &mut Context<Self>) -> Thumbnail {
        match self.slots.get(&request.key).copied() {
            Some(Slot::Pending) => return Thumbnail::Pending,
            Some(Slot::Failed) => return Thumbnail::Failed,
            Some(Slot::Ready(hash)) => {
                if let Some(i) = self.images.iter().position(|(h, _)| *h == hash) {
                    let entry = self.images.remove(i).expect("in range");
                    let image = entry.1.clone();
                    self.images.push_back(entry);
                    return Thumbnail::Ready(image);
                }
            }
            None => {}
        }
        self.slots.insert(request.key, Slot::Pending);
        let job = request.clone();
        let done = self.done.clone();
        let key = request.key;
        self.pool.submit(
            Box::new(move || {
                Some(SlideScene::build(
                    &job.theme,
                    &job.reference,
                    &job.text,
                    FRAME,
                    &job.colored,
                ))
            }),
            move |finished| {
                let result = finished.value.map(|pixels| (finished.hash, pixels));
                done.lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push((key, result));
            },
        );
        self.outstanding += 1;
        if self.drain.is_none() {
            self.drain = Some(cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(DRAIN_EVERY).await;
                    let more = this
                        .update(cx, |this, cx| this.take_finished(cx))
                        .unwrap_or(false);
                    if !more {
                        break;
                    }
                }
            }));
        }
        Thumbnail::Pending
    }

    /// Takes what the workers finished. False when nothing is outstanding.
    fn take_finished(&mut self, cx: &mut Context<Self>) -> bool {
        let finished = std::mem::take(&mut *self.done.lock().unwrap_or_else(|e| e.into_inner()));
        if !finished.is_empty() {
            self.outstanding = self.outstanding.saturating_sub(finished.len());
            for result in finished {
                self.finish(result, cx);
            }
            cx.notify();
        }
        if self.outstanding == 0 {
            self.drain = None;
            return false;
        }
        true
    }

    fn finish(&mut self, (key, result): Finished, cx: &mut Context<Self>) {
        let Some((hash, pixels)) = result else {
            self.slots.insert(key, Slot::Failed);
            self.stats.failed += 1;
            return;
        };
        self.slots.insert(key, Slot::Ready(hash));
        if self.images.iter().any(|(h, _)| *h == hash) {
            self.stats.shared += 1;
            return;
        }
        let (w, h) = self.size;
        let Some(buffer) = RgbaImage::from_raw(
            w,
            h,
            Arc::try_unwrap(pixels).unwrap_or_else(|shared| (*shared).clone()),
        ) else {
            self.slots.insert(key, Slot::Failed);
            self.stats.failed += 1;
            return;
        };
        self.images
            .push_back((hash, Arc::new(RenderImage::new([Frame::new(buffer)]))));
        self.stats.drawn += 1;
        while self.images.len() > self.capacity {
            let (old, image) = self.images.pop_front().expect("over capacity");
            // Requests that pointed at it are drawn again when next asked for.
            self.slots
                .retain(|_, slot| !matches!(slot, Slot::Ready(h) if *h == old));
            cx.drop_image(image, None);
            self.stats.evicted += 1;
        }
    }
}

/// A card's thumbnail: the image, or a placeholder until it is drawn.
pub fn thumbnail_element(thumbnail: &Thumbnail) -> AnyElement {
    match thumbnail {
        Thumbnail::Ready(image) => img(image.clone())
            .size_full()
            .object_fit(ObjectFit::Fill)
            .into_any_element(),
        Thumbnail::Pending | Thumbnail::Failed => div().size_full().into_any_element(),
    }
}

/// The text raster (premultiplied RGBA at its place in `frame`) reduced to
/// `size` as straight-alpha BGRA: each pixel is the mean of at most 8 × 8
/// source pixels under it, transparent outside what was drawn.
pub fn downscale(raster: Option<&TextRaster>, frame: (u32, u32), size: (u32, u32)) -> Vec<u8> {
    let (dw, dh) = size;
    let (fw, fh) = frame;
    let mut out = vec![0u8; dw as usize * dh as usize * 4];
    let Some(r) = raster else {
        return out;
    };
    let sample = |sx: u32, sy: u32| -> [u32; 4] {
        if sx < r.x || sy < r.y || sx >= r.x + r.width || sy >= r.y + r.height {
            return [0; 4];
        }
        let at = (((sy - r.y) * r.width + (sx - r.x)) * 4) as usize;
        [0, 1, 2, 3].map(|c| u32::from(r.pixels[at + c]))
    };
    // Only the rows and columns the raster covers can be non-transparent.
    let rows = (r.y * dh / fh).min(dh)..((r.y + r.height) * dh).div_ceil(fh).min(dh);
    let cols = (r.x * dw / fw).min(dw)..((r.x + r.width) * dw).div_ceil(fw).min(dw);
    for y in rows {
        for x in cols.clone() {
            let at = (y as usize * dw as usize + x as usize) * 4;
            out[at..at + 4].copy_from_slice(&box_pixel(sample, frame, size, x, y));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::downscale;
    use kairo_scene::TextRaster;

    #[test]
    fn a_raster_is_reduced_in_place_and_unpremultiplied() {
        // A 4 × 2 block of half-transparent red at (8, 4) in a 16 × 8 frame,
        // reduced to 4 × 2: it lands in the pixel at (2, 1).
        let raster = TextRaster {
            x: 8,
            y: 4,
            width: 4,
            height: 4,
            pixels: [100u8, 0, 0, 128].repeat(16),
        };
        let out = downscale(Some(&raster), (16, 8), (4, 2));
        let px = |x: usize, y: usize| &out[(y * 4 + x) * 4..(y * 4 + x) * 4 + 4];
        assert_eq!(px(2, 1), &[0, 0, 199, 128], "BGRA, straight alpha");
        assert_eq!(px(0, 0), &[0, 0, 0, 0]);
        assert_eq!(px(3, 1), &[0, 0, 0, 0]);
        assert_eq!(downscale(None, (16, 8), (4, 2)), vec![0; 32]);
    }
}
