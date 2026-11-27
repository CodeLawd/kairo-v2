//! Backgrounds and theme elements on their way to the compositor.
//!
//! `backgroundLayerHTML` and `elementHTML` made DOM; here they make
//! [`Content`]. Colours, gradients and the filter, blur and fit around a
//! picture are decided at once (the shader draws them). Pictures are read
//! and decoded on a worker pool, never on the winit or compositor threads,
//! and arrive later as an update to the node that asked for them, as an
//! `<img>` paints when it has loaded. Vector elements are drawn on the
//! workers too, and the slide waits for those only: they are quick and have
//! no file to read.

use kairo_compositor::{
    ColorFilter, Content, Fill, Fit, Frames, Look, MediaParams, Raster, Rect, Slot,
};
use kairo_domain::media::{normalize_media_motion, normalize_media_playback};
use kairo_domain::overlay::{MediaFit, OverlayElement, OverlayElementKind};
use kairo_domain::overlay_elements::{MAX_OVERLAY_ELEMENTS, normalize_overlay_elements};
use kairo_scene::color::{Rgba, parse_css};
use kairo_scene::element_text::render_element;
use kairo_scene::media::{BoxFit, FitMode, Picture, decode_picture};
use kairo_scene::vector::{element_opacity, snapped_box};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;

/// The largest picture file read: bigger is refused, not decoded.
const MAX_FILE: u64 = 256 << 20;

fn premultiplied(c: Rgba) -> [f32; 4] {
    let a = f32::from(c.a) / 255.0;
    [
        f32::from(c.r) / 255.0 * a,
        f32::from(c.g) / 255.0 * a,
        f32::from(c.b) / 255.0 * a,
        a,
    ]
}

/// FNV-1a: a stable id for a video source from what names it.
pub fn source_id(key: &str) -> u64 {
    kairo_engine::source_id(key)
}

fn fit_of(fit: Option<&MediaFit>) -> (Fit, FitMode) {
    match fit {
        Some(MediaFit::Contain) => (Fit::Contain, FitMode::Contain),
        Some(MediaFit::Fill) => (Fit::Fill, FitMode::Fill),
        _ => (Fit::Cover, FitMode::Cover),
    }
}

fn filter_of(hue: &Value, saturation: &Value, brightness: &Value, contrast: &Value) -> ColorFilter {
    let playback = normalize_media_playback(&json!({
        "hue": hue, "saturation": saturation, "brightness": brightness, "contrast": contrast,
    }));
    ColorFilter {
        hue_deg: playback.hue as f32,
        saturation: playback.saturation as f32,
        brightness: playback.brightness as f32,
        contrast: playback.contrast as f32,
    }
}

fn css_opacity(value: Option<&Value>) -> f32 {
    value
        .and_then(Value::as_f64)
        .map_or(1.0, element_opacity_f64)
}

fn element_opacity_f64(o: f64) -> f32 {
    element_opacity(o)
}

/// What the shell does for a slide's background.
pub struct Background {
    pub key: String,
    pub content: Option<Content>,
    /// A picture to decode, whose result replaces the node's pixels.
    pub image: Option<ImageJob>,
}

