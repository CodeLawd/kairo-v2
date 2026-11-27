//! A thin `libloading` binding to the official NDI 6 runtime (decision 008).
//!
//! The runtime is never linked: it is opened at run time from the app's own
//! folder, so a missing or broken NDI disables NDI only (Electron decision
//! 001). No SDK file is in this repository; struct layouts follow the NDI
//! 6.3.2 headers and are checked by tests.

use libloading::Library;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::Arc;

const fn fourcc(a: u8, b: u8, c: u8, d: u8) -> u32 {
    u32::from_le_bytes([a, b, c, d])
}
/// `NDIlib_FourCC_video_type_UYVA`: a UYVY plane, then an alpha plane.
pub const UYVA: u32 = fourcc(b'U', b'Y', b'V', b'A');
/// `NDIlib_FourCC_video_type_BGRA`.
pub const BGRA: u32 = fourcc(b'B', b'G', b'R', b'A');
/// `NDIlib_FourCC_audio_type_FLTP`: planar 32-bit float.
pub const FLTP: u32 = fourcc(b'F', b'L', b'T', b'p');
/// `NDIlib_send_timecode_synthesize`.
const SYNTHESIZE: i64 = i64::MAX;
/// 29.97 fps, as Electron declared its frames.
pub const FRAME_RATE: (i32, i32) = (30000, 1001);
const PROGRESSIVE: c_int = 1;
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

/// `NDIlib_video_frame_v2_t`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VideoFrame {
    pub xres: c_int,
    pub yres: c_int,
    pub fourcc: u32,
    pub frame_rate_n: c_int,
    pub frame_rate_d: c_int,
    pub aspect: f32,
    pub format: c_int,
    pub timecode: i64,
    pub data: *mut u8,
    pub stride: c_int,
    pub metadata: *const c_char,
    pub timestamp: i64,
}

/// `NDIlib_audio_frame_v3_t`.
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

type Instance = *mut c_void;
/// Source names seen, and the first wanted one's name and URL.
pub type Found = (Vec<String>, Option<(CString, CString)>);

/// The library and the symbols the sender needs, resolved once.
struct Api {
    send_create: unsafe extern "C" fn(*const SendCreate) -> Instance,
    send_destroy: unsafe extern "C" fn(Instance),
    send_video: unsafe extern "C" fn(Instance, *const VideoFrame),
    send_audio: unsafe extern "C" fn(Instance, *const AudioFrame),
    version: unsafe extern "C" fn() -> *const c_char,
    destroy: unsafe extern "C" fn(),
    // Kept last so it is dropped after every use of the pointers above.
    library: Library,
}

/// A loaded NDI runtime.
pub struct Runtime {
    api: Api,
    pub path: PathBuf,
}

/// The runtime's file name on this platform.
pub fn library_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "libndi.dylib"
    } else if cfg!(windows) {
        "Processing.NDI.Lib.x64.dll"
    } else {
        "libndi.so.6"
    }
}

/// Where the runtime is looked for, in order: `KAIRO_NDI_RUNTIME` (a file),
/// then the app's own folders, never a system path (decision 001 keeps the
/// runtime in the app folder): beside the executable, then the bundle's
/// `Frameworks` (macOS) or `lib` folder.
pub fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(path) = std::env::var_os("KAIRO_NDI_RUNTIME") {
        out.push(PathBuf::from(path));
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    {
        out.push(dir.join(library_name()));
        out.push(dir.join("../Frameworks").join(library_name()));
        out.push(dir.join("../../../../Frameworks").join(library_name()));
        out.push(dir.join("../lib").join(library_name()));
    }
    out
}

macro_rules! get {
    ($library:ident, $name:literal) => {{
        // SAFETY: the signature at each use matches the NDI 6.3.2 header.
        *unsafe { $library.get(concat!($name, "\0").as_bytes()) }
            .map_err(|e| format!("{}: {e}", $name))?
    }};
}

impl Runtime {
    /// The first candidate that loads, with every failure in the message.
    pub fn find() -> Result<Arc<Self>, String> {
        let mut tried = Vec::new();
        for path in candidates() {
            if !path.is_file() {
                continue;
            }
            match Self::load(&path) {
                Ok(runtime) => return Ok(Arc::new(runtime)),
                Err(e) => tried.push(format!("{}: {e}", path.display())),
            }
        }
        if tried.is_empty() {
            Err(format!("no {} in the app folder", library_name()))
        } else {
            Err(tried.join("; "))
        }
    }

