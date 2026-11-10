//! `videoLayout` and `applyCameraLayout` from `overlay.html`.
//!
//! The input is the stored `VideoSettings` JSON, read with JavaScript's
//! coercions (`Number(x) || default`), so odd stored values behave as they
//! did. `__configure` normalises once and `applyCameraLayout` normalises the
//! result again; both passes are kept, because the second changes some
//! values (a box at `xPct: 0` reads as falsy and moves to the legacy corner).

use crate::geometry::{FRAME_WIDTH, Rect, Transform};
use serde_json::{Map, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Full,
    Pip,
    Split,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraBox {
    pub x_pct: f64,
    pub y_pct: f64,
    pub width_pct: f64,
    pub height_pct: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VideoLayout {
    pub mode: Mode,
    pub camera_box: CameraBox,
    pub radius_px: f64,
    pub left: bool,
    pub split_pct: f64,
}

/// `videoLayout` threw: a truthy `corner` that has no `indexOf`. The
/// `__configure` call it ran in was abandoned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayoutError;

/// JavaScript `Number(value)`.
pub fn js_number(value: Option<&Value>) -> f64 {
    match value {
        None => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(s)) => string_to_number(s),
        Some(Value::Array(items)) => match items.as_slice() {
            [] => 0.0,
            [Value::String(s)] => string_to_number(s),
            [Value::Number(n)] => n.as_f64().unwrap_or(f64::NAN),
            [Value::Null] => 0.0,
            [Value::Array(inner)] => js_number(Some(&Value::Array(inner.clone()))),
            _ => f64::NAN,
        },
        Some(Value::Object(_)) => f64::NAN,
    }
}

fn string_to_number(raw: &str) -> f64 {
    let s = raw.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    if s.is_empty() {
        return 0.0;
    }
    match s {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    for (prefix, radix) in [
        ("0x", 16),
        ("0X", 16),
        ("0o", 8),
        ("0O", 8),
        ("0b", 2),
        ("0B", 2),
    ] {
        if let Some(digits) = s.strip_prefix(prefix) {
            return u128::from_str_radix(digits, radix).map_or(f64::NAN, |v| v as f64);
        }
    }
    let bytes = s.as_bytes();
    let mut i = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let digits_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    let mut digits = i - digits_start;
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        let fraction = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        digits += i - fraction;
    }
    if digits == 0 {
        return f64::NAN;
    }
    if i < bytes.len() && matches!(bytes[i], b'e' | b'E') {
        i += 1;
        if i < bytes.len() && matches!(bytes[i], b'+' | b'-') {
            i += 1;
        }
        let exponent = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == exponent {
            return f64::NAN;
        }
    }
    if i != bytes.len() {
        return f64::NAN;
    }
    s.parse().unwrap_or(f64::NAN)
}

/// `Number(x) || fallback`.
fn or(value: Option<&Value>, fallback: f64) -> f64 {
    let n = js_number(value);
    if n == 0.0 || n.is_nan() { fallback } else { n }
}

fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(_) | Value::Object(_)) => true,
    }
}

/// `legacyCorner.indexOf(word) >= 0` for a string or an array.
fn corner_has(corner: &Value, word: &str) -> Result<bool, LayoutError> {
    match corner {
        Value::String(s) => Ok(s.contains(word)),
        Value::Array(items) => Ok(items.iter().any(|v| v.as_str() == Some(word))),
        _ => Err(LayoutError),
    }
}

/// `Math.max(lo, Math.min(hi, x))`.
fn clamp(lo: f64, hi: f64, x: f64) -> f64 {
    lo.max(hi.min(x))
}