/// `backgroundLayerHTML` and `backgroundKey`.
pub fn background(bg: &Value, frame: (u32, u32)) -> Background {
    let kind = bg.get("type").and_then(Value::as_str).unwrap_or("");
    let media = bg
        .get("mediaPath")
        .and_then(Value::as_str)
        .filter(|p| !p.is_empty());
    let opacity = css_opacity(bg.get("opacity"));
    if let ("image" | "video", Some(path)) = (kind, media) {
        let video = kind == "video";
        let cue = if video {
            bg.get("mediaCue")
                .and_then(Value::as_f64)
                .filter(|c| c.fract() == 0.0 && c.abs() <= 9_007_199_254_740_991.0)
                .map(|c| format!("{c}"))
                .unwrap_or_default()
        } else {
            String::new()
        };
        let motion = (!video)
            .then(|| normalize_media_motion(bg.get("motion")))
            .flatten();
        let mut key = format!("{}:{path}:{cue}", if video { "VIDEO" } else { "IMG" });
        if let Some(m) = &motion {
            // A new move is a new picture: it starts from the beginning.
            key.push_str(&serde_json::to_string(m).unwrap_or_default());
        }
        let playback = normalize_media_playback(&json!({
            "loop": bg.get("mediaLoop"), "startSeconds": bg.get("mediaStartSeconds"),
            "endSeconds": bg.get("mediaEndSeconds"), "rate": bg.get("mediaRate"),
            "hue": bg.get("hue"), "saturation": bg.get("saturation"),
            "brightness": bg.get("brightness"), "contrast": bg.get("contrast"),
        }));
        let blur = bg
            .get("blurPx")
            .and_then(Value::as_f64)
            .filter(|b| b.is_finite() && *b > 0.0)
            .unwrap_or(0.0);
        let media_fit: Option<MediaFit> = bg
            .get("mediaFit")
            .and_then(|v| serde_json::from_value(v.clone()).ok());
        let (fit, fit_mode) = fit_of(media_fit.as_ref());
        let look = Look {
            fit,
            filter: ColorFilter {
                hue_deg: playback.hue as f32,
                saturation: playback.saturation as f32,
                brightness: playback.brightness as f32,
                contrast: playback.contrast as f32,
            },
            blur_px: blur as f32,
            blur_first: true,
            // A blur fades the frame's edges in; scaling past them keeps them solid.
            scale: if blur > 0.0 {
                1.0 + (blur as f32 * 4.0) / 1080.0
            } else {
                1.0
            },
            ..Look::default()
        };
        let mut content = Content {
            opacity,
            look,
            ..Content::new(Fill::Pending)
        };
        if video {
            // Frames come from the playback layer (prompt 055).
            content.fill = Fill::Source {
                id: source_id(&key),
                backdrop: [0.0; 4],
            };
            content.media = Some(MediaParams {
                path: path.into(),
                looped: playback.looped,
                start_seconds: playback.start_seconds,
                end_seconds: playback.end_seconds.flatten(),
                rate: playback.rate.unwrap_or(1.0),
                element: false,
            });
            return Background {
                key,
                content: Some(content),
                image: None,
            };
        }
        let image = ImageJob {
            path: path.to_owned(),
            target: BoxFit {
                width: frame.0,
                height: frame.1,
                fit: fit_mode,
            },
            template: content.clone(),
            motion,
        };
        return Background {
            key,
            content: Some(content),
            image: Some(image),
        };
    }
    // Everything else is markup the key is made of.
    let transparent = || Background {
        key: "HTML:transparent".into(),
        content: Some(Content::empty()),
        image: None,
    };
    let color = bg.get("color").and_then(Value::as_str).unwrap_or("");
    let key = format!("HTML:{bg}");
    match kind {
        "color" => Background {
            key,
            content: Some(match parse_css(color) {
                Some(rgba) => Content {
                    opacity,
                    ..Content::new(Fill::Solid(premultiplied(rgba)))
                },
                // A colour CSS cannot read drops the declaration.
                None => Content::empty(),
            }),
            image: None,
        },
        "gradient" => {
            let angle = bg.get("angleDeg").and_then(Value::as_f64).unwrap_or(0.0);
            let second = bg
                .get("color2")
                .and_then(Value::as_str)
                .filter(|c| !c.is_empty())
                .unwrap_or(color);
            let content = match (parse_css(color), parse_css(second)) {
                (Some(a), Some(b)) if angle.is_finite() => Content {
                    opacity,
                    ..Content::new(Fill::Gradient {
                        from: premultiplied(a),
                        to: premultiplied(b),
                        angle_deg: angle as f32,
                    })
                },
                _ => Content::empty(),
            };
            Background {
                key,
                content: Some(content),
                image: None,
            }
        }
        // 'transparent', and image or video with no file picked yet.
        _ => transparent(),
    }
}

/// A picture to load for a background.
#[derive(Clone)]
pub struct ImageJob {
    pub path: String,
    pub target: BoxFit,
    /// The node's content with the pixels left out.
    pub template: Content,
    /// Set when the still moves (`crate::ambience`).
    pub motion: Option<kairo_domain::media::MediaMotion>,
}

/// One layer of theme elements (below the text, or above it).
#[derive(Clone)]
pub struct ElementsPlan {
    pub key: String,
    pub elements: Vec<OverlayElement>,
    pub frame: (u32, u32),
}

