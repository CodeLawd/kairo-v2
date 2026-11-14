//! The operator preview (prompt 066): an output's frames, downscaled on
//! the GPU and published into the map the UI process made.
//!
//! Electron drew the operator's live monitor by rendering the program a
//! second time in the renderer. Here the engine sends what it actually
//! drew, only while a view asks for it (`Command::SetPreview`), at the
//! view's size and at a capped rate. Only the newest frame is kept.

use kairo_compositor::thread::CaptureSink;
use kairo_engine_proto::preview::PreviewMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// One output's preview map.
#[derive(Clone)]
pub struct Feed {
    pub path: String,
    map: Arc<PreviewMap>,
    /// A frame that does not fit is logged once, not at every frame.
    warned: Arc<AtomicBool>,
}

impl Feed {
    pub fn open(path: &str) -> std::io::Result<Self> {
        Ok(Self {
            path: path.to_owned(),
            map: Arc::new(PreviewMap::open(Path::new(path))?),
            warned: Arc::default(),
        })
    }

    pub fn capacity(&self) -> (u32, u32) {
        self.map.capacity()
    }

    /// Publishes each converted frame into the map.
    pub fn sink(&self, output: &str) -> CaptureSink {
        let map = self.map.clone();
        let warned = self.warned.clone();
        let output = output.to_owned();
        Arc::new(move |frame| {
            if let Err(error) = map.publish(frame.width, frame.height, frame.data())
                && !warned.swap(true, Ordering::Relaxed)
            {
                tracing::warn!(output, %error, "preview frame not published");
            }
        })
    }
}

/// The preview's size: the output's shape fitted inside the box the view
/// asked for and the map's capacity, never upscaled past the frame, with a
/// width that is a multiple of 4 (the converter writes four pixels at a time).
pub fn fit(frame: (u32, u32), wanted: (u32, u32), capacity: (u32, u32)) -> (u32, u32) {
    let (fw, fh) = (frame.0.max(1) as f64, frame.1.max(1) as f64);
    let bw = wanted.0.min(capacity.0).min(frame.0).max(4) as f64;
    let bh = wanted.1.min(capacity.1).min(frame.1).max(1) as f64;
    let scale = (bw / fw).min(bh / fh);
    let width = ((fw * scale).floor() as u32 / 4 * 4)
        .min(capacity.0 / 4 * 4)
        .max(4);
    // From the rounded width, so the shape holds.
    let height = ((width as f64 * fh / fw).round() as u32).clamp(1, (bh as u32).max(1));
    (width, height)
}

#[cfg(test)]
mod tests {
    use super::fit;

    #[test]
    fn the_preview_keeps_the_output_shape_inside_the_view_and_the_map() {
        // A 16:9 output in a 16:9 view.
        assert_eq!(fit((1920, 1080), (640, 360), (1920, 1080)), (640, 360));
        // A 4:3 card in a 16:9 view: pillarboxed by the view, not stretched.
        assert_eq!(fit((1920, 1440), (640, 360), (1920, 1080)), (480, 360));
        // A wide view is limited by its height.
        assert_eq!(fit((1920, 1080), (1000, 200), (1920, 1080)), (352, 198));
        // Never more than the map holds, nor larger than the frame.
        assert_eq!(fit((1920, 1080), (4000, 4000), (960, 540)), (960, 540));
        assert_eq!(fit((1280, 720), (1920, 1080), (1920, 1080)), (1280, 720));
        // Odd widths round down to a multiple of 4.
        let (w, _) = fit((1920, 1080), (333, 500), (1920, 1080));
        assert_eq!(w % 4, 0);
        // A collapsed view still gets a frame the converter accepts.
        assert_eq!(fit((1920, 1080), (0, 0), (1920, 1080)), (4, 1));
    }
}
