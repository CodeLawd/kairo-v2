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
}

impl Slot {
    pub const ALL: [Slot; 14] = [
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

/// What fills a node's box.
#[derive(Clone, Debug)]
pub enum Fill {
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

#[derive(Clone, Debug)]
pub struct Content {
    pub fill: Fill,
    /// In frame pixels, before any group transform. `None`: the whole frame.
    pub rect: Option<Rect>,
    pub radius: f32,
    pub shadow: Option<Shadow>,
    /// The node's own CSS opacity, times the fade.
    pub opacity: f32,
}

impl Content {
    pub fn new(fill: Fill) -> Self {
        Self {
            fill,
            rect: None,
            radius: 0.0,
            shadow: None,
            opacity: 1.0,
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
    slots: [Vec<Node>; 14],
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
            current.content = content;
            return true;
        }
        self.enter(slot, content, Some(key), ms, now);
        let newest = self.slots[slot.index()].last().map(|n| n.id);
        self.retire_all(slot, ms, now, newest);
        false
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
                match (fade, removal) {
                    (Some(_), _) => Some(now),
                    (None, removal) => removal,
                }
            })
            .min()
    }

    /// True while any fade runs: each frame differs from the last.
    pub fn animating(&self, now: Instant) -> bool {
        self.slots
            .iter()
            .flatten()
            .any(|n| n.animating_until(now).is_some())
    }

    /// The live source ids any node draws, so their frames mark it dirty.
    pub fn sources(&self) -> impl Iterator<Item = u64> + '_ {
        self.slots
            .iter()
            .flatten()
            .filter_map(|n| match n.content.fill {
                Fill::Source { id, .. } => Some(id),
                _ => None,
            })
    }
}