impl ElementsPlan {
    pub fn clone_plan(&self) -> Self {
        self.clone()
    }
}

/// `elementsLayerHTML`: the elements of one group that draw anything. `None`
/// when there are none, which retires the layer.
pub fn elements(theme: &Value, above_text: bool, frame: (u32, u32)) -> Option<ElementsPlan> {
    let list = normalize_overlay_elements(theme.get("elements"));
    let drawn: Vec<OverlayElement> = list
        .into_iter()
        .take(MAX_OVERLAY_ELEMENTS)
        .filter(|el| el.above_text == above_text)
        .filter(|el| match el.kind {
            OverlayElementKind::Image | OverlayElementKind::Video => {
                el.media_path.as_deref().is_some_and(|p| !p.is_empty())
            }
            OverlayElementKind::Text => el.text.as_deref().is_some_and(|t| !t.trim().is_empty()),
            OverlayElementKind::Path => el.path.as_deref().is_some_and(|p| !p.is_empty()),
            _ => true,
        })
        .filter(|el| {
            el.kind != OverlayElementKind::Shape
                || kairo_domain::overlay_shapes::overlay_shape(el.shape.as_deref()).is_some()
        })
        .collect();
    if drawn.is_empty() {
        return None;
    }
    // Identical markup keeps the node: the key is what the markup is made of.
    let key = format!(
        "{}:{}:{}",
        if above_text { "above" } else { "below" },
        frame.1,
        serde_json::to_string(&drawn).unwrap_or_default()
    );
    Some(ElementsPlan {
        key,
        elements: drawn,
        frame,
    })
}

/// A decoded picture as the compositor holds it.
#[derive(Clone)]
pub(crate) enum Decoded {
    Still(Arc<Raster>),
    Animated(Arc<Frames>),
}

impl Decoded {
    fn from_picture(picture: &Picture) -> Option<Self> {
        let rasters: Vec<Arc<Raster>> = picture
            .frames
            .iter()
            .map(|f| Raster::new(picture.width, picture.height, f.pixels.clone()))
            .collect();
        match rasters.len() {
            0 => None,
            1 => Some(Decoded::Still(rasters.into_iter().next()?)),
            _ => Some(Decoded::Animated(Arc::new(Frames {
                delays_ms: picture.frames.iter().map(|f| f.delay_ms.max(1)).collect(),
                plays: picture.plays,
                rasters,
            }))),
        }
    }

    pub(crate) fn fill(&self) -> Fill {
        match self {
            Decoded::Still(r) => Fill::Image(r.clone()),
            Decoded::Animated(f) => Fill::Frames(f.clone()),
        }
    }

    fn bytes(&self) -> usize {
        match self {
            Decoded::Still(r) => r.pixels.len(),
            Decoded::Animated(f) => f.rasters.iter().map(|r| r.pixels.len()).sum(),
        }
    }
}

/// Picture cache key: the file as it is now, and the box it was reduced for.
#[derive(Clone, PartialEq, Eq, Hash)]
struct FileKey {
    path: String,
    modified: u128,
    len: u64,
    target: BoxFit,
}

struct Lru<K, V> {
    entries: VecDeque<(K, V, usize)>,
    budget: usize,
}

impl<K: PartialEq + Clone, V: Clone> Lru<K, V> {
    fn new(budget: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            budget,
        }
    }

    fn get(&mut self, key: &K) -> Option<V> {
        let at = self.entries.iter().position(|(k, ..)| k == key)?;
        let entry = self.entries.remove(at)?;
        let value = entry.1.clone();
        self.entries.push_back(entry);
        Some(value)
    }

    fn put(&mut self, key: K, value: V, bytes: usize) {
        self.entries.retain(|(k, ..)| *k != key);
        self.entries.push_back((key, value, bytes));
        while self.entries.len() > 1
            && self.entries.iter().map(|e| e.2).sum::<usize>() > self.budget
        {
            self.entries.pop_front();
        }
    }
}

/// A vector element's pixels and where they sit.
type Tile = (Arc<Raster>, i32, i32);

pub(crate) struct Shared {
    pictures: Mutex<Lru<FileKey, Option<Decoded>>>,
    tiles: Mutex<Lru<u64, Option<Tile>>>,
}

