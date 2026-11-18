//! Live → Start Capture (prompt 064): Electron's `CaptureService`
//! (`src/main/services/capture/recorder.ts`), in the engine, which owns the
//! frames.
//!
//! The recording takes its picture from the output's own frames through a
//! compositor tap (BGRA, its own read-back ring) and writes it with macOS's
//! encoders (`kairo-native-sys::recorder`). Nothing else about the output
//! changes while it records: frames reach the recorder through a one-slot
//! hand-off on their own thread, so a slow copy drops recorder frames, never
//! the output's.
//!
//! Everything platform-specific comes in through [`Deps`], so the rules run
//! under test with a fake recorder.

use kairo_domain::capture::{
    CaptureCodec, CaptureSettings, CaptureStatus, capture_file_name, capture_frame_size,
};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// Below this much free space a recording does not start…
pub const MIN_FREE_TO_START: u64 = 2 * 1024 * 1024 * 1024;
/// …and below this one in progress stops, so the file is finished rather than cut off.
pub const MIN_FREE_WHILE_RECORDING: u64 = 512 * 1024 * 1024;
/// Checks run once a second; the disk every tenth.
pub const POLL: Duration = Duration::from_secs(1);
const DISK_CHECK_EVERY: u64 = 10;
/// How long finishing the file may take before quit gives up on it.
pub const FINISH_WAIT: Duration = Duration::from_secs(20);

/// The recorder's counts (`RecorderStats`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RecStats {
    pub frames: u64,
    pub late: u64,
    pub skipped: u64,
    pub failed: u64,
    pub seconds: f64,
}

/// What `open` is given.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenRequest {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub codec: &'static str,
    pub bitrate: f64,
    pub audio_device_id: String,
}

/// One open recording.
pub trait Rec: Send + Sync {
    /// Premultiplied BGRA rows; false when dropped.
    fn push(&self, bgra: &[u8], width: u32, height: u32, stride: usize) -> bool;
    fn stats(&self) -> (RecStats, Option<String>);
    /// Finishes the file; `Err` is the reason it could not be.
    fn stop(self: Box<Self>, wait: Duration) -> Result<(), String>;
}

/// The engine's surroundings (`CaptureDeps`).
pub trait Deps: Send + Sync {
    /// `Err(reason)` when recording cannot run here.
    fn load(&self) -> Result<(), String>;
    fn open(&self, request: &OpenRequest) -> Result<Box<dyn Rec>, String>;
    fn audio_inputs(&self) -> Vec<(String, String)>;
    fn default_folder(&self) -> PathBuf;
    fn ensure_folder(&self, folder: &Path) -> Result<(), String>;
    /// Free space on the disk holding `folder`, or `None` when it can't be told.
    fn free_bytes(&self, folder: &Path) -> Option<u64>;
    fn microphone_access(&self) -> bool;
    /// Milliseconds since the epoch, and the local calendar date and time.
    fn now(&self) -> (f64, (i32, u32, u32), (u32, u32, u32));
}

fn gigabytes(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / (1024f64 * 1024.0 * 1024.0))
}

fn codec_name(codec: CaptureCodec) -> &'static str {
    match codec {
        CaptureCodec::H264 => "h264",
        CaptureCodec::Hevc => "hevc",
        CaptureCodec::ProRes422 => "prores422",
        CaptureCodec::ProRes4444 => "prores4444",
    }
}

/// Hands frames to the recorder on its own thread, newest only.
struct Feeder {
    slot: Mutex<(Option<kairo_compositor::uyva::UyvaFrame>, bool)>,
    wake: Condvar,
}

struct Session {
    rec: Arc<Box<dyn Rec>>,
    feeder: Arc<Feeder>,
    feeding: Option<JoinHandle<()>>,
    output: String,
    source_name: String,
    path: String,
    started_at: f64,
    fps: u32,
    polls: u64,
    late_seen: u64,
    folder: PathBuf,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.feeder.slot.lock().unwrap_or_else(|e| e.into_inner()).1 = true;
        self.feeder.wake.notify_all();
        if let Some(t) = self.feeding.take() {
            let _ = t.join();
        }
    }
}

