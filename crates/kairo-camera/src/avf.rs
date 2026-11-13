//! Cameras and capture cards through AVFoundation (macOS), the system
//! Chromium itself used, so names, formats and permission behave as they
//! did in Electron.
//!
//! Frames arrive as NV12 (`420v`, studio range) on a serial dispatch queue
//! and are copied once into a [`YuvFrame`]; the GPU converts them (prompt
//! 055's `yuv.wgsl`). Late frames are discarded by AVFoundation, so the
//! picture never queues up behind the device.

use crate::backend::{Backend, CameraError, Capture, FrameSink, Health};
use crate::format::{self, FormatInfo, Request};
use crate::label::{DeviceInfo, chromium_label};
use dispatch2::{DispatchQueue, DispatchRetained};
use kairo_compositor::{YuvColor, YuvFrame};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, ProtocolObject};
use objc2::{AnyThread, DefinedClass, define_class, msg_send};
use objc2_av_foundation::{
    AVAuthorizationStatus, AVCaptureConnection, AVCaptureDevice, AVCaptureDeviceFormat,
    AVCaptureDeviceInput, AVCaptureOutput, AVCaptureSession, AVCaptureVideoDataOutput,
    AVCaptureVideoDataOutputSampleBufferDelegate, AVMediaTypeMuxed, AVMediaTypeVideo,
};
use objc2_core_media::{CMSampleBuffer, CMTime, CMVideoFormatDescriptionGetDimensions};
use objc2_core_video::{
    CVPixelBufferGetBaseAddressOfPlane, CVPixelBufferGetBytesPerRowOfPlane, CVPixelBufferGetHeight,
    CVPixelBufferGetPixelFormatType, CVPixelBufferGetPlaneCount, CVPixelBufferGetWidth,
    CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
    kCVPixelBufferPixelFormatTypeKey, kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
    kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
};
use objc2_foundation::{NSDictionary, NSNumber, NSObject, NSObjectProtocol, NSString};
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// How long the system's permission prompt may stay up.
const PERMISSION_WAIT: Duration = Duration::from_secs(120);
/// No frame for this long while another program holds the device: busy.
const STALL: Duration = Duration::from_secs(2);
/// How long an open waits for the first frame before deciding.
const FIRST_FRAME: Duration = Duration::from_secs(3);

pub struct Avf;

fn media_types() -> Vec<&'static NSString> {
    // SAFETY: framework constants, valid for the life of the process.
    unsafe { [AVMediaTypeVideo, AVMediaTypeMuxed] }
        .into_iter()
        .flatten()
        .collect()
}

/// Asks for camera access once, as `getUserMedia` did, and waits for the answer.
fn authorize() -> Result<(), CameraError> {
    // SAFETY: framework constant.
    let Some(video) = (unsafe { AVMediaTypeVideo }) else {
        return Err(CameraError::Unsupported);
    };
    // SAFETY: a class method with a valid media type.
    let status = unsafe { AVCaptureDevice::authorizationStatusForMediaType(video) };
    match status {
        AVAuthorizationStatus::Authorized => Ok(()),
        AVAuthorizationStatus::NotDetermined => {
            let (tx, rx) = mpsc::channel();
            let block = block2::RcBlock::new(move |granted: Bool| {
                let _ = tx.send(granted.as_bool());
            });
            // SAFETY: the block lives as long as the call needs it.
            unsafe { AVCaptureDevice::requestAccessForMediaType_completionHandler(video, &block) };
            match rx.recv_timeout(PERMISSION_WAIT) {
                Ok(true) => Ok(()),
                _ => Err(CameraError::Denied),
            }
        }
        _ => Err(CameraError::Denied),
    }
}

/// Every video and muxed (capture card) input, once each.
fn devices() -> Vec<Retained<AVCaptureDevice>> {
    let mut out: Vec<Retained<AVCaptureDevice>> = Vec::new();
    for media in media_types() {
        // SAFETY: a class method with a valid media type. Deprecated in
        // favour of discovery sessions, which need the device types listed
        // one by one; this returns all of them, as Chromium's list did.
        #[allow(deprecated)]
        let list = unsafe { AVCaptureDevice::devicesWithMediaType(media) };
        for device in list.iter() {
            // SAFETY: plain property reads.
            let id = unsafe { device.uniqueID() };
            if !out.iter().any(|d| unsafe { d.uniqueID() } == id) {
                out.push(device);
            }
        }
    }
    out
}