impl Shared {
    /// The picture at `path` for `target`, read and decoded once.
    pub(crate) fn picture(&self, path: &str, target: BoxFit) -> Option<Decoded> {
        let meta = match std::fs::metadata(path) {
            Ok(meta) if meta.is_file() => meta,
            Ok(_) => {
                tracing::warn!(path, "background or element picture is not a file");
                return None;
            }
            Err(error) => {
                tracing::warn!(path, %error, "picture cannot be read");
                return None;
            }
        };
        if meta.len() > MAX_FILE {
            tracing::warn!(path, bytes = meta.len(), "picture file is too large");
            return None;
        }
        let key = FileKey {
            path: path.to_owned(),
            modified: meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos()),
            len: meta.len(),
            target,
        };
        if let Some(hit) = self
            .pictures
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&key)
        {
            return hit;
        }
        let decoded = std::fs::read(path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| decode_picture(&bytes, target).map_err(|e| e.to_string()))
            .map_err(|error| tracing::warn!(path, %error, "picture cannot be shown"))
            .ok()
            .and_then(|picture| Decoded::from_picture(&picture));
        let bytes = decoded.as_ref().map_or(0, Decoded::bytes);
        self.pictures
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .put(key, decoded.clone(), bytes);
        decoded
    }

    /// A vector element's tile, drawn once per look.
    fn tile(&self, el: &OverlayElement, frame: (u32, u32)) -> Option<Tile> {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        serde_json::to_string(el).unwrap_or_default().hash(&mut h);
        frame.hash(&mut h);
        let key = h.finish();
        if let Some(hit) = self
            .tiles
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&key)
        {
            return hit;
        }
        let tile =
            render_element(el, frame).map(|t| (Raster::new(t.width, t.height, t.pixels), t.x, t.y));
        let bytes = tile.as_ref().map_or(0, |(r, ..)| r.pixels.len());
        self.tiles
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .put(key, tile.clone(), bytes);
        tile
    }
}

/// One element as the layer holds it. Media is `Pending` until `decode`
/// supplies it.
fn element_content(
    el: &OverlayElement,
    shared: &Shared,
    frame: (u32, u32),
    decode: bool,
) -> Option<Content> {
    let opacity = element_opacity(el.opacity);
    match el.kind {
        OverlayElementKind::Image | OverlayElementKind::Video => {
            let path = el.media_path.as_deref()?;
            let (bx, by, bw, bh) = snapped_box(&el.frame, frame);
            if !(bw > 0.0 && bh > 0.0) {
                return None;
            }
            let (fit, fit_mode) = fit_of(Some(&el.media_fit));
            let look = Look {
                fit,
                rotation_deg: if el.rotation_deg.is_finite() {
                    el.rotation_deg as f32
                } else {
                    0.0
                },
                flip_x: el.flip_x,
                flip_y: el.flip_y,
                filter: filter_of(
                    &json!(el.hue),
                    &json!(el.saturation),
                    &json!(el.brightness),
                    &json!(el.contrast),
                ),
                blur_px: if el.blur_px.is_finite() && el.blur_px > 0.0 {
                    el.blur_px as f32
                } else {
                    0.0
                },
                blur_first: false,
                scale: 1.0,
            };
            let mut content = Content {
                rect: Some(Rect::new(bx as f32, by as f32, bw as f32, bh as f32)),
                opacity,
                look,
                ..Content::new(Fill::Pending)
            };
            if el.kind == OverlayElementKind::Video {
                // Always muted, and deaf to the transport: decoration.
                content.fill = Fill::Source {
                    id: source_id(&format!("element:{}:{path}", el.id)),
                    backdrop: [0.0; 4],
                };
                content.media = Some(MediaParams {
                    path: path.into(),
                    looped: el.media_loop,
                    start_seconds: None,
                    end_seconds: None,
                    rate: 1.0,
                    element: true,
                });
            } else if decode {
                let target = BoxFit {
                    width: bw as u32,
                    height: bh as u32,
                    fit: fit_mode,
                };
                content.fill = shared.picture(path, target)?.fill();
            }
            Some(content)
        }
        _ => {
            let (raster, x, y) = shared.tile(el, frame)?;
            Some(Content {
                rect: Some(Rect::new(
                    x as f32,
                    y as f32,
                    raster.width as f32,
                    raster.height as f32,
                )),
                opacity,
                ..Content::new(Fill::Image(raster))
            })
        }
    }
}

