//! The prompt 011 demonstration on `overlay-boxes.ts`: 12,000 generated inputs
//! across six exports. `applyLayoutPreset` and `overlayBoxStyle` are left to
//! the real port, as the latter needs JavaScript number formatting.
//! This port exists only for the demonstration. Prompt 013 and later own real ports.

use kairo_oracle::{assert_all_match, generate, number, undefined};
use proptest::prelude::*;
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug)]
struct Box2 {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

type Min = (f64, f64);
const TEXT: Min = (8.0, 6.0);
const ELEMENT: Min = (0.5, 0.5);

// JavaScript's Math.min and Math.max propagate NaN. Rust's f64::min and max do not.
fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}
fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.min(b)
    }
}
fn clamp(n: f64, min: f64, max: f64) -> f64 {
    js_min(max, js_max(n, min))
}

fn clamp_box(b: Box2, min: Min) -> Box2 {
    let w = clamp(b.w, min.0, 100.0);
    let h = clamp(b.h, min.1, 100.0);
    Box2 {
        x: clamp(b.x, 0.0, 100.0 - w),
        y: clamp(b.y, 0.0, 100.0 - h),
        w,
        h,
    }
}

fn move_box(b: Box2, dx: f64, dy: f64, min: Min) -> Box2 {
    clamp_box(
        Box2 {
            x: b.x + dx,
            y: b.y + dy,
            ..b
        },
        min,
    )
}

fn resize(b: Box2, handle: &str, dx: f64, dy: f64, min: Min) -> Box2 {
    let (mut left, mut top, mut right, mut bottom) = (b.x, b.y, b.x + b.w, b.y + b.h);
    let has = |c| handle.contains(c);
    if has('e') {
        right += dx
    }
    if has('w') {
        left += dx
    }
    if has('s') {
        bottom += dy
    }
    if has('n') {
        top += dy
    }
    if right - left < min.0 {
        if has('w') {
            left = right - min.0
        } else {
            right = left + min.0
        }
    }
    if bottom - top < min.1 {
        if has('n') {
            top = bottom - min.1
        } else {
            bottom = top + min.1
        }
    }
    left = clamp(left, 0.0, 100.0);
    right = clamp(right, 0.0, 100.0);
    top = clamp(top, 0.0, 100.0);
    bottom = clamp(bottom, 0.0, 100.0);
    if right - left < min.0 {
        if has('w') {
            left = clamp(right - min.0, 0.0, 100.0 - min.0)
        } else {
            right = clamp(left + min.0, min.0, 100.0)
        }
    }
    if bottom - top < min.1 {
        if has('n') {
            top = clamp(bottom - min.1, 0.0, 100.0 - min.1)
        } else {
            bottom = clamp(top + min.1, min.1, 100.0)
        }
    }
    clamp_box(
        Box2 {
            x: left,
            y: top,
            w: right - left,
            h: bottom - top,
        },
        min,
    )
}

fn resize_aspect(b: Box2, handle: &str, dx: f64, dy: f64, min: Min) -> Box2 {
    let has = |c| handle.contains(c);
    let width = b.w
        + if has('e') {
            dx
        } else if has('w') {
            -dx
        } else {
            0.0
        };
    let height = b.h
        + if has('s') {
            dy
        } else if has('n') {
            -dy
        } else {
            0.0
        };
    let horizontal = has('e') || has('w');
    let vertical = has('n') || has('s');
    let mut scale = if horizontal && vertical {
        js_max(width / b.w, height / b.h)
    } else if horizontal {
        width / b.w
    } else {
        height / b.h
    };
    scale = js_max(js_max(scale, min.0 / b.w), min.1 / b.h);
    scale = js_min(js_min(scale, 100.0 / b.w), 100.0 / b.h);
    let (w, h) = (b.w * scale, b.h * scale);
    let left = if has('w') {
        b.x + b.w - w
    } else if horizontal {
        b.x
    } else {
        b.x + (b.w - w) / 2.0
    };
    let top = if has('n') {
        b.y + b.h - h
    } else if vertical {
        b.y
    } else {
        b.y + (b.h - h) / 2.0
    };
    clamp_box(
        Box2 {
            x: left,
            y: top,
            w,
            h,
        },
        min,
    )
}

fn presets(position: &str, reference: &str, max_width: f64, show: bool) -> (Box2, Box2) {
    let full = position == "full";
    let w = if full {
        100.0
    } else {
        clamp(max_width, 20.0, 100.0)
    };
    let x = (100.0 - w) / 2.0;
    let verse_h = if full {
        if show { 88.0 } else { 100.0 }
    } else {
        24.0
    };
    let ref_h = if full { 12.0 } else { 10.0 };
    let gap = if full { 0.0 } else { 2.0 };
    let pair = verse_h + if show { gap + ref_h } else { 0.0 };
    let top = match position {
        "full" => 0.0,
        "top" => 6.0,
        "center" => (100.0 - pair) / 2.0,
        _ => 100.0 - 6.0 - pair,
    };
    if show && reference == "above" {
        (
            clamp_box(
                Box2 {
                    x,
                    y: top + ref_h + gap,
                    w,
                    h: verse_h,
                },
                TEXT,
            ),
            clamp_box(
                Box2 {
                    x,
                    y: top,
                    w,
                    h: ref_h,
                },
                TEXT,
            ),
        )
    } else {
        (
            clamp_box(
                Box2 {
                    x,
                    y: top,
                    w,
                    h: verse_h,
                },
                TEXT,
            ),
            clamp_box(
                Box2 {
                    x,
                    y: top + verse_h + gap,
                    w,
                    h: ref_h,
                },
                TEXT,
            ),
        )
    }
}

