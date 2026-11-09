//! Box maths for text layers and theme elements, from `src/lib/overlay-boxes.ts`.
//! Every box is in % of the frame. Arithmetic follows JavaScript, so `NaN`
//! inputs propagate as they do there (see [`crate::js::clamp`]).

use crate::js::{self, clamp, math_max, math_min};
use crate::overlay::{OverlayBox, OverlayLayoutPosition, OverlayTheme, ReferencePosition};
use crate::serde_util::Extra;

pub const MIN_BOX_WIDTH_PCT: f64 = 8.0;
pub const MIN_BOX_HEIGHT_PCT: f64 = 6.0;
pub const VERSE_HEIGHT_PCT: f64 = 24.0;
pub const REFERENCE_HEIGHT_PCT: f64 = 10.0;
pub const STACK_GAP_PCT: f64 = 2.0;
pub const FRAME_MARGIN_PCT: f64 = 6.0;

/// `ResizeHandle`: a compass point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizeHandle {
    N,
    S,
    E,
    W,
    Ne,
    Nw,
    Se,
    Sw,
}

impl ResizeHandle {
    pub const ALL: [ResizeHandle; 8] = [
        Self::N,
        Self::S,
        Self::E,
        Self::W,
        Self::Ne,
        Self::Nw,
        Self::Se,
        Self::Sw,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::N => "n",
            Self::S => "s",
            Self::E => "e",
            Self::W => "w",
            Self::Ne => "ne",
            Self::Nw => "nw",
            Self::Se => "se",
            Self::Sw => "sw",
        }
    }

    /// `handle.includes(c)`.
    fn has(self, c: char) -> bool {
        self.as_str().contains(c)
    }
}

/// `BoxMinimum`: the smallest a layer may shrink to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxMinimum {
    pub width_pct: f64,
    pub height_pct: f64,
}

/// `TEXT_BOX_MIN`: text boxes keep room for a line of text.
pub const TEXT_BOX_MIN: BoxMinimum = BoxMinimum {
    width_pct: MIN_BOX_WIDTH_PCT,
    height_pct: MIN_BOX_HEIGHT_PCT,
};
/// `ELEMENT_BOX_MIN`: shapes and media can be thin.
pub const ELEMENT_BOX_MIN: BoxMinimum = BoxMinimum {
    width_pct: 0.5,
    height_pct: 0.5,
};

/// A box with no unknown keys.
pub fn overlay_box(x_pct: f64, y_pct: f64, width_pct: f64, height_pct: f64) -> OverlayBox {
    OverlayBox {
        x_pct,
        y_pct,
        width_pct,
        height_pct,
        extra: Extra::new(),
    }
}

/// `clampOverlayBox`. The result carries no unknown keys, as in TypeScript.
pub fn clamp_overlay_box(b: &OverlayBox, min: BoxMinimum) -> OverlayBox {
    let width_pct = clamp(b.width_pct, min.width_pct, 100.0);
    let height_pct = clamp(b.height_pct, min.height_pct, 100.0);
    overlay_box(
        clamp(b.x_pct, 0.0, 100.0 - width_pct),
        clamp(b.y_pct, 0.0, 100.0 - height_pct),
        width_pct,
        height_pct,
    )
}

/// `moveOverlayBox`.
pub fn move_overlay_box(b: &OverlayBox, dx_pct: f64, dy_pct: f64, min: BoxMinimum) -> OverlayBox {
    clamp_overlay_box(
        &overlay_box(
            b.x_pct + dx_pct,
            b.y_pct + dy_pct,
            b.width_pct,
            b.height_pct,
        ),
        min,
    )
}

/// `resizeOverlayBox`.
pub fn resize_overlay_box(
    b: &OverlayBox,
    handle: ResizeHandle,
    dx_pct: f64,
    dy_pct: f64,
    min: BoxMinimum,
) -> OverlayBox {
    let mut left = b.x_pct;
    let mut top = b.y_pct;
    let mut right = b.x_pct + b.width_pct;
    let mut bottom = b.y_pct + b.height_pct;

    if handle.has('e') {
        right += dx_pct;
    }
    if handle.has('w') {
        left += dx_pct;
    }
    if handle.has('s') {
        bottom += dy_pct;
    }
    if handle.has('n') {
        top += dy_pct;
    }

    if right - left < min.width_pct {
        if handle.has('w') {
            left = right - min.width_pct;
        } else {
            right = left + min.width_pct;
        }
    }
    if bottom - top < min.height_pct {
        if handle.has('n') {
            top = bottom - min.height_pct;
        } else {
            bottom = top + min.height_pct;
        }
    }

    left = clamp(left, 0.0, 100.0);
    right = clamp(right, 0.0, 100.0);
    top = clamp(top, 0.0, 100.0);
    bottom = clamp(bottom, 0.0, 100.0);

    if right - left < min.width_pct {
        if handle.has('w') {
            left = clamp(right - min.width_pct, 0.0, 100.0 - min.width_pct);
        } else {
            right = clamp(left + min.width_pct, min.width_pct, 100.0);
        }
    }
    if bottom - top < min.height_pct {
        if handle.has('n') {
            top = clamp(bottom - min.height_pct, 0.0, 100.0 - min.height_pct);
        } else {
            bottom = clamp(top + min.height_pct, min.height_pct, 100.0);
        }
    }

    clamp_overlay_box(&overlay_box(left, top, right - left, bottom - top), min)
}

