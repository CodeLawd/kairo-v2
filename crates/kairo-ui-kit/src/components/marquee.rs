//! `MarqueeSelect` (`components/shared/MarqueeSelect.tsx`): rubber-band
//! selection over a grid.
//!
//! The band starts from empty space between items, or from anywhere with
//! ⌘/Ctrl held, so a plain press on a card still sends it live and a plain
//! drag on a card still moves it. ⌘ or Shift adds to the selection. The list
//! scrolls when the band reaches its top or bottom edge.
//!
//! [`Marquee`] is the gesture; the grid that owns it answers which items a
//! band covers. Grids lay out by geometry (spike 007), so they can answer
//! for rows that are scrolled out of view and not rendered.

use crate::theme::KairoThemeExt;
use crate::tokens::radius;
use gpui_kit::{
    App, Bounds, IntoElement, Modifiers, Pixels, Point, RenderOnce, Styled, Window, div, point, px,
    size,
};

/// Pointer travel before a press becomes a band.
pub const START_DISTANCE: f32 = 4.0;
/// The band scrolls the list within this distance of its top or bottom.
pub const EDGE: f32 = 36.0;
/// How far one edge scroll moves.
pub const SCROLL_STEP: f32 = 14.0;

/// Where the press landed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PressTarget {
    /// On a text field, link, open menu, dialog or `data-no-marquee` area.
    pub interactive: bool,
    /// On a selectable item.
    pub item: bool,
    /// On a button.
    pub control: bool,
}

/// What a pointer move did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarqueeStep {
    /// Still within [`START_DISTANCE`] of the press.
    Pending,
    /// The band just began. The owner clears the selection unless
    /// `additive`, then selects what the band covers.
    Began {
        /// ⌘, Ctrl or Shift was held at the press.
        additive: bool,
    },
    /// The band moved.
    Moved,
}

/// One rubber-band gesture, in the list's content coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Marquee {
    start: Point<Pixels>,
    pointer: Point<Pixels>,
    start_scroll: Pixels,
    additive: bool,
    active: bool,
}

impl Marquee {
    /// A primary press at `position` (window coordinates) while the list is
    /// scrolled by `scroll_top`. `None` when the press should behave
    /// normally instead.
    pub fn press(
        position: Point<Pixels>,
        scroll_top: Pixels,
        modifiers: Modifiers,
        target: PressTarget,
    ) -> Option<Self> {
        if target.interactive {
            return None;
        }
        let modifier = modifiers.platform || modifiers.control;
        if !modifier && (target.item || target.control) {
            return None;
        }
        Some(Self {
            start: position,
            pointer: position,
            start_scroll: scroll_top,
            additive: modifier || modifiers.shift,
            active: false,
        })
    }

    /// The pointer moved to `position`.
    pub fn move_to(&mut self, position: Point<Pixels>) -> MarqueeStep {
        self.pointer = position;
        if self.active {
            return MarqueeStep::Moved;
        }
        let dx = f32::from(position.x - self.start.x);
        let dy = f32::from(position.y - self.start.y);
        if dx.hypot(dy) < START_DISTANCE {
            return MarqueeStep::Pending;
        }
        self.active = true;
        MarqueeStep::Began {
            additive: self.additive,
        }
    }

    /// Whether the band is showing.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Whether the selection is added to rather than replaced.
    pub fn is_additive(&self) -> bool {
        self.additive
    }

    /// The band in window coordinates, with the press point moved by how far
    /// the list has scrolled since.
    pub fn band(&self, scroll_top: Pixels) -> Bounds<Pixels> {
        let scrolled = scroll_top - self.start_scroll;
        let start_y = self.start.y - scrolled;
        let top = start_y.min(self.pointer.y);
        let bottom = start_y.max(self.pointer.y);
        let left = self.start.x.min(self.pointer.x);
        let right = self.start.x.max(self.pointer.x);
        Bounds::new(point(left, top), size(right - left, bottom - top))
    }

    /// The scroll to apply for the pointer near the list's edges: negative
    /// near the top, positive near the bottom, zero elsewhere.
    pub fn edge_scroll(&self, list: Bounds<Pixels>) -> Pixels {
        if !self.active {
            return px(0.);
        }
        if self.pointer.y < list.top() + px(EDGE) {
            px(-SCROLL_STEP)
        } else if self.pointer.y > list.bottom() - px(EDGE) {
            px(SCROLL_STEP)
        } else {
            px(0.)
        }
    }

    /// The release. `true` when a band was drawn, so the click that follows
    /// must not reach the card under the pointer.
    pub fn release(self) -> bool {
        self.active
    }
}

