// recorder — records one of Kairo's outputs to a movie file (Live → Start
// Capture): H.264, HEVC or ProRes through macOS's own encoders, plus sound
// from an audio input such as the sound desk.
//
// C ABI port of Electron's native/recorder/recorder.mm at kairo-presenter
// commit af978c5 (prompt 064). The recording logic is unchanged: frames
// become the "current" frame, a clock on its own queue writes the current
// frame on a fixed frame grid (a still slide is still a minute of video),
// and sound and picture share the host clock. What changed:
//  - N-API is replaced by plain C functions and C callbacks.
//  - Frames arrive as BGRA bytes read back from the engine's compositor
//    (`kr_push_pixels`); Chromium's IOSurface path (`pushTexture`) has no
//    source here, so it is not carried, and with it the Metal copy.
//  - `kr_microphone_access` asks for the microphone permission, as
//    `systemPreferences.askForMediaAccess` did in Electron's main process.
// A frame that arrives while the previous copy is still running is dropped,
// never queued.

#import <Foundation/Foundation.h>
#import <AVFoundation/AVFoundation.h>
#import <Accelerate/Accelerate.h>
#import <CoreMedia/CoreMedia.h>
#import <CoreVideo/CoreVideo.h>
#include <atomic>
#include <cmath>
#include <cstring>
#include <mutex>
#include <string>

extern "C" {

struct kr_options {
  const char* path;
  int32_t width, height, fps;
  /** "h264" | "hevc" | "prores422" | "prores4444" */
  const char* codec;
  double bitrate;
  /** Empty or null: no sound. */
  const char* audio_device_id;
};

struct kr_stats {
  uint64_t frames, late, skipped, failed, audio_buffers;
  double seconds;
};

/** `stop`'s result: error is null on success. */
typedef void (*kr_done)(void* ctx, const char* error, uint64_t frames, double seconds);
typedef void (*kr_input)(void* ctx, const char* id, const char* name);

struct kr_probe_result {
  double seconds;
  int32_t width, height;
  uint32_t frames;
  char codec[5];
  bool audio;
};

}  // extern "C"

static void copyError(const std::string& message, char* out, size_t len) {
  if (!out || len == 0) return;
  std::snprintf(out, len, "%s", message.c_str());
}

// ─── Recorder ─────────────────────────────────────────────────────────────────

struct Recorder;

/** Receives sound from the capture session and writes it on the recording's timeline. */
@interface KairoRecorderAudioTap : NSObject <AVCaptureAudioDataOutputSampleBufferDelegate>
@property(nonatomic, assign) Recorder* recorder;
@end

struct Recorder {
  AVAssetWriter* writer = nil;
  AVAssetWriterInput* video = nil;
  AVAssetWriterInputPixelBufferAdaptor* adaptor = nil;
  AVAssetWriterInput* audio = nil;
  AVCaptureSession* session = nil;
  KairoRecorderAudioTap* tap = nil;
  dispatch_queue_t clockQueue = nullptr;
  dispatch_queue_t audioQueue = nullptr;
  dispatch_source_t clock = nullptr;
  CVPixelBufferPoolRef pool = nullptr;

  std::mutex lock;
  /** The newest copied frame; the clock writes it until a newer one replaces it. */
  CVPixelBufferRef current = nullptr;
  /** What is written before the first frame arrives: transparent black. */
  CVPixelBufferRef blank = nullptr;
  bool busy = false;
  std::atomic<bool> stopping{false};
  bool closed = false;
  std::string error;

  std::string path;
  int width = 1920, height = 1080, fps = 30;
  /** Host time of frame 0. */
  CMTime start = kCMTimeInvalid;
  int64_t lastIndex = -1;
  std::atomic<uint64_t> written{0}, late{0}, skipped{0}, failed{0}, audioBuffers{0};

  ~Recorder() {
    if (current) CVPixelBufferRelease(current);
    if (blank) CVPixelBufferRelease(blank);
    if (pool) CVPixelBufferPoolRelease(pool);
  }