/// `resizeOverlayBoxKeepingAspect`: Shift while dragging a handle.
pub fn resize_overlay_box_keeping_aspect(
    b: &OverlayBox,
    handle: ResizeHandle,
    dx_pct: f64,
    dy_pct: f64,
    min: BoxMinimum,
) -> OverlayBox {
    let width = b.width_pct
        + if handle.has('e') {
            dx_pct
        } else if handle.has('w') {
            -dx_pct
        } else {
            0.0
        };
    let height = b.height_pct
        + if handle.has('s') {
            dy_pct
        } else if handle.has('n') {
            -dy_pct
        } else {
            0.0
        };
    let horizontal = handle.has('e') || handle.has('w');
    let vertical = handle.has('n') || handle.has('s');
    let mut scale = if horizontal && vertical {
        math_max(width / b.width_pct, height / b.height_pct)
    } else if horizontal {
        width / b.width_pct
    } else {
        height / b.height_pct
    };
    scale = math_max(
        math_max(scale, min.width_pct / b.width_pct),
        min.height_pct / b.height_pct,
    );
    scale = math_min(math_min(scale, 100.0 / b.width_pct), 100.0 / b.height_pct);

    let w = b.width_pct * scale;
    let h = b.height_pct * scale;
    let left = if handle.has('w') {
        b.x_pct + b.width_pct - w
    } else if horizontal {
        b.x_pct
    } else {
        b.x_pct + (b.width_pct - w) / 2.0
    };
    let top = if handle.has('n') {
        b.y_pct + b.height_pct - h
    } else if vertical {
        b.y_pct
    } else {
        b.y_pct + (b.height_pct - h) / 2.0
    };
    clamp_overlay_box(&overlay_box(left, top, w, h), min)
}

/// Verse and reference boxes for a preset.
#[derive(Clone, Debug, PartialEq)]
pub struct PresetBoxes {
    pub verse: OverlayBox,
    pub reference: OverlayBox,
}

/// `boxesForLayoutPreset`.
pub fn boxes_for_layout_preset(
    position: OverlayLayoutPosition,
    reference_position: ReferencePosition,
    max_width_pct: f64,
    show_reference: bool,
) -> PresetBoxes {
    let full = position == OverlayLayoutPosition::Full;
    let width_pct = if full {
        100.0
    } else {
        clamp(max_width_pct, 20.0, 100.0)
    };
    let x_pct = (100.0 - width_pct) / 2.0;
    let verse_h = if full {
        if show_reference { 88.0 } else { 100.0 }
    } else {
        VERSE_HEIGHT_PCT
    };
    let ref_h = if full { 12.0 } else { REFERENCE_HEIGHT_PCT };
    let gap = if full { 0.0 } else { STACK_GAP_PCT };
    let pair_height = verse_h + if show_reference { gap + ref_h } else { 0.0 };

    let pair_top = match position {
        OverlayLayoutPosition::Full => 0.0,
        OverlayLayoutPosition::Top => FRAME_MARGIN_PCT,
        OverlayLayoutPosition::Center => (100.0 - pair_height) / 2.0,
        OverlayLayoutPosition::LowerThird => 100.0 - FRAME_MARGIN_PCT - pair_height,
    };

    if show_reference && reference_position == ReferencePosition::Above {
        return PresetBoxes {
            reference: clamp_overlay_box(
                &overlay_box(x_pct, pair_top, width_pct, ref_h),
                TEXT_BOX_MIN,
            ),
            verse: clamp_overlay_box(
                &overlay_box(x_pct, pair_top + ref_h + gap, width_pct, verse_h),
                TEXT_BOX_MIN,
            ),
        };
    }

    PresetBoxes {
        verse: clamp_overlay_box(
            &overlay_box(x_pct, pair_top, width_pct, verse_h),
            TEXT_BOX_MIN,
        ),
        reference: clamp_overlay_box(
            &overlay_box(x_pct, pair_top + verse_h + gap, width_pct, ref_h),
            TEXT_BOX_MIN,
        ),
    }
}

/// `placeReferenceAgainstVerse`.
pub fn place_reference_against_verse(
    verse: &OverlayBox,
    reference: &OverlayBox,
    position: ReferencePosition,
) -> OverlayBox {
    let y_pct = match position {
        ReferencePosition::Above => verse.y_pct - reference.height_pct - STACK_GAP_PCT,
        ReferencePosition::Below => verse.y_pct + verse.height_pct + STACK_GAP_PCT,
    };
    clamp_overlay_box(
        &overlay_box(verse.x_pct, y_pct, verse.width_pct, reference.height_pct),
        TEXT_BOX_MIN,
    )
}

/// `applyLayoutPreset`. `max_width_pct` of `None` keeps the theme's own.
pub fn apply_layout_preset(
    theme: &OverlayTheme,
    position: OverlayLayoutPosition,
    max_width_pct: Option<f64>,
) -> OverlayTheme {
    let max_width_pct = max_width_pct.unwrap_or(theme.layout.max_width_pct);
    let boxes = boxes_for_layout_preset(
        position,
        theme.reference.position,
        max_width_pct,
        theme.reference.show,
    );
    let mut next = theme.clone();
    next.verse.frame = boxes.verse;
    next.reference.text.frame = boxes.reference;
    next.layout.position = position;
    next.layout.max_width_pct = max_width_pct;
    next
}

/// `overlayBoxStyle`: the CSS that places a box.
pub fn overlay_box_style(b: &OverlayBox) -> String {
    [
        "position:absolute;".to_owned(),
        format!("left:{}%;", js::number_to_string(b.x_pct)),
        format!("top:{}%;", js::number_to_string(b.y_pct)),
        format!("width:{}%;", js::number_to_string(b.width_pct)),
        format!("height:{}%;", js::number_to_string(b.height_pct)),
        "box-sizing:border-box;".to_owned(),
        "overflow:hidden;".to_owned(),
    ]
    .join(" ")
}