/// Whether `item` touches `band` (edges count, as `getBoundingClientRect`
/// comparisons with `>=` and `<=` do).
pub fn band_hits(band: Bounds<Pixels>, item: Bounds<Pixels>) -> bool {
    item.right() >= band.left()
        && item.left() <= band.right()
        && item.bottom() >= band.top()
        && item.top() <= band.bottom()
}

/// The band, drawn see-through so the swept cards stay readable. Place it
/// in an absolutely positioned overlay in window coordinates.
#[derive(IntoElement)]
pub struct MarqueeBand {
    bounds: Bounds<Pixels>,
}

impl MarqueeBand {
    /// The band over `bounds`.
    pub fn new(bounds: Bounds<Pixels>) -> Self {
        Self { bounds }
    }
}

impl RenderOnce for MarqueeBand {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = cx.kairo();
        div()
            .absolute()
            .left(self.bounds.left())
            .top(self.bounds.top())
            .w(self.bounds.size.width)
            .h(self.bounds.size.height)
            .rounded(radius::sm())
            .border_1()
            .border_color(k.marquee_border())
            .bg(k.marquee_fill())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32, y: f32) -> Point<Pixels> {
        point(px(x), px(y))
    }

    #[test]
    fn plain_press_on_an_item_or_button_is_a_click() {
        let none = Modifiers::default();
        let item = PressTarget {
            item: true,
            ..Default::default()
        };
        let control = PressTarget {
            control: true,
            ..Default::default()
        };
        assert!(Marquee::press(at(0., 0.), px(0.), none, item).is_none());
        assert!(Marquee::press(at(0., 0.), px(0.), none, control).is_none());
        let cmd = Modifiers {
            platform: true,
            ..Default::default()
        };
        let m = Marquee::press(at(0., 0.), px(0.), cmd, item).unwrap();
        assert!(m.is_additive());
    }

    #[test]
    fn interactive_targets_never_start_a_band() {
        let cmd = Modifiers {
            platform: true,
            ..Default::default()
        };
        let t = PressTarget {
            interactive: true,
            ..Default::default()
        };
        assert!(Marquee::press(at(0., 0.), px(0.), cmd, t).is_none());
    }

    #[test]
    fn begins_after_four_pixels_and_shift_adds() {
        let shift = Modifiers {
            shift: true,
            ..Default::default()
        };
        let mut m = Marquee::press(at(10., 10.), px(0.), shift, PressTarget::default()).unwrap();
        assert_eq!(m.move_to(at(12., 12.)), MarqueeStep::Pending);
        assert!(!m.release());
        let mut m = Marquee::press(at(10., 10.), px(0.), shift, PressTarget::default()).unwrap();
        assert_eq!(
            m.move_to(at(14., 14.)),
            MarqueeStep::Began { additive: true }
        );
        assert_eq!(m.move_to(at(20., 20.)), MarqueeStep::Moved);
        assert!(m.release());
    }

    #[test]
    fn band_follows_scroll_and_any_direction() {
        let mut m = Marquee::press(
            at(100., 100.),
            px(0.),
            Modifiers::default(),
            PressTarget::default(),
        )
        .unwrap();
        m.move_to(at(50., 40.));
        let b = m.band(px(0.));
        assert_eq!(
            (b.left(), b.top(), b.right(), b.bottom()),
            (px(50.), px(40.), px(100.), px(100.))
        );
        // The list scrolled 30 down: the press point moved up with the content.
        let b = m.band(px(30.));
        assert_eq!((b.top(), b.bottom()), (px(40.), px(70.)));
    }

    #[test]
    fn edges_scroll_only_while_active() {
        let list = Bounds::new(at(0., 0.), size(px(300.), px(400.)));
        let mut m = Marquee::press(
            at(10., 200.),
            px(0.),
            Modifiers::default(),
            PressTarget::default(),
        )
        .unwrap();
        assert_eq!(m.edge_scroll(list), px(0.));
        m.move_to(at(10., 390.));
        assert_eq!(m.edge_scroll(list), px(SCROLL_STEP));
        m.move_to(at(10., 10.));
        assert_eq!(m.edge_scroll(list), px(-SCROLL_STEP));
        m.move_to(at(10., 200.));
        assert_eq!(m.edge_scroll(list), px(0.));
    }

    #[test]
    fn touching_edges_count_as_hits() {
        let band = Bounds::new(at(0., 0.), size(px(10.), px(10.)));
        assert!(band_hits(
            band,
            Bounds::new(at(10., 10.), size(px(5.), px(5.)))
        ));
        assert!(!band_hits(
            band,
            Bounds::new(at(10.5, 0.), size(px(5.), px(5.)))
        ));
    }
}
