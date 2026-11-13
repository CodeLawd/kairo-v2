//! Syphon output (macOS): an offscreen output's frames published as a
//! Syphon server, so OBS, Resolume, vMix and other apps on the same Mac take
//! Kairo in, alpha included, for lyrics or a lower third over a camera.
//!
//! Ported from Electron's `src/main/services/syphon/index.ts`
//! (`SyphonChannel`) and the frame path of `gpu-capture.ts`. Frames are drawn
//! by the compositor straight into the server's own IOSurface (zero
//! read-back); when wgpu cannot take that surface, each frame is read back as
//! BGRA and copied in instead. Nothing here throws or panics on a Syphon
//! failure: an output that cannot run says why.

use kairo_compositor::{FinishedFrame, FrameSink, SinkFrame};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Frame counters, from the server and from this side.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub published: u64,
    pub skipped: u64,
    pub failed: u64,
    pub has_clients: bool,
    /// Frames that took the BGRA read-back path. Zero while frames go
    /// straight to the server's IOSurface.
    pub readbacks: u64,
}

#[cfg(target_os = "macos")]
type Server = kairo_native_sys::syphon::Server;

/// Called with a sink failure, once per distinct message in a row.
pub type Report = Arc<dyn Fn(String) + Send + Sync>;

/// One Syphon server, named after its output. Started on demand, stopped
/// with the output; other apps see it appear and leave in their source list.
pub struct SyphonChannel {
    name: String,
    #[cfg(target_os = "macos")]
    server: Option<Arc<Server>>,
    failure: Option<String>,
    readbacks: Arc<AtomicU64>,
    readback: bool,
}

impl SyphonChannel {
    pub fn new(server_name: impl Into<String>) -> Self {
        Self {
            name: server_name.into(),
            #[cfg(target_os = "macos")]
            server: None,
            failure: None,
            readbacks: Arc::new(AtomicU64::new(0)),
            readback: false,
        }
    }

    /// Always takes the BGRA read-back path, as when wgpu cannot take the
    /// server's IOSurface. For tests and measurements.
    pub fn force_readback(mut self) -> Self {
        self.readback = true;
        self
    }

    pub fn server_name(&self) -> &str {
        &self.name
    }

    /// Whether frames can go out, and if not why: for the Screens window.
    pub fn status(&self) -> Result<(), String> {
        available()?;
        match &self.failure {
            Some(failure) => Err(failure.clone()),
            None => Ok(()),
        }
    }

    pub fn running(&self) -> bool {
        #[cfg(target_os = "macos")]
        return self.server.is_some();
        #[cfg(not(target_os = "macos"))]
        false
    }

    /// Starts the server if it is not running. Never panics; false when it
    /// cannot run, with the reason in [`SyphonChannel::status`].
    pub fn start(&mut self) -> bool {
        #[cfg(target_os = "macos")]
        {
            if self.server.is_some() {
                return true;
            }
            if available().is_err() {
                return false;
            }
            match Server::open(&self.name) {
                Ok(server) => {
                    self.server = Some(Arc::new(server));
                    self.failure = None;
                    tracing::info!(name = self.name, "Syphon server started");
                    true
                }
                Err(error) => {
                    tracing::warn!(name = self.name, %error, "Syphon server could not start");
                    self.failure = Some(error);
                    false
                }
            }
        }
        #[cfg(not(target_os = "macos"))]
        false
    }

    /// The compositor side: draws each frame into the server. Starts the
    /// server first; `None` when it cannot run.
    pub fn sink(&mut self, report: Report) -> Option<Box<dyn FrameSink>> {
        if !self.start() {
            return None;
        }
        #[cfg(target_os = "macos")]
        return self.server.clone().map(|server| {
            Box::new(gpu::SyphonSink::new(
                server,
                self.name.clone(),
                report,
                self.readbacks.clone(),
                self.readback,
            )) as Box<dyn FrameSink>
        });
        #[cfg(not(target_os = "macos"))]
        {
            let _ = report;
            None
        }
    }