/// Whether a surface is showing, and its picture's width over height.
pub type Aspect = Option<f64>;

pub struct Capture {
    deps: Arc<dyn Deps>,
    session: Option<Session>,
    /// `path`, `sourceName` and `error` of the last recording.
    last: (Option<String>, Option<String>, Option<String>),
    warning: Option<String>,
    /// A file being finished on another thread.
    finishing: Option<JoinHandle<Option<String>>>,
    finishing_session: Option<(String, String)>,
}

impl Capture {
    pub fn new(deps: Arc<dyn Deps>) -> Self {
        Self {
            deps,
            session: None,
            last: (None, None, None),
            warning: None,
            finishing: None,
            finishing_session: None,
        }
    }

    /// Recording, or finishing a file: checks are due once a second.
    pub fn active(&self) -> bool {
        self.session.is_some() || self.finishing.is_some()
    }

    pub fn recording(&self) -> bool {
        self.session.is_some()
    }

    /// The output being recorded.
    pub fn output(&self) -> Option<&str> {
        self.session.as_ref().map(|s| s.output.as_str())
    }

    pub fn status(&self) -> CaptureStatus {
        let loaded = self.deps.load();
        let s = self.session.as_ref();
        CaptureStatus {
            available: loaded.is_ok(),
            reason: loaded.err(),
            recording: s.is_some(),
            started_at: s.map(|s| s.started_at),
            path: s.map(|s| s.path.clone()).or_else(|| self.last.0.clone()),
            source_name: s
                .map(|s| s.source_name.clone())
                .or_else(|| self.last.1.clone()),
            error: if s.is_some() {
                None
            } else {
                self.last.2.clone()
            },
            warning: if s.is_some() {
                self.warning.clone()
            } else {
                None
            },
        }
    }

    fn refuse(&mut self, error: String) -> CaptureStatus {
        tracing::warn!(%error, "capture could not start");
        self.last.2 = Some(error);
        self.status()
    }

    /// `start()`: never fails; a failure is in `error`. `aspect` is the
    /// source's when it is showing.
    pub fn start(
        &mut self,
        output: &str,
        source_name: &str,
        settings: &CaptureSettings,
        aspect: Aspect,
    ) -> CaptureStatus {
        if self.session.is_some() {
            return self.status();
        }
        if let Err(reason) = self.deps.load() {
            return self.refuse(reason);
        }
        let Some(aspect) = aspect else {
            return self.refuse(format!("{source_name} isn’t showing — check it in Screens"));
        };
        let folder = if settings.folder.is_empty() {
            self.deps.default_folder()
        } else {
            PathBuf::from(&settings.folder)
        };
        if let Err(error) = self.deps.ensure_folder(&folder) {
            return self.refuse(format!("Can’t save to {}: {error}", folder.display()));
        }
        if let Some(free) = self.deps.free_bytes(&folder)
            && free < MIN_FREE_TO_START
        {
            return self.refuse(format!(
                "Only {} free on that disk — free some space or choose another folder",
                gigabytes(free)
            ));
        }
        if !settings.audio_device_id.is_empty() {
            if !self.deps.microphone_access() {
                return self.refuse(
                    "Kairo isn’t allowed to record sound — allow it in System Settings → Privacy & Security → Microphone".into(),
                );
            }
            if !self
                .deps
                .audio_inputs()
                .iter()
                .any(|(id, _)| id == &settings.audio_device_id)
            {
                let name = if settings.audio_device_name.is_empty() {
                    "The audio input"
                } else {
                    &settings.audio_device_name
                };
                return self.refuse(format!(
                    "{name} isn’t connected — plug it in or choose another in Capture Settings"
                ));
            }
        }
        let (width, height) = capture_frame_size(settings.resolution, aspect);
        let fps = settings.frame_rate.value();
        let (started_at, date, time) = self.deps.now();
        let file = folder.join(capture_file_name(
            kairo_domain::brand::PRODUCT_NAME,
            source_name,
            date,
            time,
        ));
        let request = OpenRequest {
            path: file.clone(),
            width,
            height,
            fps,
            codec: codec_name(settings.codec),
            bitrate: settings.bitrate_mbps * 1_000_000.0,
            audio_device_id: settings.audio_device_id.clone(),
        };
        let rec = match self.deps.open(&request) {
            Ok(rec) => Arc::new(rec),
            Err(error) => {
                return self.refuse(error.strip_prefix("open: ").unwrap_or(&error).to_owned());
            }
        };
        let feeder = Arc::new(Feeder {
            slot: Mutex::new((None, false)),
            wake: Condvar::new(),
        });
        let (fed, target) = (feeder.clone(), rec.clone());
        let feeding = std::thread::Builder::new()
            .name("kairo-capture-feed".into())
            .spawn(move || feed(&fed, &**target))
            .ok();
        let path = file.to_string_lossy().into_owned();
        tracing::info!(file = %path, source = source_name, width, height, fps, codec = request.codec, audio = !settings.audio_device_id.is_empty(), "recording");
        self.session = Some(Session {
            rec,
            feeder,
            feeding,
            output: output.to_owned(),
            source_name: source_name.to_owned(),
            path: path.clone(),
            started_at,
            fps,
            polls: 0,
            late_seen: 0,
            folder,
        });
        self.warning = None;
        self.last = (Some(path), Some(source_name.to_owned()), None);
        self.status()
    }

