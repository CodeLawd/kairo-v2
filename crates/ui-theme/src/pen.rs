//! The Pen (after ProPresenter's): click to place points, drag while
//! placing one to curve through it, click the first point to close the
//! shape, Esc or Return to finish an open line. Points are in pixels from
//! the frame's top left; the finished path is an element whose box is the
//! points' bounds and whose path data fills a 100 × 100 box.

use kairo_domain::overlay::{OverlayElement, OverlayElementKind};
use kairo_domain::overlay_boxes::overlay_box;
use kairo_domain::overlay_elements::create_overlay_element;

/// How close (in pixels) a press must land to the first point to close.
pub const CLOSE_PX: f64 = 8.0;
/// A drag shorter than this places a corner, not a curve.
const CURVE_MIN_PX: f64 = 3.0;
/// The thinnest box a finished path gets, in % of the frame.
const MIN_PCT: f64 = 0.5;

/// One placed point and, when dragged, the control point it leaves by.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PenPoint {
    pub at: (f64, f64),
    pub out: Option<(f64, f64)>,
}

impl PenPoint {
    /// The control point it arrives by: `out` mirrored through the point.
    pub fn inward(&self) -> Option<(f64, f64)> {
        self.out
            .map(|o| (2.0 * self.at.0 - o.0, 2.0 * self.at.1 - o.1))
    }
}

/// What a press did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PenPress {
    Placed,
    Closed,
}

/// A path being drawn.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PenDraft {
    pub points: Vec<PenPoint>,
    /// The button is down on the newest point: moving curves it.
    pub dragging: bool,
    /// Where the pointer is, for the line to the next point.
    pub cursor: Option<(f64, f64)>,
}

fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

impl PenDraft {
    /// Whether a press at `p` would close the path.
    pub fn closes_at(&self, p: (f64, f64)) -> bool {
        self.points.len() >= 2 && dist(p, self.points[0].at) <= CLOSE_PX
    }

    pub fn press(&mut self, p: (f64, f64)) -> PenPress {
        if self.closes_at(p) {
            return PenPress::Closed;
        }
        self.points.push(PenPoint { at: p, out: None });
        self.dragging = true;
        PenPress::Placed
    }

    pub fn moved(&mut self, p: (f64, f64)) {
        self.cursor = Some(p);
        if !self.dragging {
            return;
        }
        if let Some(last) = self.points.last_mut() {
            last.out = (dist(p, last.at) >= CURVE_MIN_PX).then_some(p);
        }
    }

    pub fn released(&mut self) {
        self.dragging = false;
    }
}

fn segment(d: &mut String, a: &PenPoint, b: &PenPoint, map: impl Fn((f64, f64)) -> (f64, f64)) {
    let to = map(b.at);
    if a.out.is_none() && b.out.is_none() {
        d.push_str(&format!(" L{:.2} {:.2}", to.0, to.1));
        return;
    }
    let c1 = map(a.out.unwrap_or(a.at));
    let c2 = map(b.inward().unwrap_or(b.at));
    d.push_str(&format!(
        " C{:.2} {:.2} {:.2} {:.2} {:.2} {:.2}",
        c1.0, c1.1, c2.0, c2.1, to.0, to.1
    ));
}

/// The finished path as a theme element, or `None` with fewer than two
/// points. `frame` is the frame's size in pixels.
pub fn to_element(
    points: &[PenPoint],
    closed: bool,
    frame: (f64, f64),
    id: &str,
) -> Option<OverlayElement> {
    if points.len() < 2 || !(frame.0 > 0.0 && frame.1 > 0.0) {
        return None;
    }
    // Everything in % of the frame, kept on the slide.
    let pct = |p: (f64, f64)| {
        (
            (p.0 / frame.0 * 100.0).clamp(0.0, 100.0),
            (p.1 / frame.1 * 100.0).clamp(0.0, 100.0),
        )
    };
    let all = points.iter().flat_map(|p| {
        [Some(p.at), p.out, p.inward()]
            .into_iter()
            .flatten()
            .map(pct)
    });
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for (x, y) in all {
        (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
    }
    // A straight line still needs a box: grow it about its middle.
    let grow = |lo: f64, hi: f64| {
        if hi - lo >= MIN_PCT {
            (lo, hi)
        } else {
            let mid = ((lo + hi) / 2.0).clamp(MIN_PCT / 2.0, 100.0 - MIN_PCT / 2.0);
            (mid - MIN_PCT / 2.0, mid + MIN_PCT / 2.0)
        }
    };
    let (x0, x1) = grow(x0, x1);
    let (y0, y1) = grow(y0, y1);
    let (bw, bh) = (x1 - x0, y1 - y0);
    let local = |p: (f64, f64)| {
        let (x, y) = pct(p);
        ((x - x0) / bw * 100.0, (y - y0) / bh * 100.0)
    };
    let start = local(points[0].at);
    let mut d = format!("M{:.2} {:.2}", start.0, start.1);
    for pair in points.windows(2) {
        segment(&mut d, &pair[0], &pair[1], local);
    }
    if closed {
        segment(&mut d, &points[points.len() - 1], &points[0], local);
        d.push_str(" Z");
    }
    let mut el = create_overlay_element(OverlayElementKind::Path, None, id, None);
    el.path = Some(d);
    el.frame = overlay_box(x0, y0, bw, bh);
    el.stroke.enabled = true;
    el.stroke.color = "#ffffff".into();
    el.stroke.width_px = 6.0;
    Some(el)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: (f64, f64) = (1000.0, 500.0);

    fn corner(x: f64, y: f64) -> PenPoint {
        PenPoint {
            at: (x, y),
            out: None,
        }
    }

    #[test]
    fn clicks_make_straight_segments_in_the_points_box() {
        let el = to_element(
            &[corner(100.0, 100.0), corner(300.0, 200.0)],
            false,
            FRAME,
            "p",
        )
        .expect("path");
        assert_eq!(el.kind, OverlayElementKind::Path);
        assert_eq!(el.path.as_deref(), Some("M0.00 0.00 L100.00 100.00"));
        assert!(!el.path_closed());
        let b = el.frame;
        assert!((b.x_pct - 10.0).abs() < 1e-9 && (b.y_pct - 20.0).abs() < 1e-9);
        assert!((b.width_pct - 20.0).abs() < 1e-9 && (b.height_pct - 20.0).abs() < 1e-9);
    }

    #[test]
    fn a_drag_curves_and_the_first_point_closes() {
        let mut pen = PenDraft::default();
        assert_eq!(pen.press((100.0, 100.0)), PenPress::Placed);
        pen.released();
        assert_eq!(pen.press((300.0, 100.0)), PenPress::Placed);
        pen.moved((350.0, 150.0));
        pen.released();
        assert!(pen.points[1].out.is_some());
        assert_eq!(pen.press((200.0, 300.0)), PenPress::Placed);
        pen.released();
        assert_eq!(pen.press((103.0, 98.0)), PenPress::Closed);
        let el = to_element(&pen.points, true, FRAME, "p").expect("path");
        let d = el.path.as_deref().unwrap();
        assert!(d.contains(" C"), "{d}");
        assert!(el.path_closed());
    }

    #[test]
    fn a_flat_line_still_gets_a_box_and_one_point_is_nothing() {
        let el = to_element(
            &[corner(100.0, 250.0), corner(400.0, 250.0)],
            false,
            FRAME,
            "p",
        )
        .expect("path");
        assert!(el.frame.height_pct >= MIN_PCT - 1e-9);
        assert!(to_element(&[corner(1.0, 1.0)], false, FRAME, "p").is_none());
    }
}