/// `videoLayout(raw)`.
pub fn video_layout(raw: Option<&Value>) -> Result<VideoLayout, LayoutError> {
    let empty = Map::new();
    let value = match raw {
        Some(Value::Object(map)) if truthy(raw) => map,
        _ => &empty,
    };
    let mode = match value.get("layout").and_then(Value::as_str) {
        Some("pip") => Mode::Pip,
        Some("split") => Mode::Split,
        _ => Mode::Full,
    };
    let legacy_width = clamp(10.0, 60.0, or(value.get("sizePct"), 30.0));
    let legacy_margin = clamp(0.0, 8.0, or(value.get("marginPct"), 0.0));
    let default_corner = Value::String("bottom-right".into());
    let corner = if truthy(value.get("corner")) {
        value.get("corner").expect("truthy")
    } else {
        &default_corner
    };
    let camera_box = match value.get("box") {
        Some(Value::Object(map)) => map,
        _ => &empty,
    };
    let width_pct = clamp(10.0, 90.0, or(camera_box.get("widthPct"), legacy_width));
    let height_pct = clamp(10.0, 90.0, or(camera_box.get("heightPct"), legacy_width));
    let legacy_x = if corner_has(corner, "right")? {
        100.0 - legacy_width - legacy_margin
    } else {
        legacy_margin
    };
    let legacy_y = if corner_has(corner, "bottom")? {
        100.0 - legacy_width - legacy_margin
    } else {
        legacy_margin
    };
    Ok(VideoLayout {
        mode,
        camera_box: CameraBox {
            x_pct: clamp(0.0, 100.0 - width_pct, or(camera_box.get("xPct"), legacy_x)),
            y_pct: clamp(
                0.0,
                100.0 - height_pct,
                or(camera_box.get("yPct"), legacy_y),
            ),
            width_pct,
            height_pct,
        },
        radius_px: clamp(0.0, 48.0, or(value.get("radiusPx"), 0.0)),
        left: value.get("side").and_then(Value::as_str) == Some("left"),
        split_pct: clamp(20.0, 80.0, or(value.get("splitPct"), 50.0)),
    })
}

impl VideoLayout {
    /// The normalised object `__configure` stores as `config.video`.
    pub fn to_value(&self) -> Value {
        serde_json::json!({
            "layout": match self.mode { Mode::Full => "full", Mode::Pip => "pip", Mode::Split => "split" },
            "box": {
                "xPct": self.camera_box.x_pct,
                "yPct": self.camera_box.y_pct,
                "widthPct": self.camera_box.width_pct,
                "heightPct": self.camera_box.height_pct,
            },
            "radiusPx": self.radius_px,
            "side": if self.left { "left" } else { "right" },
            "splitPct": self.split_pct,
        })
    }

    /// `DEFAULT_VIDEO_LAYOUT`, normalised.
    pub fn default_layout() -> Self {
        video_layout(Some(&serde_json::json!({
            "layout": "full", "corner": "bottom-right", "sizePct": 30, "marginPct": 2,
            "radiusPx": 12, "side": "right", "splitPct": 50
        })))
        .expect("default layout")
    }
}

/// Where everything goes for one frame (`applyCameraLayout` plus
/// `applyCameraNodeStyle`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub mode: Mode,
    /// Applied to `#pa-content-bg` and `#pa-content-fore`.
    pub content: Transform,
    /// `#pa-split-backdrop` shows (opaque black).
    pub backdrop: bool,
    /// The camera node's box, radius and whether it casts the pip shadow.
    pub camera: Rect,
    pub radius: f32,
    pub shadow: bool,
}