  void setError(const std::string& message) {
    std::lock_guard<std::mutex> guard(lock);
    if (error.empty()) error = message;
  }

  std::string writerError() {
    NSError* e = writer.error;
    return e ? std::string(e.localizedDescription.UTF8String) : std::string("the movie file could not be written");
  }

  /** The newest frame becomes the one the clock writes. Takes ownership of `frame`. */
  void setCurrent(CVPixelBufferRef frame) {
    std::lock_guard<std::mutex> guard(lock);
    if (current) CVPixelBufferRelease(current);
    current = frame;
  }

  /** One clock tick, on `clockQueue`: writes the current frame at its slot on the frame grid. */
  void tick() {
    if (stopping) return;
    CMTime now = CMClockGetTime(CMClockGetHostTimeClock());
    const double elapsed = CMTimeGetSeconds(CMTimeSubtract(now, start));
    const int64_t index = std::max<int64_t>(0, llround(elapsed * fps));
    if (index <= lastIndex) return;
    append(index);
  }

  void append(int64_t index) {
    CVPixelBufferRef frame;
    {
      std::lock_guard<std::mutex> guard(lock);
      frame = CVPixelBufferRetain(current ? current : blank);
    }
    if (writer.status == AVAssetWriterStatusFailed) {
      setError(writerError());
    } else if (!video.readyForMoreMediaData) {
      // The encoder is behind; the slot stays empty and the previous frame holds.
      ++late;
    } else if ([adaptor appendPixelBuffer:frame withPresentationTime:CMTimeMake(index, fps)]) {
      ++written;
      lastIndex = index;
    } else {
      ++failed;
      setError(writerError());
    }
    CVPixelBufferRelease(frame);
  }

  void appendAudio(CMSampleBufferRef buffer) {
    if (stopping || !audio) return;
    CMClockRef sourceClock = nullptr;
    if (@available(macOS 12.3, *)) sourceClock = session.synchronizationClock;
    if (!sourceClock) sourceClock = session.masterClock;
    CMTime pts = CMSampleBufferGetPresentationTimeStamp(buffer);
    if (sourceClock) pts = CMSyncConvertTime(pts, sourceClock, CMClockGetHostTimeClock());
    CMTime at = CMTimeSubtract(pts, start);
    // Sound from before the first frame has no picture to sit under.
    if (CMTIME_COMPARE_INLINE(at, <, kCMTimeZero)) return;
    CMSampleTimingInfo timing = { CMSampleBufferGetDuration(buffer), at, kCMTimeInvalid };
    CMSampleBufferRef retimed = nullptr;
    if (CMSampleBufferCreateCopyWithNewTiming(kCFAllocatorDefault, buffer, 1, &timing, &retimed) != noErr || !retimed) return;
    if (writer.status == AVAssetWriterStatusWriting && audio.readyForMoreMediaData && [audio appendSampleBuffer:retimed]) {
      ++audioBuffers;
    }
    CFRelease(retimed);
  }
};

@implementation KairoRecorderAudioTap
- (void)captureOutput:(AVCaptureOutput*)output didOutputSampleBuffer:(CMSampleBufferRef)buffer fromConnection:(AVCaptureConnection*)connection {
  if (self.recorder) self.recorder->appendAudio(buffer);
}
@end

/** Stops the clock and the sound. Idempotent. */
static void halt(Recorder* r) {
  if (r->stopping.exchange(true)) return;
  if (r->clock) {
    dispatch_source_cancel(r->clock);
    dispatch_sync(r->clockQueue, ^{});
    r->clock = nullptr;
  }
  if (r->session) {
    [r->session stopRunning];
    dispatch_sync(r->audioQueue, ^{});
    r->tap.recorder = nullptr;
  }
}

