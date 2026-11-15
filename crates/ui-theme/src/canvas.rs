//! The canvas's geometry and drag rules (`OverlayCanvas.tsx`), with no
//! drawing. Every box is in % of the frame; pointers are in pixels relative
//! to the frame's top-left corner.

use kairo_domain::js::math_round;
use kairo_domain::overlay::{OverlayBox, OverlayContentKind, OverlayTheme};
use kairo_domain::overlay_boxes::{
    BoxMinimum, ELEMENT_BOX_MIN, ResizeHandle, TEXT_BOX_MIN, move_overlay_box, overlay_box,
    resize_overlay_box, resize_overlay_box_keeping_aspect,
};
use kairo_domain::overlay_elements::overlay_element_label;
use kairo_domain::overlay_outputs::{OverlayLayer, overlay_layer_label};

pub const VERSE: &str = "verse";
pub const REFERENCE: &str = "reference";

/// Shift while rotating snaps to this many degrees.
pub const ROTATE_SNAP_DEG: f64 = 15.0;
/// How close (in % of the frame) a box centre has to come to the frame
/// centre to snap.
pub const SNAP_PCT: f64 = 1.5;
/// Arrow-key nudge, and with Shift.
pub const NUDGE_PCT: f64 = 0.5;
pub const NUDGE_SHIFT_PCT: f64 = 5.0;

/// A resize handle is a 10 px square on the frame's edge.
pub const HANDLE_PX: f64 = 10.0;
/// The rotate knob is 12 px across, centred this far below the box.
pub const ROTATE_KNOB_PX: f64 = 12.0;
pub const ROTATE_KNOB_OFFSET_PX: f64 = 22.0;
/// The delete button: 16 px, its top-right corner 4 px past the box's.
pub const DELETE_PX: f64 = 16.0;
pub const DELETE_RIGHT_OUT_PX: f64 = 4.0;
pub const DELETE_ABOVE_PX: f64 = 20.0;
/// A little slack around the small targets, for a finger on a trackpad.
const SLOP_PX: f64 = 1.0;

/// The two text boxes.
pub fn is_text_layer(id: &str) -> bool {
    id == VERSE || id == REFERENCE
}

/// The reference hides and elements delete. The verse box holds the content
/// itself, and the theme has no flag that would hide it, so Delete on it is
/// a no-op rather than an error.
pub fn is_deletable(id: &str) -> bool {
    id != VERSE
}

pub fn delete_verb(id: &str) -> &'static str {
    if is_text_layer(id) { "Hide" } else { "Delete" }
}

pub fn layer_minimum(id: &str) -> BoxMinimum {
    if is_text_layer(id) {
        TEXT_BOX_MIN
    } else {
        ELEMENT_BOX_MIN
    }
}

/// The box `id` occupies, or `None` when it is gone (a deleted element).
pub fn layer_box(theme: &OverlayTheme, id: &str) -> Option<OverlayBox> {
    match id {
        VERSE => Some(theme.verse.frame.clone()),
        REFERENCE => Some(theme.reference.text.frame.clone()),
        _ => theme
            .elements
            .iter()
            .find(|el| el.id == id)
            .map(|el| el.frame.clone()),
    }
}

pub fn layer_rotation(theme: &OverlayTheme, id: &str) -> f64 {
    theme
        .elements
        .iter()
        .find(|el| el.id == id)
        .map_or(0.0, |el| el.rotation_deg)
}

/// One thing on the canvas.
#[derive(Clone, Debug, PartialEq)]
pub struct CanvasLayer {
    pub id: String,
    pub label: String,
    pub frame: OverlayBox,
    /// Stacking order, low first.
    pub z: i32,
    /// Elements turn; text boxes do not, and get no rotate handle.
    pub rotation: Option<f64>,
}

