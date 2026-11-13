// syphon_c — the C ABI port of Electron's N-API shim, so the native engine
// can publish its output frames as Syphon servers that OBS, Resolume, vMix
// and friends on the same Mac take in, alpha included.
//
// Ported from ../kairo-presenter/native/syphon/syphon.mm at commit 5679dc5
// (2026-10-20; tree checked at af978c5). The N-API layer is gone: errors come
// back as messages in a caller buffer, the completion callback is a plain C
// function, and every handle is an opaque pointer. Behaviour kept:
//   - a Metal device and queue are made once; without them nothing opens;
//   - a server falls back to the name "Kairo" when given none;
//   - `publish` copies a BGRA IOSurface on the GPU (a blit, no shader) into the
//     server's surface and publishes once the copy is done; a frame arriving
//     while a copy runs is dropped, never queued;
//   - `publish_pixels` copies CPU BGRA rows into the server surface; dropped
//     while a GPU copy runs;
//   - a closed server refuses frames; close waits for the last copy;
//   - stats count published, skipped and failed frames and report clients;
//   - a test client reads the newest frame back the way OBS would.
// Added for the native engine (prompt 063):
//   - `surface` hands out the server's own IOSurface so the compositor can
//     draw into it directly (zero readback), and `publish_surface` publishes it;
//   - `metal_texture` wraps an IOSurface for wgpu's Metal device;
//   - a misaligned IOSurface is refused instead of letting Metal abort
//     (spike 005 finding);
//   - a test surface helper, standing in for gpu-frames' createTestSurface.
//
// Only Syphon's messaging and its base server/client classes are compiled in
// (vendor/Syphon, BSD licence, unchanged from Electron's copy).

#import <Foundation/Foundation.h>
#import <IOSurface/IOSurface.h>
#import <Metal/Metal.h>
#import <Syphon/SyphonServerBase.h>
#import <Syphon/SyphonClientBase.h>
#import <Syphon/SyphonSubclassing.h>
#import <Syphon/SyphonServerDirectory.h>
#include <atomic>
#include <cstdint>
#include <cstring>
#include <memory>
#include <mutex>
#include <string>

// ─── GPU ──────────────────────────────────────────────────────────────────────

struct Gpu {
  id<MTLDevice> device = nil;
  id<MTLCommandQueue> queue = nil;
  std::string error;
};

static Gpu& gpu() {
  static Gpu instance = [] {
    Gpu g;
    g.device = MTLCreateSystemDefaultDevice();
    if (!g.device) { g.error = "no Metal device"; return g; }
    g.queue = [g.device newCommandQueue];
    if (!g.queue) g.error = "no Metal command queue";
    return g;
  }();
  return instance;
}

static void put(char* out, size_t cap, const std::string& message) {
  if (!out || cap == 0) return;
  const size_t n = std::min(cap - 1, message.size());
  std::memcpy(out, message.data(), n);
  out[n] = '\0';
}

/** Metal aborts the process on an IOSurface whose rows are misaligned. */
static bool aligned(id<MTLDevice> device, IOSurfaceRef surface) {
  const NSUInteger alignment = [device minimumLinearTextureAlignmentForPixelFormat:MTLPixelFormatBGRA8Unorm];
  return alignment == 0 || IOSurfaceGetBytesPerRow(surface) % alignment == 0;
}

static id<MTLTexture> wrap(id<MTLDevice> device, IOSurfaceRef surface, MTLTextureUsage usage) {
  if (!device || !surface || !aligned(device, surface)) return nil;
  MTLTextureDescriptor* desc = [MTLTextureDescriptor texture2DDescriptorWithPixelFormat:MTLPixelFormatBGRA8Unorm
                                                                                  width:IOSurfaceGetWidth(surface)
                                                                                 height:IOSurfaceGetHeight(surface)
                                                                              mipmapped:NO];
  desc.usage = usage;
  desc.storageMode = MTLStorageModeShared;
  return [device newTextureWithDescriptor:desc iosurface:surface plane:0];
}

// ─── Server ───────────────────────────────────────────────────────────────────

/** Syphon's base server; the frame copy is done here. */
@interface KairoSyphonServer : SyphonServerBase
@end
@implementation KairoSyphonServer
@end

/** Shared with in-flight completion handlers, so freeing never races them. */
struct State {
  std::mutex lock;
  bool busy = false;
  bool closed = false;
  KairoSyphonServer* server = nil;
  id<MTLCommandBuffer> last = nil;
  std::atomic<uint64_t> published{0}, skipped{0}, failed{0};
};