static CVPixelBufferRef newBuffer(Recorder* r) {
  CVPixelBufferRef buffer = nullptr;
  if (CVPixelBufferPoolCreatePixelBuffer(kCFAllocatorDefault, r->pool, &buffer) != kCVReturnSuccess) return nullptr;
  // Rec. 709, like the encoder settings, so colours don't shift; the frames
  // are premultiplied, which ProRes 4444 needs to know to key cleanly.
  CVBufferSetAttachment(buffer, kCVImageBufferColorPrimariesKey, kCVImageBufferColorPrimaries_ITU_R_709_2, kCVAttachmentMode_ShouldPropagate);
  CVBufferSetAttachment(buffer, kCVImageBufferTransferFunctionKey, kCVImageBufferTransferFunction_ITU_R_709_2, kCVAttachmentMode_ShouldPropagate);
  CVBufferSetAttachment(buffer, kCVImageBufferYCbCrMatrixKey, kCVImageBufferYCbCrMatrix_ITU_R_709_2, kCVAttachmentMode_ShouldPropagate);
  CVBufferSetAttachment(buffer, kCVImageBufferAlphaChannelModeKey, kCVImageBufferAlphaChannelMode_PremultipliedAlpha, kCVAttachmentMode_ShouldPropagate);
  return buffer;
}

static AVCaptureDevice* findAudioDevice(const std::string& id) {
  NSString* wanted = [NSString stringWithUTF8String:id.c_str()];
  for (AVCaptureDevice* device in [AVCaptureDevice devicesWithMediaType:AVMediaTypeAudio]) {
    if ([device.uniqueID isEqualToString:wanted]) return device;
  }
  return nil;
}