    /// The tap's frame sink for the session's output (the compositor calls
    /// it on its own thread): newest frame wins, never a queue.
    pub fn sink(&self) -> Option<kairo_compositor::thread::CaptureSink> {
        let feeder = self.session.as_ref()?.feeder.clone();
        Some(Arc::new(move |frame| {
            let mut slot = feeder.slot.lock().unwrap_or_else(|e| e.into_inner());
            if !slot.1 {
                // A frame the feeder has not taken yet is replaced (dropped).
                slot.0 = Some(frame);
                drop(slot);
                feeder.wake.notify_one();
            }
        }))
    }

    /// Once a second: writer failures, a struggling encoder, a vanished
    /// source, a filling disk. `true` when the status changed.
    pub fn check(&mut self, aspect: Aspect) -> bool {
        let Some(session) = self.session.as_mut() else {
            return self.collect();
        };
        session.polls += 1;
        let (stats, error) = session.rec.stats();
        if let Some(error) = error {
            self.finish(Some(format!("Recording stopped: {error}")));
            return true;
        }
        if session.polls.is_multiple_of(DISK_CHECK_EVERY)
            && let Some(free) = self.deps.free_bytes(&session.folder)
            && free < MIN_FREE_WHILE_RECORDING
        {
            self.finish(Some(format!(
                "Recording stopped: the disk is nearly full ({} left)",
                gigabytes(free)
            )));
            return true;
        }
        let newly_late = stats.late.saturating_sub(session.late_seen);
        session.late_seen = stats.late;
        let warning = if aspect.is_none() {
            Some(format!(
                "{} isn’t showing — recording black until it’s back",
                session.source_name
            ))
        } else if newly_late as f64 > f64::from(session.fps) / 2.0 {
            Some("This Mac is struggling to keep up — H.264 or 720p records with less work".into())
        } else {
            None
        };
        if warning != self.warning {
            self.warning = warning;
            return true;
        }
        false
    }

    /// `stop()`: finishes the file on another thread; [`Capture::check`]
    /// reports the result. Nothing when nothing records.
    pub fn stop(&mut self) {
        self.finish(None);
    }

