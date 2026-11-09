//! Theme elements (shapes, images and videos on the slide), from
//! `src/lib/overlay-elements.ts`: defaults, the normaliser and list edits.
//!
//! TypeScript draws a random id (`crypto.randomUUID`) when none is given;
//! here the caller always passes the id, so these functions stay pure.

use crate::js;
use crate::normalize::{as_object, clamp_num, safe_bool, safe_color, safe_enum, safe_string};
use crate::overlay::{
    MediaFit, OverlayBox, OverlayElement, OverlayElementKind, OverlayElementStroke,
};
use crate::overlay_boxes::{ELEMENT_BOX_MIN, clamp_overlay_box, overlay_box};
use crate::overlay_shapes::overlay_shape;
use crate::serde_util::Extra;
use serde_json::Value;

/// `OVERLAY_ELEMENT_KINDS`.
pub const OVERLAY_ELEMENT_KINDS: &[OverlayElementKind] = OverlayElementKind::ALL;

/// `MAX_OVERLAY_ELEMENTS`: each element is a layer on every output.
pub const MAX_OVERLAY_ELEMENTS: usize = 40;

/// `isMediaElement`.
pub fn is_media_element(kind: OverlayElementKind) -> bool {
    matches!(kind, OverlayElementKind::Image | OverlayElementKind::Video)
}

/// `isLineElement`: a line or connector, border only.
pub fn is_line_element(kind: OverlayElementKind, shape: Option<&str>) -> bool {
    kind == OverlayElementKind::Shape && overlay_shape(shape).is_some_and(|s| s.line_only)
}

/// `overlayElementLabel`.
pub fn overlay_element_label(kind: OverlayElementKind, shape: Option<&str>) -> &'static str {
    match kind {
        OverlayElementKind::Shape => overlay_shape(shape).map_or("Shape", |s| s.label),
        OverlayElementKind::Rectangle => "Rectangle",
        OverlayElementKind::Ellipse => "Ellipse",
        OverlayElementKind::Image => "Image",
        OverlayElementKind::Video => "Video",
    }
}

fn default_box() -> OverlayBox {
    overlay_box(40.0, 32.22, 20.0, 35.56)
}
fn default_line_box() -> OverlayBox {
    overlay_box(35.0, 45.0, 30.0, 10.0)
}
fn default_media_box() -> OverlayBox {
    overlay_box(35.0, 32.5, 30.0, 35.0)
}

/// `createOverlayElement(kind, mediaPath, id, shape)`.
pub fn create_overlay_element(
    kind: OverlayElementKind,
    media_path: Option<&str>,
    id: &str,
    shape: Option<&str>,
) -> OverlayElement {
    let line = is_line_element(kind, shape);
    OverlayElement {
        id: id.to_owned(),
        kind,
        shape: if kind == OverlayElementKind::Shape {
            shape.map(str::to_owned)
        } else {
            None
        },
        frame: if is_media_element(kind) {
            default_media_box()
        } else if line {
            default_line_box()
        } else {
            default_box()
        },
        above_text: false,
        opacity: 1.0,
        rotation_deg: 0.0,
        flip_x: false,
        flip_y: false,
        fill: "#ffffff".into(),
        fill_opacity: 1.0,
        stroke: if line {
            stroke(true, "#ffffff", 1.0, 6.0)
        } else {
            stroke(false, "#000000", 1.0, 4.0)
        },
        radius_px: 0.0,
        media_path: media_path.map(str::to_owned),
        media_fit: MediaFit::Cover,
        media_loop: true,
        hue: 0.0,
        saturation: 1.0,
        brightness: 1.0,
        contrast: 1.0,
        blur_px: 0.0,
        extra: Extra::new(),
    }
}

fn stroke(enabled: bool, color: &str, opacity: f64, width_px: f64) -> OverlayElementStroke {
    OverlayElementStroke {
        enabled,
        color: color.into(),
        opacity,
        width_px,
        extra: Extra::new(),
    }
}