fn info(device: &AVCaptureDevice) -> DeviceInfo {
    // SAFETY: plain property reads.
    let (name, id) = unsafe {
        (
            device.localizedName().to_string(),
            device.uniqueID().to_string(),
        )
    };
    DeviceInfo {
        label: chromium_label(&name, &id),
        name,
        id,
    }
}

fn format_info(format: &AVCaptureDeviceFormat) -> FormatInfo {
    // SAFETY: plain property reads on a valid format.
    unsafe {
        let dims = CMVideoFormatDescriptionGetDimensions(&format.formatDescription());
        let mut min_fps = f64::MAX;
        let mut max_fps: f64 = 0.0;
        for range in format.videoSupportedFrameRateRanges().iter() {
            min_fps = min_fps.min(range.minFrameRate());
            max_fps = max_fps.max(range.maxFrameRate());
        }
        if max_fps <= 0.0 {
            min_fps = 0.0;
        }
        FormatInfo {
            width: dims.width.max(0) as u32,
            height: dims.height.max(0) as u32,
            min_fps,
            max_fps,
        }
    }
}

/// The frame duration for `fps` that one of the format's rate ranges
/// allows, taken from the range itself at its ends so it is never a hair
/// outside it (1/30 s rounded to 33 ms is 30.3 fps, which is refused).
fn frame_duration(format: &AVCaptureDeviceFormat, fps: f64) -> Option<CMTime> {
    if !(fps.is_finite() && fps > 0.0) {
        return None;
    }
    // SAFETY: plain property reads on a valid format.
    unsafe {
        for range in format.videoSupportedFrameRateRanges().iter() {
            let (min, max) = (range.minFrameRate(), range.maxFrameRate());
            if fps < min - 0.01 || fps > max + 0.01 {
                continue;
            }
            return Some(if (fps - max).abs() <= 0.01 {
                range.minFrameDuration()
            } else if (fps - min).abs() <= 0.01 {
                range.maxFrameDuration()
            } else {
                CMTime::new(1000, (fps * 1000.0).round() as i32)
            });
        }
    }
    None
}

fn error_text(error: &objc2_foundation::NSError) -> String {
    error.localizedDescription().to_string()
}

/// Runs AVFoundation calls with Objective-C exceptions caught, so a
/// device that refuses a call fails the request instead of aborting the
/// engine.
fn guarded<R>(what: &str, f: impl FnOnce() -> Result<R, CameraError>) -> Result<R, CameraError> {
    match objc2::exception::catch(AssertUnwindSafe(f)) {
        Ok(result) => result,
        Err(exception) => {
            let reason = exception.map_or_else(|| "unknown".to_owned(), |e| format!("{e:?}"));
            tracing::error!(what, %reason, "AVFoundation raised an exception");
            Err(CameraError::Failed(reason))
        }
    }
}

impl Backend for Avf {
    fn video_devices(&self) -> Result<Vec<DeviceInfo>, CameraError> {
        guarded("listing cameras", || {
            authorize()?;
            Ok(devices().iter().map(|d| info(d)).collect())
        })
    }

    fn open(
        &self,
        wanted: &DeviceInfo,
        request: Request,
        sink: FrameSink,
    ) -> Result<Box<dyn Capture>, CameraError> {
        guarded("opening a camera", || open(wanted, request, sink))
    }
}