    /// Loads one file; there is no search and no global install.
    pub fn load(path: &Path) -> Result<Self, String> {
        // SAFETY: loading the vendor library runs its initialisers only.
        let library = unsafe { Library::new(path) }.map_err(|e| format!("load failed: {e}"))?;
        let initialize: unsafe extern "C" fn() -> bool = get!(library, "NDIlib_initialize");
        let api = Api {
            send_create: get!(library, "NDIlib_send_create"),
            send_destroy: get!(library, "NDIlib_send_destroy"),
            send_video: get!(library, "NDIlib_send_send_video_v2"),
            send_audio: get!(library, "NDIlib_send_send_audio_v3"),
            version: get!(library, "NDIlib_version"),
            destroy: get!(library, "NDIlib_destroy"),
            library,
        };
        // SAFETY: no arguments; false means an unsupported CPU.
        if !unsafe { initialize() } {
            return Err("NDI could not initialise on this CPU".into());
        }
        Ok(Self {
            api,
            path: path.to_path_buf(),
        })
    }

    pub fn version(&self) -> String {
        // SAFETY: returns a static NUL-terminated string.
        unsafe { CStr::from_ptr((self.api.version)()) }
            .to_string_lossy()
            .into_owned()
    }

    /// A sender named `name`, clocked to the video frame rate.
    pub fn sender(self: &Arc<Self>, name: &str) -> Result<Sender, String> {
        let name = CString::new(name).map_err(|_| "the NDI name has a NUL byte".to_owned())?;
        let settings = SendCreate {
            name: name.as_ptr(),
            groups: null(),
            clock_video: true,
            clock_audio: false,
        };
        // SAFETY: settings outlive the call, which copies them.
        let instance = unsafe { (self.api.send_create)(&settings) };
        if instance.is_null() {
            return Err("NDI could not create the sender".into());
        }
        Ok(Sender {
            runtime: self.clone(),
            instance,
        })
    }