/// `normalizeOverlayElement`: `None` for a missing id or a shape the catalog
/// no longer has. An unknown kind becomes a rectangle.
pub fn normalize_overlay_element(raw: Option<&Value>) -> Option<OverlayElement> {
    let r = as_object(raw);
    let kind = safe_enum(r.get("kind"), OverlayElementKind::Rectangle);
    let id = safe_string(r.get("id"), "");
    let id = js::trim(&id);
    if id.is_empty() {
        return None;
    }
    let shape = (kind == OverlayElementKind::Shape).then(|| safe_string(r.get("shape"), ""));
    if kind == OverlayElementKind::Shape && overlay_shape(shape.as_deref()).is_none() {
        return None;
    }
    let d = create_overlay_element(kind, None, id, shape.as_deref());
    let b = as_object(r.get("box"));
    let s = as_object(r.get("stroke"));
    let media_path = match r.get("mediaPath") {
        Some(Value::String(path)) if !js::trim(path).is_empty() => Some(js::trim(path).to_owned()),
        _ => None,
    };
    Some(OverlayElement {
        frame: clamp_overlay_box(
            &overlay_box(
                clamp_num(b.get("xPct"), 0.0, 100.0, d.frame.x_pct),
                clamp_num(b.get("yPct"), 0.0, 100.0, d.frame.y_pct),
                clamp_num(b.get("widthPct"), 0.0, 100.0, d.frame.width_pct),
                clamp_num(b.get("heightPct"), 0.0, 100.0, d.frame.height_pct),
            ),
            ELEMENT_BOX_MIN,
        ),
        above_text: safe_bool(r.get("aboveText"), d.above_text),
        opacity: clamp_num(r.get("opacity"), 0.0, 1.0, d.opacity),
        rotation_deg: clamp_num(r.get("rotationDeg"), -180.0, 180.0, d.rotation_deg),
        flip_x: safe_bool(r.get("flipX"), false),
        flip_y: safe_bool(r.get("flipY"), false),
        fill: safe_color(r.get("fill"), &d.fill),
        fill_opacity: clamp_num(r.get("fillOpacity"), 0.0, 1.0, d.fill_opacity),
        stroke: stroke(
            safe_bool(s.get("enabled"), d.stroke.enabled),
            &safe_color(s.get("color"), &d.stroke.color),
            clamp_num(s.get("opacity"), 0.0, 1.0, d.stroke.opacity),
            clamp_num(s.get("widthPx"), 0.0, 200.0, d.stroke.width_px),
        ),
        radius_px: clamp_num(r.get("radiusPx"), 0.0, 1080.0, d.radius_px),
        media_path: if is_media_element(kind) {
            media_path
        } else {
            None
        },
        media_fit: safe_enum(r.get("mediaFit"), d.media_fit),
        media_loop: safe_bool(r.get("mediaLoop"), d.media_loop),
        hue: clamp_num(r.get("hue"), -180.0, 180.0, d.hue),
        saturation: clamp_num(r.get("saturation"), 0.0, 2.0, d.saturation),
        brightness: clamp_num(r.get("brightness"), 0.25, 1.75, d.brightness),
        contrast: clamp_num(r.get("contrast"), 0.25, 1.75, d.contrast),
        blur_px: clamp_num(r.get("blurPx"), 0.0, 40.0, d.blur_px),
        ..d
    })
}

/// `normalizeOverlayElements`: drops invalid entries and repeated ids, keeps
/// at most [`MAX_OVERLAY_ELEMENTS`]. A non-array is an empty list.
pub fn normalize_overlay_elements(raw: Option<&Value>) -> Vec<OverlayElement> {
    let Some(Value::Array(items)) = raw else {
        return Vec::new();
    };
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for item in items {
        let Some(element) = normalize_overlay_element(Some(item)) else {
            continue;
        };
        if !seen.insert(element.id.clone()) {
            continue;
        }
        out.push(element);
        if out.len() == MAX_OVERLAY_ELEMENTS {
            break;
        }
    }
    out
}

// ─── List edits ────────────────────────────────────────────────────────────────

