//! The program layer stack of `overlay.html`, without the DOM.
//!
//! Each slot holds nodes in insertion order. A swap appends the new node and
//! retires the old one; both fade linearly over the same time, the new one
//! drawn above. A retiring node keeps fading from wherever its opacity was,
//! is removed 50 ms after its fade ends, and is never "the" node of its slot
//! again (`data-leaving`).

use crate::geometry::Rect;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Program layers, bottom to top, as `#pa-program` stacks them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Slot {
    /// `#pa-bg-layer`, alone in `#pa-content-bg`.
    Background,
    /// `#pa-camera-layer`: between background and words, or above all in pip.
    Camera,
    // `#pa-content-fore`, in DOM order:
    Web,
    WebPointer,
    ElementsBelow,
    Text,
    ElementsAbove,
    Props,
    Timer,
    Message,
    Poll,
    Question,
    Info,
    Logo,
    /// `#kairo-diagnostic-pattern`: on the body, over everything, never
    /// moved by a camera layout.
    Pattern,
}

impl Slot {
    pub const ALL: [Slot; 15] = [
        Slot::Background,
        Slot::Camera,
        Slot::Web,
        Slot::WebPointer,
        Slot::ElementsBelow,
        Slot::Text,
        Slot::ElementsAbove,
        Slot::Props,
        Slot::Timer,
        Slot::Message,
        Slot::Poll,
        Slot::Question,
        Slot::Info,
        Slot::Logo,
        Slot::Pattern,
    ];

    /// The slots inside `#pa-content-fore`, bottom to top.
    pub const FORE: [Slot; 12] = [
        Slot::Web,
        Slot::WebPointer,
        Slot::ElementsBelow,
        Slot::Text,
        Slot::ElementsAbove,
        Slot::Props,
        Slot::Timer,
        Slot::Message,
        Slot::Poll,
        Slot::Question,
        Slot::Info,
        Slot::Logo,
    ];

    fn index(self) -> usize {
        Self::ALL.iter().position(|s| *s == self).expect("listed")
    }
}

/// CPU pixels, RGBA8 with premultiplied alpha, uploaded once per raster.
#[derive(Debug)]
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    id: u64,
}

impl Raster {
    /// `pixels` must be `width * height * 4` premultiplied RGBA bytes.
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Arc<Self> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        assert_eq!(
            pixels.len(),
            width as usize * height as usize * 4,
            "raster size"
        );
        Arc::new(Self {
            width,
            height,
            pixels,
            id: NEXT.fetch_add(1, Ordering::Relaxed),
        })
    }

    pub fn id(&self) -> u64 {
        self.id
    }
}

/// The frames of an animated image, as a browser plays them: each frame
/// holds for its delay, the whole loop repeats as the file says, and the
/// last frame stays once the plays are done.
#[derive(Debug)]
pub struct Frames {
    pub rasters: Vec<Arc<Raster>>,
    /// Milliseconds each frame shows for; never zero.
    pub delays_ms: Vec<u32>,
    /// `None`: forever.
    pub plays: Option<u32>,
}

impl Frames {
    fn loop_ms(&self) -> u64 {
        self.delays_ms
            .iter()
            .map(|d| u64::from(*d))
            .sum::<u64>()
            .max(1)
    }

    /// The frame showing `elapsed` after the image started.
    pub fn index_at(&self, elapsed: Duration) -> usize {
        let total = self.loop_ms();
        let ms = elapsed.as_millis().min(u128::from(u64::MAX)) as u64;
        let ms = match self.plays {
            None => ms % total,
            Some(plays) => {
                let span = total.saturating_mul(u64::from(plays.max(1)));
                if ms >= span {
                    return self.rasters.len().saturating_sub(1);
                }
                ms % total
            }
        };
        let mut at = 0u64;
        for (i, d) in self.delays_ms.iter().enumerate() {
            at += u64::from(*d);
            if ms < at {
                return i;
            }
        }
        self.rasters.len().saturating_sub(1)
    }

    /// How long after the image started the frame after the one showing at
    /// `elapsed` begins, or `None` when the animation is over.
    pub fn next_after(&self, elapsed: Duration) -> Option<Duration> {
        if self.rasters.len() < 2 {
            return None;
        }
        let total = self.loop_ms();
        let ms = elapsed.as_millis().min(u128::from(u64::MAX)) as u64;
        if let Some(plays) = self.plays
            && ms >= total.saturating_mul(u64::from(plays.max(1)))
        {
            return None;
        }
        let in_loop = ms % total;
        let base = ms - in_loop;
        let mut at = 0u64;
        for d in &self.delays_ms {
            at += u64::from(*d);
            if in_loop < at {
                return Some(Duration::from_millis(base + at));
            }
        }
        None
    }
}