    /// A clear frame, so receivers show nothing rather than the last slide.
    /// Never starts a server just to clear it.
    pub fn clear(&self, width: u32, height: u32) {
        #[cfg(target_os = "macos")]
        if let Some(server) = &self.server {
            let pixels = vec![0u8; width as usize * height as usize * 4];
            if let Err(error) = server.publish_pixels(&pixels, width, height, 0) {
                tracing::warn!(name = self.name, %error, "Syphon frame failed");
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (width, height);
    }

    /// Stops the server; a later `start` makes a fresh one. Idempotent.
    pub fn stop(&mut self) {
        #[cfg(target_os = "macos")]
        if let Some(server) = self.server.take() {
            // A sink still holding it finds it closed and goes quiet.
            server.close();
            tracing::info!(name = self.name, "Syphon server stopped");
        }
    }

    pub fn stats(&self) -> Option<Stats> {
        #[cfg(target_os = "macos")]
        return self.server.as_ref().map(|server| {
            let s = server.stats();
            Stats {
                published: s.published,
                skipped: s.skipped,
                failed: s.failed,
                has_clients: s.has_clients,
                readbacks: self.readbacks.load(Ordering::Relaxed),
            }
        });
        #[cfg(not(target_os = "macos"))]
        None
    }

    /// Reads the newest published frame through a Syphon client, as OBS
    /// would: tight BGRA rows. For tests and verification tools.
    #[cfg(target_os = "macos")]
    pub fn read_latest(&self, timeout_ms: f64) -> Option<(u32, u32, Vec<u8>)> {
        self.server
            .as_ref()
            .and_then(|s| s.read_latest(timeout_ms).ok().flatten())
    }
}

impl Drop for SyphonChannel {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Whether Syphon can run on this machine.
pub fn available() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return kairo_native_sys::syphon::available();
    #[cfg(not(target_os = "macos"))]
    Err("Syphon is only available on a Mac".into())
}

#[cfg(target_os = "macos")]
mod gpu {
    use super::*;
    use kairo_native_sys::syphon::IoSurface;
    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2_metal::{MTLTexture, MTLTextureType};
    use wgpu::hal::api::Metal;

    /// `MTLTextureUsageShaderRead | MTLTextureUsageRenderTarget`.
    const USAGE: u64 = 0x1 | 0x4;

    struct Imported {
        // Held only to keep them alive; the texture is dropped before the
        // surface it wraps (field order).
        _texture: wgpu::Texture,
        view: wgpu::TextureView,
        _surface: IoSurface,
        size: (u32, u32),
    }

    enum Path {
        Gpu(Option<Imported>),
        Readback,
    }

    pub(super) struct SyphonSink {
        server: Arc<Server>,
        name: String,
        path: Path,
        drawn: bool,
        report: Report,
        last_error: Option<String>,
        readbacks: Arc<AtomicU64>,
    }

    impl SyphonSink {
        pub(super) fn new(
            server: Arc<Server>,
            name: String,
            report: Report,
            readbacks: Arc<AtomicU64>,
            readback: bool,
        ) -> Self {
            Self {
                server,
                name,
                path: if readback {
                    Path::Readback
                } else {
                    Path::Gpu(None)
                },
                drawn: false,
                report,
                last_error: None,
                readbacks,
            }
        }

        fn fail(&mut self, error: String) {
            // Stopped on purpose: the sink is being detached.
            if error.ends_with("server closed") {
                return;
            }
            if self.last_error.as_ref() != Some(&error) {
                tracing::warn!(name = self.name, %error, "Syphon frame failed");
                (self.report)(format!("Syphon: {error}"));
                self.last_error = Some(error);
            }
        }

        fn ok(&mut self) {
            self.last_error = None;
        }
    }

    /// The server's surface as a wgpu render target.
    fn import(
        device: &wgpu::Device,
        surface: IoSurface,
        size: (u32, u32),
    ) -> Result<Imported, String> {
        // SAFETY: the Metal device outlives this call (the guard holds it),
        // the texture made over the surface is owned by the returned value,
        // and the surface is kept alive alongside it.
        let texture = unsafe {
            let hal = device
                .as_hal::<Metal>()
                .ok_or("the compositor GPU is not a Metal device")?;
            let raw_device = Retained::as_ptr(hal.raw_device()) as *mut std::ffi::c_void;
            let raw = surface.metal_texture(raw_device, USAGE);
            let raw: Retained<ProtocolObject<dyn MTLTexture>> =
                Retained::from_raw(raw.cast()).ok_or("Metal refused the Syphon surface")?;
            drop(hal);
            let hal_texture = wgpu::hal::metal::Device::texture_from_raw(
                raw,
                wgpu::TextureFormat::Bgra8Unorm,
                MTLTextureType::Type2D,
                1,
                1,
                wgpu::hal::CopyExtent {
                    width: size.0,
                    height: size.1,
                    depth: 1,
                },
                None,
            );
            device.create_texture_from_hal::<Metal>(
                hal_texture,
                &wgpu::TextureDescriptor {
                    label: Some("syphon surface"),
                    size: wgpu::Extent3d {
                        width: size.0,
                        height: size.1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Bgra8Unorm,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                },
                wgpu::TextureUses::COLOR_TARGET,
            )
        };
        let view = texture.create_view(&Default::default());
        Ok(Imported {
            _texture: texture,
            view,
            _surface: surface,
            size,
        })
    }

    impl FrameSink for SyphonSink {
        fn encode(&mut self, frame: &mut SinkFrame<'_>) {
            self.drawn = false;
            let size = frame.size();
            let Path::Gpu(imported) = &mut self.path else {
                return;
            };
            if imported.as_ref().is_none_or(|i| i.size != size) {
                *imported = None;
                let made = self
                    .server
                    .surface(size.0, size.1)
                    .and_then(|surface| import(frame.device(), surface, size));
                match made {
                    Ok(made) => *imported = Some(made),
                    Err(error) if error.contains("closed") => return,
                    Err(error) => {
                        // Slow but correct: every frame read back as BGRA.
                        tracing::warn!(name = self.name, %error, "Syphon frames will be read back");
                        self.path = Path::Readback;
                        return;
                    }
                }
            }
            if let Some(imported) = imported {
                frame.copy_to(&imported.view, wgpu::TextureFormat::Bgra8Unorm);
                self.drawn = true;
            }
        }

        fn finished(&mut self, frame: &FinishedFrame<'_>) {
            match self.path {
                Path::Gpu(_) => {
                    if !self.drawn {
                        return;
                    }
                    match self.server.publish_surface() {
                        Ok(()) => self.ok(),
                        Err(error) => self.fail(error),
                    }
                }
                Path::Readback => {
                    let (width, height) = frame.size();
                    let mut pixels = frame.read_rgba();
                    for px in pixels.as_chunks_mut::<4>().0 {
                        px.swap(0, 2);
                    }
                    self.readbacks.fetch_add(1, Ordering::Relaxed);
                    match self.server.publish_pixels(&pixels, width, height, 0) {
                        Ok(_) => self.ok(),
                        Err(error) => self.fail(error),
                    }
                }
            }
        }
    }
}

#[cfg(all(test, not(target_os = "macos")))]
mod tests {
    use super::*;

    #[test]
    fn syphon_is_only_offered_on_a_mac() {
        let mut channel = SyphonChannel::new("Kairo");
        assert!(!channel.start());
        assert_eq!(
            channel.status(),
            Err("Syphon is only available on a Mac".into())
        );
        assert!(channel.sink(Arc::new(|_| {})).is_none());
    }
}