fn open(
    wanted: &DeviceInfo,
    request: Request,
    sink: FrameSink,
) -> Result<Box<dyn Capture>, CameraError> {
    authorize()?;
    let device = devices()
        .into_iter()
        .find(|d| unsafe { d.uniqueID() }.to_string() == wanted.id)
        .ok_or_else(|| CameraError::NotFound(wanted.label.clone()))?;
    // SAFETY: AVFoundation calls on objects this function owns; the
    // session is configured before it runs, from one thread.
    unsafe {
        let input = AVCaptureDeviceInput::deviceInputWithDevice_error(&device)
            .map_err(|e| open_error(&device, &error_text(&e)))?;
        let session = AVCaptureSession::new();
        session.beginConfiguration();
        if !session.canAddInput(&input) {
            session.commitConfiguration();
            return Err(open_error(&device, "the input cannot be added"));
        }
        session.addInput(&input);
        let output = AVCaptureVideoDataOutput::new();
        let key: &NSString = &*(kCVPixelBufferPixelFormatTypeKey as *const _ as *const NSString);
        let value =
            NSNumber::numberWithUnsignedInt(kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange);
        let settings: Retained<NSDictionary<NSString, AnyObject>> =
            NSDictionary::from_slices(&[key], &[&**value as &AnyObject]);
        output.setVideoSettings(Some(&settings));
        output.setAlwaysDiscardsLateVideoFrames(true);
        let shared = Arc::new(Shared {
            sink,
            start: Instant::now(),
            last_ms: AtomicU64::new(0),
            frames: AtomicU64::new(0),
        });
        let delegate = Delegate::new(shared.clone());
        let queue = DispatchQueue::new("kairo.camera", None);
        output.setSampleBufferDelegate_queue(
            Some(ProtocolObject::from_ref(&*delegate)),
            Some(&queue),
        );
        if !session.canAddOutput(&output) {
            session.commitConfiguration();
            return Err(open_error(&device, "the output cannot be added"));
        }
        session.addOutput(&output);
        session.commitConfiguration();

        // The format nearest the request, set after the session is
        // configured so the session does not override it.
        let formats: Vec<Retained<AVCaptureDeviceFormat>> = device.formats().iter().collect();
        let infos: Vec<FormatInfo> = formats.iter().map(|f| format_info(f)).collect();
        let mut mode = (0, 0, request.fps);
        if let Some(i) = format::choose(&infos, request) {
            mode = (
                infos[i].width,
                infos[i].height,
                format::rate_for(&infos[i], request.fps),
            );
            let format = &formats[i];
            let rate = mode.2;
            // Setting a format or rate the device refuses raises an
            // Objective-C exception: caught, the device keeps its own.
            let set = objc2::exception::catch(AssertUnwindSafe(|| {
                match device.lockForConfiguration() {
                    Ok(()) => {
                        device.setActiveFormat(format);
                        if let Some(duration) = frame_duration(format, rate) {
                            device.setActiveVideoMinFrameDuration(duration);
                            device.setActiveVideoMaxFrameDuration(duration);
                        }
                        device.unlockForConfiguration();
                    }
                    Err(e) => {
                        tracing::warn!(error = %error_text(&e), "camera format not set; using its own");
                    }
                }
            }));
            if let Err(exception) = set {
                device.unlockForConfiguration();
                tracing::warn!(?exception, "camera refused the format; using its own");
            }
        }
        if let Err(exception) = objc2::exception::catch(AssertUnwindSafe(|| session.startRunning()))
        {
            return Err(CameraError::Failed(format!(
                "the capture session did not start: {exception:?}"
            )));
        }
        let opened = Instant::now();
        while shared.frames.load(Ordering::Acquire) == 0 && opened.elapsed() < FIRST_FRAME {
            std::thread::sleep(Duration::from_millis(10));
        }
        if shared.frames.load(Ordering::Acquire) == 0 {
            if !device.isConnected() {
                session.stopRunning();
                return Err(CameraError::Unplugged);
            }
            if device.isInUseByAnotherApplication() {
                session.stopRunning();
                return Err(CameraError::Busy);
            }
            if !session.isRunning() {
                return Err(CameraError::Failed(
                    "the capture session did not start".into(),
                ));
            }
            // A capture card with no signal: live, black until frames come.
            tracing::warn!(label = %wanted.label, "camera running with no frames yet");
        }
        Ok(Box::new(AvfCapture {
            session,
            output,
            device,
            _delegate: delegate,
            _queue: queue,
            shared,
            mode,
        }))
    }
}

fn open_error(device: &AVCaptureDevice, message: &str) -> CameraError {
    // SAFETY: plain property reads.
    unsafe {
        if !device.isConnected() {
            CameraError::Unplugged
        } else if device.isInUseByAnotherApplication() {
            CameraError::Busy
        } else {
            CameraError::Failed(message.to_owned())
        }
    }
}

struct Shared {
    sink: FrameSink,
    start: Instant,
    /// When the newest frame arrived, in ms after `start` (0: never).
    last_ms: AtomicU64,
    frames: AtomicU64,
}

struct AvfCapture {
    session: Retained<AVCaptureSession>,
    output: Retained<AVCaptureVideoDataOutput>,
    device: Retained<AVCaptureDevice>,
    _delegate: Retained<Delegate>,
    _queue: DispatchRetained<DispatchQueue>,
    shared: Arc<Shared>,
    mode: (u32, u32, f64),
}

// SAFETY: AVCaptureSession and its device and output are thread-safe for
// the calls made here (start, stop, property reads), as Apple documents for
// sessions driven from a background queue.
unsafe impl Send for AvfCapture {}