/// `applyCameraLayout`: with no camera live the presentation keeps the
/// whole frame. `stored` is `config.video`, already normalised once.
pub fn place(stored: &VideoLayout, camera_live: bool, frame_height: u32) -> Placement {
    let settings = if camera_live {
        video_layout(Some(&stored.to_value())).expect("normalised layouts have a string corner")
    } else {
        video_layout(Some(&serde_json::json!({ "layout": "full" }))).expect("full")
    };
    let fw = f64::from(FRAME_WIDTH);
    let fh = f64::from(frame_height);
    let content = if settings.mode == Mode::Split {
        let share = settings.split_pct / 100.0;
        let scale = 1.0 - share;
        Transform {
            x: if settings.left { fw * share } else { 0.0 } as f32,
            y: ((fh - fh * scale) / 2.0) as f32,
            scale: scale as f32,
        }
    } else {
        Transform::IDENTITY
    };
    let (camera, radius, shadow) = match settings.mode {
        Mode::Full => (Rect::new(0.0, 0.0, fw as f32, fh as f32), 0.0, false),
        Mode::Split => {
            let width = fw * settings.split_pct / 100.0;
            let left = if settings.left { 0.0 } else { fw - width };
            (
                Rect::new(left as f32, 0.0, width as f32, fh as f32),
                0.0,
                false,
            )
        }
        Mode::Pip => {
            let b = settings.camera_box;
            (
                Rect::new(
                    (b.x_pct / 100.0 * fw) as f32,
                    (b.y_pct / 100.0 * fh) as f32,
                    (b.width_pct / 100.0 * fw) as f32,
                    (b.height_pct / 100.0 * fh) as f32,
                ),
                settings.radius_px as f32,
                true,
            )
        }
    };
    Placement {
        mode: settings.mode,
        content,
        backdrop: settings.mode == Mode::Split,
        camera,
        radius,
        shadow,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn js_number_follows_javascript() {
        let n = |v: Value| js_number(Some(&v));
        assert!(js_number(None).is_nan());
        assert_eq!(n(json!(null)), 0.0);
        assert_eq!(n(json!(true)), 1.0);
        assert_eq!(n(json!(" 12.5 ")), 12.5);
        assert_eq!(n(json!("")), 0.0);
        assert_eq!(n(json!("0x10")), 16.0);
        assert_eq!(n(json!("1e2")), 100.0);
        assert_eq!(n(json!(".5")), 0.5);
        assert_eq!(n(json!("5.")), 5.0);
        assert!(n(json!("12px")).is_nan());
        assert!(n(json!("inf")).is_nan());
        assert!(n(json!("NaN")).is_nan());
        assert_eq!(n(json!("-Infinity")), f64::NEG_INFINITY);
        assert_eq!(n(json!([])), 0.0);
        assert_eq!(n(json!(["7"])), 7.0);
        assert!(n(json!([1, 2])).is_nan());
        assert!(n(json!({})).is_nan());
    }

    #[test]
    fn a_box_at_zero_moves_to_the_legacy_corner_on_the_second_pass() {
        let stored = video_layout(Some(&json!({
            "layout": "pip", "box": {"xPct": 0, "yPct": 0, "widthPct": 25, "heightPct": 25}
        })))
        .unwrap();
        // First pass: zero is falsy, so the legacy bottom-right corner wins.
        assert_eq!(stored.camera_box.x_pct, 70.0);
        assert_eq!(stored.camera_box.y_pct, 70.0);
        let placed = place(&stored, true, 1080);
        assert_eq!(placed.camera, Rect::new(1344.0, 756.0, 480.0, 270.0));
    }

    #[test]
    fn a_corner_without_index_of_abandons_configure() {
        assert_eq!(video_layout(Some(&json!({"corner": 5}))), Err(LayoutError));
        assert!(
            video_layout(Some(&json!({"corner": 0}))).is_ok(),
            "falsy uses the default"
        );
        assert!(video_layout(Some(&json!({"corner": ["left"]}))).is_ok());
    }

    #[test]
    fn without_a_camera_the_presentation_keeps_the_frame() {
        let stored = video_layout(Some(&json!({"layout": "split", "splitPct": 40}))).unwrap();
        let placed = place(&stored, false, 1080);
        assert_eq!(placed.mode, Mode::Full);
        assert_eq!(placed.content, Transform::IDENTITY);
        assert!(!placed.backdrop);
    }

    #[test]
    fn split_shrinks_the_presentation_and_centres_it_vertically() {
        let stored = video_layout(Some(
            &json!({"layout": "split", "splitPct": 40, "side": "left"}),
        ))
        .unwrap();
        let placed = place(&stored, true, 1080);
        assert!(placed.backdrop);
        assert!((placed.content.scale - 0.6).abs() < 1e-6);
        assert_eq!((placed.content.x, placed.content.y), (768.0, 216.0));
        assert_eq!(placed.camera, Rect::new(0.0, 0.0, 768.0, 1080.0));
    }
}