    fn finish(&mut self, error: Option<String>) {
        let Some(mut session) = self.session.take() else {
            return;
        };
        self.warning = None;
        // Stop feeding before the file closes.
        session
            .feeder
            .slot
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .1 = true;
        session.feeder.wake.notify_all();
        if let Some(t) = session.feeding.take() {
            let _ = t.join();
        }
        let rec = std::mem::replace(&mut session.rec, Arc::new(Box::new(Gone)));
        let path = session.path.clone();
        self.finishing_session = Some((path.clone(), session.source_name.clone()));
        self.last = (
            Some(path.clone()),
            Some(session.source_name.clone()),
            error.clone(),
        );
        let handle = std::thread::Builder::new()
            .name("kairo-capture-finish".into())
            .spawn(move || {
                let rec = match Arc::try_unwrap(rec) {
                    Ok(rec) => rec,
                    Err(_) => return Some("The file could not be finished: still in use".into()),
                };
                match rec.stop(FINISH_WAIT) {
                    Ok(()) => {
                        tracing::info!(file = %path, "capture saved");
                        error
                    }
                    Err(e) => error.or(Some(format!("The file could not be finished: {e}"))),
                }
            })
            .ok();
        self.finishing = handle;
    }

    /// Picks up a finished file's result. `true` when it arrived.
    fn collect(&mut self) -> bool {
        if self.finishing.as_ref().is_some_and(|h| h.is_finished())
            && let Some(handle) = self.finishing.take()
        {
            let problem = handle
                .join()
                .unwrap_or(Some("The file could not be finished".into()));
            if let Some((path, source)) = self.finishing_session.take() {
                if let Some(p) = &problem {
                    tracing::warn!(file = %path, error = %p, "capture stopped with a problem");
                }
                self.last = (Some(path), Some(source), problem);
            }
            return true;
        }
        false
    }

    /// `before-quit`: finishes any recording and waits for the file.
    pub fn finish_before_quit(&mut self) {
        self.finish(None);
        if let Some(handle) = self.finishing.take() {
            let _ = handle.join();
        }
    }
}

/// A recorder already handed to the finishing thread.
struct Gone;

impl Rec for Gone {
    fn push(&self, _: &[u8], _: u32, _: u32, _: usize) -> bool {
        false
    }
    fn stats(&self) -> (RecStats, Option<String>) {
        (RecStats::default(), None)
    }
    fn stop(self: Box<Self>, _: Duration) -> Result<(), String> {
        Ok(())
    }
}

fn feed(feeder: &Feeder, rec: &dyn Rec) {
    let mut warned = false;
    loop {
        let frame = {
            let mut slot = feeder.slot.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if slot.1 {
                    return;
                }
                if let Some(frame) = slot.0.take() {
                    break frame;
                }
                slot = feeder.wake.wait(slot).unwrap_or_else(|e| e.into_inner());
            }
        };
        if !frame.bgra {
            continue;
        }
        let stride = frame.width as usize * 4;
        if !rec.push(frame.data(), frame.width, frame.height, stride) && !warned {
            // Busy or out of buffers: dropped; the next frame carries newer pixels.
            warned = true;
            tracing::debug!("a frame was not recorded");
        }
    }
}

/// The real surroundings: the macOS recorder, the Movies folder, statfs.
pub struct System;

#[cfg(target_os = "macos")]
struct Native(kairo_native_sys::recorder::Recorder);

#[cfg(target_os = "macos")]
impl Rec for Native {
    fn push(&self, bgra: &[u8], width: u32, height: u32, stride: usize) -> bool {
        self.0.push(bgra, width, height, stride) == kairo_native_sys::recorder::Pushed::Taken
    }
    fn stats(&self) -> (RecStats, Option<String>) {
        let (s, e) = self.0.stats();
        (
            RecStats {
                frames: s.frames,
                late: s.late,
                skipped: s.skipped,
                failed: s.failed,
                seconds: s.seconds,
            },
            e,
        )
    }
    fn stop(self: Box<Self>, wait: Duration) -> Result<(), String> {
        self.0.stop(wait).map(|_| ())
    }
}