/// `object-fit` of an image or video inside its box.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Fit {
    /// Stretched to the box (`fill`, and every raster that already is the
    /// box's size).
    #[default]
    Fill,
    Contain,
    Cover,
}

/// The four CSS colour filters of `mediaFilterCss`, applied in that order:
/// `hue-rotate`, `saturate`, `brightness`, `contrast`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorFilter {
    pub hue_deg: f32,
    pub saturation: f32,
    pub brightness: f32,
    pub contrast: f32,
}

impl ColorFilter {
    pub const IDENTITY: Self = Self {
        hue_deg: 0.0,
        saturation: 1.0,
        brightness: 1.0,
        contrast: 1.0,
    };

    pub fn is_identity(&self) -> bool {
        *self == Self::IDENTITY
    }
}

impl Default for ColorFilter {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// How a node's pixels are placed and filtered on the way to the frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub fit: Fit,
    /// Clockwise about the box centre, then mirrored axes (flips come first).
    pub rotation_deg: f32,
    pub flip_x: bool,
    pub flip_y: bool,
    pub filter: ColorFilter,
    /// CSS `blur()` radius, which is the Gaussian's standard deviation.
    pub blur_px: f32,
    /// `true`: blur, then the colour filter (a background, whose wrapper
    /// carries the colour filter over a blurred picture). `false`: colour
    /// filter, then blur (an element's `filter: hue-rotate() blur()`).
    pub blur_first: bool,
    /// Uniform scale about the box centre, clipped to the box; the
    /// background's `1 + blur * 4 / 1080` edge cover.
    pub scale: f32,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            fit: Fit::Fill,
            rotation_deg: 0.0,
            flip_x: false,
            flip_y: false,
            filter: ColorFilter::IDENTITY,
            blur_px: 0.0,
            blur_first: false,
            scale: 1.0,
        }
    }
}

impl Look {
    pub fn is_plain(&self) -> bool {
        *self == Self::default()
    }
}

/// What a video node asks of the playback layer (prompt 055). The compositor
/// only carries it: a keyed node that is kept takes the new values, so a
/// background that is the same file and cue keeps playing and just changes
/// loop and the rest.
#[derive(Clone, Debug, PartialEq)]
pub struct MediaParams {
    /// The file the frames come from.
    pub path: std::sync::Arc<str>,
    pub looped: bool,
    pub start_seconds: Option<f64>,
    pub end_seconds: Option<f64>,
    pub rate: f64,
    /// Element videos are decoration: muted, and deaf to the transport.
    pub element: bool,
}

/// A video node as the playback layer sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct MediaUse {
    /// The compositor node: a new node is a new `<video>` element.
    pub node: u64,
    /// The source the node draws; outputs showing the same file at the same
    /// cue draw the same one.
    pub id: u64,
    pub params: MediaParams,
    /// The output's current background video, which pause and seek address.
    pub transport: bool,
}

/// What fills a node's box.
#[derive(Clone, Debug)]
pub enum Fill {
    /// Pixels on their way (a picture still being decoded). Draws nothing;
    /// a keyed node that is kept while its new content is `Pending` keeps the
    /// pixels it has and takes everything else.
    Pending,
    /// A linear gradient across the box at a CSS angle, colours
    /// premultiplied, interpolated premultiplied as CSS does.
    Gradient {
        from: [f32; 4],
        to: [f32; 4],
        angle_deg: f32,
    },
    /// An animated image.
    Frames(Arc<Frames>),
    /// Several contents drawn in order, in frame coordinates, faded as one
    /// layer (`.pa-elements-below`).
    Group(Arc<Vec<Content>>),
    /// The node exists (it can be retired, it blocks nothing) but has no
    /// pixels of its own yet.
    Empty,
    /// Premultiplied RGBA, 0 to 1.
    Solid([f32; 4]),
    Image(Arc<Raster>),
    /// A live texture (video, camera) whose frames arrive separately,
    /// drawn over `backdrop`.
    Source {
        id: u64,
        backdrop: [f32; 4],
    },
}

/// CSS `box-shadow` outside a node.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub offset: (f32, f32),
    pub blur: f32,
    /// Premultiplied.
    pub color: [f32; 4],
}

/// The camera's pip shadow: `0 16px 48px rgba(0, 0, 0, 0.5)`.
pub const PIP_SHADOW: Shadow = Shadow {
    offset: (0.0, 16.0),
    blur: 48.0,
    color: [0.0, 0.0, 0.0, 0.5],
};

