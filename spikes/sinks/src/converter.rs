//! Safe wrapper over the asynchronous IOSurface converter C ABI.

use std::ffi::{CStr, CString, c_char, c_int, c_void};

enum RawRing {}
enum RawConverter {}
type Done = unsafe extern "C" fn(*mut c_void, *const c_char, *const u8, usize, u32, u64);

unsafe extern "C" {
    fn kgf_ring_create(
        name: *const c_char,
        w: u32,
        h: u32,
        error: *mut c_char,
        len: usize,
    ) -> *mut RawRing;
    fn kgf_ring_close(ring: *mut RawRing);
    fn kgf_ring_free(ring: *mut RawRing);
    fn kgf_ring_read(
        ring: *mut RawRing,
        last: *mut u64,
        out: *mut u8,
        len: usize,
        even: *mut bool,
    ) -> c_int;
    fn kgf_open(w: u32, h: u32, alpha: bool, ring: *mut RawRing) -> *mut RawConverter;
    fn kgf_convert(
        c: *mut RawConverter,
        surface: *mut c_void,
        done: Done,
        user: *mut c_void,
    ) -> c_int;
    fn kgf_stats(c: *mut RawConverter, converted: *mut u64, skipped: *mut u64, failed: *mut u64);
    fn kgf_close(c: *mut RawConverter);
    fn kgf_test_surface(
        bytes: *const u8,
        len: usize,
        w: u32,
        h: u32,
        format: u32,
        aligned: bool,
    ) -> *mut c_void;
    fn kgf_test_surface_release(surface: *mut c_void);
}

pub const FORMAT_BGRA: u32 = u32::from_be_bytes(*b"BGRA");

#[derive(Debug)]
pub struct Completed {
    pub data: Option<Vec<u8>>,
    pub slot: u32,
    pub sequence: u64,
}

type Callback = Box<dyn FnOnce(Result<Completed, String>) + Send>;

unsafe extern "C" fn trampoline(
    user: *mut c_void,
    error: *const c_char,
    data: *const u8,
    length: usize,
    slot: u32,
    sequence: u64,
) {
    // SAFETY: `user` is the box leaked in `convert`, delivered exactly once.
    let callback = unsafe { Box::from_raw(user as *mut Callback) };
    let result = if error.is_null() {
        let data =
            (!data.is_null()).then(|| unsafe { std::slice::from_raw_parts(data, length) }.to_vec());
        Ok(Completed {
            data,
            slot,
            sequence,
        })
    } else {
        Err(unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned())
    };
    callback(result);
}

pub struct GpuRing(*mut RawRing);

impl GpuRing {
    pub fn create(name: &str, width: u32, height: u32) -> Result<Self, String> {
        let name = CString::new(name).map_err(|e| e.to_string())?;
        let mut error = [0 as c_char; 256];
        // SAFETY: the name is NUL-terminated and the error buffer is sized.
        let ring = unsafe {
            kgf_ring_create(
                name.as_ptr(),
                width,
                height,
                error.as_mut_ptr(),
                error.len(),
            )
        };
        if ring.is_null() {
            return Err(unsafe { CStr::from_ptr(error.as_ptr()) }
                .to_string_lossy()
                .into_owned());
        }
        Ok(Self(ring))
    }

    pub fn close(&self) {
        // SAFETY: the pointer is live until drop, and closing is idempotent.
        unsafe { kgf_ring_close(self.0) }
    }

    /// The newest frame if one is new, and whether every slot's sequence is even.
    pub fn read(&self, last: &mut u64, out: &mut [u8]) -> (bool, bool) {
        let mut even = false;
        // SAFETY: the length is checked in C against the ring's raster.
        let status = unsafe { kgf_ring_read(self.0, last, out.as_mut_ptr(), out.len(), &mut even) };
        (status == 1, even)
    }
}

impl Drop for GpuRing {
    fn drop(&mut self) {
        // SAFETY: released once; a converter holds its own reference.
        unsafe { kgf_ring_free(self.0) }
    }
}

pub struct Converter(*mut RawConverter);

// SAFETY: the C side synchronises slot state, and `Metal` objects are thread safe.
unsafe impl Send for Converter {}
unsafe impl Sync for Converter {}

impl Converter {
    pub fn open(width: u32, height: u32, alpha: bool, ring: Option<&GpuRing>) -> Option<Self> {
        let ring = ring.map_or(std::ptr::null_mut(), |r| r.0);
        // SAFETY: the ring is live and reference counted by the converter.
        let converter = unsafe { kgf_open(width, height, alpha, ring) };
        (!converter.is_null()).then_some(Self(converter))
    }

