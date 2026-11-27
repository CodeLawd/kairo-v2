//! Motion for Imagine's pictures: how a saved image moves on screen
//! (`MediaMotion` in `kairo-domain`), the depth layer that gives it
//! parallax, and motion loops drawn locally with no image request at all.

use tiny_skia::{
    Color, GradientStop, Paint, Pixmap, Point, RadialGradient, Rect, SpreadMode, Transform,
};

use crate::ImagineError;

/// One way to save a picture: a camera move, an atmosphere, and whether to
/// cut a depth layer for parallax (one more image request).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MotionPreset {
    pub key: &'static str,
    pub label: &'static str,
    pub camera: Option<&'static str>,
    pub atmosphere: Option<&'static str>,
    pub depth: bool,
}

pub const PRESETS: [MotionPreset; 7] = [
    MotionPreset {
        key: "still",
        label: "Still",
        camera: None,
        atmosphere: None,
        depth: false,
    },
    MotionPreset {
        key: "zoom",
        label: "Slow zoom",
        camera: Some("zoom"),
        atmosphere: None,
        depth: false,
    },
    MotionPreset {
        key: "dust",
        label: "Drift + dust",
        camera: Some("drift"),
        atmosphere: Some("dust"),
        depth: false,
    },
    MotionPreset {
        key: "rays",
        label: "Pan + light rays",
        camera: Some("pan"),
        atmosphere: Some("rays"),
        depth: false,
    },
    MotionPreset {
        key: "bokeh",
        label: "Zoom + bokeh",
        camera: Some("zoom"),
        atmosphere: Some("bokeh"),
        depth: false,
    },
    MotionPreset {
        key: "haze",
        label: "Drift + haze",
        camera: Some("drift"),
        atmosphere: Some("haze"),
        depth: false,
    },
    MotionPreset {
        key: "depth",
        label: "Depth (parallax)",
        camera: Some("drift"),
        atmosphere: None,
        depth: true,
    },
];

impl MotionPreset {
    pub fn by_key(key: &str) -> Option<Self> {
        PRESETS.into_iter().find(|p| p.key == key)
    }

    pub fn is_still(&self) -> bool {
        self.camera.is_none() && self.atmosphere.is_none() && !self.depth
    }

    /// The `motion` playback value, with `layers` (absolute paths).
    pub fn value(&self, layers: &[String]) -> serde_json::Value {
        let mut m = serde_json::Map::new();
        if let Some(c) = self.camera {
            m.insert("camera".into(), c.into());
        }
        if let Some(a) = self.atmosphere {
            m.insert("atmosphere".into(), a.into());
        }
        if !layers.is_empty() {
            m.insert("layers".into(), layers.into());
        }
        serde_json::Value::Object(m)
    }
}

/// The motion a message plainly asks for, when the assistant left it out:
/// `None` unless it asks for movement at all. Words are checked in order of
/// how specific they are (parallax before sunbeams before haze).
pub fn motion_hint(message: &str) -> Option<&'static str> {
    let m = message.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| m.contains(w));
    let moving = has(&[
        "motion", "moving", "animat", "loop", "flowing", "drifting", "drift", "slow-mo",
        "parallax", "living", "alive",
    ]);
    if !moving {
        return None;
    }
    Some(if has(&["parallax", "depth", "3d"]) {
        "depth"
    } else if has(&[
        "sunbeam",
        "light ray",
        "god ray",
        "volumetric",
        "rays",
        "beams",
    ]) {
        "rays"
    } else if has(&["bokeh", "light spots", "orbs"]) {
        "bokeh"
    } else if has(&["particle", "dust", "sparkle", "embers", "glitter"]) {
        "dust"
    } else if has(&["haze", "fog", "mist", "smoke", "cloud"]) {
        "haze"
    } else if has(&["zoom", "push in"]) {
        "zoom"
    } else {
        "dust"
    })
}

/// What the image model is asked for to cut the depth layer: the same
/// picture, foreground only, on transparency, so it lines up exactly.
pub const DEPTH_PROMPT: &str = "Keep only the nearest foreground elements of this exact image, \
unchanged in position, size and colour. Make everything else, including the sky and the \
background, fully transparent. Do not add anything.";