struct KairoSyphon {
  std::shared_ptr<State> state;
};

struct KairoSyphonStats {
  uint64_t published;
  uint64_t skipped;
  uint64_t failed;
  bool has_clients;
};

typedef void (*KairoSyphonDone)(void* context, const char* error);

static void closeState(State& s) {
  id<MTLCommandBuffer> last = nil;
  KairoSyphonServer* server = nil;
  {
    std::lock_guard<std::mutex> guard(s.lock);
    if (s.closed) return;
    s.closed = true;
    last = s.last;
    server = s.server;
    s.server = nil;
    s.last = nil;
  }
  if (last) [last waitUntilCompleted];
  @autoreleasepool {
    [server stop];
  }
}

extern "C" {

/** Whether Syphon can run here; `reason` says why not. */
bool kairo_syphon_available(char* reason, size_t cap) {
  if (!gpu().error.empty()) { put(reason, cap, gpu().error); return false; }
  return true;
}

/** Starts a server named `name` ("Kairo" when null or empty). Null on failure, with `error`. */
KairoSyphon* kairo_syphon_open(const char* name, char* error, size_t cap) {
  if (!gpu().error.empty()) { put(error, cap, gpu().error); return nullptr; }
  NSString* serverName = @"Kairo";
  if (name && name[0]) {
    NSString* given = [NSString stringWithUTF8String:name];
    if (!given) { put(error, cap, "open: the server name is not UTF-8"); return nullptr; }
    serverName = given;
  }
  auto state = std::make_shared<State>();
  @autoreleasepool {
    state->server = [[KairoSyphonServer alloc] initWithName:serverName options:nil];
  }
  if (!state->server) { put(error, cap, "open: Syphon server could not start"); return nullptr; }
  return new KairoSyphon{state};
}

/**
 * GPU path: copies the BGRA IOSurface `source` into the server and publishes
 * it once the copy is done. 1: started, and `done(context, error)` runs once
 * the GPU is finished with `source` (on a Metal thread). 0: the previous copy
 * is still running, so this frame is dropped. -1: refused, see `error`.
 */
int kairo_syphon_publish(KairoSyphon* handle, IOSurfaceRef source, KairoSyphonDone done, void* context,
                         char* error, size_t cap) {
  if (!handle) { put(error, cap, "publish: server closed"); return -1; }
  std::shared_ptr<State> s = handle->state;
  KairoSyphonServer* server = nil;
  {
    std::lock_guard<std::mutex> guard(s->lock);
    if (s->closed) { put(error, cap, "publish: server closed"); return -1; }
    server = s->server;
  }
  if (!source) { put(error, cap, "publish: needs an IOSurfaceRef"); return -1; }
  if (IOSurfaceGetPixelFormat(source) != 'BGRA') {
    ++s->failed;
    put(error, cap, "publish: IOSurface is not BGRA");
    return -1;
  }
  {
    std::lock_guard<std::mutex> guard(s->lock);
    if (s->busy) { ++s->skipped; return 0; }
    s->busy = true;
  }
  @autoreleasepool {
    const size_t width = IOSurfaceGetWidth(source), height = IOSurfaceGetHeight(source);
    IOSurfaceRef target = [server newSurfaceForWidth:width height:height options:nil];
    id<MTLTexture> src = wrap(gpu().device, source, MTLTextureUsageShaderRead);
    id<MTLTexture> dst = target ? wrap(gpu().device, target, MTLTextureUsageShaderWrite | MTLTextureUsageRenderTarget) : nil;
    if (!src || !dst) {
      if (target) CFRelease(target);
      {
        std::lock_guard<std::mutex> guard(s->lock);
        s->busy = false;
      }
      ++s->failed;
      put(error, cap, "publish: could not wrap IOSurface");
      return -1;
    }
    id<MTLCommandBuffer> cb = [gpu().queue commandBuffer];
    id<MTLBlitCommandEncoder> blit = [cb blitCommandEncoder];
    [blit copyFromTexture:src sourceSlice:0 sourceLevel:0 sourceOrigin:MTLOriginMake(0, 0, 0)
               sourceSize:MTLSizeMake(width, height, 1)
                toTexture:dst destinationSlice:0 destinationLevel:0 destinationOrigin:MTLOriginMake(0, 0, 0)];
    [blit endEncoding];
    [cb addCompletedHandler:^(id<MTLCommandBuffer> finished) {
      // `src` and `dst` are captured so both IOSurfaces stay wrapped until the GPU is done.
      (void)src; (void)dst;
      std::string message;
      if (finished.status == MTLCommandBufferStatusCompleted) {
        [server publish];
      } else {
        ++s->failed;
        message = std::string("Syphon copy failed: ") +
          (finished.error ? finished.error.localizedDescription.UTF8String : "unknown");
      }
      CFRelease(target);
      {
        std::lock_guard<std::mutex> guard(s->lock);
        s->busy = false;
      }
      if (done) done(context, message.empty() ? nullptr : message.c_str());
    }];
    {
      std::lock_guard<std::mutex> guard(s->lock);
      s->last = cb;
    }
    [cb commit];
    ++s->published;
  }
  return 1;
}

/**
 * CPU path: `width` × `height` BGRA rows (`stride` bytes apart) copied into the
 * server's surface and published. 1: published. 0: a GPU copy is running, so
 * the frame is dropped. -1: refused, see `error`.
 */
int kairo_syphon_publish_pixels(KairoSyphon* handle, const uint8_t* pixels, size_t length, uint32_t width,
                                uint32_t height, size_t stride, char* error, size_t cap) {
  if (!handle) { put(error, cap, "publishPixels: server closed"); return -1; }
  std::shared_ptr<State> s = handle->state;
  KairoSyphonServer* server = nil;
  {
    std::lock_guard<std::mutex> guard(s->lock);
    if (s->closed) { put(error, cap, "publishPixels: server closed"); return -1; }
    if (s->busy) { ++s->skipped; return 0; }
    server = s->server;
  }
  if (stride == 0) stride = size_t(width) * 4;
  if (!pixels || width == 0 || height == 0 || stride < size_t(width) * 4
      || length < stride * (height - 1) + size_t(width) * 4) {
    put(error, cap, "publishPixels: needs a BGRA buffer of width × height × 4 bytes");
    return -1;
  }
  @autoreleasepool {
    IOSurfaceRef target = [server newSurfaceForWidth:width height:height options:nil];
    if (!target) { ++s->failed; put(error, cap, "publishPixels: no server surface"); return -1; }
    IOSurfaceLock(target, 0, nullptr);
    auto* base = static_cast<uint8_t*>(IOSurfaceGetBaseAddress(target));
    const size_t rowBytes = IOSurfaceGetBytesPerRow(target);
    for (uint32_t y = 0; y < height; ++y) {
      std::memcpy(base + y * rowBytes, pixels + size_t(y) * stride, size_t(width) * 4);
    }
    IOSurfaceUnlock(target, 0, nullptr);
    [server publish];
    CFRelease(target);
  }
  ++s->published;
  return 1;
}

/**
 * The server's own BGRA surface for `width` × `height`, retained (the caller
 * releases it). A size change makes a new one, which `publish_surface` then
 * announces to clients. Null on failure, with `error`.
 */
IOSurfaceRef kairo_syphon_surface(KairoSyphon* handle, uint32_t width, uint32_t height, char* error, size_t cap) {
  if (!handle) { put(error, cap, "surface: server closed"); return nullptr; }
  std::shared_ptr<State> s = handle->state;
  KairoSyphonServer* server = nil;
  {
    std::lock_guard<std::mutex> guard(s->lock);
    if (s->closed) { put(error, cap, "surface: server closed"); return nullptr; }
    server = s->server;
  }
  if (width == 0 || height == 0) { put(error, cap, "surface: empty size"); return nullptr; }
  IOSurfaceRef surface = nullptr;
  @autoreleasepool {
    surface = [server newSurfaceForWidth:width height:height options:nil];
  }
  if (!surface) { ++s->failed; put(error, cap, "surface: no server surface"); return nullptr; }
  return surface;
}

/** Publishes what was drawn into the server's surface; the GPU must be done with it. */
int kairo_syphon_publish_surface(KairoSyphon* handle, char* error, size_t cap) {
  if (!handle) { put(error, cap, "publish: server closed"); return -1; }
  std::shared_ptr<State> s = handle->state;
  KairoSyphonServer* server = nil;
  {
    std::lock_guard<std::mutex> guard(s->lock);
    if (s->closed) { put(error, cap, "publish: server closed"); return -1; }
    server = s->server;
  }
  @autoreleasepool {
    [server publish];
  }
  ++s->published;
  return 1;
}

/** Counts a frame the caller dropped or failed to draw, for `stats`. */
void kairo_syphon_count(KairoSyphon* handle, bool failed) {
  if (!handle) return;
  if (failed) ++handle->state->failed; else ++handle->state->skipped;
}

/** Stops the server: clients see it leave. Waits for the last copy. Idempotent. */
void kairo_syphon_close(KairoSyphon* handle) {
  if (handle) closeState(*handle->state);
}

/** Closes and frees the handle. */
void kairo_syphon_free(KairoSyphon* handle) {
  if (!handle) return;
  closeState(*handle->state);
  delete handle;
}

KairoSyphonStats kairo_syphon_stats(KairoSyphon* handle) {
  KairoSyphonStats out{0, 0, 0, false};
  if (!handle) return out;
  State& s = *handle->state;
  out.published = s.published;
  out.skipped = s.skipped;
  out.failed = s.failed;
  std::lock_guard<std::mutex> guard(s.lock);
  out.has_clients = s.server && !s.closed && s.server.hasClients;
  return out;
}

/**
 * A retained `id<MTLTexture>` over `surface` (BGRA8Unorm, plane 0) on `device`
 * (an `id<MTLDevice>`, borrowed), for wgpu's Metal backend. Null when the
 * surface is misaligned or Metal refuses it.
 */
void* kairo_metal_texture(void* device, IOSurfaceRef surface, uint64_t usage) {
  if (!device || !surface) return nullptr;
  id<MTLDevice> mtl = (__bridge id<MTLDevice>)device;
  id<MTLTexture> texture = wrap(mtl, surface, MTLTextureUsage(usage));
  return texture ? (__bridge_retained void*)texture : nullptr;
}

/** Releases an IOSurface handed out by this library. */
void kairo_iosurface_release(IOSurfaceRef surface) {
  if (surface) CFRelease(surface);
}

uint32_t kairo_iosurface_id(IOSurfaceRef surface) {
  return surface ? IOSurfaceGetID(surface) : 0;
}

// ─── Test helpers ─────────────────────────────────────────────────────────────

/** A BGRA IOSurface holding `bgra` (tightly packed), retained. Tests only. */
IOSurfaceRef kairo_iosurface_create_bgra(const uint8_t* bgra, uint32_t width, uint32_t height) {
  if (!bgra || width == 0 || height == 0) return nullptr;
  NSDictionary* attributes = @{(NSString*)kIOSurfaceWidth: @(width),
                               (NSString*)kIOSurfaceHeight: @(height),
                               (NSString*)kIOSurfacePixelFormat: @(uint32_t(0x42475241)),  // 'BGRA'
                               (NSString*)kIOSurfaceBytesPerElement: @(4U)};
  IOSurfaceRef surface = IOSurfaceCreate((__bridge CFDictionaryRef)attributes);
  if (!surface) return nullptr;
  IOSurfaceLock(surface, 0, nullptr);
  auto* base = static_cast<uint8_t*>(IOSurfaceGetBaseAddress(surface));
  const size_t stride = IOSurfaceGetBytesPerRow(surface);
  for (uint32_t y = 0; y < height; ++y) std::memcpy(base + y * stride, bgra + size_t(y) * width * 4, size_t(width) * 4);
  IOSurfaceUnlock(surface, 0, nullptr);
  return surface;
}

@interface KairoSyphonTestClient : SyphonClientBase
@end
@implementation KairoSyphonTestClient
@end

/**
 * Connects a Syphon client to the server, as OBS would, and copies the newest
 * frame into `out` as tight BGRA rows. 1: a frame. 0: none within the timeout.
 * -1: refused, see `error`. -2: `out` too small; `width`/`height` say what is
 * needed.
 */
int kairo_syphon_read_latest(KairoSyphon* handle, double timeout_ms, uint8_t* out, size_t cap, uint32_t* width,
                             uint32_t* height, char* error, size_t error_cap) {
  if (!handle) { put(error, error_cap, "readLatest: server closed"); return -1; }
  std::shared_ptr<State> s = handle->state;
  KairoSyphonServer* server = nil;
  {
    std::lock_guard<std::mutex> guard(s->lock);
    if (s->closed) { put(error, error_cap, "readLatest: server closed"); return -1; }
    server = s->server;
  }
  int result = 0;
  @autoreleasepool {
    KairoSyphonTestClient* client = [[KairoSyphonTestClient alloc] initWithServerDescription:server.serverDescription
                                                                                     options:nil
                                                                             newFrameHandler:nil];
    NSDate* deadline = [NSDate dateWithTimeIntervalSinceNow:timeout_ms / 1000.0];
    IOSurfaceRef surface = nullptr;
    while (!surface && [deadline timeIntervalSinceNow] > 0) {
      surface = [client newSurface];
      if (!surface) [NSThread sleepForTimeInterval:0.01];
    }
    if (surface) {
      const size_t w = IOSurfaceGetWidth(surface), h = IOSurfaceGetHeight(surface);
      if (width) *width = uint32_t(w);
      if (height) *height = uint32_t(h);
      if (!out || cap < w * h * 4) {
        result = -2;
      } else {
        IOSurfaceLock(surface, kIOSurfaceLockReadOnly, nullptr);
        const auto* base = static_cast<const uint8_t*>(IOSurfaceGetBaseAddress(surface));
        const size_t stride = IOSurfaceGetBytesPerRow(surface);
        for (size_t y = 0; y < h; ++y) std::memcpy(out + y * w * 4, base + y * stride, w * 4);
        IOSurfaceUnlock(surface, kIOSurfaceLockReadOnly, nullptr);
        result = 1;
      }
      CFRelease(surface);
    }
    [client stop];
  }
  return result;
}

/**
 * A client in this process for a server in any process, found by `name`
 * through the Syphon directory, as OBS or Resolume find it. Spins the calling
 * thread's run loop (call it from the main thread) for up to `discover_ms` to
 * find the server, then counts the frames it publishes for `watch_ms`, then
 * copies the newest frame into `out` as tight BGRA rows. 1: a frame. 0: no
 * server or no frame. -1: refused, see `error`. -2: `out` too small;
 * `width`/`height` say what is needed (the frame count is still set).
 */
int kairo_syphon_client_read(const char* name, double discover_ms, double watch_ms, uint8_t* out, size_t cap,
                             uint32_t* width, uint32_t* height, uint64_t* frames, char* error, size_t error_cap) {
  if (!name) { put(error, error_cap, "client: needs a server name"); return -1; }
  NSString* wanted = [NSString stringWithUTF8String:name];
  if (!wanted) { put(error, error_cap, "client: the server name is not UTF-8"); return -1; }
  int result = 0;
  @autoreleasepool {
    SyphonServerDirectory* directory = [SyphonServerDirectory sharedDirectory];
    NSDictionary* description = nil;
    NSDate* deadline = [NSDate dateWithTimeIntervalSinceNow:discover_ms / 1000.0];
    while (!description && [deadline timeIntervalSinceNow] > 0) {
      description = [[directory serversMatchingName:wanted appName:nil] firstObject];
      if (!description) [[NSRunLoop currentRunLoop] runUntilDate:[NSDate dateWithTimeIntervalSinceNow:0.02]];
    }
    if (!description) { put(error, error_cap, "client: no Syphon server with that name"); return 0; }
    auto seen = std::make_shared<std::atomic<uint64_t>>(0);
    KairoSyphonTestClient* client = [[KairoSyphonTestClient alloc] initWithServerDescription:description
                                                                                     options:nil
                                                                             newFrameHandler:^(id) { ++*seen; }];
    NSDate* until = [NSDate dateWithTimeIntervalSinceNow:watch_ms / 1000.0];
    while ([until timeIntervalSinceNow] > 0) {
      [[NSRunLoop currentRunLoop] runUntilDate:[NSDate dateWithTimeIntervalSinceNow:0.01]];
    }
    IOSurfaceRef surface = nullptr;
    NSDate* last = [NSDate dateWithTimeIntervalSinceNow:2.0];
    while (!surface && [last timeIntervalSinceNow] > 0) {
      surface = [client newSurface];
      if (!surface) [[NSRunLoop currentRunLoop] runUntilDate:[NSDate dateWithTimeIntervalSinceNow:0.01]];
    }
    if (frames) *frames = *seen;
    if (surface) {
      const size_t w = IOSurfaceGetWidth(surface), h = IOSurfaceGetHeight(surface);
      if (width) *width = uint32_t(w);
      if (height) *height = uint32_t(h);
      if (!out || cap < w * h * 4) {
        result = -2;
      } else {
        IOSurfaceLock(surface, kIOSurfaceLockReadOnly, nullptr);
        const auto* base = static_cast<const uint8_t*>(IOSurfaceGetBaseAddress(surface));
        const size_t stride = IOSurfaceGetBytesPerRow(surface);
        for (size_t y = 0; y < h; ++y) std::memcpy(out + y * w * 4, base + y * stride, w * 4);
        IOSurfaceUnlock(surface, kIOSurfaceLockReadOnly, nullptr);
        result = 1;
      }
      CFRelease(surface);
    } else {
      put(error, error_cap, "client: the server published no frame");
    }
    [client stop];
  }
  return result;
}

}  // extern "C"