extern "C" {

/** Always available on a Mac: AVFoundation does the encoding. */
bool kr_available(char* reason, size_t len) {
  (void)reason; (void)len;
  return true;
}

/** `audioInputs()`: each input's id and name. */
void kr_audio_inputs(kr_input each, void* ctx) {
  @autoreleasepool {
    for (AVCaptureDevice* device in [AVCaptureDevice devicesWithMediaType:AVMediaTypeAudio]) {
      each(ctx, device.uniqueID.UTF8String, device.localizedName.UTF8String);
    }
  }
}

/** Asks for the microphone permission if it was never asked; true when granted. */
bool kr_microphone_access(void) {
  const AVAuthorizationStatus status = [AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeAudio];
  if (status == AVAuthorizationStatusAuthorized) return true;
  if (status != AVAuthorizationStatusNotDetermined) return false;
  dispatch_semaphore_t done = dispatch_semaphore_create(0);
  __block bool granted = false;
  [AVCaptureDevice requestAccessForMediaType:AVMediaTypeAudio completionHandler:^(BOOL ok) {
    granted = ok;
    dispatch_semaphore_signal(done);
  }];
  dispatch_semaphore_wait(done, dispatch_time(DISPATCH_TIME_NOW, int64_t(120) * NSEC_PER_SEC));
  return granted;
}

/**
 * `open(options)`: null with the reason in `error` on failure. Always a
 * QuickTime movie, written in fragments so a crash mid-service still leaves
 * a playable file.
 */
Recorder* kr_open(const kr_options* o, char* error_out, size_t error_len) {
  if (!o || !o->path || !*o->path) { copyError("open: path is required", error_out, error_len); return nullptr; }
  const std::string path = o->path;
  const std::string codec = o->codec ? o->codec : "h264";
  const std::string audioId = o->audio_device_id ? o->audio_device_id : "";
  const double width = o->width, height = o->height, fps = o->fps, bitrate = o->bitrate;
  if (width < 16 || height < 16 || width > 7680 || height > 4320 || int(width) % 2 || int(height) % 2) {
    copyError("open: unsupported size", error_out, error_len);
    return nullptr;
  }
  if (fps < 1 || fps > 120) { copyError("open: unsupported frame rate", error_out, error_len); return nullptr; }

  AVVideoCodecType codecType =
    codec == "h264" ? AVVideoCodecTypeH264
    : codec == "hevc" ? AVVideoCodecTypeHEVC
    : codec == "prores422" ? AVVideoCodecTypeAppleProRes422
    : codec == "prores4444" ? AVVideoCodecTypeAppleProRes4444
    : nil;
  if (!codecType) { copyError("open: unknown codec", error_out, error_len); return nullptr; }

  auto* r = new Recorder();
  r->path = path;
  r->width = int(width);
  r->height = int(height);
  r->fps = int(std::lround(fps));

  std::string problem;
  @autoreleasepool {
    NSURL* url = [NSURL fileURLWithPath:[NSString stringWithUTF8String:path.c_str()]];
    [[NSFileManager defaultManager] removeItemAtURL:url error:nil];
    NSError* error = nil;
    r->writer = [[AVAssetWriter alloc] initWithURL:url fileType:AVFileTypeQuickTimeMovie error:&error];
    if (!r->writer) problem = error ? error.localizedDescription.UTF8String : "could not create the movie file";

    if (problem.empty()) {
      r->writer.movieFragmentInterval = CMTimeMakeWithSeconds(5, 600);
      NSDictionary* color = @{
        AVVideoColorPrimariesKey: AVVideoColorPrimaries_ITU_R_709_2,
        AVVideoTransferFunctionKey: AVVideoTransferFunction_ITU_R_709_2,
        AVVideoYCbCrMatrixKey: AVVideoYCbCrMatrix_ITU_R_709_2,
      };
      NSMutableDictionary* settings = [@{
        AVVideoCodecKey: codecType,
        AVVideoWidthKey: @(r->width),
        AVVideoHeightKey: @(r->height),
        AVVideoColorPropertiesKey: color,
      } mutableCopy];
      if (codec == "h264" || codec == "hevc") {
        NSMutableDictionary* compression = [@{
          AVVideoExpectedSourceFrameRateKey: @(r->fps),
          AVVideoMaxKeyFrameIntervalKey: @(r->fps * 2),
          AVVideoAllowFrameReorderingKey: @NO,
        } mutableCopy];
        if (bitrate > 0) compression[AVVideoAverageBitRateKey] = @(bitrate);
        if (codec == "h264") compression[AVVideoProfileLevelKey] = AVVideoProfileLevelH264HighAutoLevel;
        settings[AVVideoCompressionPropertiesKey] = compression;
      }
      r->video = [AVAssetWriterInput assetWriterInputWithMediaType:AVMediaTypeVideo outputSettings:settings];
      r->video.expectsMediaDataInRealTime = YES;
      NSDictionary* attributes = @{
        (id)kCVPixelBufferPixelFormatTypeKey: @(kCVPixelFormatType_32BGRA),
        (id)kCVPixelBufferWidthKey: @(r->width),
        (id)kCVPixelBufferHeightKey: @(r->height),
        (id)kCVPixelBufferIOSurfacePropertiesKey: @{},
      };
      r->adaptor = [AVAssetWriterInputPixelBufferAdaptor assetWriterInputPixelBufferAdaptorWithAssetWriterInput:r->video sourcePixelBufferAttributes:attributes];
      if ([r->writer canAddInput:r->video]) [r->writer addInput:r->video];
      else problem = "this Mac cannot encode that codec at that size";

      // Our own pool: frames are copied in as they arrive, before the encoder asks.
      NSDictionary* poolOptions = @{ (id)kCVPixelBufferPoolMinimumBufferCountKey: @4 };
      if (problem.empty() && CVPixelBufferPoolCreate(kCFAllocatorDefault, (__bridge CFDictionaryRef)poolOptions, (__bridge CFDictionaryRef)attributes, &r->pool) != kCVReturnSuccess) {
        problem = "could not allocate frame buffers";
      }
      if (problem.empty()) {
        r->blank = newBuffer(r);
        if (!r->blank) problem = "could not allocate frame buffers";
        else {
          CVPixelBufferLockBaseAddress(r->blank, 0);
          std::memset(CVPixelBufferGetBaseAddress(r->blank), 0, CVPixelBufferGetDataSize(r->blank));
          CVPixelBufferUnlockBaseAddress(r->blank, 0);
        }
      }
    }

    if (problem.empty() && !audioId.empty()) {
      AVCaptureDevice* device = findAudioDevice(audioId);
      if (!device) problem = "the audio input is not connected";
      else if ([AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeAudio] != AVAuthorizationStatusAuthorized) {
        problem = "Kairo is not allowed to use the microphone (System Settings → Privacy & Security → Microphone)";
      } else {
        AVCaptureDeviceInput* input = [AVCaptureDeviceInput deviceInputWithDevice:device error:&error];
        AVCaptureAudioDataOutput* output = [[AVCaptureAudioDataOutput alloc] init];
        // One format whatever the device: 48 kHz stereo float, upmixed or downmixed here.
        output.audioSettings = @{
          AVFormatIDKey: @(kAudioFormatLinearPCM),
          AVSampleRateKey: @48000,
          AVNumberOfChannelsKey: @2,
          AVLinearPCMBitDepthKey: @32,
          AVLinearPCMIsFloatKey: @YES,
          AVLinearPCMIsNonInterleaved: @NO,
          AVLinearPCMIsBigEndianKey: @NO,
        };
        r->session = [[AVCaptureSession alloc] init];
        if (!input || ![r->session canAddInput:input] || ![r->session canAddOutput:output]) {
          problem = input ? "the audio input could not be opened" : (error ? error.localizedDescription.UTF8String : "the audio input could not be opened");
        } else {
          [r->session addInput:input];
          [r->session addOutput:output];
          AudioChannelLayout stereo = {};
          stereo.mChannelLayoutTag = kAudioChannelLayoutTag_Stereo;
          NSData* layout = [NSData dataWithBytes:&stereo length:sizeof(stereo)];
          // AAC beside H.264/HEVC; uncompressed 24-bit beside ProRes, as an editor expects.
          NSDictionary* audioSettings = codec.rfind("prores", 0) == 0
            ? @{ AVFormatIDKey: @(kAudioFormatLinearPCM), AVSampleRateKey: @48000, AVNumberOfChannelsKey: @2,
                 AVChannelLayoutKey: layout, AVLinearPCMBitDepthKey: @24, AVLinearPCMIsFloatKey: @NO,
                 AVLinearPCMIsBigEndianKey: @NO, AVLinearPCMIsNonInterleaved: @NO }
            : @{ AVFormatIDKey: @(kAudioFormatMPEG4AAC), AVSampleRateKey: @48000, AVNumberOfChannelsKey: @2,
                 AVChannelLayoutKey: layout, AVEncoderBitRateKey: @256000 };
          r->audio = [AVAssetWriterInput assetWriterInputWithMediaType:AVMediaTypeAudio outputSettings:audioSettings];
          r->audio.expectsMediaDataInRealTime = YES;
          if ([r->writer canAddInput:r->audio]) [r->writer addInput:r->audio];
          else problem = "the audio could not be added to the recording";
          r->tap = [[KairoRecorderAudioTap alloc] init];
          r->tap.recorder = r;
          r->audioQueue = dispatch_queue_create("kairo.recorder.audio", DISPATCH_QUEUE_SERIAL);
          [output setSampleBufferDelegate:r->tap queue:r->audioQueue];
        }
      }
    }

    if (problem.empty() && ![r->writer startWriting]) problem = r->writerError();
  }
  if (!problem.empty()) {
    r->session = nil;
    delete r;
    copyError("open: " + problem, error_out, error_len);
    return nullptr;
  }

  [r->writer startSessionAtSourceTime:kCMTimeZero];
  r->start = CMClockGetTime(CMClockGetHostTimeClock());
  if (r->session) [r->session startRunning];
  r->clockQueue = dispatch_queue_create("kairo.recorder.clock", DISPATCH_QUEUE_SERIAL);
  r->clock = dispatch_source_create(DISPATCH_SOURCE_TYPE_TIMER, 0, 0, r->clockQueue);
  const uint64_t interval = uint64_t(1e9 / r->fps);
  dispatch_source_set_timer(r->clock, dispatch_time(DISPATCH_TIME_NOW, 0), interval, interval / 10);
  Recorder* captured = r;
  dispatch_source_set_event_handler(r->clock, ^{ captured->tick(); });
  dispatch_resume(r->clock);
  return r;
}

/**
 * `pushPixels`: premultiplied BGRA rows of `stride` bytes, scaled on the CPU
 * when the size differs from the recording's. 1 taken, 0 dropped (busy, or
 * every buffer still with the encoder), -1 refused (stopped, bad sizes).
 */
int kr_push_pixels(Recorder* r, const uint8_t* bgra, uint32_t width, uint32_t height, size_t stride, size_t len) {
  if (!r || r->stopping) return -1;
  if (!bgra || width == 0 || height == 0 || stride < size_t(width) * 4 || len < stride * (height - 1) + size_t(width) * 4) return -1;
  {
    std::lock_guard<std::mutex> guard(r->lock);
    if (r->busy) { ++r->skipped; return 0; }
    r->busy = true;
  }
  CVPixelBufferRef target = newBuffer(r);
  if (!target) {
    std::lock_guard<std::mutex> guard(r->lock);
    r->busy = false;
    ++r->skipped;
    return 0;
  }
  CVPixelBufferLockBaseAddress(target, 0);
  auto* base = static_cast<uint8_t*>(CVPixelBufferGetBaseAddress(target));
  const size_t out = CVPixelBufferGetBytesPerRow(target);
  if (width == uint32_t(r->width) && height == uint32_t(r->height)) {
    for (uint32_t y = 0; y < height; ++y) std::memcpy(base + y * out, bgra + size_t(y) * stride, size_t(width) * 4);
  } else {
    vImage_Buffer src = { const_cast<uint8_t*>(bgra), height, width, stride };
    vImage_Buffer dst = { base, vImagePixelCount(r->height), vImagePixelCount(r->width), out };
    vImageScale_ARGB8888(&src, &dst, nullptr, kvImageHighQualityResampling);
  }
  CVPixelBufferUnlockBaseAddress(target, 0);
  r->setCurrent(target);
  {
    std::lock_guard<std::mutex> guard(r->lock);
    r->busy = false;
  }
  return 1;
}

/**
 * `stop`: writes out what is buffered and closes the file; `done` runs on
 * another thread when the file is finished. False when already stopped.
 */
bool kr_stop(Recorder* r, kr_done done, void* ctx) {
  if (!r || r->closed) return false;
  const CMTime ended = CMClockGetTime(CMClockGetHostTimeClock());
  halt(r);
  r->closed = true;
  // A recording stopped before the clock's first tick still gets its first frame.
  if (r->written == 0 && r->writer.status == AVAssetWriterStatusWriting) r->append(0);
  const double seconds = CMTimeGetSeconds(CMTimeSubtract(ended, r->start));
  [r->video markAsFinished];
  [r->audio markAsFinished];
  // End on the last frame slot, so a still slide lasts until Stop was pressed.
  [r->writer endSessionAtSourceTime:CMTimeMake(std::max<int64_t>(r->lastIndex + 1, 1), r->fps)];
  AVAssetWriter* writer = r->writer;
  Recorder* captured = r;
  [writer finishWritingWithCompletionHandler:^{
    std::string error;
    if (writer.status != AVAssetWriterStatusCompleted) error = captured->writerError();
    done(ctx, error.empty() ? nullptr : error.c_str(), captured->written.load(), seconds);
  }];
  return true;
}

/** `stats`: the counts, and the first writer error (empty when none). */
void kr_stats(Recorder* r, kr_stats* out, char* error_out, size_t error_len) {
  if (!r || !out) return;
  out->frames = r->written;
  out->late = r->late;
  out->skipped = r->skipped;
  out->failed = r->failed;
  out->audio_buffers = r->audioBuffers;
  out->seconds = CMTimeGetSeconds(CMTimeSubtract(CMClockGetTime(CMClockGetHostTimeClock()), r->start));
  std::lock_guard<std::mutex> guard(r->lock);
  copyError(r->error, error_out, error_len);
}

/**
 * Frees the recorder once `stop` has completed. Without `stop`, the file is
 * abandoned rather than finished (the handle was dropped).
 */
void kr_free(Recorder* r) {
  if (!r) return;
  if (!r->closed) {
    halt(r);
    if (r->writer.status == AVAssetWriterStatusWriting) [r->writer cancelWriting];
    r->closed = true;
  }
  r->session = nil;
  delete r;
}

/**
 * Tests: reads a finished recording back with AVFoundation. `first` and
 * `last` receive BGRA frames of `cap` bytes when not null. False when there
 * is no video track.
 */
bool kr_probe(const char* path, kr_probe_result* out, uint8_t* first, uint8_t* last, size_t cap) {
  if (!path || !out) return false;
  bool found = false;
  @autoreleasepool {
    AVURLAsset* asset = [AVURLAsset URLAssetWithURL:[NSURL fileURLWithPath:[NSString stringWithUTF8String:path]] options:nil];
    AVAssetTrack* track = [asset tracksWithMediaType:AVMediaTypeVideo].firstObject;
    if (track) {
      found = true;
      out->seconds = CMTimeGetSeconds(asset.duration);
      out->width = int32_t(track.naturalSize.width);
      out->height = int32_t(track.naturalSize.height);
      CMFormatDescriptionRef format = (__bridge CMFormatDescriptionRef)track.formatDescriptions.firstObject;
      const FourCharCode code = format ? CMFormatDescriptionGetMediaSubType(format) : 0;
      out->codec[0] = char(code >> 24); out->codec[1] = char(code >> 16);
      out->codec[2] = char(code >> 8); out->codec[3] = char(code); out->codec[4] = 0;
      out->audio = [asset tracksWithMediaType:AVMediaTypeAudio].count > 0;
      NSError* error = nil;
      AVAssetReader* reader = [AVAssetReader assetReaderWithAsset:asset error:&error];
      AVAssetReaderTrackOutput* output = [AVAssetReaderTrackOutput assetReaderTrackOutputWithTrack:track outputSettings:@{
        (id)kCVPixelBufferPixelFormatTypeKey: @(kCVPixelFormatType_32BGRA),
      }];
      [reader addOutput:output];
      [reader startReading];
      uint32_t frames = 0;
      while (CMSampleBufferRef sample = [output copyNextSampleBuffer]) {
        CVImageBufferRef image = CMSampleBufferGetImageBuffer(sample);
        if (image) {
          const size_t w = CVPixelBufferGetWidth(image), h = CVPixelBufferGetHeight(image);
          uint8_t* targets[2] = { frames == 0 ? first : nullptr, last };
          CVPixelBufferLockBaseAddress(image, kCVPixelBufferLock_ReadOnly);
          const auto* base = static_cast<const uint8_t*>(CVPixelBufferGetBaseAddress(image));
          const size_t stride = CVPixelBufferGetBytesPerRow(image);
          for (uint8_t* target : targets) {
            if (!target || cap < w * h * 4) continue;
            for (size_t y = 0; y < h; ++y) std::memcpy(target + y * w * 4, base + y * stride, w * 4);
          }
          CVPixelBufferUnlockBaseAddress(image, kCVPixelBufferLock_ReadOnly);
          ++frames;
        }
        CFRelease(sample);
      }
      out->frames = frames;
    }
  }
  return found;
}

}  // extern "C"