/// `updateOverlayElement`: applies `patch` to every element with `id`.
/// TypeScript's patch cannot change `id` or `kind`; callers keep that rule.
pub fn update_overlay_element(
    elements: &[OverlayElement],
    id: &str,
    patch: impl Fn(&mut OverlayElement),
) -> Vec<OverlayElement> {
    elements
        .iter()
        .map(|el| {
            let mut el = el.clone();
            if el.id == id {
                patch(&mut el);
            }
            el
        })
        .collect()
}

/// `ElementAlignment`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElementAlignment {
    Left,
    Center,
    Right,
    Top,
    Middle,
    Bottom,
}

/// `alignOverlayElement`: aligns the box to the slide, keeping its size.
pub fn align_overlay_element(frame: &OverlayBox, alignment: ElementAlignment) -> OverlayBox {
    let mut b = clamp_overlay_box(frame, ELEMENT_BOX_MIN);
    match alignment {
        ElementAlignment::Left => b.x_pct = 0.0,
        ElementAlignment::Center => b.x_pct = (100.0 - b.width_pct) / 2.0,
        ElementAlignment::Right => b.x_pct = 100.0 - b.width_pct,
        ElementAlignment::Top => b.y_pct = 0.0,
        ElementAlignment::Middle => b.y_pct = (100.0 - b.height_pct) / 2.0,
        ElementAlignment::Bottom => b.y_pct = 100.0 - b.height_pct,
    }
    b
}

/// `removeOverlayElement`.
pub fn remove_overlay_element(elements: &[OverlayElement], id: &str) -> Vec<OverlayElement> {
    elements.iter().filter(|el| el.id != id).cloned().collect()
}

/// `duplicateOverlayElement`: a nudged copy of `id`, placed just above it.
/// Returns the list and the new id, or `None` when `id` is missing or the
/// theme is full.
pub fn duplicate_overlay_element(
    elements: &[OverlayElement],
    id: &str,
    new_element_id: &str,
) -> (Vec<OverlayElement>, Option<String>) {
    let Some(index) = elements.iter().position(|el| el.id == id) else {
        return (elements.to_vec(), None);
    };
    if elements.len() >= MAX_OVERLAY_ELEMENTS {
        return (elements.to_vec(), None);
    }
    let source = &elements[index];
    let mut copy = source.clone();
    copy.id = new_element_id.to_owned();
    copy.frame = clamp_overlay_box(
        &overlay_box(
            source.frame.x_pct + 2.0,
            source.frame.y_pct + 2.0,
            source.frame.width_pct,
            source.frame.height_pct,
        ),
        ELEMENT_BOX_MIN,
    );
    let mut next = elements.to_vec();
    next.insert(index + 1, copy);
    (next, Some(new_element_id.to_owned()))
}

/// Where `reorderOverlayElement` moves an element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReorderTarget {
    Forward,
    Backward,
    Front,
    Back,
}

/// `reorderOverlayElement`: moves `id` within its own group (under or over the text).
pub fn reorder_overlay_element(
    elements: &[OverlayElement],
    id: &str,
    to: ReorderTarget,
) -> Vec<OverlayElement> {
    let Some(target_index) = elements.iter().position(|el| el.id == id) else {
        return elements.to_vec();
    };
    let above = elements[target_index].above_text;
    let group: Vec<usize> = (0..elements.len())
        .filter(|&i| elements[i].above_text == above)
        .collect();
    let from = group
        .iter()
        .position(|&i| i == target_index)
        .expect("the target is in its own group");
    let mut rest: Vec<usize> = group.into_iter().filter(|&i| i != target_index).collect();
    let at = match to {
        ReorderTarget::Front => rest.len(),
        ReorderTarget::Back => 0,
        ReorderTarget::Forward => rest.len().min(from + 1),
        ReorderTarget::Backward => from.saturating_sub(1),
    };
    rest.insert(at, target_index);
    elements
        .iter()
        .filter(|el| el.above_text != above)
        .cloned()
        .chain(rest.into_iter().map(|i| elements[i].clone()))
        .collect()
}
