//! Safe Rust over `native/recorder.mm`'s C ABI (macOS only).

use std::ffi::{CStr, CString, c_char, c_void};
use std::sync::mpsc;
use std::time::Duration;

#[repr(C)]
struct Options {
    path: *const c_char,
    width: i32,
    height: i32,
    fps: i32,
    codec: *const c_char,
    bitrate: f64,
    audio_device_id: *const c_char,
}

/// `stats(recorder)`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    pub frames: u64,
    pub late: u64,
    pub skipped: u64,
    pub failed: u64,
    pub audio_buffers: u64,
    pub seconds: f64,
}

#[repr(C)]
#[derive(Default)]
struct RawProbe {
    seconds: f64,
    width: i32,
    height: i32,
    frames: u32,
    codec: [c_char; 5],
    audio: bool,
}

enum Raw {}
type Done = unsafe extern "C" fn(*mut c_void, *const c_char, u64, f64);
type Input = unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char);

unsafe extern "C" {
    fn kr_available(reason: *mut c_char, len: usize) -> bool;
    fn kr_audio_inputs(each: Input, ctx: *mut c_void);
    fn kr_microphone_access() -> bool;
    fn kr_open(o: *const Options, error: *mut c_char, len: usize) -> *mut Raw;
    fn kr_push_pixels(
        r: *mut Raw,
        bgra: *const u8,
        w: u32,
        h: u32,
        stride: usize,
        len: usize,
    ) -> i32;
    fn kr_stop(r: *mut Raw, done: Done, ctx: *mut c_void) -> bool;
    fn kr_stats(r: *mut Raw, out: *mut Stats, error: *mut c_char, len: usize);
    fn kr_free(r: *mut Raw);
    fn kr_probe(
        path: *const c_char,
        out: *mut RawProbe,
        first: *mut u8,
        last: *mut u8,
        cap: usize,
    ) -> bool;
}

fn text(buffer: &[c_char]) -> String {
    // SAFETY: the native side NUL-terminates within the buffer.
    unsafe { CStr::from_ptr(buffer.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}

/// `available()`: `Err(reason)` when recording cannot run.
pub fn available() -> Result<(), String> {
    let mut reason = [0 as c_char; 256];
    // SAFETY: the buffer and its length are valid.
    if unsafe { kr_available(reason.as_mut_ptr(), reason.len()) } {
        Ok(())
    } else {
        Err(text(&reason))
    }
}

/// `audioInputs()`: `(id, name)` of each audio input.
pub fn audio_inputs() -> Vec<(String, String)> {
    unsafe extern "C" fn each(ctx: *mut c_void, id: *const c_char, name: *const c_char) {
        // SAFETY: `ctx` is the Vec below; the strings live for the call.
        let out = unsafe { &mut *ctx.cast::<Vec<(String, String)>>() };
        let read = |p: *const c_char| {
            if p.is_null() {
                String::new()
            } else {
                unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
            }
        };
        out.push((read(id), read(name)));
    }
    let mut out: Vec<(String, String)> = Vec::new();
    // SAFETY: the callback runs synchronously, before `out` goes.
    unsafe { kr_audio_inputs(each, (&mut out as *mut Vec<(String, String)>).cast()) };
    out
}

/// Asks for the microphone permission if it was never asked.
pub fn microphone_access() -> bool {
    // SAFETY: no arguments.
    unsafe { kr_microphone_access() }
}

/// What a recording is made with.
#[derive(Clone, Debug, PartialEq)]
pub struct Open<'a> {
    pub path: &'a str,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// `h264`, `hevc`, `prores422` or `prores4444`.
    pub codec: &'a str,
    /// Bits a second for H.264 and HEVC; 0 lets the encoder choose.
    pub bitrate: f64,
    /// Empty: no sound.
    pub audio_device_id: &'a str,
}

/// How `push` went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pushed {
    Taken,
    /// Busy or out of buffers: this frame is dropped; the next carries newer pixels.
    Dropped,
    /// Stopped, or the bytes do not match their size.
    Refused,
}

/// The finished file.
#[derive(Clone, Debug, PartialEq)]
pub struct Finished {
    pub frames: u64,
    pub seconds: f64,
}

/// One recording. Dropped without [`Recorder::stop`], the file is abandoned.
pub struct Recorder {
    raw: *mut Raw,
    path: String,
}

// SAFETY: the native recorder guards its state with its own lock and queues;
// pushes and stats may come from any thread.
unsafe impl Send for Recorder {}
unsafe impl Sync for Recorder {}