    /// `Ok(true)` started, `Ok(false)` dropped, and `done` runs only when started.
    pub fn convert(
        &self,
        surface: &TestSurface,
        done: impl FnOnce(Result<Completed, String>) + Send + 'static,
    ) -> Result<bool, i32> {
        let boxed: Callback = Box::new(done);
        let user = Box::into_raw(Box::new(boxed));
        // SAFETY: both pointers are live; the C side retains the surface as a
        // texture until completion.
        let status = unsafe { kgf_convert(self.0, surface.0, trampoline, user.cast()) };
        if status != 1 {
            // SAFETY: never handed to Metal, so ownership is still here.
            drop(unsafe { Box::from_raw(user) });
        }
        match status {
            1 => Ok(true),
            0 => Ok(false),
            error => Err(error),
        }
    }

    pub fn stats(&self) -> (u64, u64, u64) {
        let (mut converted, mut skipped, mut failed) = (0, 0, 0);
        // SAFETY: the out-pointers are live locals.
        unsafe { kgf_stats(self.0, &mut converted, &mut skipped, &mut failed) };
        (converted, skipped, failed)
    }
}

impl Drop for Converter {
    fn drop(&mut self) {
        // SAFETY: drains in-flight work, then frees exactly once.
        unsafe { kgf_close(self.0) }
    }
}

pub struct TestSurface(*mut c_void);

// SAFETY: IOSurface references may be used from any thread.
unsafe impl Send for TestSurface {}
unsafe impl Sync for TestSurface {}

impl TestSurface {
    pub fn new(bgra: &[u8], width: u32, height: u32, format: u32) -> Option<Self> {
        Self::with_alignment(bgra, width, height, format, true)
    }

    pub fn with_alignment(
        bgra: &[u8],
        width: u32,
        height: u32,
        format: u32,
        aligned: bool,
    ) -> Option<Self> {
        // SAFETY: the length is validated in C.
        let surface =
            unsafe { kgf_test_surface(bgra.as_ptr(), bgra.len(), width, height, format, aligned) };
        (!surface.is_null()).then_some(Self(surface))
    }

    pub fn null() -> Self {
        Self(std::ptr::null_mut())
    }
}

