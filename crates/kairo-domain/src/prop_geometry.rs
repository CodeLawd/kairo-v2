//! Dragging a prop object on the Props editor stage, from `src/lib/prop-geometry.ts`.

use crate::js::clamp;

/// `MIN_PROP_OBJECT_SIZE`: the smallest share of the frame an object can be.
pub const MIN_PROP_OBJECT_SIZE: f64 = 0.02;

/// `PropBox`: position and size as shares of the frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PropBox {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// `PropCorner`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropCorner {
    Nw,
    Ne,
    Sw,
    Se,
}

/// `PropGesture`. The pointer start and object id ride along for callers;
/// the geometry uses only the kind, corner and origin.
#[derive(Clone, Debug, PartialEq)]
pub enum PropGesture {
    Move {
        object_id: String,
        start_x: f64,
        start_y: f64,
        origin: PropBox,
    },
    Resize {
        object_id: String,
        corner: PropCorner,
        start_x: f64,
        start_y: f64,
        origin: PropBox,
    },
}

/// `applyPropGesture`: moves or resizes by a pointer delta in shares of the
/// frame. The object stays on the frame; a resize stops at the minimum size
/// and the opposite corner holds still.
pub fn apply_prop_gesture(gesture: &PropGesture, dx: f64, dy: f64) -> PropBox {
    let (o, corner) = match gesture {
        PropGesture::Move { origin: o, .. } => {
            return PropBox {
                x: clamp(o.x + dx, 0.0, 1.0 - o.w),
                y: clamp(o.y + dy, 0.0, 1.0 - o.h),
                ..*o
            };
        }
        PropGesture::Resize { origin, corner, .. } => (*origin, *corner),
    };
    let PropBox {
        mut x,
        mut y,
        mut w,
        mut h,
    } = o;
    use PropCorner::*;
    if matches!(corner, Ne | Se) {
        w = clamp(o.w + dx, MIN_PROP_OBJECT_SIZE, 1.0 - o.x);
    }
    if matches!(corner, Nw | Sw) {
        let right = o.x + o.w;
        x = clamp(o.x + dx, 0.0, right - MIN_PROP_OBJECT_SIZE);
        w = right - x;
    }
    if matches!(corner, Sw | Se) {
        h = clamp(o.h + dy, MIN_PROP_OBJECT_SIZE, 1.0 - o.y);
    }
    if matches!(corner, Nw | Ne) {
        let bottom = o.y + o.h;
        y = clamp(o.y + dy, 0.0, bottom - MIN_PROP_OBJECT_SIZE);
        h = bottom - y;
    }
    PropBox { x, y, w, h }
}