impl Capture for AvfCapture {
    fn health(&self) -> Health {
        // SAFETY: plain property reads.
        unsafe {
            if !self.device.isConnected() {
                return Health::Unplugged;
            }
            let last = self.shared.last_ms.load(Ordering::Acquire);
            let now = self.shared.start.elapsed().as_millis() as u64;
            let stalled = last == 0 || now.saturating_sub(last) > STALL.as_millis() as u64;
            if stalled && (self.device.isInUseByAnotherApplication() || !self.session.isRunning()) {
                Health::Busy
            } else {
                Health::Running
            }
        }
    }

    fn mode(&self) -> (u32, u32, f64) {
        self.mode
    }
}

impl Drop for AvfCapture {
    fn drop(&mut self) {
        // SAFETY: stopping a session this value owns.
        unsafe {
            self.output.setSampleBufferDelegate_queue(None, None);
            self.session.stopRunning();
        }
    }
}

/// The matrix of a frame: BT.709 for HD sizes, BT.601 below, as
/// untagged camera video is read.
fn color_for(height: u32, full_range: bool) -> YuvColor {
    let base = if height >= 720 {
        YuvColor::BT709
    } else {
        YuvColor::BT601
    };
    YuvColor { full_range, ..base }
}

fn copy_frame(sample: &CMSampleBuffer) -> Option<YuvFrame> {
    // SAFETY: the pixel buffer is locked read-only for the copy and its
    // planes are read within the sizes CoreVideo reports.
    unsafe {
        let buffer = sample.image_buffer()?;
        let format = CVPixelBufferGetPixelFormatType(&buffer);
        let full_range = match format {
            f if f == kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange => false,
            f if f == kCVPixelFormatType_420YpCbCr8BiPlanarFullRange => true,
            _ => return None,
        };
        if CVPixelBufferGetPlaneCount(&buffer) < 2 {
            return None;
        }
        if CVPixelBufferLockBaseAddress(&buffer, CVPixelBufferLockFlags::ReadOnly) != 0 {
            return None;
        }
        let width = CVPixelBufferGetWidth(&buffer) as u32;
        let height = CVPixelBufferGetHeight(&buffer) as u32;
        let plane = |i: usize, rows: u32| -> Option<(Vec<u8>, u32)> {
            let base = CVPixelBufferGetBaseAddressOfPlane(&buffer, i) as *const u8;
            let stride = CVPixelBufferGetBytesPerRowOfPlane(&buffer, i);
            if base.is_null() || stride == 0 {
                return None;
            }
            let bytes = std::slice::from_raw_parts(base, stride * rows as usize);
            Some((bytes.to_vec(), stride as u32))
        };
        let luma = plane(0, height);
        let chroma = plane(1, height.div_ceil(2));
        CVPixelBufferUnlockBaseAddress(&buffer, CVPixelBufferLockFlags::ReadOnly);
        let ((luma, luma_stride), (chroma, chroma_stride)) = (luma?, chroma?);
        Some(YuvFrame {
            width,
            height,
            bits: 8,
            color: color_for(height, full_range),
            luma,
            luma_stride,
            chroma,
            chroma_stride,
            alpha: None,
        })
    }
}

struct Ivars {
    shared: Arc<Shared>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and `Delegate` does
    // not implement Drop.
    #[unsafe(super(NSObject))]
    #[name = "KairoCameraDelegate"]
    #[ivars = Ivars]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl AVCaptureVideoDataOutputSampleBufferDelegate for Delegate {
        #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
        #[allow(non_snake_case)]
        fn captureOutput_didOutputSampleBuffer_fromConnection(
            &self,
            _output: &AVCaptureOutput,
            sample_buffer: &CMSampleBuffer,
            _connection: &AVCaptureConnection,
        ) {
            let shared = &self.ivars().shared;
            let copied = objc2::exception::catch(AssertUnwindSafe(|| copy_frame(sample_buffer)));
            if let Ok(Some(frame)) = copied {
                let ms = shared.start.elapsed().as_millis() as u64;
                shared.last_ms.store(ms.max(1), Ordering::Release);
                shared.frames.fetch_add(1, Ordering::AcqRel);
                (shared.sink)(frame);
            }
        }
    }
);

impl Delegate {
    fn new(shared: Arc<Shared>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(Ivars { shared });
        // SAFETY: NSObject's init.
        unsafe { msg_send![super(this), init] }
    }
}