fn group(children: Vec<Content>) -> Content {
    Content::new(Fill::Group(Arc::new(children)))
}

/// What a worker gives back.
pub enum Output {
    /// The layer without its pictures (vector elements drawn, pictures
    /// pending). Final when the layer has no pictures.
    Fast(Content),
    /// The layer with its pictures.
    Full(Content),
    /// The decoded background; `None` when it could not be shown.
    Image(Option<Content>),
    /// A program layer's pieces, without (`full: false`) or with its pictures.
    Layer(crate::layers::LayerOut),
    /// A redrawn readout.
    Live(Box<crate::layers::LiveOut>),
}

pub enum Job {
    Elements(ElementsPlan),
    Image(ImageJob),
    /// A program layer, the info layer or a test pattern.
    Layer(crate::layers::LayerJob),
    /// One countdown or clock readout, redrawn on a tick.
    Live(crate::layers::LiveJob),
}

pub(crate) type Callback = Box<dyn FnOnce(Output) + Send>;

struct Task {
    job: Job,
    done: Callback,
    /// Called for the fast stage of an elements layer when it also has a full one.
    fast: Option<Callback>,
}

pub struct VisualWork {
    tx: Sender<Task>,
}

impl VisualWork {
    pub fn new() -> Self {
        let (tx, rx) = channel::<Task>();
        let rx: Arc<Mutex<Receiver<Task>>> = Arc::new(Mutex::new(rx));
        let shared = Arc::new(Shared {
            pictures: Mutex::new(Lru::new(256 << 20)),
            tiles: Mutex::new(Lru::new(96 << 20)),
        });
        for i in 0..2 {
            let (rx, shared) = (rx.clone(), shared.clone());
            std::thread::Builder::new()
                .name(format!("visuals-{i}"))
                .spawn(move || {
                    loop {
                        let task = {
                            let guard = rx.lock().unwrap_or_else(|e| e.into_inner());
                            match guard.recv() {
                                Ok(task) => task,
                                Err(_) => return,
                            }
                        };
                        // A panicking decoder must not take the worker with it.
                        let shared = shared.clone();
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            run(&shared, task)
                        }));
                        if result.is_err() {
                            tracing::error!("a picture or shape worker panicked; skipping it");
                        }
                    }
                })
                .expect("spawn visuals worker");
        }
        Self { tx }
    }

    /// Queues `job`. For an elements layer with pictures, `fast` gets the
    /// layer without them at once and `done` gets the full layer; for the
    /// rest `done` gets the only result.
    pub fn submit(&self, job: Job, fast: Option<Callback>, done: Callback) {
        let _ = self.tx.send(Task { job, done, fast });
    }
}

impl Default for VisualWork {
    fn default() -> Self {
        Self::new()
    }
}

fn run(shared: &Shared, task: Task) {
    match task.job {
        Job::Layer(job) => crate::layers::run_layer(shared, job, task.fast, task.done),
        Job::Live(job) => (task.done)(Output::Live(Box::new(crate::layers::run_live(job)))),
        Job::Image(job) => {
            let content = shared.picture(&job.path, job.target).map(|decoded| {
                let base = Content {
                    fill: decoded.fill(),
                    ..job.template
                };
                match &job.motion {
                    Some(motion) => crate::ambience::compose(
                        base,
                        motion,
                        (job.target.width, job.target.height),
                        |path| shared.picture(path, job.target).map(|d| d.fill()),
                    ),
                    None => base,
                }
            });
            (task.done)(Output::Image(content));
        }
        Job::Elements(plan) => {
            let has_pictures = plan
                .elements
                .iter()
                .any(|e| e.kind == OverlayElementKind::Image);
            let fast: Vec<Content> = plan
                .elements
                .iter()
                .filter_map(|e| element_content(e, shared, plan.frame, false))
                .collect();
            if !has_pictures {
                (task.done)(Output::Fast(group(fast)));
                return;
            }
            match task.fast {
                Some(fast_done) => {
                    fast_done(Output::Fast(group(fast)));
                    let full: Vec<Content> = plan
                        .elements
                        .iter()
                        .filter_map(|e| element_content(e, shared, plan.frame, true))
                        .collect();
                    (task.done)(Output::Full(group(full)));
                }
                None => {
                    let full: Vec<Content> = plan
                        .elements
                        .iter()
                        .filter_map(|e| element_content(e, shared, plan.frame, true))
                        .collect();
                    (task.done)(Output::Full(group(full)));
                }
            }
        }
    }
}