/// Drawing order: elements under the text, the text, elements over it. A
/// hidden reference is not on the canvas.
pub fn layers(theme: &OverlayTheme, kind: OverlayContentKind) -> Vec<CanvasLayer> {
    let element = |el: &kairo_domain::overlay::OverlayElement, z: i32| CanvasLayer {
        id: el.id.clone(),
        label: overlay_element_label(el.kind, el.shape.as_deref()).to_owned(),
        frame: el.frame.clone(),
        z,
        rotation: Some(el.rotation_deg),
    };
    let mut out = Vec::new();
    for (i, el) in theme.elements.iter().filter(|e| !e.above_text).enumerate() {
        out.push(element(el, 100 + i as i32));
    }
    out.push(CanvasLayer {
        id: VERSE.into(),
        label: overlay_layer_label(kind, OverlayLayer::Verse).into(),
        frame: theme.verse.frame.clone(),
        z: 200,
        rotation: None,
    });
    if theme.reference.show {
        out.push(CanvasLayer {
            id: REFERENCE.into(),
            label: overlay_layer_label(kind, OverlayLayer::Reference).into(),
            frame: theme.reference.text.frame.clone(),
            z: 210,
            rotation: None,
        });
    }
    for (i, el) in theme.elements.iter().filter(|e| e.above_text).enumerate() {
        out.push(element(el, 300 + i as i32));
    }
    out
}

// ─── Resize, snap, rotate, nudge ───────────────────────────────────────────

/// A resize drag on a turned box: the pointer moves in screen space, the
/// handles in the box's own. Turn the pointer delta into the box's axes,
/// resize there, then move the centre back out so the opposite edge stays
/// where it was.
pub fn resize_turned_box(
    start: &OverlayBox,
    handle: ResizeHandle,
    delta_px: (f64, f64),
    frame: (f64, f64),
    rotation_deg: f64,
    keep_aspect: bool,
    min: BoxMinimum,
) -> OverlayBox {
    let rad = rotation_deg.to_radians();
    let (sin, cos) = rad.sin_cos();
    let (dx_px, dy_px) = delta_px;
    let local_x = dx_px * cos + dy_px * sin;
    let local_y = -dx_px * sin + dy_px * cos;
    let next = if keep_aspect {
        resize_overlay_box_keeping_aspect(
            start,
            handle,
            local_x / frame.0 * 100.0,
            local_y / frame.1 * 100.0,
            min,
        )
    } else {
        resize_overlay_box(
            start,
            handle,
            local_x / frame.0 * 100.0,
            local_y / frame.1 * 100.0,
            min,
        )
    };
    if rotation_deg == 0.0 {
        return next;
    }
    let shift_x = (next.x_pct + next.width_pct / 2.0 - (start.x_pct + start.width_pct / 2.0))
        / 100.0
        * frame.0;
    let shift_y = (next.y_pct + next.height_pct / 2.0 - (start.y_pct + start.height_pct / 2.0))
        / 100.0
        * frame.1;
    let cx =
        start.x_pct + start.width_pct / 2.0 + (shift_x * cos - shift_y * sin) / frame.0 * 100.0;
    let cy =
        start.y_pct + start.height_pct / 2.0 + (shift_x * sin + shift_y * cos) / frame.1 * 100.0;
    overlay_box(
        cx - next.width_pct / 2.0,
        cy - next.height_pct / 2.0,
        next.width_pct,
        next.height_pct,
    )
}

/// A moved box, centred on the frame's axes when it comes close, and which
/// guides to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapped {
    pub frame: OverlayBox,
    pub vertical: bool,
    pub horizontal: bool,
}

pub fn snap_to_centre(b: &OverlayBox) -> Snapped {
    let cx = b.x_pct + b.width_pct / 2.0;
    let cy = b.y_pct + b.height_pct / 2.0;
    let vertical = (cx - 50.0).abs() < SNAP_PCT;
    let horizontal = (cy - 50.0).abs() < SNAP_PCT;
    Snapped {
        frame: overlay_box(
            if vertical {
                50.0 - b.width_pct / 2.0
            } else {
                b.x_pct
            },
            if horizontal {
                50.0 - b.height_pct / 2.0
            } else {
                b.y_pct
            },
            b.width_pct,
            b.height_pct,
        ),
        vertical,
        horizontal,
    }
}

