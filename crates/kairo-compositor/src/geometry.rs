//! Frame sizes and the display letterbox.

/// Theme pixel space is always 1920 wide.
pub const FRAME_WIDTH: u32 = 1920;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

/// `FRAME_H`: `?h=` clamped to 360..=4320, 1080 when absent or zero.
pub fn frame_height(requested: u32) -> u32 {
    if requested == 0 {
        return 1080;
    }
    requested.clamp(360, 4320)
}

/// A translate-then-scale transform, as `#pa-content-*` use in split mode.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub x: f32,
    pub y: f32,
    pub scale: f32,
}

impl Transform {
    pub const IDENTITY: Self = Self {
        x: 0.0,
        y: 0.0,
        scale: 1.0,
    };

    pub fn apply(&self, rect: Rect) -> Rect {
        Rect::new(
            self.x + rect.x * self.scale,
            self.y + rect.y * self.scale,
            rect.width * self.scale,
            rect.height * self.scale,
        )
    }
}

/// `fitToWindow`: the frame scaled to fit the window, centred, the rest
/// black. Returns the frame's rectangle in window pixels.
pub fn letterbox(window: (u32, u32), frame: (u32, u32)) -> Rect {
    let (ww, wh) = (window.0 as f32, window.1 as f32);
    let (fw, fh) = (frame.0 as f32, frame.1 as f32);
    let scale = (ww / fw).min(wh / fh);
    let (w, h) = (fw * scale, fh * scale);
    Rect::new((ww - w) / 2.0, (wh - h) / 2.0, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_height_is_clamped() {
        assert_eq!(frame_height(0), 1080);
        assert_eq!(frame_height(100), 360);
        assert_eq!(frame_height(9000), 4320);
        assert_eq!(frame_height(1200), 1200);
    }

    #[test]
    fn letterbox_centres_on_the_short_axis() {
        assert_eq!(
            letterbox((3600, 2338), (1920, 1080)),
            Rect::new(0.0, (2338.0 - 2025.0) / 2.0, 3600.0, 2025.0)
        );
        let square = letterbox((1000, 1000), (1920, 1080));
        assert!((square.height - 562.5).abs() < 1e-3 && (square.y - 218.75).abs() < 1e-3);
        assert!(square.x.abs() < 1e-3 && (square.width - 1000.0).abs() < 1e-3);
        assert_eq!(
            letterbox((1920, 1080), (1920, 1080)),
            Rect::new(0.0, 0.0, 1920.0, 1080.0)
        );
    }
}
