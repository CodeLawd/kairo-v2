//! Thin `libloading` binding. Layouts follow the NDI 6.3.2 headers.

use libloading::{Library, Symbol};
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::Path;
use std::ptr::{null, null_mut};

const fn fourcc(a: u8, b: u8, c: u8, d: u8) -> u32 {
    u32::from_le_bytes([a, b, c, d])
}
pub const UYVA: u32 = fourcc(b'U', b'Y', b'V', b'A');
pub const BGRA: u32 = fourcc(b'B', b'G', b'R', b'A');
pub const FLTP: u32 = fourcc(b'F', b'L', b'T', b'p');
/// `NDIlib_send_timecode_synthesize`.
const SYNTHESIZE: i64 = i64::MAX;
const FRAME_VIDEO: c_int = 1;
const FRAME_ERROR: c_int = 4;

#[repr(C)]
struct SendCreate {
    name: *const c_char,
    groups: *const c_char,
    clock_video: bool,
    clock_audio: bool,
}

#[repr(C)]
struct FindCreate {
    show_local: bool,
    groups: *const c_char,
    extra_ips: *const c_char,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Source {
    name: *const c_char,
    url: *const c_char,
}

#[repr(C)]
struct RecvCreate {
    source: Source,
    color_format: c_int,
    bandwidth: c_int,
    allow_fields: bool,
    name: *const c_char,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VideoFrame {
    pub xres: c_int,
    pub yres: c_int,
    pub fourcc: u32,
    frame_rate_n: c_int,
    frame_rate_d: c_int,
    aspect: f32,
    format: c_int,
    timecode: i64,
    pub data: *mut u8,
    pub stride: c_int,
    metadata: *const c_char,
    timestamp: i64,
}

#[repr(C)]
struct AudioFrame {
    sample_rate: c_int,
    channels: c_int,
    samples: c_int,
    timecode: i64,
    fourcc: u32,
    data: *mut u8,
    stride: c_int,
    metadata: *const c_char,
    timestamp: i64,
}

impl VideoFrame {
    fn outgoing(width: i32, height: i32, fourcc: u32, stride: i32, data: *mut u8) -> Self {
        Self {
            xres: width,
            yres: height,
            fourcc,
            frame_rate_n: 30000,
            frame_rate_d: 1001,
            aspect: width as f32 / height as f32,
            format: 1,
            timecode: SYNTHESIZE,
            data,
            stride,
            metadata: null(),
            timestamp: 0,
        }
    }
}

pub struct Ndi {
    library: Library,
}

type Instance = *mut c_void;
type Found = (Vec<String>, Option<(CString, CString)>);

macro_rules! symbol {
    ($self:ident, $name:literal, $ty:ty) => {{
        // SAFETY: the signature is copied from the NDI 6.3.2 header.
        let symbol: Symbol<$ty> = unsafe { $self.library.get(concat!($name, "\0").as_bytes()) }
            .map_err(|e| e.to_string())?;
        symbol
    }};
}

impl Ndi {
    /// Loads from an explicit path; there is no search and no global install.
    pub fn load(path: &Path) -> Result<Self, String> {
        // SAFETY: loading the vendor library runs its initialisers only.
        let library = unsafe { Library::new(path) }.map_err(|e| format!("load failed: {e}"))?;
        let ndi = Self { library };
        let ok = {
            let initialize = symbol!(ndi, "NDIlib_initialize", unsafe extern "C" fn() -> bool);
            // SAFETY: no arguments; false means an unsupported CPU.
            unsafe { initialize() }
        };
        if !ok {
            return Err("NDI could not initialise on this CPU".into());
        }
        Ok(ndi)
    }

    pub fn version(&self) -> Result<String, String> {
        let version = symbol!(
            self,
            "NDIlib_version",
            unsafe extern "C" fn() -> *const c_char
        );
        // SAFETY: returns a static NUL-terminated string.
        Ok(unsafe { CStr::from_ptr(version()) }
            .to_string_lossy()
            .into_owned())
    }