/// The angle of a rotate drag: 0° with the pointer straight above the
/// centre, clockwise positive, −180–180°, whole degrees. Shift snaps to 15°.
pub fn rotation_degrees(
    start: &OverlayBox,
    frame: (f64, f64),
    pointer: (f64, f64),
    shift: bool,
) -> f64 {
    let cx = (start.x_pct + start.width_pct / 2.0) / 100.0 * frame.0;
    let cy = (start.y_pct + start.height_pct / 2.0) / 100.0 * frame.1;
    let mut deg = (pointer.0 - cx).atan2(cy - pointer.1).to_degrees();
    if shift {
        deg = math_round(deg / ROTATE_SNAP_DEG) * ROTATE_SNAP_DEG;
    }
    if deg > 180.0 {
        deg -= 360.0;
    }
    math_round(deg)
}

/// What an arrow key moves a layer by, in % of the frame.
pub fn nudge(key: &str, shift: bool) -> Option<(f64, f64)> {
    let step = if shift { NUDGE_SHIFT_PCT } else { NUDGE_PCT };
    match key {
        "left" | "ArrowLeft" => Some((-step, 0.0)),
        "right" | "ArrowRight" => Some((step, 0.0)),
        "up" | "ArrowUp" => Some((0.0, -step)),
        "down" | "ArrowDown" => Some((0.0, step)),
        _ => None,
    }
}

// ─── Dragging ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragMode {
    Move,
    Resize(ResizeHandle),
    Rotate,
}

/// A drag in progress.
#[derive(Clone, Debug, PartialEq)]
pub struct Drag {
    pub id: String,
    pub mode: DragMode,
    pub start: (f64, f64),
    pub start_box: OverlayBox,
    pub rotation_deg: f64,
}