/// A CSS `linear infinite` translate along x, timed by the node's age:
/// `@keyframes pa-ticker` (from 0 to `distance`) and the diagnostic sweep
/// (`alternate`: there and back).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motion {
    /// Frame pixels moved over one period; negative moves left.
    pub distance: f32,
    pub period_ms: u32,
    pub alternate: bool,
}

impl Motion {
    /// The x offset `age` into the animation.
    pub fn offset(&self, age: Duration) -> f32 {
        if self.period_ms == 0 {
            return 0.0;
        }
        let period = f64::from(self.period_ms);
        let ms = age.as_secs_f64() * 1000.0;
        let cycles = ms / period;
        let mut t = cycles.fract();
        if self.alternate && (cycles.floor() as u64) % 2 == 1 {
            t = 1.0 - t;
        }
        self.distance * t as f32
    }
}

#[derive(Clone, Debug)]
pub struct Content {
    pub fill: Fill,
    /// In frame pixels, before any group transform. `None`: the whole frame.
    pub rect: Option<Rect>,
    /// `overflow: hidden` on a box of its own, in frame pixels.
    pub clip: Option<Rect>,
    /// Moves the content along x for as long as it is shown.
    pub motion: Option<Motion>,
    pub radius: f32,
    pub shadow: Option<Shadow>,
    /// The node's own CSS opacity, times the fade.
    pub opacity: f32,
    pub look: Look,
    /// Set for video nodes.
    pub media: Option<MediaParams>,
}

impl Content {
    pub fn new(fill: Fill) -> Self {
        Self {
            fill,
            rect: None,
            clip: None,
            motion: None,
            radius: 0.0,
            shadow: None,
            opacity: 1.0,
            look: Look::default(),
            media: None,
        }
    }

    pub fn empty() -> Self {
        Self::new(Fill::Empty)
    }
}

#[derive(Clone, Debug)]
struct Leaving {
    at: Instant,
    ms: u32,
    from: f32,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub id: u64,
    pub content: Content,
    /// What makes two nodes "the same" for keyed slots (background file,
    /// element markup); `None` for slots that always swap.
    pub key: Option<String>,
    entered: Instant,
    enter_ms: u32,
    leaving: Option<Leaving>,
    /// The animated frames last drawn, to tell when the picture changed.
    frame_sig: u64,
    /// When its pictures started playing: when the node was made, or when
    /// its pixels arrived (an image animates once it is decoded).
    anim_start: Instant,
}

fn progress(since: Instant, ms: u32, now: Instant) -> f32 {
    if ms == 0 {
        return 1.0;
    }
    let elapsed = now.saturating_duration_since(since).as_secs_f64() * 1000.0;
    (elapsed / f64::from(ms)).clamp(0.0, 1.0) as f32
}

impl Node {
    pub fn is_leaving(&self) -> bool {
        self.leaving.is_some()
    }

    /// Time since its pictures started playing.
    pub fn age(&self, now: Instant) -> Duration {
        now.saturating_duration_since(self.anim_start)
    }

