//! The shape catalog, from `src/lib/overlay-shapes.ts`. Every preset is one
//! SVG path in a 100×100 box, stretched to the element's box when drawn.
//! Computed paths use JavaScript rounding and number printing, so their text
//! matches the TypeScript exactly.

use crate::js::{math_round, number_to_string};
use serde::Serialize;
use std::collections::HashMap;
use std::f64::consts::{FRAC_1_SQRT_2, PI};
use std::sync::LazyLock;

/// `OverlayShapeDef`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayShapeDef {
    pub id: &'static str,
    pub label: &'static str,
    pub path: String,
    /// Lines and connectors: drawn with the border only, never filled.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub line_only: bool,
}

/// `OverlayShapeGroup`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OverlayShapeGroup {
    pub label: &'static str,
    pub shapes: Vec<OverlayShapeDef>,
}

/// `r`: two decimals, printed as JavaScript prints numbers.
fn r(n: f64) -> String {
    number_to_string(math_round(n * 100.0) / 100.0)
}

/// A regular polygon, first corner straight up.
fn polygon(sides: u32) -> String {
    let sides_f = f64::from(sides);
    let points: Vec<String> = (0..sides)
        .map(|i| {
            let a = -PI / 2.0 + (f64::from(i) * 2.0 * PI) / sides_f;
            format!("{} {}", r(50.0 + 50.0 * a.cos()), r(50.0 + 50.0 * a.sin()))
        })
        .collect();
    format!("M{} Z", points.join(" L"))
}

/// A star with `points` tips; `inner` is the valley radius as a share of the tip radius.
fn star(points: u32, inner: f64) -> String {
    let points_f = f64::from(points);
    let corners: Vec<String> = (0..points * 2)
        .map(|i| {
            let a = -PI / 2.0 + (f64::from(i) * PI) / points_f;
            let radius = if i % 2 == 0 { 50.0 } else { 50.0 * inner };
            format!(
                "{} {}",
                r(50.0 + radius * a.cos()),
                r(50.0 + radius * a.sin())
            )
        })
        .collect();
    format!("M{} Z", corners.join(" L"))
}

/// A disc with separate triangular rays.
fn sun(rays: u32) -> String {
    let rays_f = f64::from(rays);
    let disc = "M24 50 A26 26 0 1 0 76 50 A26 26 0 1 0 24 50 Z".to_owned();
    let p = |angle: f64, radius: f64| {
        format!(
            "{} {}",
            r(50.0 + radius * angle.sin()),
            r(50.0 - radius * angle.cos())
        )
    };
    let spikes = (0..rays).map(|i| {
        let a = (f64::from(i) * 2.0 * PI) / rays_f;
        let half = PI / rays_f / 1.8;
        format!(
            "M{} L{} L{} Z",
            p(a - half, 32.0),
            p(a, 50.0),
            p(a + half, 32.0)
        )
    });
    std::iter::once(disc)
        .chain(spikes)
        .collect::<Vec<_>>()
        .join(" ")
}

/// A ring with a diagonal bar: the two openings are holes in the disc.
fn no_symbol() -> String {
    const R: f64 = 34.0;
    const D: f64 = 7.0;
    // Math.SQRT1_2.
    let s2 = FRAC_1_SQRT_2;
    let along = (R * R - D * D).sqrt();
    let piece = |side: f64| {
        let nx = s2 * side;
        let ny = -s2 * side;
        let ax = 50.0 + D * nx - along * s2;
        let ay = 50.0 + D * ny - along * s2;
        let bx = 50.0 + D * nx + along * s2;
        let by = 50.0 + D * ny + along * s2;
        format!(
            "M{} {} L{} {} A34 34 0 0 {} {} {} Z",
            r(ax),
            r(ay),
            r(bx),
            r(by),
            if side == 1.0 { 0 } else { 1 },
            r(ax),
            r(ay)
        )
    };
    format!(
        "M0 50 A50 50 0 1 0 100 50 A50 50 0 1 0 0 50 Z {} {}",
        piece(1.0),
        piece(-1.0)
    )
}

fn s(path: &str) -> String {
    path.to_owned()
}

fn shape(id: &'static str, label: &'static str, path: String, line_only: bool) -> OverlayShapeDef {
    OverlayShapeDef {
        id,
        label,
        path,
        line_only,
    }
}

fn group(label: &'static str, shapes: Vec<OverlayShapeDef>) -> OverlayShapeGroup {
    OverlayShapeGroup { label, shapes }
}

