//! Choosing a capture format as `getUserMedia` does for
//! `{ width: { ideal: 1920 }, height: { ideal: 1080 }, frameRate: { ideal: 30 } }`.
//!
//! An ideal constraint never fails: the format closest to it wins. The
//! distance is the spec's fitness distance, `|actual - ideal| / max(actual,
//! ideal)` summed over width, height and frame rate, where a frame-rate
//! range that contains the ideal is distance 0. Ties keep the larger frame,
//! then the device's own order.

/// What the camera asks for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Request {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
}

impl Request {
    /// `overlay.html`'s constraints.
    pub const PROGRAM: Self = Self {
        width: 1920,
        height: 1080,
        fps: 30.0,
    };
}

/// One format a device offers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FormatInfo {
    pub width: u32,
    pub height: u32,
    pub min_fps: f64,
    pub max_fps: f64,
}

fn fitness(actual: f64, ideal: f64) -> f64 {
    if actual == ideal {
        0.0
    } else {
        (actual - ideal).abs() / actual.max(ideal)
    }
}

/// The frame rate a format runs at for `fps`: the ideal when the format
/// allows it, else the nearest end of its range.
pub fn rate_for(format: &FormatInfo, fps: f64) -> f64 {
    if !(format.min_fps.is_finite() && format.max_fps.is_finite()) || format.max_fps <= 0.0 {
        return fps;
    }
    fps.clamp(format.min_fps.min(format.max_fps), format.max_fps)
}

/// The index of the format closest to `request`, if there is any.
pub fn choose(formats: &[FormatInfo], request: Request) -> Option<usize> {
    let mut best: Option<(usize, f64, u64)> = None;
    for (i, f) in formats.iter().enumerate() {
        if f.width == 0 || f.height == 0 {
            continue;
        }
        let rate = rate_for(f, request.fps);
        let d = fitness(f64::from(f.width), f64::from(request.width))
            + fitness(f64::from(f.height), f64::from(request.height))
            + fitness(rate, request.fps);
        let area = u64::from(f.width) * u64::from(f.height);
        let better = match best {
            None => true,
            Some((_, bd, ba)) => d < bd - 1e-9 || ((d - bd).abs() <= 1e-9 && area > ba),
        };
        if better {
            best = Some((i, d, area));
        }
    }
    best.map(|(i, _, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(width: u32, height: u32, min_fps: f64, max_fps: f64) -> FormatInfo {
        FormatInfo {
            width,
            height,
            min_fps,
            max_fps,
        }
    }

    #[test]
    fn the_exact_format_wins_and_rates_are_clamped() {
        let formats = [
            f(640, 480, 1.0, 30.0),
            f(3840, 2160, 1.0, 30.0),
            f(1920, 1080, 1.0, 60.0),
            f(1280, 720, 1.0, 60.0),
        ];
        assert_eq!(choose(&formats, Request::PROGRAM), Some(2));
        assert_eq!(rate_for(&formats[2], 30.0), 30.0);
        assert_eq!(rate_for(&f(1920, 1080, 50.0, 60.0), 30.0), 50.0);
        assert_eq!(rate_for(&f(1920, 1080, 1.0, 25.0), 30.0), 25.0);
    }

    #[test]
    fn capture_cards_and_odd_sets_get_the_nearest() {
        // A capture card offering only 1080p at 59.94 and 720p at 30.
        let formats = [f(1280, 720, 30.0, 30.0), f(1920, 1080, 59.94, 59.94)];
        assert_eq!(choose(&formats, Request::PROGRAM), Some(1));
        // A webcam with no 1080p: 4K is farther than 720p.
        let formats = [f(3840, 2160, 1.0, 30.0), f(1280, 720, 1.0, 30.0)];
        assert_eq!(choose(&formats, Request::PROGRAM), Some(1));
        // Ties keep the larger frame.
        let formats = [f(1920, 1080, 1.0, 30.0), f(1920, 1080, 1.0, 30.0)];
        assert_eq!(choose(&formats, Request::PROGRAM), Some(0));
        assert_eq!(choose(&[], Request::PROGRAM), None);
        assert_eq!(choose(&[f(0, 0, 1.0, 30.0)], Request::PROGRAM), None);
    }
}