    pub fn sender(&self, name: &str) -> Result<Sender<'_>, String> {
        let name = CString::new(name).map_err(|e| e.to_string())?;
        let create = symbol!(
            self,
            "NDIlib_send_create",
            unsafe extern "C" fn(*const SendCreate) -> Instance
        );
        let settings = SendCreate {
            name: name.as_ptr(),
            groups: null(),
            clock_video: false,
            clock_audio: false,
        };
        // SAFETY: settings outlive the call, which copies them.
        let instance = unsafe { create(&settings) };
        if instance.is_null() {
            return Err("sender creation failed".into());
        }
        Ok(Sender {
            ndi: self,
            instance,
        })
    }

    /// Source names seen within the timeout.
    pub fn find(&self, timeout_ms: u32, wanted: &str) -> Result<Vec<String>, String> {
        Ok(self.find_source(timeout_ms, wanted)?.0)
    }

    fn find_source(&self, timeout_ms: u32, wanted: &str) -> Result<Found, String> {
        let create = symbol!(
            self,
            "NDIlib_find_create_v2",
            unsafe extern "C" fn(*const FindCreate) -> Instance
        );
        let wait = symbol!(
            self,
            "NDIlib_find_wait_for_sources",
            unsafe extern "C" fn(Instance, u32) -> bool
        );
        let current = symbol!(
            self,
            "NDIlib_find_get_current_sources",
            unsafe extern "C" fn(Instance, *mut u32) -> *const Source
        );
        let destroy = symbol!(self, "NDIlib_find_destroy", unsafe extern "C" fn(Instance));
        let settings = FindCreate {
            show_local: true,
            groups: null(),
            extra_ips: null(),
        };
        // SAFETY: the instance is checked and destroyed below; source strings
        // are copied before `destroy` invalidates them.
        unsafe {
            let finder = create(&settings);
            if finder.is_null() {
                return Err("finder creation failed".into());
            }
            let deadline =
                std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms.into());
            let mut names = Vec::new();
            let mut matched = None;
            while std::time::Instant::now() < deadline && matched.is_none() {
                wait(finder, 200);
                let mut count = 0;
                let sources = current(finder, &mut count);
                names.clear();
                for index in 0..count as usize {
                    let source = *sources.add(index);
                    let name = CStr::from_ptr(source.name);
                    names.push(name.to_string_lossy().into_owned());
                    if !wanted.is_empty() && name.to_string_lossy().contains(wanted) {
                        let url = if source.url.is_null() {
                            CString::default()
                        } else {
                            CStr::from_ptr(source.url).to_owned()
                        };
                        matched = Some((name.to_owned(), url));
                    }
                }
            }
            destroy(finder);
            Ok((names, matched))
        }
    }

    /// Receives one video frame from the named source, as a loopback check.
    pub fn receive_one(
        &self,
        wanted: &str,
        timeout_ms: u32,
        mut inspect: impl FnMut(&VideoFrame),
    ) -> Result<bool, String> {
        let (_, matched) = self.find_source(timeout_ms, wanted)?;
        let Some((name, url)) = matched else {
            return Ok(false);
        };
        let create = symbol!(
            self,
            "NDIlib_recv_create_v3",
            unsafe extern "C" fn(*const RecvCreate) -> Instance
        );
        let capture = symbol!(
            self,
            "NDIlib_recv_capture_v3",
            unsafe extern "C" fn(Instance, *mut VideoFrame, *mut c_void, *mut c_void, u32) -> c_int
        );
        let free = symbol!(
            self,
            "NDIlib_recv_free_video_v2",
            unsafe extern "C" fn(Instance, *const VideoFrame)
        );
        let destroy = symbol!(self, "NDIlib_recv_destroy", unsafe extern "C" fn(Instance));
        let settings = RecvCreate {
            source: Source {
                name: name.as_ptr(),
                url: if url.is_empty() { null() } else { url.as_ptr() },
            },
            color_format: 1,
            bandwidth: 100,
            allow_fields: false,
            name: null(),
        };
        // SAFETY: each captured frame is freed; the receiver is destroyed once.
        unsafe {
            let receiver = create(&settings);
            if receiver.is_null() {
                return Err("receiver creation failed".into());
            }
            let deadline =
                std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms.into());
            let mut received = false;
            while std::time::Instant::now() < deadline && !received {
                let mut frame = std::mem::zeroed::<VideoFrame>();
                match capture(receiver, &mut frame, null_mut(), null_mut(), 200) {
                    FRAME_VIDEO => {
                        inspect(&frame);
                        free(receiver, &frame);
                        received = true;
                    }
                    FRAME_ERROR => break,
                    _ => {}
                }
            }
            destroy(receiver);
            Ok(received)
        }
    }
}