impl Drop for TestSurface {
    fn drop(&mut self) {
        // SAFETY: created retained by `kgf_test_surface`; null is ignored.
        unsafe { kgf_test_surface_release(self.0) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu;
    use std::sync::mpsc::{Receiver, channel};
    use std::time::Duration;

    fn gradient(width: u32, height: u32) -> Vec<u8> {
        (0..width * height)
            .flat_map(|i| [i as u8, (i >> 3) as u8, (i >> 7) as u8, 255])
            .collect()
    }

    fn start(c: &Converter, surface: &TestSurface) -> (bool, Receiver<Result<Completed, String>>) {
        let (send, receive) = channel();
        let started = c.convert(surface, move |r| send.send(r).unwrap()).unwrap();
        (started, receive)
    }

    fn wait(r: &Receiver<Result<Completed, String>>) -> Completed {
        r.recv_timeout(Duration::from_secs(10)).unwrap().unwrap()
    }

    #[test]
    fn iosurface_golden_vectors_match_uyvy_h() {
        let c = Converter::open(2, 1, false, None).unwrap();
        for (b, g, r, expected) in [
            (0, 0, 0, [128, 16, 128, 16]),
            (255, 255, 255, [127, 235, 128, 235]),
            (0, 0, 255, [102, 63, 240, 63]),
        ] {
            let pixels = [b, g, r, 255, b, g, r, 255];
            let surface = TestSurface::new(&pixels, 2, 1, FORMAT_BGRA).unwrap();
            let (started, done) = start(&c, &surface);
            assert!(started);
            assert_eq!(wait(&done).data.unwrap(), expected);
            assert_eq!(gpu::reference(&pixels, 2, 1, 2, 1), expected);
        }
    }

    #[test]
    fn iosurface_1080p_matches_reference_and_scales() {
        let bgra = gradient(1920, 1080);
        let surface = TestSurface::new(&bgra, 1920, 1080, FORMAT_BGRA).unwrap();
        for (w, h) in [(1920, 1080), (1280, 720)] {
            let c = Converter::open(w, h, false, None).unwrap();
            let (_, done) = start(&c, &surface);
            assert!(wait(&done).data.unwrap() == gpu::reference(&bgra, 1920, 1080, w, h));
        }
    }

    #[test]
    fn ring_mode_publishes_a_complete_frame() {
        let name = format!("/kc-{}", std::process::id());
        let ring = GpuRing::create(&name, 1920, 1080).unwrap();
        let bgra = gradient(1920, 1080);
        let surface = TestSurface::new(&bgra, 1920, 1080, FORMAT_BGRA).unwrap();
        let c = Converter::open(1920, 1080, false, Some(&ring)).unwrap();
        let (_, done) = start(&c, &surface);
        let completed = wait(&done);
        assert_eq!((completed.data.is_none(), completed.sequence), (true, 1));
        drop(c);
        let (mut last, mut out) = (0, vec![0; 1920 * 1080 * 2]);
        assert_eq!(ring.read(&mut last, &mut out), (true, true));
        assert!(out == gpu::reference(&bgra, 1920, 1080, 1920, 1080));
    }

    #[test]
    fn busy_converter_drops_instead_of_queueing() {
        let c = Converter::open(1920, 1080, true, None).unwrap();
        let surface = TestSurface::new(&gradient(1920, 1080), 1920, 1080, FORMAT_BGRA).unwrap();
        let (send, receive) = channel();
        let accepted: Vec<bool> = (0..6)
            .map(|_| {
                let send = send.clone();
                c.convert(&surface, move |r| {
                    send.send(r.unwrap().data.unwrap().len()).unwrap()
                })
                .unwrap()
            })
            .collect();
        assert_eq!(accepted, [true, true, true, false, false, false]);
        for _ in 0..3 {
            assert_eq!(
                receive.recv_timeout(Duration::from_secs(10)).unwrap(),
                1920 * 1080 * 3
            );
        }
        assert_eq!(c.stats(), (3, 3, 0));
    }

    #[test]
    fn ring_never_overwrites_the_newest_slot() {
        let name = format!("/ks-{}", std::process::id());
        let ring = GpuRing::create(&name, 2, 1).unwrap();
        let surface = TestSurface::new(&[0; 8], 2, 1, FORMAT_BGRA).unwrap();
        let c = Converter::open(2, 1, false, Some(&ring)).unwrap();
        let mut previous = u32::MAX;
        for _ in 0..12 {
            let (_, done) = start(&c, &surface);
            let slot = wait(&done).slot;
            assert_ne!(slot, previous);
            previous = slot;
        }
    }

    #[test]
    fn closing_waits_for_in_flight_work() {
        let c = Converter::open(1920, 1080, true, None).unwrap();
        let surface = TestSurface::new(&gradient(1920, 1080), 1920, 1080, FORMAT_BGRA).unwrap();
        let (started, done) = start(&c, &surface);
        assert!(started);
        drop(c);
        assert!(done.try_recv().unwrap().is_ok());
    }

    #[test]
    fn a_closed_ring_drops_quietly() {
        let name = format!("/kq-{}", std::process::id());
        let ring = GpuRing::create(&name, 2, 1).unwrap();
        let surface = TestSurface::new(&[0; 8], 2, 1, FORMAT_BGRA).unwrap();
        let c = Converter::open(2, 1, false, Some(&ring)).unwrap();
        ring.close();
        assert_eq!(c.convert(&surface, |_| panic!("not called")), Ok(false));
        assert_eq!(c.stats().1, 1);
    }

    #[test]
    fn invalid_inputs_are_errors() {
        assert!(Converter::open(3, 1, false, None).is_none());
        assert!(TestSurface::new(&[0; 7], 2, 1, FORMAT_BGRA).is_none());
        let name = format!("/kv-{}", std::process::id());
        let ring = GpuRing::create(&name, 2, 1).unwrap();
        assert!(Converter::open(2, 1, true, Some(&ring)).is_none());
        assert!(Converter::open(4, 1, false, Some(&ring)).is_none());
        assert!(GpuRing::create("bad", 2, 1).is_err());
        let c = Converter::open(2, 1, false, None).unwrap();
        assert_eq!(c.convert(&TestSurface::null(), |_| {}), Err(-2));
        let wrong = TestSurface::new(&[0; 8], 2, 1, u32::from_be_bytes(*b"RGBA")).unwrap();
        assert_eq!(c.convert(&wrong, |_| {}), Err(-3));
        assert_eq!(c.stats().2, 1);
        let unaligned = TestSurface::with_alignment(&[0; 8], 2, 1, FORMAT_BGRA, false).unwrap();
        assert_eq!(c.convert(&unaligned, |_| {}), Err(-5));
        assert_eq!(c.stats().2, 2);
    }
}
