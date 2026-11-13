//! Syphon servers over the C ABI in `native/syphon/syphon_c.mm`.
//!
//! Ported from `../kairo-presenter/native/syphon/syphon.mm` at commit
//! `5679dc5` (tree checked at `af978c5`). Re-check for drift at cutover.

use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr::NonNull;

#[repr(C)]
struct RawServer {
    _private: [u8; 0],
}

#[repr(C)]
struct RawSurface {
    _private: [u8; 0],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub published: u64,
    pub skipped: u64,
    pub failed: u64,
    pub has_clients: bool,
}

type Done = extern "C" fn(*mut c_void, *const c_char);

unsafe extern "C" {
    fn kairo_syphon_available(reason: *mut c_char, cap: usize) -> bool;
    fn kairo_syphon_open(name: *const c_char, error: *mut c_char, cap: usize) -> *mut RawServer;
    fn kairo_syphon_publish(
        server: *mut RawServer,
        source: *mut RawSurface,
        done: Option<Done>,
        context: *mut c_void,
        error: *mut c_char,
        cap: usize,
    ) -> i32;
    fn kairo_syphon_publish_pixels(
        server: *mut RawServer,
        pixels: *const u8,
        length: usize,
        width: u32,
        height: u32,
        stride: usize,
        error: *mut c_char,
        cap: usize,
    ) -> i32;
    fn kairo_syphon_surface(
        server: *mut RawServer,
        width: u32,
        height: u32,
        error: *mut c_char,
        cap: usize,
    ) -> *mut RawSurface;
    fn kairo_syphon_publish_surface(server: *mut RawServer, error: *mut c_char, cap: usize) -> i32;
    fn kairo_syphon_count(server: *mut RawServer, failed: bool);
    fn kairo_syphon_close(server: *mut RawServer);
    fn kairo_syphon_free(server: *mut RawServer);
    fn kairo_syphon_stats(server: *mut RawServer) -> Stats;
    fn kairo_metal_texture(
        device: *mut c_void,
        surface: *mut RawSurface,
        usage: u64,
    ) -> *mut c_void;
    fn kairo_iosurface_release(surface: *mut RawSurface);
    fn kairo_iosurface_id(surface: *mut RawSurface) -> u32;
    fn kairo_iosurface_create_bgra(bgra: *const u8, width: u32, height: u32) -> *mut RawSurface;
    fn kairo_syphon_client_read(
        name: *const c_char,
        discover_ms: f64,
        watch_ms: f64,
        out: *mut u8,
        cap: usize,
        width: *mut u32,
        height: *mut u32,
        frames: *mut u64,
        error: *mut c_char,
        error_cap: usize,
    ) -> i32;
    fn kairo_syphon_read_latest(
        server: *mut RawServer,
        timeout_ms: f64,
        out: *mut u8,
        cap: usize,
        width: *mut u32,
        height: *mut u32,
        error: *mut c_char,
        error_cap: usize,
    ) -> i32;
}

const ERROR_CAP: usize = 512;