fn place_reference(verse: Box2, reference: Box2, position: &str) -> Box2 {
    let y = if position == "above" {
        verse.y - reference.h - 2.0
    } else {
        verse.y + verse.h + 2.0
    };
    clamp_box(
        Box2 {
            x: verse.x,
            y,
            w: verse.w,
            h: reference.h,
        },
        TEXT,
    )
}

fn input(b: Box2) -> Value {
    json!({"xPct": number(b.x), "yPct": number(b.y), "widthPct": number(b.w), "heightPct": number(b.h)})
}
fn min_json(m: Min) -> Value {
    json!({"widthPct": m.0, "heightPct": m.1})
}

fn coordinate() -> impl Strategy<Value = f64> {
    prop_oneof![
        6 => -200.0..300.0f64,
        1 => prop::sample::select(vec![0.0, -0.0, 8.0, 6.0, 100.0, 0.5]),
        1 => any::<i16>().prop_map(f64::from),
    ]
}
fn a_box() -> impl Strategy<Value = Box2> {
    (coordinate(), coordinate(), coordinate(), coordinate()).prop_map(|(x, y, w, h)| Box2 {
        x,
        y,
        w,
        h,
    })
}
fn handle() -> impl Strategy<Value = &'static str> {
    prop::sample::select(vec!["n", "s", "e", "w", "ne", "nw", "se", "sw"])
}
/// `None` leaves the argument out or passes `undefined`, so the TypeScript
/// default `TEXT_BOX_MIN` applies.
fn minimum() -> impl Strategy<Value = Option<Min>> {
    prop::sample::select(vec![Some(TEXT), Some(ELEMENT), None])
}

/// The argument list, with the optional minimum last.
fn args(mut fixed: Vec<Value>, min: Option<Min>, omit: bool) -> Value {
    match min {
        Some(m) => fixed.push(min_json(m)),
        None if !omit => fixed.push(undefined()),
        None => {}
    }
    Value::Array(fixed)
}

const EACH: usize = 2_000;

#[test]
fn clamp_matches() {
    let cases: Vec<_> = generate(11, EACH, (a_box(), minimum()))
        .into_iter()
        .enumerate()
        .map(|(i, (b, m))| {
            (
                args(vec![input(b)], m, i % 2 == 0),
                input(clamp_box(b, m.unwrap_or(TEXT))),
            )
        })
        .collect();
    assert_all_match("overlay-boxes.clampOverlayBox", &cases);
}

#[test]
fn move_matches() {
    let cases: Vec<_> = generate(12, EACH, (a_box(), coordinate(), coordinate(), minimum()))
        .into_iter()
        .enumerate()
        .map(|(i, (b, dx, dy, m))| {
            (
                args(vec![input(b), json!(dx), json!(dy)], m, i % 2 == 0),
                input(move_box(b, dx, dy, m.unwrap_or(TEXT))),
            )
        })
        .collect();
    assert_all_match("overlay-boxes.moveOverlayBox", &cases);
}

/// Checks a resize port against its TypeScript export over generated drags.
fn check_resize(seed: u64, name: &str, port: fn(Box2, &str, f64, f64, Min) -> Box2) {
    let cases: Vec<_> = generate(
        seed,
        EACH,
        (a_box(), handle(), coordinate(), coordinate(), minimum()),
    )
    .into_iter()
    .enumerate()
    .map(|(i, (b, h, dx, dy, m))| {
        (
            args(
                vec![input(b), json!(h), json!(dx), json!(dy)],
                m,
                i % 2 == 0,
            ),
            input(port(b, h, dx, dy, m.unwrap_or(TEXT))),
        )
    })
    .collect();
    assert_all_match(name, &cases);
}

#[test]
fn resize_matches() {
    check_resize(13, "overlay-boxes.resizeOverlayBox", resize);
}

#[test]
fn resize_keeping_aspect_matches() {
    check_resize(
        14,
        "overlay-boxes.resizeOverlayBoxKeepingAspect",
        resize_aspect,
    );
}

#[test]
fn presets_match() {
    let strategy = (
        prop::sample::select(vec!["full", "top", "center", "bottom"]),
        prop::sample::select(vec!["above", "below"]),
        coordinate(),
        any::<bool>(),
    );
    let cases: Vec<_> = generate(15, EACH, strategy)
        .into_iter()
        .map(|(p, r, w, s)| {
            let (verse, reference) = presets(p, r, w, s);
            (
                json!([p, r, w, s]),
                json!({"verse": input(verse), "reference": input(reference)}),
            )
        })
        .collect();
    assert_all_match("overlay-boxes.boxesForLayoutPreset", &cases);
}

#[test]
fn place_reference_matches() {
    let strategy = (
        a_box(),
        a_box(),
        prop::sample::select(vec!["above", "below"]),
    );
    let cases: Vec<_> = generate(16, EACH, strategy)
        .into_iter()
        .map(|(v, r, p)| {
            (
                json!([input(v), input(r), p]),
                input(place_reference(v, r, p)),
            )
        })
        .collect();
    assert_all_match("overlay-boxes.placeReferenceAgainstVerse", &cases);
}

/// A wrong port must fail, or the harness proves nothing.
#[test]
#[should_panic(expected = "cases differ")]
fn a_wrong_port_is_caught() {
    let cases: Vec<_> = generate(11, EACH, (a_box(), minimum()))
        .into_iter()
        .enumerate()
        .map(|(i, (b, m))| {
            let mut wrong = clamp_box(b, m.unwrap_or(TEXT));
            wrong.x += 0.25;
            (args(vec![input(b)], m, i % 2 == 0), input(wrong))
        })
        .collect();
    assert_all_match("overlay-boxes.clampOverlayBox", &cases);
}