/// An abstract loop made with no image request: a palette drawn as soft
/// glows, which the engine then moves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotionLoop {
    pub key: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    /// Background, then two glows (0xRRGGBB).
    pub colors: [u32; 3],
    pub preset: &'static str,
}

pub const LOOPS: [MotionLoop; 4] = [
    MotionLoop {
        key: "calm-blue",
        title: "Calm blue",
        description: "Deep blue glow with drifting light.",
        colors: [0x0a1630, 0x2f6fd6, 0x7fb4ff],
        preset: "bokeh",
    },
    MotionLoop {
        key: "warm-gold",
        title: "Warm gold",
        description: "Amber light with slow rays.",
        colors: [0x1c1006, 0xc77a1e, 0xffd27a],
        preset: "rays",
    },
    MotionLoop {
        key: "royal",
        title: "Royal purple",
        description: "Violet haze for quiet worship.",
        colors: [0x140a24, 0x6a35c9, 0xd39bff],
        preset: "haze",
    },
    MotionLoop {
        key: "dawn",
        title: "Dawn",
        description: "Rose and peach with soft dust.",
        colors: [0x1a0d16, 0xd9577a, 0xffb48a],
        preset: "dust",
    },
];

impl MotionLoop {
    pub fn by_key(key: &str) -> Option<Self> {
        LOOPS.into_iter().find(|l| l.key == key)
    }
}

fn color(rgb: u32, alpha: f32) -> Color {
    let [_, r, g, b] = rgb.to_be_bytes();
    Color::from_rgba8(r, g, b, (alpha * 255.) as u8)
}

/// The loop's still, as PNG bytes.
pub fn render_loop(l: &MotionLoop, (w, h): (u32, u32)) -> Result<Vec<u8>, ImagineError> {
    let mut pixmap = Pixmap::new(w, h).ok_or(ImagineError::Missing)?;
    pixmap.fill(color(l.colors[0], 1.));
    let (wf, hf) = (w as f32, h as f32);
    let glows = [
        (0.28, 0.62, 0.75, l.colors[1], 0.85),
        (0.74, 0.34, 0.55, l.colors[2], 0.55),
        (0.55, 0.9, 0.5, l.colors[1], 0.4),
    ];
    for (x, y, r, rgb, a) in glows {
        let (cx, cy, r) = (x * wf, y * hf, r * wf);
        let shader = RadialGradient::new(
            Point::from_xy(cx, cy),
            Point::from_xy(cx, cy),
            r,
            vec![
                GradientStop::new(0., color(rgb, a)),
                GradientStop::new(1., color(rgb, 0.)),
            ],
            SpreadMode::Pad,
            Transform::identity(),
        );
        if let (Some(shader), Some(rect)) = (shader, Rect::from_xywh(0., 0., wf, hf)) {
            let paint = Paint {
                shader,
                anti_alias: true,
                ..Paint::default()
            };
            pixmap.fill_rect(rect, &paint, Transform::identity(), None);
        }
    }
    pixmap.encode_png().map_err(|_| ImagineError::Missing)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_and_loops_are_known_and_render() {
        assert!(MotionPreset::by_key("still").unwrap().is_still());
        let depth = MotionPreset::by_key("depth").unwrap();
        assert_eq!(
            depth.value(&["/l.png".into()]),
            serde_json::json!({"camera": "drift", "layers": ["/l.png"]})
        );
        for l in LOOPS {
            assert!(MotionPreset::by_key(l.preset).is_some_and(|p| !p.is_still()));
        }
        assert_eq!(
            motion_hint(
                "Slow, gentle flowing light particles and soft volumetric sunbeams, seamless loop"
            ),
            Some("rays")
        );
        assert_eq!(motion_hint("a calm lake at dawn"), None);
        assert_eq!(motion_hint("animate it with parallax"), Some("depth"));
        let png = render_loop(&LOOPS[0], (64, 36)).unwrap();
        let decoded = image::load_from_memory(&png).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (64, 36));
    }
}