/// The compositor slot an elements layer fills.
pub fn element_slot(above_text: bool) -> Slot {
    if above_text {
        Slot::ElementsAbove
    } else {
        Slot::ElementsBelow
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn theme_with(elements: Value) -> Value {
        json!({ "elements": elements })
    }

    fn rect_json(id: &str, above: bool) -> Value {
        serde_json::to_value(kairo_domain::overlay_elements::create_overlay_element(
            OverlayElementKind::Rectangle,
            None,
            id,
            None,
        ))
        .map(|mut v| {
            v["aboveText"] = json!(above);
            v
        })
        .unwrap()
    }

    #[test]
    fn colour_gradient_and_transparent_backgrounds_are_decided_at_once() {
        let f = (1920, 1080);
        let c = background(&json!({"type":"color","color":"#102030","opacity":0.5}), f);
        let content = c.content.unwrap();
        assert!(matches!(content.fill, Fill::Solid(_)) && content.opacity == 0.5);
        let g = background(
            &json!({"type":"gradient","color":"#ff0000","color2":"#0000ff","angleDeg":90,"opacity":1}),
            f,
        );
        assert!(matches!(
            g.content.unwrap().fill,
            Fill::Gradient { angle_deg, .. } if angle_deg == 90.0
        ));
        // A missing second colour repeats the first; a bad one drops the declaration.
        let same = background(&json!({"type":"gradient","color":"#ff0000","opacity":1}), f);
        assert!(matches!(same.content.unwrap().fill, Fill::Gradient { .. }));
        let bad = background(&json!({"type":"gradient","color":"nope","opacity":1}), f);
        assert!(matches!(bad.content.unwrap().fill, Fill::Empty));
        let t = background(&json!({"type":"transparent","color":"#000","opacity":1}), f);
        assert_eq!(t.key, "HTML:transparent");
        // An image with no file picked yet is transparent too.
        let none = background(&json!({"type":"image","color":"#000","opacity":1}), f);
        assert_eq!(none.key, "HTML:transparent");
    }

    #[test]
    fn a_picture_background_is_keyed_on_file_and_carries_its_look() {
        let bg = json!({"type":"image","mediaPath":"/m/a.png","mediaFit":"contain","opacity":0.8,
            "hue":30,"saturation":1.5,"blurPx":10,"color":"#000"});
        let plan = background(&bg, (1920, 1080));
        assert_eq!(plan.key, "IMG:/m/a.png:");
        let c = plan.content.unwrap();
        assert!(matches!(c.fill, Fill::Pending));
        assert_eq!(c.look.fit, Fit::Contain);
        assert_eq!(c.look.filter.hue_deg, 30.0);
        assert!(c.look.blur_first);
        assert!((c.look.scale - (1.0 + 40.0 / 1080.0)).abs() < 1e-6);
        assert!(plan.image.is_some());
        // The cue is only part of a video's identity.
        let video = background(
            &json!({"type":"video","mediaPath":"/m/a.mp4","mediaCue":7,"mediaLoop":true,"opacity":1,"color":"#000"}),
            (1920, 1080),
        );
        assert_eq!(video.key, "VIDEO:/m/a.mp4:7");
        let c = video.content.unwrap();
        let media = c.media.as_ref().unwrap();
        assert!(media.looped && !media.element);
        assert_eq!(&*media.path, "/m/a.mp4");
        assert!(video.image.is_none());
        // A cue that is not a safe integer is dropped.
        let odd = background(
            &json!({"type":"video","mediaPath":"/m/a.mp4","mediaCue":1.5,"opacity":1,"color":"#000"}),
            (1920, 1080),
        );
        assert_eq!(odd.key, "VIDEO:/m/a.mp4:");
    }

    #[test]
    fn element_layers_split_by_group_and_skip_what_draws_nothing() {
        let theme = theme_with(json!([
            rect_json("a", false),
            rect_json("b", true),
            { "id": "m", "kind": "image", "aboveText": false },
        ]));
        let below = elements(&theme, false, (1920, 1080)).unwrap();
        assert_eq!(
            below.elements.len(),
            1,
            "an image with no file draws nothing"
        );
        let above = elements(&theme, true, (1920, 1080)).unwrap();
        assert_eq!(above.elements.len(), 1);
        assert_ne!(below.key, above.key);
        assert!(elements(&json!({}), false, (1920, 1080)).is_none());
        // The same elements are the same node.
        assert_eq!(
            elements(&theme, false, (1920, 1080)).unwrap().key,
            below.key
        );
    }

    #[test]
    fn at_most_forty_elements_are_drawn() {
        let many: Vec<Value> = (0..60)
            .map(|i| rect_json(&format!("r{i}"), false))
            .collect();
        let plan = elements(&theme_with(json!(many)), false, (1920, 1080)).unwrap();
        assert_eq!(plan.elements.len(), MAX_OVERLAY_ELEMENTS);
    }

    fn run_job(work: &VisualWork, job: Job) -> Vec<Output> {
        let (tx, rx) = channel();
        let fast_tx = tx.clone();
        work.submit(
            job,
            Some(Box::new(move |o| {
                let _ = fast_tx.send(o);
            })),
            Box::new(move |o| {
                let _ = tx.send(o);
            }),
        );
        let mut out = Vec::new();
        while let Ok(o) = rx.recv_timeout(std::time::Duration::from_secs(20)) {
            let last = !matches!(o, Output::Fast(_)) || out.len() == 1;
            out.push(o);
            if last {
                break;
            }
        }
        out
    }

    #[test]
    fn a_vector_layer_is_ready_in_one_stage_and_a_picture_layer_in_two() {
        let work = VisualWork::new();
        let plan = elements(
            &theme_with(json!([rect_json("a", false)])),
            false,
            (1920, 1080),
        )
        .unwrap();
        let out = run_job(&work, Job::Elements(plan));
        assert_eq!(out.len(), 1);
        let Output::Fast(c) = &out[0] else {
            panic!("fast")
        };
        let Fill::Group(children) = &c.fill else {
            panic!("group")
        };
        assert_eq!(children.len(), 1);
        assert!(matches!(children[0].fill, Fill::Image(_)));

        let dir = std::env::temp_dir().join(format!("kairo-visuals-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("p.png");
        image_png(&file);
        let mut el = serde_json::to_value(kairo_domain::overlay_elements::create_overlay_element(
            OverlayElementKind::Image,
            Some(file.to_str().unwrap()),
            "i",
            None,
        ))
        .unwrap();
        el["aboveText"] = json!(false);
        let plan = elements(
            &theme_with(json!([el, rect_json("a", false)])),
            false,
            (1920, 1080),
        )
        .unwrap();
        let out = run_job(&work, Job::Elements(plan));
        assert_eq!(out.len(), 2);
        let Output::Fast(c) = &out[0] else {
            panic!("fast first")
        };
        let Fill::Group(children) = &c.fill else {
            panic!()
        };
        assert!(matches!(children[0].fill, Fill::Pending));
        let Output::Full(c) = &out[1] else {
            panic!("full second")
        };
        let Fill::Group(children) = &c.fill else {
            panic!()
        };
        assert!(matches!(children[0].fill, Fill::Image(_)));
        assert_eq!(children.len(), 2);
        std::fs::remove_dir_all(&dir).ok();
    }

    fn image_png(path: &std::path::Path) {
        let mut bytes = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut bytes, 4, 4);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().unwrap();
            w.write_image_data(&[200u8; 64]).unwrap();
        }
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn missing_unreadable_and_non_picture_files_give_nothing_and_do_not_stop_the_worker() {
        let work = VisualWork::new();
        for path in ["/definitely/not/here.png", "/", "/etc/hosts"] {
            let job = ImageJob {
                path: path.into(),
                target: BoxFit {
                    width: 100,
                    height: 100,
                    fit: FitMode::Cover,
                },
                template: Content::empty(),
                motion: None,
            };
            let (tx, rx) = channel();
            work.submit(
                Job::Image(job),
                None,
                Box::new(move |o| {
                    let _ = tx.send(o);
                }),
            );
            let Output::Image(content) =
                rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap()
            else {
                panic!("image output")
            };
            assert!(content.is_none(), "{path}");
        }
    }
}
