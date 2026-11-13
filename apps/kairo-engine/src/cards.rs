//! Video card outputs in the engine (prompt 062): which providers run, the
//! frame shape for a card mode, and how a card surface's frames reach the
//! helper.

use kairo_compositor::thread::Compositor;
use kairo_sinks::video_devices::{Managed, Provider};
use std::sync::Arc;

/// The DeckLink provider on macOS, unless `KAIRO_ENABLE_DECKLINK=0`
/// (kept until hardware qualification, prompt 110).
pub fn providers() -> Vec<Box<dyn Provider>> {
    #[cfg(unix)]
    {
        use kairo_sinks::decklink;
        let flag = std::env::var("KAIRO_ENABLE_DECKLINK").ok();
        if !decklink::enabled(std::env::consts::OS, flag.as_deref()) {
            return Vec::new();
        }
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_default();
        // Development builds also look in this checkout.
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let repo = repo.exists().then_some(repo);
        // `KAIRO_DECKLINK_HELPER` points at another helper build (development
        // and qualification, such as `kairo-sinks`' `decklink_sim` example).
        let helper = std::env::var_os("KAIRO_DECKLINK_HELPER")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| decklink::helper_path(&exe_dir, repo.as_deref(), |p| p.exists()));
        vec![Box::new(decklink::DeckLinkProvider::new(
            helper,
            decklink::RUNTIME.into(),
        ))]
    }
    #[cfg(not(unix))]
    Vec::new()
}

/// The compositor frame (always 1920 wide) with the card mode's shape: a
/// 1280 × 720 mode draws 1920 × 1080 and is scaled to the raster.
pub fn frame_height((width, height): (u32, u32)) -> u32 {
    if width == 0 {
        return 1080;
    }
    let height =
        (u64::from(kairo_compositor::FRAME_WIDTH) * u64::from(height) / u64::from(width)) as u32;
    height.clamp(360, 4320)
}

/// Each drawn frame goes to the card: UYVY at the mode raster into the
/// frame ring (no alpha: key/fill stays off), or, without a ring, BGRA
/// read back and sent down the helper's stdin.
pub fn attach(compositor: &Compositor, output: &str, sink: &Arc<Managed>) {
    #[cfg(unix)]
    if let Some(ring) = sink.frame_ring() {
        let managed = sink.clone();
        compositor.capture_raster(
            output,
            sink.raster,
            false,
            Some(Arc::new(move |frame| {
                let data = frame.data();
                // A slot still being read is skipped: the helper repeats
                // the frame it has on the hardware clock.
                if ring.write(|slot| {
                    let n = slot.len().min(data.len());
                    slot[..n].copy_from_slice(&data[..n]);
                }) {
                    managed.count_capture();
                }
            })),
        );
        return;
    }
    compositor.attach(output, Some(Box::new(Bgra(sink.clone()))));
}

/// The CPU fallback: the finished frame as BGRA.
struct Bgra(Arc<Managed>);

impl kairo_compositor::FrameSink for Bgra {
    fn encode(&mut self, _: &mut kairo_compositor::SinkFrame<'_>) {}

    fn finished(&mut self, frame: &kairo_compositor::FinishedFrame<'_>) {
        if !self.0.active() {
            return;
        }
        let (width, height) = frame.size();
        let mut pixels = frame.read_rgba();
        for px in pixels.as_chunks_mut::<4>().0 {
            px.swap(0, 2);
        }
        if let Err(error) = self.0.set_frame(&pixels, width, height) {
            tracing::warn!(%error, "video card frame refused");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_modes_keep_their_shape_in_the_1920_wide_frame() {
        assert_eq!(frame_height((1920, 1080)), 1080);
        assert_eq!(frame_height((1280, 720)), 1080);
        assert_eq!(frame_height((3840, 2160)), 1080);
        assert_eq!(frame_height((720, 576)), 1536);
        assert_eq!(frame_height((0, 0)), 1080);
    }
}