fn message(buffer: &[u8]) -> String {
    CStr::from_bytes_until_nul(buffer)
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Calls `f` with an error buffer; the buffer's text afterwards.
fn with_error<T>(f: impl FnOnce(*mut c_char, usize) -> T) -> (T, String) {
    let mut buffer = [0u8; ERROR_CAP];
    let value = f(buffer.as_mut_ptr().cast(), buffer.len());
    (value, message(&buffer))
}

/// Whether Syphon can run here (a Metal device and queue); why not.
pub fn available() -> Result<(), String> {
    // SAFETY: the buffer outlives the call and its length is passed.
    let (ok, reason) = with_error(|e, n| unsafe { kairo_syphon_available(e, n) });
    if ok {
        Ok(())
    } else {
        Err(if reason.is_empty() {
            "Metal unavailable".into()
        } else {
            reason
        })
    }
}

/// A retained IOSurface, released on drop.
#[derive(Debug)]
pub struct IoSurface(NonNull<RawSurface>);

// SAFETY: IOSurface references may be used and released from any thread.
unsafe impl Send for IoSurface {}
unsafe impl Sync for IoSurface {}

impl IoSurface {
    /// A BGRA surface holding `bgra` (tight rows). For tests and tools.
    pub fn from_bgra(bgra: &[u8], width: u32, height: u32) -> Option<Self> {
        if bgra.len() < width as usize * height as usize * 4 {
            return None;
        }
        // SAFETY: the length was checked against width × height × 4.
        NonNull::new(unsafe { kairo_iosurface_create_bgra(bgra.as_ptr(), width, height) }).map(Self)
    }

    /// The system-wide surface id.
    pub fn id(&self) -> u32 {
        // SAFETY: the surface is retained by `self`.
        unsafe { kairo_iosurface_id(self.0.as_ptr()) }
    }

    /// A retained `id<MTLTexture>` (BGRA8Unorm) over this surface, made on
    /// `device`, or null when Metal refuses it (misaligned rows, say).
    ///
    /// # Safety
    /// `device` must be a live `id<MTLDevice>`. The caller owns the returned
    /// reference and must release it exactly once.
    pub unsafe fn metal_texture(&self, device: *mut c_void, usage: u64) -> *mut c_void {
        // SAFETY: upheld by the caller; the surface is retained by `self`.
        unsafe { kairo_metal_texture(device, self.0.as_ptr(), usage) }
    }
}

impl Drop for IoSurface {
    fn drop(&mut self) {
        // SAFETY: `self` holds one retain.
        unsafe { kairo_iosurface_release(self.0.as_ptr()) }
    }
}

/// One Syphon server. Dropping it closes it.
#[derive(Debug)]
pub struct Server(NonNull<RawServer>);

// SAFETY: the C side guards its state with a mutex and atomics; Syphon's
// server publishes through its own dispatch queue.
unsafe impl Send for Server {}
unsafe impl Sync for Server {}

/// Boxed completion for [`Server::publish`].
type Completion = Box<dyn FnOnce(Option<String>) + Send>;

extern "C" fn completed(context: *mut c_void, error: *const c_char) {
    // SAFETY: `context` is the `Box<Completion>` leaked by `publish`, handed
    // back exactly once by the completion handler.
    let done = unsafe { Box::from_raw(context.cast::<Completion>()) };
    let error = (!error.is_null())
        // SAFETY: a NUL-terminated message valid for this call.
        .then(|| {
            unsafe { CStr::from_ptr(error) }
                .to_string_lossy()
                .into_owned()
        });
    done(error);
}

impl Server {
    /// Starts a server called `name` ("Kairo" when empty). Other apps see it
    /// in their Syphon source list. Create it on a thread with a run loop
    /// (the main thread) so it answers discovery requests.
    pub fn open(name: &str) -> Result<Self, String> {
        let name = CString::new(name).map_err(|_| "open: the server name holds a NUL")?;
        // SAFETY: both buffers outlive the call.
        let (raw, error) = with_error(|e, n| unsafe { kairo_syphon_open(name.as_ptr(), e, n) });
        NonNull::new(raw).map(Self).ok_or(error)
    }

    /// GPU path: copies a BGRA `source` into the server on the GPU and
    /// publishes it when done. `Ok(false)`: the previous copy still runs, so
    /// this frame is dropped and `done` is not called. On `Ok(true)`, `done`
    /// runs on a Metal thread once the GPU no longer reads `source`.
    pub fn publish(
        &self,
        source: &IoSurface,
        done: impl FnOnce(Option<String>) + Send + 'static,
    ) -> Result<bool, String> {
        let context = Box::into_raw(Box::new(Box::new(done) as Completion));
        // SAFETY: `context` is reclaimed by `completed` only when the call
        // returned 1; otherwise here.
        let (code, error) = with_error(|e, n| unsafe {
            kairo_syphon_publish(
                self.0.as_ptr(),
                source.0.as_ptr(),
                Some(completed),
                context.cast(),
                e,
                n,
            )
        });
        if code != 1 {
            // SAFETY: not handed to the GPU; reclaimed once.
            drop(unsafe { Box::from_raw(context) });
        }
        match code {
            1 => Ok(true),
            0 => Ok(false),
            _ => Err(error),
        }
    }

    /// CPU path: BGRA rows `stride` bytes apart (0: tight). `Ok(false)`: a
    /// GPU copy runs, so the frame is dropped.
    pub fn publish_pixels(
        &self,
        bgra: &[u8],
        width: u32,
        height: u32,
        stride: usize,
    ) -> Result<bool, String> {
        // SAFETY: the slice and its length are passed together.
        let (code, error) = with_error(|e, n| unsafe {
            kairo_syphon_publish_pixels(
                self.0.as_ptr(),
                bgra.as_ptr(),
                bgra.len(),
                width,
                height,
                stride,
                e,
                n,
            )
        });
        match code {
            1 => Ok(true),
            0 => Ok(false),
            _ => Err(error),
        }
    }

    /// The server's own BGRA surface at `width` × `height`, to draw into
    /// directly; publish it with [`Server::publish_surface`].
    pub fn surface(&self, width: u32, height: u32) -> Result<IoSurface, String> {
        // SAFETY: the buffer outlives the call.
        let (raw, error) = with_error(|e, n| unsafe {
            kairo_syphon_surface(self.0.as_ptr(), width, height, e, n)
        });
        NonNull::new(raw).map(IoSurface).ok_or(error)
    }

    /// Publishes what was drawn into [`Server::surface`]. The GPU must have
    /// finished writing it.
    pub fn publish_surface(&self) -> Result<(), String> {
        // SAFETY: the buffer outlives the call.
        let (code, error) =
            with_error(|e, n| unsafe { kairo_syphon_publish_surface(self.0.as_ptr(), e, n) });
        if code == 1 { Ok(()) } else { Err(error) }
    }

    /// Counts a frame the caller dropped (`failed` false) or could not make.
    pub fn count(&self, failed: bool) {
        // SAFETY: the handle is live.
        unsafe { kairo_syphon_count(self.0.as_ptr(), failed) }
    }

    pub fn stats(&self) -> Stats {
        // SAFETY: the handle is live.
        unsafe { kairo_syphon_stats(self.0.as_ptr()) }
    }

    /// Stops the server (clients see it leave); later frames are refused.
    /// Waits for a copy in flight. Idempotent.
    pub fn close(&self) {
        // SAFETY: the handle is live.
        unsafe { kairo_syphon_close(self.0.as_ptr()) }
    }

    /// Connects a Syphon client, as OBS would, and reads the newest frame
    /// as tight BGRA rows. `None` when nothing arrives within `timeout_ms`.
    pub fn read_latest(&self, timeout_ms: f64) -> Result<Option<(u32, u32, Vec<u8>)>, String> {
        let mut buffer = Vec::new();
        loop {
            let (mut width, mut height) = (0u32, 0u32);
            // SAFETY: the output buffer and its length are passed together.
            let (code, error) = with_error(|e, n| unsafe {
                kairo_syphon_read_latest(
                    self.0.as_ptr(),
                    timeout_ms,
                    buffer.as_mut_ptr(),
                    buffer.len(),
                    &mut width,
                    &mut height,
                    e,
                    n,
                )
            });
            match code {
                1 => {
                    buffer.truncate(width as usize * height as usize * 4);
                    return Ok(Some((width, height, buffer)));
                }
                0 => return Ok(None),
                -2 => buffer = vec![0; width as usize * height as usize * 4],
                _ => return Err(error),
            }
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        // SAFETY: the handle is freed exactly once, here.
        unsafe { kairo_syphon_free(self.0.as_ptr()) }
    }
}

/// What a client saw of a server in another process.
#[derive(Debug)]
pub struct Received {
    pub width: u32,
    pub height: u32,
    /// The newest frame, tight BGRA rows.
    pub bgra: Vec<u8>,
    /// Frames the server announced while watched.
    pub frames: u64,
}

/// A Syphon client for the server called `name` in any process, found
/// through the Syphon directory as OBS finds it: waits up to `discover_ms`
/// for it, counts its frames for `watch_ms`, then reads the newest one. Call
/// it from the main thread: discovery needs the run loop.
pub fn receive(name: &str, discover_ms: f64, watch_ms: f64) -> Result<Received, String> {
    let name = CString::new(name).map_err(|_| "client: the server name holds a NUL")?;
    let mut buffer = Vec::new();
    loop {
        let (mut width, mut height, mut frames) = (0u32, 0u32, 0u64);
        // SAFETY: every buffer is passed with its length and outlives the call.
        let (code, error) = with_error(|e, n| unsafe {
            kairo_syphon_client_read(
                name.as_ptr(),
                discover_ms,
                watch_ms,
                buffer.as_mut_ptr(),
                buffer.len(),
                &mut width,
                &mut height,
                &mut frames,
                e,
                n,
            )
        });
        match code {
            1 => {
                buffer.truncate(width as usize * height as usize * 4);
                return Ok(Received {
                    width,
                    height,
                    bgra: buffer,
                    frames,
                });
            }
            -2 => buffer = vec![0; width as usize * height as usize * 4],
            _ => return Err(error),
        }
    }
}