impl Recorder {
    /// `open(options)`: starts writing at once.
    pub fn open(o: &Open) -> Result<Self, String> {
        let path = CString::new(o.path).map_err(|_| "open: the path has a NUL byte".to_owned())?;
        let codec = CString::new(o.codec).map_err(|_| "open: unknown codec".to_owned())?;
        let audio =
            CString::new(o.audio_device_id).map_err(|_| "open: bad audio input".to_owned())?;
        let options = Options {
            path: path.as_ptr(),
            width: o.width as i32,
            height: o.height as i32,
            fps: o.fps as i32,
            codec: codec.as_ptr(),
            bitrate: o.bitrate,
            audio_device_id: audio.as_ptr(),
        };
        let mut error = [0 as c_char; 512];
        // SAFETY: every pointer is valid for the call, which copies what it keeps.
        let raw = unsafe { kr_open(&options, error.as_mut_ptr(), error.len()) };
        if raw.is_null() {
            return Err(text(&error));
        }
        Ok(Self {
            raw,
            path: o.path.to_owned(),
        })
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    /// Premultiplied BGRA rows of `stride` bytes; scaled when the size differs.
    pub fn push(&self, bgra: &[u8], width: u32, height: u32, stride: usize) -> Pushed {
        // SAFETY: the slice and its length are passed together.
        match unsafe { kr_push_pixels(self.raw, bgra.as_ptr(), width, height, stride, bgra.len()) }
        {
            1 => Pushed::Taken,
            0 => Pushed::Dropped,
            _ => Pushed::Refused,
        }
    }

    /// The counts, and the first writer error.
    pub fn stats(&self) -> (Stats, Option<String>) {
        let mut stats = Stats::default();
        let mut error = [0 as c_char; 512];
        // SAFETY: valid out pointers.
        unsafe { kr_stats(self.raw, &mut stats, error.as_mut_ptr(), error.len()) };
        let error = text(&error);
        (stats, (!error.is_empty()).then_some(error))
    }

    /// Finishes the file, waiting at most `wait` for the encoder to flush.
    pub fn stop(self, wait: Duration) -> Result<Finished, String> {
        type Reply = Result<Finished, String>;
        unsafe extern "C" fn done(
            ctx: *mut c_void,
            error: *const c_char,
            frames: u64,
            seconds: f64,
        ) {
            // SAFETY: `ctx` is the boxed sender below, taken back exactly once.
            let tx = unsafe { Box::from_raw(ctx.cast::<mpsc::Sender<Reply>>()) };
            let reply = if error.is_null() {
                Ok(Finished { frames, seconds })
            } else {
                Err(unsafe { CStr::from_ptr(error) }
                    .to_string_lossy()
                    .into_owned())
            };
            let _ = tx.send(reply);
        }
        let (tx, rx) = mpsc::channel::<Reply>();
        let ctx = Box::into_raw(Box::new(tx)).cast::<c_void>();
        // SAFETY: on true the native side calls `done` exactly once.
        let started = unsafe { kr_stop(self.raw, done, ctx) };
        if !started {
            // SAFETY: not handed over, so still ours.
            drop(unsafe { Box::from_raw(ctx.cast::<mpsc::Sender<Reply>>()) });
            return Err("the recorder was already stopped".into());
        }
        let reply = rx.recv_timeout(wait);
        match reply {
            Ok(reply) => reply,
            Err(_) => {
                // The completion handler still holds the recorder: never free
                // it under that handler; it is leaked instead.
                std::mem::forget(self);
                Err("the movie file did not finish in time".into())
            }
        }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        // SAFETY: freed once; after `stop` the completion has run (see `stop`).
        unsafe { kr_free(self.raw) };
    }
}

/// A finished movie read back with AVFoundation (tests).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Probe {
    pub seconds: f64,
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    /// The video FourCC: `avc1`, `hvc1`, `apcn`, `ap4h`.
    pub codec: String,
    pub audio: bool,
    /// BGRA of the first and last frames.
    pub first: Vec<u8>,
    pub last: Vec<u8>,
}

/// `probe(path)`: `None` when the file has no video track. Frames up to
/// `max_w × max_h` are copied out.
pub fn probe(path: &str, max_w: u32, max_h: u32) -> Option<Probe> {
    let path = CString::new(path).ok()?;
    let cap = max_w as usize * max_h as usize * 4;
    let (mut first, mut last) = (vec![0u8; cap], vec![0u8; cap]);
    let mut raw = RawProbe::default();
    // SAFETY: buffers of `cap` bytes each.
    let found = unsafe {
        kr_probe(
            path.as_ptr(),
            &mut raw,
            first.as_mut_ptr(),
            last.as_mut_ptr(),
            cap,
        )
    };
    if !found {
        return None;
    }
    let n = raw.width.max(0) as usize * raw.height.max(0) as usize * 4;
    first.truncate(n.min(cap));
    last.truncate(n.min(cap));
    Some(Probe {
        seconds: raw.seconds,
        width: raw.width.max(0) as u32,
        height: raw.height.max(0) as u32,
        frames: raw.frames,
        codec: text(&raw.codec),
        audio: raw.audio,
        first,
        last,
    })
}