/// `OVERLAY_SHAPE_GROUPS`.
pub static OVERLAY_SHAPE_GROUPS: LazyLock<Vec<OverlayShapeGroup>> = LazyLock::new(|| {
    vec![
        group(
            "Lines",
            vec![
                shape("line", "Line", s("M0 50 H100"), true),
                shape("line-diagonal", "Diagonal line", s("M0 0 L100 100"), true),
                shape(
                    "line-arrow",
                    "Arrow",
                    s("M0 50 H100 M86 36 L100 50 L86 64"),
                    true,
                ),
                shape(
                    "line-double-arrow",
                    "Double arrow",
                    s("M0 50 H100 M14 36 L0 50 L14 64 M86 36 L100 50 L86 64"),
                    true,
                ),
                shape("line-elbow", "Elbow", s("M0 0 H50 V100 H100"), true),
                shape("line-curve", "Curve", s("M0 100 C30 100 70 0 100 0"), true),
                shape(
                    "line-wave",
                    "Wavy line",
                    s("M0 50 C12.5 0 37.5 0 50 50 C62.5 100 87.5 100 100 50"),
                    true,
                ),
            ],
        ),
        group(
            "Rectangles",
            vec![
                shape(
                    "snip-corner",
                    "Snipped corner",
                    s("M0 0 H80 L100 20 V100 H0 Z"),
                    false,
                ),
                shape(
                    "snip-corners",
                    "Snipped corners",
                    s("M20 0 H80 L100 20 V100 H0 V20 Z"),
                    false,
                ),
                shape(
                    "snip-diagonal",
                    "Snipped diagonal",
                    s("M0 0 H80 L100 20 V100 H20 L0 80 Z"),
                    false,
                ),
                shape(
                    "round-top",
                    "Round top",
                    s("M0 100 V30 C0 10 10 0 30 0 H70 C90 0 100 10 100 30 V100 Z"),
                    false,
                ),
                shape(
                    "frame",
                    "Frame",
                    s("M0 0 H100 V100 H0 Z M12 12 V88 H88 V12 Z"),
                    false,
                ),
                shape(
                    "half-frame",
                    "Corner",
                    s("M0 0 H100 L82 18 H18 V82 L0 100 Z"),
                    false,
                ),
                shape(
                    "l-shape",
                    "L-shape",
                    s("M0 0 H30 V70 H100 V100 H0 Z"),
                    false,
                ),
            ],
        ),
        group(
            "Basic shapes",
            vec![
                shape("triangle", "Triangle", s("M50 0 L100 100 L0 100 Z"), false),
                shape(
                    "right-triangle",
                    "Right triangle",
                    s("M0 0 L100 100 L0 100 Z"),
                    false,
                ),
                shape(
                    "parallelogram",
                    "Parallelogram",
                    s("M25 0 H100 L75 100 H0 Z"),
                    false,
                ),
                shape(
                    "trapezoid",
                    "Trapezoid",
                    s("M20 0 H80 L100 100 H0 Z"),
                    false,
                ),
                shape(
                    "diamond",
                    "Diamond",
                    s("M50 0 L100 50 L50 100 L0 50 Z"),
                    false,
                ),
                shape("pentagon", "Pentagon", polygon(5), false),
                shape(
                    "hexagon",
                    "Hexagon",
                    s("M25 0 H75 L100 50 L75 100 H25 L0 50 Z"),
                    false,
                ),
                shape("heptagon", "Heptagon", polygon(7), false),
                shape(
                    "octagon",
                    "Octagon",
                    s("M30 0 H70 L100 30 V70 L70 100 H30 L0 70 V30 Z"),
                    false,
                ),
                shape("decagon", "Decagon", polygon(10), false),
                shape("dodecagon", "Dodecagon", polygon(12), false),
                shape(
                    "donut",
                    "Donut",
                    s(
                        "M0 50 A50 50 0 1 0 100 50 A50 50 0 1 0 0 50 Z M25 50 A25 25 0 1 1 75 50 A25 25 0 1 1 25 50 Z",
                    ),
                    false,
                ),
                shape("pie", "Pie", s("M50 50 V0 A50 50 0 1 1 0 50 Z"), false),
                shape(
                    "chord",
                    "Half circle",
                    s("M0 50 A50 50 0 0 1 100 50 Z"),
                    false,
                ),
                shape(
                    "arch",
                    "Arch",
                    s("M0 100 V50 A50 50 0 0 1 100 50 V100 H78 V50 A28 28 0 0 0 22 50 V100 Z"),
                    false,
                ),
                shape(
                    "teardrop",
                    "Teardrop",
                    s("M50 0 H100 V50 A50 50 0 1 1 50 0 Z"),
                    false,
                ),
                shape(
                    "cylinder",
                    "Cylinder",
                    s("M0 15 A50 15 0 0 1 100 15 V85 A50 15 0 0 1 0 85 Z"),
                    false,
                ),
                shape(
                    "cube",
                    "Cube",
                    s("M0 25 L25 0 H100 V75 L75 100 H0 Z"),
                    false,
                ),
                shape(
                    "plaque",
                    "Plaque",
                    s(
                        "M15 0 H85 A15 15 0 0 0 100 15 V85 A15 15 0 0 0 85 100 H15 A15 15 0 0 0 0 85 V15 A15 15 0 0 0 15 0 Z",
                    ),
                    false,
                ),
                shape(
                    "heart",
                    "Heart",
                    s(
                        "M50 95 C20 72 0 54 0 30 C0 13 13 2 28 2 C39 2 46 8 50 17 C54 8 61 2 72 2 C87 2 100 13 100 30 C100 54 80 72 50 95 Z",
                    ),
                    false,
                ),
                shape(
                    "lightning",
                    "Lightning",
                    s("M38 0 H78 L56 38 H82 L22 100 L40 56 H18 Z"),
                    false,
                ),
                shape("sun", "Sun", sun(10), false),
                shape(
                    "moon",
                    "Moon",
                    s("M75 4 A48 48 0 1 0 75 96 A40 46 0 1 1 75 4 Z"),
                    false,
                ),
                shape(
                    "cloud",
                    "Cloud",
                    s(
                        "M24 88 C8 88 0 78 0 66 C0 54 9 46 20 46 C20 30 32 20 46 20 C56 20 64 25 68 33 C72 30 77 28 82 28 C93 28 100 37 100 48 C100 50 100 52 99 54 C100 58 100 61 100 64 C100 77 91 88 78 88 Z",
                    ),
                    false,
                ),
                shape(
                    "smiley",
                    "Smiley",
                    s(
                        "M0 50 A50 50 0 1 0 100 50 A50 50 0 1 0 0 50 Z M30 35 A6 6 0 1 0 42 35 A6 6 0 1 0 30 35 Z M58 35 A6 6 0 1 0 70 35 A6 6 0 1 0 58 35 Z M26 60 Q50 84 74 60 Q50 74 26 60 Z",
                    ),
                    false,
                ),
                shape("no-symbol", "No symbol", no_symbol(), false),
                shape(
                    "brackets",
                    "Brackets",
                    s("M20 0 H35 V8 H28 V92 H35 V100 H20 Z M80 0 H65 V8 H72 V92 H65 V100 H80 Z"),
                    false,
                ),
            ],
        ),
        group(
            "Block arrows",
            vec![
                shape(
                    "arrow-right",
                    "Right arrow",
                    s("M0 30 H60 V5 L100 50 L60 95 V70 H0 Z"),
                    false,
                ),
                shape(
                    "arrow-left",
                    "Left arrow",
                    s("M100 30 H40 V5 L0 50 L40 95 V70 H100 Z"),
                    false,
                ),
                shape(
                    "arrow-up",
                    "Up arrow",
                    s("M30 100 V40 H5 L50 0 L95 40 H70 V100 Z"),
                    false,
                ),
                shape(
                    "arrow-down",
                    "Down arrow",
                    s("M30 0 V60 H5 L50 100 L95 60 H70 V0 Z"),
                    false,
                ),
                shape(
                    "arrow-left-right",
                    "Left-right arrow",
                    s("M0 50 L25 10 V30 H75 V10 L100 50 L75 90 V70 H25 V90 Z"),
                    false,
                ),
                shape(
                    "arrow-up-down",
                    "Up-down arrow",
                    s("M50 0 L90 25 H70 V75 H90 L50 100 L10 75 H30 V25 H10 Z"),
                    false,
                ),
                shape(
                    "arrow-quad",
                    "Four-way arrow",
                    s(
                        "M50 0 L68 18 H58 V42 H82 V32 L100 50 L82 68 V58 H58 V82 H68 L50 100 L32 82 H42 V58 H18 V68 L0 50 L18 32 V42 H42 V18 H32 Z",
                    ),
                    false,
                ),
                shape(
                    "arrow-notched",
                    "Notched arrow",
                    s("M0 30 H60 V5 L100 50 L60 95 V70 H0 L12 50 Z"),
                    false,
                ),
                shape(
                    "arrow-striped",
                    "Striped arrow",
                    s(
                        "M0 30 H6 V70 H0 Z M12 30 H18 V70 H12 Z M24 30 H60 V5 L100 50 L60 95 V70 H24 Z",
                    ),
                    false,
                ),
                shape(
                    "arrow-bent",
                    "Bent arrow",
                    s(
                        "M0 100 V45 C0 30 10 20 25 20 H65 V0 L100 32 L65 64 V44 H30 C27 44 25 46 25 49 V100 Z",
                    ),
                    false,
                ),
                shape(
                    "arrow-u-turn",
                    "U-turn arrow",
                    s(
                        "M0 100 V40 A40 40 0 0 1 80 40 V55 H100 L70 90 L40 55 H58 V40 A18 18 0 0 0 22 40 V100 Z",
                    ),
                    false,
                ),
                shape(
                    "pentagon-arrow",
                    "Pentagon",
                    s("M0 0 H75 L100 50 L75 100 H0 Z"),
                    false,
                ),
                shape(
                    "chevron",
                    "Chevron",
                    s("M0 0 H75 L100 50 L75 100 H0 L25 50 Z"),
                    false,
                ),
                shape(
                    "chevron-left",
                    "Left chevron",
                    s("M100 0 H25 L0 50 L25 100 H100 L75 50 Z"),
                    false,
                ),
            ],
        ),
        group(
            "Equation shapes",
            vec![
                shape(
                    "plus",
                    "Plus",
                    s("M38 0 H62 V38 H100 V62 H62 V100 H38 V62 H0 V38 H38 Z"),
                    false,
                ),
                shape("minus", "Minus", s("M0 38 H100 V62 H0 Z"), false),
                shape(
                    "multiply",
                    "Multiply",
                    s(
                        "M17 0 L50 33 L83 0 L100 17 L67 50 L100 83 L83 100 L50 67 L17 100 L0 83 L33 50 L0 17 Z",
                    ),
                    false,
                ),
                shape(
                    "divide",
                    "Divide",
                    s(
                        "M0 40 H100 V60 H0 Z M38 16 A12 12 0 1 0 62 16 A12 12 0 1 0 38 16 Z M38 84 A12 12 0 1 0 62 84 A12 12 0 1 0 38 84 Z",
                    ),
                    false,
                ),
                shape(
                    "equal",
                    "Equal",
                    s("M0 20 H100 V42 H0 Z M0 58 H100 V80 H0 Z"),
                    false,
                ),
            ],
        ),
        group(
            "Flowchart",
            vec![
                shape(
                    "flow-document",
                    "Document",
                    s("M0 0 H100 V84 C75 66 50 100 25 92 C13 88 6 85 0 84 Z"),
                    false,
                ),
                shape(
                    "flow-terminator",
                    "Terminator",
                    s("M25 0 H75 A25 50 0 0 1 75 100 H25 A25 50 0 0 1 25 0 Z"),
                    false,
                ),
                shape(
                    "flow-delay",
                    "Delay",
                    s("M0 0 H50 A50 50 0 0 1 50 100 H0 Z"),
                    false,
                ),
                shape(
                    "flow-manual-input",
                    "Manual input",
                    s("M0 25 L100 0 V100 H0 Z"),
                    false,
                ),
                shape(
                    "flow-manual-operation",
                    "Manual operation",
                    s("M0 0 H100 L80 100 H20 Z"),
                    false,
                ),
                shape(
                    "flow-off-page",
                    "Off-page",
                    s("M0 0 H100 V75 L50 100 L0 75 Z"),
                    false,
                ),
                shape("flow-card", "Card", s("M20 0 H100 V100 H0 V20 Z"), false),
                shape(
                    "flow-preparation",
                    "Preparation",
                    s("M20 0 H80 L100 50 L80 100 H20 L0 50 Z"),
                    false,
                ),
                shape("flow-merge", "Merge", s("M0 0 H100 L50 100 Z"), false),
                shape(
                    "flow-collate",
                    "Collate",
                    s("M0 0 H100 L0 100 H100 Z"),
                    false,
                ),
            ],
        ),
        group(
            "Stars and banners",
            vec![
                shape("star-4", "4-point star", star(4, 0.38), false),
                shape("star-5", "5-point star", star(5, 0.4), false),
                shape("star-6", "6-point star", star(6, 0.55), false),
                shape("star-7", "7-point star", star(7, 0.6), false),
                shape("star-8", "8-point star", star(8, 0.62), false),
                shape("star-10", "10-point star", star(10, 0.7), false),
                shape("star-12", "12-point star", star(12, 0.75), false),
                shape("star-16", "16-point star", star(16, 0.8), false),
                shape("star-24", "24-point star", star(24, 0.85), false),
                shape("star-32", "32-point star", star(32, 0.88), false),
                shape(
                    "burst",
                    "Explosion",
                    s(
                        "M50 0 L58 26 L82 8 L74 36 L100 34 L80 52 L98 72 L70 68 L72 98 L52 76 L34 100 L32 72 L4 82 L22 58 L0 42 L26 36 L14 10 L40 24 Z",
                    ),
                    false,
                ),
                shape(
                    "ribbon",
                    "Ribbon",
                    s("M0 20 H100 L88 50 L100 80 H0 L12 50 Z"),
                    false,
                ),
                shape(
                    "banner-wave",
                    "Wave",
                    s(
                        "M0 15 C25 -5 50 35 75 15 C88 5 95 10 100 15 V85 C75 105 50 65 25 85 C12 95 5 90 0 85 Z",
                    ),
                    false,
                ),
                shape(
                    "banner-double-wave",
                    "Double wave",
                    s(
                        "M0 15 C17 0 33 30 50 15 C67 0 83 30 100 15 V85 C83 100 67 70 50 85 C33 100 17 70 0 85 Z",
                    ),
                    false,
                ),
                shape(
                    "scroll",
                    "Scroll",
                    s(
                        "M10 0 H100 V10 C100 15 95 15 90 15 V90 C90 96 86 100 80 100 H0 V90 C0 85 5 85 10 85 Z",
                    ),
                    false,
                ),
            ],
        ),
        group(
            "Callouts",
            vec![
                shape(
                    "callout-rect",
                    "Speech rectangle",
                    s("M0 0 H100 V72 H42 L20 100 L26 72 H0 Z"),
                    false,
                ),
                shape(
                    "callout-round-rect",
                    "Speech rounded",
                    s(
                        "M12 0 H88 C95 0 100 5 100 12 V60 C100 67 95 72 88 72 H42 L20 100 L26 72 H12 C5 72 0 67 0 60 V12 C0 5 5 0 12 0 Z",
                    ),
                    false,
                ),
                shape(
                    "callout-oval",
                    "Speech oval",
                    s(
                        "M50 0 C78 0 100 16 100 37 C100 58 78 74 50 74 C44 74 39 73 34 72 L15 98 L20 66 C8 59 0 49 0 37 C0 16 22 0 50 0 Z",
                    ),
                    false,
                ),
                shape(
                    "callout-cloud",
                    "Thought",
                    s(
                        "M22 70 C8 70 0 62 0 52 C0 43 7 37 15 36 C14 22 25 12 40 12 C47 6 56 4 64 8 C72 4 84 6 90 14 C97 18 100 26 100 34 C100 46 92 56 80 58 C76 66 66 70 56 68 C48 72 34 74 22 70 Z M14 84 A6 6 0 1 0 26 84 A6 6 0 1 0 14 84 Z M4 96 A4 4 0 1 0 12 96 A4 4 0 1 0 4 96 Z",
                    ),
                    false,
                ),
            ],
        ),
    ]
});

static BY_ID: LazyLock<HashMap<&'static str, &'static OverlayShapeDef>> = LazyLock::new(|| {
    let mut map = HashMap::new();
    for shape in OVERLAY_SHAPE_GROUPS.iter().flat_map(|g| &g.shapes) {
        // A later duplicate wins, as `new Map(entries)` does.
        map.insert(shape.id, shape);
    }
    map
});

/// `OVERLAY_SHAPE_IDS`, in catalog order.
pub static OVERLAY_SHAPE_IDS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    let mut seen = std::collections::HashSet::new();
    OVERLAY_SHAPE_GROUPS
        .iter()
        .flat_map(|g| &g.shapes)
        .map(|shape| shape.id)
        .filter(|id| seen.insert(*id))
        .collect()
});

/// `overlayShape`: the catalog entry for `id`, or `None` for an unknown or empty id.
pub fn overlay_shape(id: Option<&str>) -> Option<&'static OverlayShapeDef> {
    id.filter(|id| !id.is_empty())
        .and_then(|id| BY_ID.get(id).copied())
}