impl Drag {
    /// Starts a drag of `id` at `pointer`. `None` when the layer is gone.
    pub fn begin(
        theme: &OverlayTheme,
        id: &str,
        mode: DragMode,
        pointer: (f64, f64),
    ) -> Option<Self> {
        Some(Self {
            id: id.to_owned(),
            mode,
            start: pointer,
            start_box: layer_box(theme, id)?,
            rotation_deg: layer_rotation(theme, id),
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum DragEdit {
    Box(OverlayBox),
    Rotate(f64),
}

#[derive(Clone, Debug, PartialEq)]
pub struct DragStep {
    pub edit: DragEdit,
    /// Centre guides to draw: (vertical line, horizontal line).
    pub guides: (bool, bool),
}

/// What the pointer at `pointer` does to the dragged layer. `None` when the
/// frame has no size yet.
pub fn drag_step(
    drag: &Drag,
    pointer: (f64, f64),
    frame: (f64, f64),
    shift: bool,
) -> Option<DragStep> {
    if frame.0 <= 0.0 || frame.1 <= 0.0 {
        return None;
    }
    let min = layer_minimum(&drag.id);
    match drag.mode {
        DragMode::Rotate => Some(DragStep {
            edit: DragEdit::Rotate(rotation_degrees(&drag.start_box, frame, pointer, shift)),
            guides: (false, false),
        }),
        DragMode::Resize(handle) => Some(DragStep {
            edit: DragEdit::Box(resize_turned_box(
                &drag.start_box,
                handle,
                (pointer.0 - drag.start.0, pointer.1 - drag.start.1),
                frame,
                drag.rotation_deg,
                shift,
                min,
            )),
            guides: (false, false),
        }),
        DragMode::Move => {
            let dx = (pointer.0 - drag.start.0) / frame.0 * 100.0;
            let dy = (pointer.1 - drag.start.1) / frame.1 * 100.0;
            let snapped = snap_to_centre(&move_overlay_box(&drag.start_box, dx, dy, min));
            Some(DragStep {
                edit: DragEdit::Box(snapped.frame),
                guides: (snapped.vertical, snapped.horizontal),
            })
        }
    }
}

// ─── Hit testing ───────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum Hit {
    Delete(String),
    Rotate(String),
    Resize(String, ResizeHandle),
    Body(String),
}

impl Hit {
    pub fn id(&self) -> &str {
        match self {
            Hit::Delete(id) | Hit::Rotate(id) | Hit::Resize(id, _) | Hit::Body(id) => id,
        }
    }
}

/// A box in pixels: centre, size and turn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PixelBox {
    pub cx: f64,
    pub cy: f64,
    pub w: f64,
    pub h: f64,
    pub rotation_deg: f64,
}

impl PixelBox {
    pub fn new(b: &OverlayBox, rotation_deg: f64, frame: (f64, f64)) -> Self {
        Self {
            cx: (b.x_pct + b.width_pct / 2.0) / 100.0 * frame.0,
            cy: (b.y_pct + b.height_pct / 2.0) / 100.0 * frame.1,
            w: b.width_pct / 100.0 * frame.0,
            h: b.height_pct / 100.0 * frame.1,
            rotation_deg,
        }
    }

    /// A point in the box's own axes, from its centre.
    pub fn to_local(&self, p: (f64, f64)) -> (f64, f64) {
        let (sin, cos) = (-self.rotation_deg).to_radians().sin_cos();
        let (dx, dy) = (p.0 - self.cx, p.1 - self.cy);
        (dx * cos - dy * sin, dx * sin + dy * cos)
    }

    /// A point given in the box's own axes, in frame pixels.
    pub fn to_frame(&self, local: (f64, f64)) -> (f64, f64) {
        let (sin, cos) = self.rotation_deg.to_radians().sin_cos();
        (
            self.cx + local.0 * cos - local.1 * sin,
            self.cy + local.0 * sin + local.1 * cos,
        )
    }

    pub fn contains(&self, p: (f64, f64)) -> bool {
        let (x, y) = self.to_local(p);
        x.abs() <= self.w / 2.0 && y.abs() <= self.h / 2.0
    }

    /// The four corners, clockwise from the top left, in frame pixels.
    pub fn corners(&self) -> [(f64, f64); 4] {
        let (hw, hh) = (self.w / 2.0, self.h / 2.0);
        [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)].map(|c| self.to_frame(c))
    }

    /// Where a handle's centre sits, in the box's own axes.
    pub fn handle_local(&self, handle: ResizeHandle) -> (f64, f64) {
        let (hw, hh) = (self.w / 2.0, self.h / 2.0);
        match handle {
            ResizeHandle::N => (0.0, -hh),
            ResizeHandle::S => (0.0, hh),
            ResizeHandle::E => (hw, 0.0),
            ResizeHandle::W => (-hw, 0.0),
            ResizeHandle::Ne => (hw, -hh),
            ResizeHandle::Nw => (-hw, -hh),
            ResizeHandle::Se => (hw, hh),
            ResizeHandle::Sw => (-hw, hh),
        }
    }

    pub fn rotate_knob_local(&self) -> (f64, f64) {
        (0.0, self.h / 2.0 + ROTATE_KNOB_OFFSET_PX)
    }

    pub fn delete_local(&self) -> (f64, f64) {
        (
            self.w / 2.0 + DELETE_RIGHT_OUT_PX - DELETE_PX / 2.0,
            -self.h / 2.0 - DELETE_ABOVE_PX + DELETE_PX / 2.0,
        )
    }
}

/// Which handle's square the point is on, topmost first (the DOM stacked
/// them in `n s e w ne nw se sw` order, later on top).
fn handle_at(b: &PixelBox, local: (f64, f64)) -> Option<ResizeHandle> {
    let reach = HANDLE_PX / 2.0 + SLOP_PX;
    ResizeHandle::ALL.iter().rev().copied().find(|&h| {
        let c = b.handle_local(h);
        (local.0 - c.0).abs() <= reach && (local.1 - c.1).abs() <= reach
    })
}

/// What is under `pointer`. The selected layer's handles, rotate knob and
/// delete button come first, then layers from the top of the stack down.
pub fn hit_test(
    layers: &[CanvasLayer],
    selected: Option<&str>,
    pointer: (f64, f64),
    frame: (f64, f64),
) -> Option<Hit> {
    if let Some(layer) = selected.and_then(|id| layers.iter().find(|l| l.id == id)) {
        let b = PixelBox::new(&layer.frame, layer.rotation.unwrap_or(0.0), frame);
        let local = b.to_local(pointer);
        if is_deletable(&layer.id) {
            let c = b.delete_local();
            let reach = DELETE_PX / 2.0 + SLOP_PX;
            if (local.0 - c.0).abs() <= reach && (local.1 - c.1).abs() <= reach {
                return Some(Hit::Delete(layer.id.clone()));
            }
        }
        if layer.rotation.is_some() {
            let c = b.rotate_knob_local();
            let reach = ROTATE_KNOB_PX / 2.0 + SLOP_PX;
            if (local.0 - c.0).hypot(local.1 - c.1) <= reach {
                return Some(Hit::Rotate(layer.id.clone()));
            }
        }
        if let Some(handle) = handle_at(&b, local) {
            return Some(Hit::Resize(layer.id.clone(), handle));
        }
        if b.contains(pointer) {
            return Some(Hit::Body(layer.id.clone()));
        }
    }
    let mut stack: Vec<&CanvasLayer> = layers.iter().collect();
    stack.sort_by_key(|l| std::cmp::Reverse(l.z));
    stack
        .into_iter()
        .find(|l| PixelBox::new(&l.frame, l.rotation.unwrap_or(0.0), frame).contains(pointer))
        .map(|l| Hit::Body(l.id.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairo_domain::overlay::OverlayElementKind;
    use kairo_domain::overlay_defaults::DEFAULT_OVERLAY_THEME;
    use kairo_domain::overlay_elements::create_overlay_element;

    const FRAME: (f64, f64) = (1000.0, 500.0);

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    fn themed() -> OverlayTheme {
        let mut t = DEFAULT_OVERLAY_THEME.clone();
        t.reference.show = true;
        let mut under = create_overlay_element(OverlayElementKind::Rectangle, None, "under", None);
        under.frame = overlay_box(10.0, 10.0, 30.0, 30.0);
        let mut over = create_overlay_element(OverlayElementKind::Ellipse, None, "over", None);
        over.above_text = true;
        over.frame = overlay_box(50.0, 50.0, 20.0, 20.0);
        t.elements = vec![under, over];
        t
    }

    #[test]
    fn layers_draw_under_then_text_then_over() {
        let t = themed();
        let ids: Vec<_> = layers(&t, OverlayContentKind::Scripture)
            .into_iter()
            .map(|l| l.id)
            .collect();
        assert_eq!(ids, ["under", "verse", "reference", "over"]);
        let mut hidden = t.clone();
        hidden.reference.show = false;
        assert!(
            layers(&hidden, OverlayContentKind::Scripture)
                .iter()
                .all(|l| l.id != "reference")
        );
    }

    #[test]
    fn labels_follow_the_content_kind() {
        let t = themed();
        let ls = layers(&t, OverlayContentKind::Lyrics);
        assert_eq!(ls[1].label, "Lyrics");
        assert_eq!(ls[2].label, "Song title");
        assert_eq!(ls[0].label, "Rectangle");
    }

    #[test]
    fn text_layers_do_not_turn_and_verse_cannot_be_deleted() {
        let ls = layers(&themed(), OverlayContentKind::Scripture);
        assert!(
            ls.iter()
                .filter(|l| is_text_layer(&l.id))
                .all(|l| l.rotation.is_none())
        );
        assert!(!is_deletable("verse"));
        assert!(is_deletable("reference") && is_deletable("anything"));
        assert_eq!(delete_verb("reference"), "Hide");
        assert_eq!(delete_verb("under"), "Delete");
    }

    #[test]
    fn move_snaps_to_the_centre_and_shows_guides() {
        let t = themed();
        let drag = Drag::begin(&t, "over", DragMode::Move, (600.0, 300.0)).unwrap();
        // Box centre starts at (60%, 60%). Move it to within 1.5% of 50%.
        let step = drag_step(&drag, (600.0 - 95.0, 300.0 - 48.0), FRAME, false).unwrap();
        let DragEdit::Box(b) = step.edit else {
            panic!()
        };
        assert_eq!(step.guides, (true, true));
        assert!(close(b.x_pct + b.width_pct / 2.0, 50.0));
        assert!(close(b.y_pct + b.height_pct / 2.0, 50.0));
        let far = drag_step(&drag, (600.0 - 50.0, 300.0), FRAME, false).unwrap();
        assert_eq!(far.guides, (false, false));
    }

    #[test]
    fn move_stays_inside_the_frame() {
        let t = themed();
        let drag = Drag::begin(&t, "over", DragMode::Move, (0.0, 0.0)).unwrap();
        let DragEdit::Box(b) = drag_step(&drag, (5000.0, 5000.0), FRAME, false)
            .unwrap()
            .edit
        else {
            panic!()
        };
        assert!(close(b.x_pct + b.width_pct, 100.0) && close(b.y_pct + b.height_pct, 100.0));
    }

    #[test]
    fn nothing_moves_in_a_frame_with_no_size() {
        let t = themed();
        let drag = Drag::begin(&t, "over", DragMode::Move, (0.0, 0.0)).unwrap();
        assert!(drag_step(&drag, (10.0, 10.0), (0.0, 500.0), false).is_none());
        assert!(drag_step(&drag, (10.0, 10.0), (500.0, 0.0), false).is_none());
    }

    #[test]
    fn a_deleted_layer_cannot_be_dragged() {
        assert!(Drag::begin(&themed(), "gone", DragMode::Move, (0.0, 0.0)).is_none());
    }

    #[test]
    fn resize_east_edge_grows_the_box() {
        let t = themed();
        let drag = Drag::begin(
            &t,
            "under",
            DragMode::Resize(ResizeHandle::E),
            (400.0, 100.0),
        )
        .unwrap();
        let DragEdit::Box(b) = drag_step(&drag, (500.0, 100.0), FRAME, false).unwrap().edit else {
            panic!()
        };
        assert!(close(b.width_pct, 40.0) && close(b.x_pct, 10.0));
    }

    #[test]
    fn resize_with_shift_keeps_the_aspect() {
        let t = themed();
        let drag = Drag::begin(
            &t,
            "under",
            DragMode::Resize(ResizeHandle::Se),
            (400.0, 200.0),
        )
        .unwrap();
        let DragEdit::Box(b) = drag_step(&drag, (500.0, 200.0), FRAME, true).unwrap().edit else {
            panic!()
        };
        assert!(close(b.width_pct / b.height_pct, 1.0));
    }

    #[test]
    fn resize_a_turned_box_keeps_the_opposite_edge_still() {
        // A 20 × 20 % box turned 90°: its east handle now points down the
        // screen. Dragging it down by 50 px grows the local width by 5 %
        // of the frame width (1000 px), and the west edge stays put.
        let start = overlay_box(40.0, 40.0, 20.0, 40.0);
        let next = resize_turned_box(
            &start,
            ResizeHandle::E,
            (0.0, 50.0),
            FRAME,
            90.0,
            false,
            ELEMENT_BOX_MIN,
        );
        assert!(close(next.width_pct, 25.0), "{next:?}");
        let before = PixelBox::new(&start, 90.0, FRAME);
        let after = PixelBox::new(&next, 90.0, FRAME);
        let west_before = before.to_frame((-before.w / 2.0, 0.0));
        let west_after = after.to_frame((-after.w / 2.0, 0.0));
        assert!(close(west_before.0, west_after.0) && close(west_before.1, west_after.1));
    }

    #[test]
    fn unturned_resize_is_the_plain_resize() {
        let start = overlay_box(10.0, 10.0, 30.0, 30.0);
        let a = resize_turned_box(
            &start,
            ResizeHandle::Nw,
            (20.0, 10.0),
            FRAME,
            0.0,
            false,
            TEXT_BOX_MIN,
        );
        let b = resize_overlay_box(&start, ResizeHandle::Nw, 2.0, 2.0, TEXT_BOX_MIN);
        assert_eq!(a, b);
    }

    #[test]
    fn rotation_is_zero_straight_above_and_clockwise() {
        let b = overlay_box(40.0, 40.0, 20.0, 20.0); // centre (500, 250)
        assert_eq!(rotation_degrees(&b, FRAME, (500.0, 100.0), false), 0.0);
        assert_eq!(rotation_degrees(&b, FRAME, (700.0, 250.0), false), 90.0);
        assert_eq!(rotation_degrees(&b, FRAME, (500.0, 400.0), false), 180.0);
        assert_eq!(rotation_degrees(&b, FRAME, (300.0, 250.0), false), -90.0);
    }

    #[test]
    fn shift_snaps_rotation_to_fifteen_degrees() {
        let b = overlay_box(40.0, 40.0, 20.0, 20.0);
        // About 22° off vertical.
        let p = (
            500.0 + 100.0 * 22f64.to_radians().sin(),
            250.0 - 100.0 * 22f64.to_radians().cos(),
        );
        assert_eq!(rotation_degrees(&b, FRAME, p, false), 22.0);
        assert_eq!(rotation_degrees(&b, FRAME, p, true), 15.0);
    }

    #[test]
    fn nudge_steps_and_ignores_other_keys() {
        assert_eq!(nudge("left", false), Some((-0.5, 0.0)));
        assert_eq!(nudge("down", true), Some((0.0, 5.0)));
        assert_eq!(nudge("a", false), None);
    }

    #[test]
    fn hit_test_prefers_the_top_of_the_stack_and_the_selected_layers_handles() {
        let mut t = themed();
        t.elements[0].frame = overlay_box(45.0, 45.0, 20.0, 20.0);
        let ls = layers(&t, OverlayContentKind::Scripture);
        // (55%, 55%) lies in the over element, the under one and maybe verse.
        let p = (550.0, 275.0);
        assert_eq!(
            hit_test(&ls, None, p, FRAME),
            Some(Hit::Body("over".into()))
        );
        // Select `under`: its south-east handle is at (65%, 65%).
        let se = (650.0, 325.0);
        assert_eq!(
            hit_test(&ls, Some("under"), se, FRAME),
            Some(Hit::Resize("under".into(), ResizeHandle::Se))
        );
        // Without a selection that point is the over element's body.
        assert_eq!(
            hit_test(&ls, None, se, FRAME),
            Some(Hit::Body("over".into()))
        );
    }

    #[test]
    fn hit_test_finds_rotate_and_delete_for_the_selected_element() {
        let t = themed();
        let ls = layers(&t, OverlayContentKind::Scripture);
        let b = PixelBox::new(&t.elements[0].frame, 0.0, FRAME);
        let knob = b.to_frame(b.rotate_knob_local());
        assert_eq!(
            hit_test(&ls, Some("under"), knob, FRAME),
            Some(Hit::Rotate("under".into()))
        );
        let x = b.to_frame(b.delete_local());
        assert_eq!(
            hit_test(&ls, Some("under"), x, FRAME),
            Some(Hit::Delete("under".into()))
        );
        // The verse has no delete button and no knob.
        let v = PixelBox::new(&t.verse.frame, 0.0, FRAME);
        assert_ne!(
            hit_test(&ls, Some("verse"), v.to_frame(v.delete_local()), FRAME),
            Some(Hit::Delete("verse".into()))
        );
    }

    #[test]
    fn hit_test_follows_a_turned_box() {
        let mut t = themed();
        t.elements[0].frame = overlay_box(40.0, 40.0, 40.0, 4.0); // 400 × 20 px at (600, 225)
        t.elements[0].rotation_deg = 90.0;
        let ls = layers(&t, OverlayContentKind::Scripture);
        // Turned a quarter, it is 20 px wide and 400 px tall around (600, 225).
        assert_eq!(
            hit_test(&ls, None, (600.0, 60.0), FRAME),
            Some(Hit::Body("under".into()))
        );
        assert_ne!(
            hit_test(&ls, None, (780.0, 225.0), FRAME),
            Some(Hit::Body("under".into()))
        );
    }

    #[test]
    fn a_click_on_empty_canvas_hits_nothing() {
        let mut t = themed();
        t.reference.show = false;
        let ls = layers(&t, OverlayContentKind::Scripture);
        assert_eq!(hit_test(&ls, None, (999.0, 1.0), FRAME), None);
    }
}