    /// Source names seen within `timeout_ms`, and the first containing
    /// `wanted` (name and URL), as a receiver on the network sees them.
    pub fn find_sources(&self, timeout_ms: u32, wanted: &str) -> Result<Found, String> {
        let library = &self.api.library;
        let create: unsafe extern "C" fn(*const FindCreate) -> Instance =
            get!(library, "NDIlib_find_create_v2");
        let wait: unsafe extern "C" fn(Instance, u32) -> bool =
            get!(library, "NDIlib_find_wait_for_sources");
        let current: unsafe extern "C" fn(Instance, *mut u32) -> *const Source =
            get!(library, "NDIlib_find_get_current_sources");
        let destroy: unsafe extern "C" fn(Instance) = get!(library, "NDIlib_find_destroy");
        let settings = FindCreate {
            show_local: true,
            groups: null(),
            extra_ips: null(),
        };
        // SAFETY: the finder is checked and destroyed once; source strings
        // are copied before `destroy` invalidates them.
        unsafe {
            let finder = create(&settings);
            if finder.is_null() {
                return Err("NDI could not create a finder".into());
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

    /// Receives one video frame from the source named like `wanted`, as a
    /// receiver on the network would; `inspect` sees it before it is freed.
    pub fn receive_one(
        &self,
        wanted: &str,
        timeout_ms: u32,
        mut inspect: impl FnMut(&VideoFrame),
    ) -> Result<bool, String> {
        let (_, matched) = self.find_sources(timeout_ms, wanted)?;
        let Some((name, url)) = matched else {
            return Ok(false);
        };
        let library = &self.api.library;
        let create: unsafe extern "C" fn(*const RecvCreate) -> Instance =
            get!(library, "NDIlib_recv_create_v3");
        let capture: unsafe extern "C" fn(
            Instance,
            *mut VideoFrame,
            *mut c_void,
            *mut c_void,
            u32,
        ) -> c_int = get!(library, "NDIlib_recv_capture_v3");
        let free: unsafe extern "C" fn(Instance, *const VideoFrame) =
            get!(library, "NDIlib_recv_free_video_v2");
        let destroy: unsafe extern "C" fn(Instance) = get!(library, "NDIlib_recv_destroy");
        let settings = RecvCreate {
            source: Source {
                name: name.as_ptr(),
                url: if url.is_empty() { null() } else { url.as_ptr() },
            },
            // `NDIlib_recv_color_format_fastest`: UYVA as sent.
            color_format: 100,
            bandwidth: 100,
            allow_fields: false,
            name: null(),
        };
        // SAFETY: each captured frame is freed; the receiver is destroyed once.
        unsafe {
            let receiver = create(&settings);
            if receiver.is_null() {
                return Err("NDI could not create a receiver".into());
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

impl Drop for Runtime {
    fn drop(&mut self) {
        // SAFETY: every sender holds an `Arc<Runtime>`, so none remains.
        unsafe { (self.api.destroy)() };
    }
}

/// One NDI source on the network. Dropping it removes the source.
pub struct Sender {
    runtime: Arc<Runtime>,
    instance: Instance,
}

// SAFETY: the NDI headers document sending as safe from any thread, and
// video and audio may be sent from different threads at once.
unsafe impl Send for Sender {}
unsafe impl Sync for Sender {}

impl Sender {
    /// One frame. `data` holds `stride × height` bytes, plus the alpha plane
    /// for UYVA. With the video clock on, this waits for the frame's turn.
    pub fn send_video(
        &self,
        width: u32,
        height: u32,
        fourcc: u32,
        data: &[u8],
    ) -> Result<(), String> {
        let (stride, expected) = match fourcc {
            UYVA => (width * 2, width as usize * height as usize * 3),
            BGRA => (width * 4, width as usize * height as usize * 4),
            _ => return Err("unsupported NDI video format".into()),
        };
        if width == 0 || height == 0 || data.len() != expected {
            return Err("video frame size does not match its raster".into());
        }
        let frame = VideoFrame {
            xres: width as c_int,
            yres: height as c_int,
            fourcc,
            frame_rate_n: FRAME_RATE.0,
            frame_rate_d: FRAME_RATE.1,
            aspect: width as f32 / height as f32,
            format: PROGRESSIVE,
            timecode: SYNTHESIZE,
            data: data.as_ptr().cast_mut(),
            stride: stride as c_int,
            metadata: null(),
            timestamp: 0,
        };
        // SAFETY: the synchronous variant copies before returning.
        unsafe { (self.runtime.api.send_video)(self.instance, &frame) };
        Ok(())
    }

    /// Planar 32-bit float: every channel's samples back to back.
    pub fn send_audio(
        &self,
        sample_rate: u32,
        channels: u32,
        samples: u32,
        data: &[f32],
    ) -> Result<(), String> {
        if channels == 0 || samples == 0 || data.len() != (channels * samples) as usize {
            return Err("audio frame size does not match its layout".into());
        }
        let frame = AudioFrame {
            sample_rate: sample_rate as c_int,
            channels: channels as c_int,
            samples: samples as c_int,
            timecode: SYNTHESIZE,
            fourcc: FLTP,
            data: data.as_ptr().cast_mut().cast(),
            stride: (samples * 4) as c_int,
            metadata: null(),
            timestamp: 0,
        };
        // SAFETY: synchronous; the slice length was checked.
        unsafe { (self.runtime.api.send_audio)(self.instance, &frame) };
        Ok(())
    }
}

impl Drop for Sender {
    fn drop(&mut self) {
        // SAFETY: the instance is valid and destroyed exactly once.
        unsafe { (self.runtime.api.send_destroy)(self.instance) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    #[test]
    fn layouts_match_the_lp64_header() {
        assert_eq!(UYVA, 0x4156_5955);
        assert_eq!(UYVA, 1_096_178_005, "Electron's FOURCC_UYVA");
        assert_eq!(BGRA, 1_095_911_234, "Electron's FOURCC_BGRA");
        assert_eq!(FLTP, 1_884_572_742, "Electron's FOURCC_AUDIO_FLTP");
        assert_eq!(offset_of!(VideoFrame, data), 40);
        assert_eq!(offset_of!(VideoFrame, stride), 48);
        assert_eq!(size_of::<VideoFrame>(), 72);
        assert_eq!(offset_of!(AudioFrame, data), 32);
        assert_eq!(size_of::<AudioFrame>(), 64);
        assert_eq!(size_of::<RecvCreate>(), 40);
        assert_eq!(size_of::<SendCreate>(), 24);
    }

    #[test]
    fn a_missing_library_is_an_error_not_a_crash() {
        let error = Runtime::load(Path::new("/nonexistent/libndi.dylib"))
            .err()
            .unwrap();
        assert!(error.starts_with("load failed"), "{error}");
        // A file that is not a library.
        let dir = std::env::temp_dir().join(format!("kairo-ndi-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bogus = dir.join(library_name());
        std::fs::write(&bogus, b"not a library").unwrap();
        assert!(Runtime::load(&bogus).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_runtime_is_looked_for_in_the_app_folder_only() {
        let c = candidates();
        assert!(
            c.iter()
                .all(|p| !p.starts_with("/usr") && !p.starts_with("/Library"))
        );
        assert!(c.iter().any(|p| p.ends_with(library_name())));
    }
}