impl Deps for System {
    fn load(&self) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            kairo_native_sys::recorder::available()
        }
        #[cfg(not(target_os = "macos"))]
        {
            Err("Recording is only available on a Mac for now".into())
        }
    }

    fn open(&self, request: &OpenRequest) -> Result<Box<dyn Rec>, String> {
        #[cfg(target_os = "macos")]
        {
            let path = request.path.to_string_lossy();
            kairo_native_sys::recorder::Recorder::open(&kairo_native_sys::recorder::Open {
                path: &path,
                width: request.width,
                height: request.height,
                fps: request.fps,
                codec: request.codec,
                bitrate: request.bitrate,
                audio_device_id: &request.audio_device_id,
            })
            .map(|r| Box::new(Native(r)) as Box<dyn Rec>)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = request;
            Err("Recording is only available on a Mac for now".into())
        }
    }

    fn audio_inputs(&self) -> Vec<(String, String)> {
        #[cfg(target_os = "macos")]
        {
            kairo_native_sys::recorder::audio_inputs()
        }
        #[cfg(not(target_os = "macos"))]
        {
            Vec::new()
        }
    }

    /// `app.getPath('videos')/Kairo`: ~/Movies/Kairo on a Mac.
    fn default_folder(&self) -> PathBuf {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        home.join(if cfg!(target_os = "macos") {
            "Movies"
        } else {
            "Videos"
        })
        .join(kairo_domain::brand::PRODUCT_NAME)
    }

    fn ensure_folder(&self, folder: &Path) -> Result<(), String> {
        std::fs::create_dir_all(folder).map_err(|e| e.to_string())
    }

    fn free_bytes(&self, folder: &Path) -> Option<u64> {
        free_bytes(folder)
    }

    fn microphone_access(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            kairo_native_sys::recorder::microphone_access()
        }
        #[cfg(not(target_os = "macos"))]
        {
            true
        }
    }

    fn now(&self) -> (f64, (i32, u32, u32), (u32, u32, u32)) {
        use chrono::{Datelike, Timelike};
        let now = chrono::Local::now();
        (
            now.timestamp_millis() as f64,
            (now.year(), now.month(), now.day()),
            (now.hour(), now.minute(), now.second()),
        )
    }
}