    /// The animated images this node draws, directly or in a group.
    fn frames(&self) -> Vec<&Arc<Frames>> {
        fn walk<'a>(content: &'a Content, out: &mut Vec<&'a Arc<Frames>>) {
            match &content.fill {
                Fill::Frames(f) => out.push(f),
                Fill::Group(children) => children.iter().for_each(|c| walk(c, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        walk(&self.content, &mut out);
        out
    }

    /// True when something in it moves continuously (`Motion`).
    fn moving(&self) -> bool {
        fn walk(content: &Content) -> bool {
            content.motion.is_some()
                || matches!(&content.fill, Fill::Group(children) if children.iter().any(walk))
        }
        walk(&self.content)
    }

    /// The soonest moment an animated frame of this node changes.
    fn next_frame(&self, now: Instant) -> Option<Instant> {
        let age = self.age(now);
        self.frames()
            .iter()
            .filter_map(|f| f.next_after(age))
            .min()
            .map(|d| self.anim_start + d)
    }

    /// The fade alone, 0 to 1.
    pub fn fade(&self, now: Instant) -> f32 {
        match &self.leaving {
            Some(leaving) => leaving.from * (1.0 - progress(leaving.at, leaving.ms, now)),
            None => progress(self.entered, self.enter_ms, now),
        }
    }

    /// When this node's fade ends, if it is still running.
    fn animating_until(&self, now: Instant) -> Option<Instant> {
        let (since, ms) = match &self.leaving {
            Some(leaving) => (leaving.at, leaving.ms),
            None => (self.entered, self.enter_ms),
        };
        let end = since + Duration::from_millis(u64::from(ms));
        (ms > 0 && end > now).then_some(end)
    }

    /// `setTimeout(remove, ms + 50)`.
    fn removed_at(&self) -> Option<Instant> {
        self.leaving.as_ref().map(|l| {
            if l.ms == 0 {
                l.at
            } else {
                l.at + Duration::from_millis(u64::from(l.ms) + 50)
            }
        })
    }
}

/// `transitionMs`, clamped as `__configure` clamps it. NaN and negatives cut.
pub fn clamp_transition(ms: f64) -> u32 {
    if ms.is_nan() {
        return 0;
    }
    ms.clamp(0.0, 5000.0) as u32
}

#[derive(Default, Debug)]
pub struct Stack {
    slots: [Vec<Node>; 15],
    next_id: u64,
}

impl Stack {
    pub fn nodes(&self, slot: Slot) -> &[Node] {
        &self.slots[slot.index()]
    }

    /// `currentNode`: the newest node that is not leaving.
    pub fn current(&self, slot: Slot) -> Option<&Node> {
        self.slots[slot.index()]
            .iter()
            .rev()
            .find(|n| !n.is_leaving())
    }

    pub fn current_mut(&mut self, slot: Slot) -> Option<&mut Node> {
        self.slots[slot.index()]
            .iter_mut()
            .rev()
            .find(|n| !n.is_leaving())
    }

    fn enter(&mut self, slot: Slot, content: Content, key: Option<String>, ms: u32, now: Instant) {
        self.next_id += 1;
        self.slots[slot.index()].push(Node {
            id: self.next_id,
            content,
            key,
            entered: now,
            enter_ms: ms,
            leaving: None,
            frame_sig: 0,
            anim_start: now,
        });
    }

    /// `retire` for every node of the slot that is not already leaving.
    /// Only the newest such node exists in normal use; `__setWeb` and
    /// `__setCamera` retire all of them, and so does this.
    fn retire_all(&mut self, slot: Slot, ms: u32, now: Instant, except: Option<u64>) {
        let nodes = &mut self.slots[slot.index()];
        for node in nodes.iter_mut() {
            if node.is_leaving() || Some(node.id) == except {
                continue;
            }
            let from = node.fade(now);
            node.leaving = Some(Leaving { at: now, ms, from });
        }
        if ms == 0 {
            nodes.retain(|n| {
                Some(n.id) == except || !n.is_leaving() || n.removed_at().is_some_and(|at| at > now)
            });
        }
    }

    /// `swapLayer`: `None` clears the slot.
    pub fn swap(&mut self, slot: Slot, content: Option<Content>, ms: u32, now: Instant) {
        match content {
            None => self.retire_all(slot, ms, now, None),
            Some(content) => {
                self.enter(slot, content, None, ms, now);
                let newest = self.slots[slot.index()].last().map(|n| n.id);
                self.retire_all(slot, ms, now, newest);
            }
        }
    }

    /// `setBackground` and `setElements`: a node with the same key stays and
    /// takes the new content at once (a video keeps playing); otherwise the
    /// new node crossfades in. Returns true when the node was kept.
    pub fn set_keyed(
        &mut self,
        slot: Slot,
        content: Option<(Content, String)>,
        ms: u32,
        now: Instant,
    ) -> bool {
        let Some((content, key)) = content else {
            self.retire_all(slot, ms, now, None);
            return false;
        };
        if let Some(current) = self.current_mut(slot)
            && current.key.as_deref() == Some(key.as_str())
        {
            current.content = if matches!(content.fill, Fill::Pending) {
                // Same file, new opacity, filter, fit or loop: the picture
                // (or the playing video) stays.
                Content {
                    fill: current.content.fill.clone(),
                    rect: content.rect.or(current.content.rect),
                    ..content
                }
            } else {
                content
            };
            return true;
        }
        self.enter(slot, content, Some(key), ms, now);
        let newest = self.slots[slot.index()].last().map(|n| n.id);
        self.retire_all(slot, ms, now, newest);
        false
    }

    /// The pixels of a keyed node arrive after the node was made. Applies
    /// only while that node is still the slot's current one; returns whether
    /// it did.
    pub fn update_keyed(&mut self, slot: Slot, key: &str, content: Content, now: Instant) -> bool {
        match self.current_mut(slot) {
            Some(current) if current.key.as_deref() == Some(key) => {
                current.content = content;
                current.anim_start = now;
                true
            }
            _ => false,
        }
    }

    /// Drops retired nodes whose removal time has passed.
    pub fn prune(&mut self, now: Instant) {
        for nodes in &mut self.slots {
            nodes.retain(|n| n.removed_at().is_none_or(|at| at > now));
        }
    }

    /// The next moment something changes on its own: a fade still running,
    /// or a retired node due for removal. `None` when the stack is still.
    pub fn next_change(&self, now: Instant) -> Option<Instant> {
        self.slots
            .iter()
            .flatten()
            .filter_map(|n| {
                let fade = n.animating_until(now);
                let removal = n.removed_at().filter(|at| *at > now);
                let frame = n.next_frame(now);
                let own = match (fade, removal) {
                    (Some(_), _) => Some(now),
                    _ if n.moving() && !n.is_leaving() => Some(now),
                    (None, removal) => removal,
                };
                [own, frame].into_iter().flatten().min()
            })
            .min()
    }

    /// True when an animated image has moved to another frame since the
    /// last call: the picture changed with nothing else happening.
    pub fn frames_changed(&mut self, now: Instant) -> bool {
        let mut changed = false;
        for node in self.slots.iter_mut().flatten() {
            let age = node.age(now);
            let sig = node.frames().iter().fold(0u64, |acc, f| {
                acc.wrapping_mul(1_000_003)
                    .wrapping_add(f.index_at(age) as u64 + 1)
            });
            if sig != node.frame_sig {
                node.frame_sig = sig;
                changed = true;
            }
        }
        changed
    }

    /// True while any fade runs: each frame differs from the last.
    pub fn animating(&self, now: Instant) -> bool {
        self.slots
            .iter()
            .flatten()
            .any(|n| n.animating_until(now).is_some() || n.moving())
    }

    /// Every video node in the stack, leaving ones included (a fading video
    /// keeps playing until it is removed), in slot order. `transport` marks
    /// the one the operator's transport drives: the current background's.
    pub fn media_uses(&self) -> Vec<MediaUse> {
        fn walk(content: &Content, node: u64, transport: bool, out: &mut Vec<MediaUse>) {
            match (&content.fill, &content.media) {
                (Fill::Source { id, .. }, Some(media)) => out.push(MediaUse {
                    node,
                    id: *id,
                    params: media.clone(),
                    transport: transport && !media.element,
                }),
                (Fill::Group(children), _) => {
                    children.iter().for_each(|c| walk(c, node, false, out))
                }
                _ => {}
            }
        }
        let current = self.current(Slot::Background).map(|n| n.id);
        let mut out = Vec::new();
        for node in self.slots.iter().flatten() {
            walk(&node.content, node.id, Some(node.id) == current, &mut out);
        }
        out
    }

    /// The live source ids any node draws, so their frames mark it dirty.
    pub fn sources(&self) -> impl Iterator<Item = u64> + '_ {
        self.slots.iter().flatten().flat_map(|n| {
            fn ids(content: &Content, out: &mut Vec<u64>) {
                match &content.fill {
                    Fill::Source { id, .. } => out.push(*id),
                    Fill::Group(children) => children.iter().for_each(|c| ids(c, out)),
                    _ => {}
                }
            }
            let mut out = Vec::new();
            ids(&n.content, &mut out);
            out
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ticker_runs_linearly_and_starts_again() {
        let m = Motion {
            distance: -1000.0,
            period_ms: 10_000,
            alternate: false,
        };
        assert_eq!(m.offset(Duration::ZERO), 0.0);
        assert!((m.offset(Duration::from_millis(2_500)) + 250.0).abs() < 1e-3);
        assert!((m.offset(Duration::from_millis(12_500)) + 250.0).abs() < 1e-3);
    }

    #[test]
    fn an_alternating_sweep_comes_back() {
        let m = Motion {
            distance: 1800.0,
            period_ms: 3_000,
            alternate: true,
        };
        assert!((m.offset(Duration::from_millis(1_500)) - 900.0).abs() < 1e-3);
        assert!((m.offset(Duration::from_millis(4_500)) - 900.0).abs() < 1e-3);
        assert!((m.offset(Duration::from_millis(3_750)) - 1350.0).abs() < 1e-3);
    }

    #[test]
    fn a_moving_node_keeps_the_output_drawing() {
        let mut stack = Stack::default();
        let now = Instant::now();
        let content = Content {
            motion: Some(Motion {
                distance: -10.0,
                period_ms: 1000,
                alternate: false,
            }),
            ..Content::new(Fill::Solid([1.0; 4]))
        };
        stack.swap(Slot::Message, Some(content), 0, now);
        assert!(stack.animating(now));
        assert_eq!(stack.next_change(now), Some(now));
        stack.swap(Slot::Message, None, 0, now);
        assert!(!stack.animating(now));
    }
}