impl Drop for Ndi {
    fn drop(&mut self) {
        // SAFETY: senders borrow `Ndi`, so none remains at this point.
        if let Ok(destroy) = unsafe {
            self.library
                .get::<unsafe extern "C" fn()>(b"NDIlib_destroy\0")
        } {
            unsafe { destroy() };
        }
    }
}

pub struct Sender<'a> {
    ndi: &'a Ndi,
    instance: Instance,
}

// SAFETY: the NDI headers document sending as safe from any thread.
unsafe impl Sync for Sender<'_> {}

impl Sender<'_> {
    /// `data` holds `stride * height` bytes, plus the alpha plane for UYVA.
    pub fn send_video(
        &self,
        width: u32,
        height: u32,
        fourcc: u32,
        stride: u32,
        data: &[u8],
    ) -> Result<(), String> {
        let expected = stride as usize * height as usize
            + if fourcc == UYVA {
                (width * height) as usize
            } else {
                0
            };
        if data.len() != expected || width == 0 || height == 0 || stride < width * 2 {
            return Err("video frame size does not match its raster".into());
        }
        let ndi = self.ndi;
        let send = symbol!(
            ndi,
            "NDIlib_send_send_video_v2",
            unsafe extern "C" fn(Instance, *const VideoFrame)
        );
        let frame = VideoFrame::outgoing(
            width as i32,
            height as i32,
            fourcc,
            stride as i32,
            data.as_ptr().cast_mut(),
        );
        // SAFETY: the synchronous variant copies before returning.
        unsafe { send(self.instance, &frame) };
        Ok(())
    }

    /// Planar 32-bit float: every channel's samples back to back.
    pub fn send_audio(
        &self,
        sample_rate: i32,
        channels: i32,
        samples: i32,
        data: &[f32],
    ) -> Result<(), String> {
        if channels < 1 || samples < 1 || data.len() != (channels * samples) as usize {
            return Err("audio frame size does not match its layout".into());
        }
        let ndi = self.ndi;
        let send = symbol!(
            ndi,
            "NDIlib_send_send_audio_v3",
            unsafe extern "C" fn(Instance, *const AudioFrame)
        );
        let frame = AudioFrame {
            sample_rate,
            channels,
            samples,
            timecode: SYNTHESIZE,
            fourcc: FLTP,
            data: data.as_ptr().cast_mut().cast(),
            stride: samples * 4,
            metadata: null(),
            timestamp: 0,
        };
        // SAFETY: the call is synchronous and the slice length was validated.
        unsafe { send(self.instance, &frame) };
        Ok(())
    }
}

impl Drop for Sender<'_> {
    fn drop(&mut self) {
        // SAFETY: the instance is valid and destroyed exactly once.
        if let Ok(destroy) = unsafe {
            self.ndi
                .library
                .get::<unsafe extern "C" fn(Instance)>(b"NDIlib_send_destroy\0")
        } {
            unsafe { destroy(self.instance) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    #[test]
    fn layouts_match_the_lp64_header() {
        assert_eq!(UYVA, 0x41565955);
        assert_eq!(offset_of!(VideoFrame, data), 40);
        assert_eq!(offset_of!(VideoFrame, stride), 48);
        assert_eq!(size_of::<VideoFrame>(), 72);
        assert_eq!(offset_of!(AudioFrame, data), 32);
        assert_eq!(size_of::<AudioFrame>(), 64);
        assert_eq!(size_of::<RecvCreate>(), 40);
    }

    #[test]
    fn a_missing_library_is_an_error_not_a_crash() {
        assert!(Ndi::load(Path::new("/nonexistent/libndi.dylib")).is_err());
    }
}