/// `statfs(folder)`: `bavail × bsize`.
#[cfg(unix)]
pub fn free_bytes(folder: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(folder.as_os_str().as_bytes()).ok()?;
    let mut stats = std::mem::MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: a NUL-terminated path and space for the result.
    if unsafe { libc::statfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: statfs succeeded and filled it.
    let stats = unsafe { stats.assume_init() };
    // Field widths differ between platforms.
    #[allow(clippy::unnecessary_cast)]
    Some(stats.f_bavail as u64 * stats.f_bsize as u64)
}

#[cfg(not(unix))]
pub fn free_bytes(_: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[derive(Default)]
    struct Fake {
        free: Mutex<Option<u64>>,
        refuse: Mutex<Option<String>>,
        error: Arc<Mutex<Option<String>>>,
        late: Arc<AtomicU64>,
        pushed: Arc<AtomicU64>,
        opened: Mutex<Vec<OpenRequest>>,
        stopped: Arc<AtomicU64>,
        mic: Mutex<bool>,
        inputs: Mutex<Vec<(String, String)>>,
        unavailable: Mutex<Option<String>>,
    }

    struct FakeRec {
        error: Arc<Mutex<Option<String>>>,
        late: Arc<AtomicU64>,
        pushed: Arc<AtomicU64>,
        stopped: Arc<AtomicU64>,
    }

    impl Rec for FakeRec {
        fn push(&self, _: &[u8], _: u32, _: u32, _: usize) -> bool {
            self.pushed.fetch_add(1, Ordering::Relaxed);
            true
        }
        fn stats(&self) -> (RecStats, Option<String>) {
            (
                RecStats {
                    late: self.late.load(Ordering::Relaxed),
                    ..RecStats::default()
                },
                self.error.lock().unwrap().clone(),
            )
        }
        fn stop(self: Box<Self>, _: Duration) -> Result<(), String> {
            self.stopped.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    impl Deps for Fake {
        fn load(&self) -> Result<(), String> {
            self.unavailable.lock().unwrap().clone().map_or(Ok(()), Err)
        }
        fn open(&self, r: &OpenRequest) -> Result<Box<dyn Rec>, String> {
            if let Some(e) = self.refuse.lock().unwrap().clone() {
                return Err(e);
            }
            self.opened.lock().unwrap().push(r.clone());
            Ok(Box::new(FakeRec {
                error: self.error.clone(),
                late: self.late.clone(),
                pushed: self.pushed.clone(),
                stopped: self.stopped.clone(),
            }))
        }
        fn audio_inputs(&self) -> Vec<(String, String)> {
            self.inputs.lock().unwrap().clone()
        }
        fn default_folder(&self) -> PathBuf {
            PathBuf::from("/Movies/Kairo")
        }
        fn ensure_folder(&self, folder: &Path) -> Result<(), String> {
            if folder.starts_with("/readonly") {
                Err("permission denied".into())
            } else {
                Ok(())
            }
        }
        fn free_bytes(&self, _: &Path) -> Option<u64> {
            *self.free.lock().unwrap()
        }
        fn microphone_access(&self) -> bool {
            *self.mic.lock().unwrap()
        }
        fn now(&self) -> (f64, (i32, u32, u32), (u32, u32, u32)) {
            (1_700_000_000_000.0, (2026, 11, 13), (9, 5, 7))
        }
    }

    fn capture() -> (Capture, Arc<Fake>) {
        let fake = Arc::new(Fake::default());
        (Capture::new(fake.clone()), fake)
    }

    fn wait_finished(c: &mut Capture) {
        let t = std::time::Instant::now();
        while c.finishing.is_some() {
            c.check(Some(16.0 / 9.0));
            assert!(t.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    const GB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn starts_in_movies_kairo_with_the_file_name_size_and_codec() {
        let (mut c, fake) = capture();
        let settings = CaptureSettings {
            codec: CaptureCodec::ProRes4444,
            ..CaptureSettings::default()
        };
        let status = c.start("screen-1", "Main", &settings, Some(16.0 / 9.0));
        assert!(status.recording && status.available, "{status:?}");
        let opened = fake.opened.lock().unwrap()[0].clone();
        assert_eq!(
            opened.path,
            PathBuf::from("/Movies/Kairo/Kairo Main 2026-11-13 09.05.07.mov")
        );
        assert_eq!((opened.width, opened.height, opened.fps), (1920, 1080, 30));
        assert_eq!(opened.codec, "prores4444");
        assert_eq!(opened.bitrate, 12_000_000.0);
        assert_eq!(status.path.as_deref(), opened.path.to_str());
        assert_eq!(status.started_at, Some(1_700_000_000_000.0));
        // A second start is the same recording.
        c.start("screen-1", "Main", &settings, Some(16.0 / 9.0));
        assert_eq!(fake.opened.lock().unwrap().len(), 1);
        assert_eq!(c.output(), Some("screen-1"));
    }

    #[test]
    fn refusals_have_the_operator_messages() {
        let (mut c, fake) = capture();
        let s = CaptureSettings::default();
        *fake.unavailable.lock().unwrap() =
            Some("Recording is only available on a Mac for now".into());
        assert_eq!(
            c.start("o", "Main", &s, Some(1.0)).error.as_deref(),
            Some("Recording is only available on a Mac for now")
        );
        *fake.unavailable.lock().unwrap() = None;
        assert_eq!(
            c.start("o", "Main", &s, None).error.as_deref(),
            Some("Main isn’t showing — check it in Screens")
        );
        let ro = CaptureSettings {
            folder: "/readonly/x".into(),
            ..s.clone()
        };
        assert_eq!(
            c.start("o", "Main", &ro, Some(1.0)).error.as_deref(),
            Some("Can’t save to /readonly/x: permission denied")
        );
        *fake.free.lock().unwrap() = Some(GB);
        assert_eq!(
            c.start("o", "Main", &s, Some(1.0)).error.as_deref(),
            Some("Only 1.0 GB free on that disk — free some space or choose another folder")
        );
        *fake.free.lock().unwrap() = Some(3 * GB);
        let mic = CaptureSettings {
            audio_device_id: "desk".into(),
            audio_device_name: "Sound desk".into(),
            ..s.clone()
        };
        assert!(
            c.start("o", "Main", &mic, Some(1.0))
                .error
                .unwrap()
                .starts_with("Kairo isn’t allowed to record sound")
        );
        *fake.mic.lock().unwrap() = true;
        assert_eq!(
            c.start("o", "Main", &mic, Some(1.0)).error.as_deref(),
            Some("Sound desk isn’t connected — plug it in or choose another in Capture Settings")
        );
        *fake.refuse.lock().unwrap() =
            Some("open: this Mac cannot encode that codec at that size".into());
        assert_eq!(
            c.start("o", "Main", &s, Some(1.0)).error.as_deref(),
            Some("this Mac cannot encode that codec at that size")
        );
        assert!(!c.recording());
        // Unknown free space does not block.
        *fake.refuse.lock().unwrap() = None;
        *fake.free.lock().unwrap() = None;
        assert!(c.start("o", "Main", &s, Some(1.0)).recording);
    }

    #[test]
    fn a_filling_disk_stops_on_the_tenth_poll_and_finishes_the_file() {
        let (mut c, fake) = capture();
        c.start("o", "Main", &CaptureSettings::default(), Some(16.0 / 9.0));
        *fake.free.lock().unwrap() = Some(400 * 1024 * 1024);
        for _ in 0..9 {
            c.check(Some(16.0 / 9.0));
            assert!(c.recording());
        }
        assert!(c.check(Some(16.0 / 9.0)));
        assert!(!c.recording());
        wait_finished(&mut c);
        assert_eq!(fake.stopped.load(Ordering::Relaxed), 1);
        assert_eq!(
            c.status().error.as_deref(),
            Some("Recording stopped: the disk is nearly full (0.4 GB left)")
        );
    }

    #[test]
    fn writer_errors_stop_and_warnings_follow_the_source_and_the_encoder() {
        let (mut c, fake) = capture();
        c.start("o", "Main", &CaptureSettings::default(), Some(16.0 / 9.0));
        assert!(c.check(None));
        assert_eq!(
            c.status().warning.as_deref(),
            Some("Main isn’t showing — recording black until it’s back")
        );
        assert!(c.check(Some(16.0 / 9.0)));
        assert_eq!(c.status().warning, None);
        fake.late.store(20, Ordering::Relaxed);
        assert!(c.check(Some(16.0 / 9.0)));
        assert!(
            c.status()
                .warning
                .unwrap()
                .starts_with("This Mac is struggling")
        );
        *fake.error.lock().unwrap() = Some("disk write failed".into());
        c.check(Some(16.0 / 9.0));
        assert!(!c.recording());
        wait_finished(&mut c);
        assert_eq!(
            c.status().error.as_deref(),
            Some("Recording stopped: disk write failed")
        );
    }

    #[test]
    fn frames_reach_the_recorder_and_quit_waits_for_the_file() {
        let (mut c, fake) = capture();
        c.start("o", "Main", &CaptureSettings::default(), Some(16.0 / 9.0));
        let sink = c.sink().unwrap();
        // A BGRA frame from a real converter needs a GPU; the hand-off is
        // checked through the recorder's push count with one when present.
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        if let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default()))
            && let Ok((d, q)) = pollster::block_on(adapter.request_device(&Default::default()))
        {
            let t = d.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: 64,
                    height: 36,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let conv = kairo_compositor::uyva::UyvaConverter::bgra(&d, 64, 36).unwrap();
            let s = sink.clone();
            conv.convert(&d, &q, &t, Box::new(move |f| s(f.unwrap())));
            d.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            let t0 = std::time::Instant::now();
            while fake.pushed.load(Ordering::Relaxed) == 0 {
                assert!(t0.elapsed() < Duration::from_secs(5));
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        c.finish_before_quit();
        assert!(!c.recording());
        assert_eq!(fake.stopped.load(Ordering::Relaxed), 1);
        assert!(c.finishing.is_none());
        // Stop with nothing recording is a no-op.
        c.stop();
        assert_eq!(fake.stopped.load(Ordering::Relaxed), 1);
    }

    #[cfg(unix)]
    #[test]
    fn free_bytes_reads_the_disk() {
        let free = free_bytes(Path::new("/")).unwrap();
        assert!(free > 0);
        assert_eq!(free_bytes(Path::new("/no/such/folder/here")), None);
    }
}
