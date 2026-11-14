//! Pinned scrolling (`useContentAutoScroll`): a list follows its newest edge
//! once per content change and leaves manual scrolling and resizing alone.

use gpui_kit::{ScrollHandle, point, px};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    /// Newest first (the detected-verse queue).
    Top,
    /// Newest last (the transcript).
    Bottom,
}

/// Owns a list's scroll handle and the content version it last followed.
pub struct PinnedScroll {
    edge: Edge,
    handle: ScrollHandle,
    version: Option<u64>,
}

impl PinnedScroll {
    pub fn new(edge: Edge) -> Self {
        Self {
            edge,
            handle: ScrollHandle::new(),
            version: None,
        }
    }

    pub fn handle(&self) -> &ScrollHandle {
        &self.handle
    }

    /// Call with a hash of the content on every render. A new version moves
    /// the list to its edge; the same version leaves the offset alone.
    pub fn follow(&mut self, version: u64) {
        if self.version == Some(version) {
            return;
        }
        self.version = Some(version);
        match self.edge {
            Edge::Top => self.handle.set_offset(point(px(0.), px(0.))),
            // Applied at the next layout, once the new height is known.
            Edge::Bottom => self.handle.scroll_to_bottom(),
        }
    }
}
