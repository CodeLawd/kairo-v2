// C ABI replacing the N-API shim of gpu_frames.mm. metal_core.inc is the
// Metal section of that file, unmodified; see native-migration/SYNC.md.
#import <CoreFoundation/CoreFoundation.h>
#import <Foundation/Foundation.h>
#import <IOSurface/IOSurface.h>
#import <Metal/Metal.h>

#include <time.h>
#include <atomic>
#include <cstddef>
#include <cstring>
#include <mutex>
#include <string>

#include "ring.h"
#include "uyvy.h"
#include "metal_core.inc"

static std::atomic<uint64_t> lastGpuNs{0};
extern "C" uint64_t kgf_last_gpu_ns(void) { return lastGpuNs.load(); }

static bool validRaster(uint32_t w, uint32_t h) {
  return w >= 2 && h >= 1 && w % 2 == 0 && w <= 4096 && h <= 2160;
}

extern "C" const char* kgf_error(void) {
  return gpu().error.empty() ? nullptr : gpu().error.c_str();
}

// 0 ok, 1 Metal unavailable, 2 bad dimensions, 3 GPU error.
extern "C" int kgf_convert_pixels(const uint8_t* bgra, size_t length, uint32_t sw, uint32_t sh,
                                  uint32_t dw, uint32_t dh, bool alpha, uint8_t* out, size_t out_length) {
  if (!gpu().error.empty()) return 1;
  const size_t outBytes = validRaster(dw, dh) ? kairo_ring::frameBytes(dw, dh) + (alpha ? size_t(dw) * dh : 0) : 0;
  if (!bgra || !out || !sw || !sh || length != size_t(sw) * sh * 4 || !outBytes || out_length != outBytes) return 2;
  @autoreleasepool {
    MTLTextureDescriptor* desc = [MTLTextureDescriptor texture2DDescriptorWithPixelFormat:MTLPixelFormatBGRA8Unorm width:sw height:sh mipmapped:NO];
    desc.usage = MTLTextureUsageShaderRead;
    id<MTLTexture> src = [gpu().device newTextureWithDescriptor:desc];
    [src replaceRegion:MTLRegionMake2D(0, 0, sw, sh) mipmapLevel:0 withBytes:bgra bytesPerRow:sw * 4];
    id<MTLBuffer> dst = [gpu().device newBufferWithLength:outBytes options:MTLResourceStorageModeShared];
    id<MTLCommandBuffer> cb = [gpu().queue commandBuffer];
    encode(cb, src, dst, Params{sw, sh, dw, dh, alpha ? 1u : 0u, 0});
    const uint64_t began = clock_gettime_nsec_np(CLOCK_UPTIME_RAW);
    [cb commit];
    [cb waitUntilCompleted];
    lastGpuNs.store(clock_gettime_nsec_np(CLOCK_UPTIME_RAW) - began);
    if (cb.status != MTLCommandBufferStatusCompleted) return 3;
    std::memcpy(out, dst.contents, outBytes);
  }
  return 0;
}

// The reference the shader must match, from uyvy.h.
extern "C" void kgf_cpu_convert(const uint8_t* bgra, uint32_t sw, uint32_t sh, uint8_t* out, long dw, long dh) {
  convertFrame(bgra, sw, sh, out, dw, dh);
}

// Layout facts, so Rust is tested against the real header.
extern "C" size_t kgf_ring_layout(int what) {
  using kairo_ring::Header;
  switch (what) {
    case 0: return sizeof(Header);
    case 1: return offsetof(Header, published);
    case 2: return offsetof(Header, latest);
    case 3: return offsetof(Header, slots);
    case 4: return sizeof(kairo_ring::Slot);
    case 5: return kairo_ring::slotBytesFor(1920, 1080);
    case 6: return kairo_ring::MAGIC;
    default: return 0;
  }
}

// ─── Ring ─────────────────────────────────────────────────────────────────────
// Ported from the N-API section of gpu_frames.mm. `refs` replaces the JS
// reference a converter held on its ring.

#include <fcntl.h>
#include <sys/mman.h>
#include <unistd.h>
#include <cerrno>
#include <condition_variable>

struct KgfRing {
  std::string name;
  void* base = nullptr;
  size_t bytes = 0;
  kairo_ring::Header* header = nullptr;
  uint32_t width = 0, height = 0;
  size_t slotBytes = 0;
  std::atomic<bool> closed{false};
  std::atomic<int> attached{0};
  std::atomic<int> refs{1};
  std::mutex mapLock;

  void close() {
    if (closed.exchange(true)) return;
    shm_unlink(name.c_str());
    if (attached.load() == 0) unmap();
  }
  void detach() {
    if (--attached == 0 && closed.load()) unmap();
  }
  void unmap() {
    std::lock_guard<std::mutex> guard(mapLock);
    if (base) munmap(base, bytes);
    base = nullptr; header = nullptr;
  }
};

static void unrefRing(KgfRing* ring) {
  if (--ring->refs == 0) delete ring;
}

// Null on failure, with the reason in `error`.
extern "C" KgfRing* kgf_ring_create(const char* name, uint32_t w, uint32_t h, char* error, size_t error_length) {
  const auto fail = [&](const std::string& why) -> KgfRing* {
    if (error && error_length) { std::strncpy(error, why.c_str(), error_length - 1); error[error_length - 1] = 0; }
    return nullptr;
  };
  const size_t len = name ? std::strlen(name) : 0;
  if (len == 0 || len > 30 || name[0] != '/') return fail("Ring name must start with / and be at most 30 characters");
  if (!validRaster(w, h)) return fail("Invalid ring raster");
  auto* ring = new KgfRing();
  ring->name = name; ring->width = w; ring->height = h;
  ring->slotBytes = kairo_ring::slotBytesFor(w, h);
  ring->bytes = kairo_ring::totalBytes(ring->slotBytes);
  shm_unlink(name); // A stale ring from a crashed run must not be reused.
  const int fd = shm_open(name, O_CREAT | O_EXCL | O_RDWR, 0600);
  if (fd < 0) { delete ring; return fail(std::string("shm_open failed: ") + strerror(errno)); }
  if (ftruncate(fd, static_cast<off_t>(ring->bytes)) != 0) {
    const std::string why = strerror(errno);
    ::close(fd); shm_unlink(name); delete ring;
    return fail("ftruncate failed: " + why);
  }
  ring->base = mmap(nullptr, ring->bytes, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
  ::close(fd);
  if (ring->base == MAP_FAILED) { ring->base = nullptr; shm_unlink(name); delete ring; return fail("mmap failed"); }
  ring->header = static_cast<kairo_ring::Header*>(ring->base);
  kairo_ring::initHeader(ring->header, w, h);
  return ring;
}

extern "C" void kgf_ring_close(KgfRing* ring) { if (ring) ring->close(); }
extern "C" void kgf_ring_free(KgfRing* ring) { if (ring) { ring->close(); unrefRing(ring); } }

// 1 when a new frame was copied; `all_even` reports that no slot is mid-write.
extern "C" int kgf_ring_read(KgfRing* ring, uint64_t* last, uint8_t* out, size_t length, bool* all_even) {
  if (!ring || !ring->header || !last || length != kairo_ring::frameBytes(ring->width, ring->height)) return -1;
  *all_even = true;
  for (auto& slot : ring->header->slots) if (slot.seq.load() & 1) *all_even = false;
  return kairo_ring::readNewest(ring->header, ring->base, *last, out, length) ? 1 : 0;
}

// ─── Converter ────────────────────────────────────────────────────────────────

// Runs on a Metal completion thread. `data` is valid only during the call and
// is null in ring mode or on error. The callback must not close the converter.
typedef void (*KgfDone)(void* user, const char* error, const uint8_t* data, size_t length, uint32_t slot, uint64_t sequence);

struct KgfConverter {
  uint32_t width = 0, height = 0;
  bool alpha = false;
  KgfRing* ring = nullptr;
  size_t outBytes = 0;
  id<MTLBuffer> buffers[kairo_ring::SLOTS] = {nil, nil, nil};
  bool busy[kairo_ring::SLOTS] = {false, false, false};
  uint32_t inFlight = 0;
  std::mutex lock;
  std::condition_variable drained;
  std::atomic<bool> closed{false};
  std::atomic<uint64_t> converted{0}, skipped{0}, failed{0};
};

extern "C" KgfConverter* kgf_open(uint32_t width, uint32_t height, bool alpha, KgfRing* ring) {
  if (!gpu().error.empty() || !validRaster(width, height)) return nullptr;
  if (ring && (ring->closed || alpha || ring->width != width || ring->height != height)) return nullptr;
  auto* c = new KgfConverter();
  c->width = width; c->height = height; c->alpha = alpha;
  c->outBytes = kairo_ring::frameBytes(width, height) + (alpha ? size_t(width) * height : 0);
  @autoreleasepool {
    for (uint32_t i = 0; i < kairo_ring::SLOTS; ++i) {
      c->buffers[i] = ring
        ? [gpu().device newBufferWithBytesNoCopy:kairo_ring::slotData(ring->base, i, ring->slotBytes)
                                          length:ring->slotBytes
                                         options:MTLResourceStorageModeShared
                                     deallocator:nil]
        : [gpu().device newBufferWithLength:c->outBytes options:MTLResourceStorageModeShared];
      if (!c->buffers[i]) { delete c; return nullptr; }
    }
  }
  if (ring) { ++ring->refs; ++ring->attached; c->ring = ring; }
  return c;
}

static int takeSlot(KgfConverter* c) {
  std::lock_guard<std::mutex> guard(c->lock);
  const uint32_t avoid = c->ring ? c->ring->header->latest.load(std::memory_order_acquire) : kairo_ring::SLOTS;
  const bool anyPublished = c->ring && c->ring->header->published.load(std::memory_order_acquire) > 0;
  for (uint32_t i = 0; i < kairo_ring::SLOTS; ++i) {
    if (c->busy[i] || (anyPublished && i == avoid)) continue;
    c->busy[i] = true; ++c->inFlight;
    return int(i);
  }
  return -1;
}

// 1 started, 0 frame dropped (done is not called), negative on error:
// -1 closed, -2 null surface, -3 not BGRA, -4 could not wrap, -5 misaligned rows.
extern "C" int kgf_convert(KgfConverter* c, IOSurfaceRef surface, KgfDone done, void* user) {
  if (!c || c->closed) return -1;
  if (c->ring && c->ring->closed.load()) { ++c->skipped; return 0; }
  if (!surface) return -2;
  if (IOSurfaceGetPixelFormat(surface) != 'BGRA') { ++c->failed; return -3; }
  // Metal aborts the process on a misaligned surface instead of returning nil.
  if (IOSurfaceGetBytesPerRow(surface) % [gpu().device minimumLinearTextureAlignmentForPixelFormat:MTLPixelFormatBGRA8Unorm]) { ++c->failed; return -5; }
  @autoreleasepool {
    MTLTextureDescriptor* desc = [MTLTextureDescriptor texture2DDescriptorWithPixelFormat:MTLPixelFormatBGRA8Unorm
                                                                                    width:IOSurfaceGetWidth(surface)
                                                                                   height:IOSurfaceGetHeight(surface)
                                                                                mipmapped:NO];
    desc.usage = MTLTextureUsageShaderRead;
    id<MTLTexture> src = [gpu().device newTextureWithDescriptor:desc iosurface:surface plane:0];
    if (!src) { ++c->failed; return -4; }
    const int taken = takeSlot(c);
    if (taken < 0) { ++c->skipped; return 0; }
    const uint32_t slot = uint32_t(taken);
    KgfRing* ring = c->ring;
    if (ring) kairo_ring::beginWrite(ring->header, slot);
    const Params params{uint32_t(src.width), uint32_t(src.height), c->width, c->height, c->alpha ? 1u : 0u, 0};
    id<MTLCommandBuffer> cb = [gpu().queue commandBuffer];
    encode(cb, src, c->buffers[slot], params);
    [cb addCompletedHandler:^(id<MTLCommandBuffer> finished) {
      (void)src; // Keeps the IOSurface wrapped until the GPU is done.
      std::string error;
      uint64_t sequence = 0;
      if (finished.status != MTLCommandBufferStatusCompleted) {
        error = std::string("GPU conversion failed: ") +
          (finished.error ? finished.error.localizedDescription.UTF8String : "unknown");
      }
      if (ring && !ring->closed) {
        if (error.empty()) {
          kairo_ring::endWrite(ring->header, slot);
          sequence = ring->header->published.load(std::memory_order_acquire);
        } else {
          ring->header->slots[slot].seq.fetch_add(1, std::memory_order_release); // back to even, unpublished
        }
      }
      const bool copyOut = !ring && error.empty();
      done(user, error.empty() ? nullptr : error.c_str(),
           copyOut ? static_cast<const uint8_t*>(c->buffers[slot].contents) : nullptr,
           copyOut ? c->outBytes : 0, slot, sequence);
      std::lock_guard<std::mutex> guard(c->lock);
      c->busy[slot] = false;
      --c->inFlight;
      c->drained.notify_all();
    }];
    [cb commit];
    ++c->converted;
  }
  return 1;
}

extern "C" void kgf_stats(KgfConverter* c, uint64_t* converted, uint64_t* skipped, uint64_t* failed) {
  *converted = c->converted; *skipped = c->skipped; *failed = c->failed;
}

// Waits for every completion, because Metal may run handlers after
// waitUntilCompleted returns. Then the ring's memory may go.
extern "C" void kgf_close(KgfConverter* c) {
  if (!c) return;
  c->closed = true;
  {
    std::unique_lock<std::mutex> guard(c->lock);
    c->drained.wait(guard, [c] { return c->inFlight == 0; });
  }
  if (c->ring) { c->ring->detach(); unrefRing(c->ring); }
  delete c;
}

extern "C" IOSurfaceRef kgf_test_surface(const uint8_t* bytes, size_t length, uint32_t w, uint32_t h, uint32_t format, bool aligned) {
  if (!bytes || !w || !h || length != size_t(w) * h * 4) return nullptr;
  NSDictionary* props = @{
    (id)kIOSurfaceWidth: @(w), (id)kIOSurfaceHeight: @(h), (id)kIOSurfaceBytesPerElement: @4,
    (id)kIOSurfaceBytesPerRow: @(aligned ? IOSurfaceAlignProperty(kIOSurfaceBytesPerRow, w * 4) : w * 4), (id)kIOSurfacePixelFormat: @(format),
  };
  IOSurfaceRef surface = IOSurfaceCreate((__bridge CFDictionaryRef)props);
  if (!surface) return nullptr;
  IOSurfaceLock(surface, 0, nullptr);
  auto* base = static_cast<uint8_t*>(IOSurfaceGetBaseAddress(surface));
  const size_t stride = IOSurfaceGetBytesPerRow(surface);
  for (uint32_t y = 0; y < h; ++y) std::memcpy(base + y * stride, bytes + size_t(y) * w * 4, size_t(w) * 4);
  IOSurfaceUnlock(surface, 0, nullptr);
  return surface;
}

extern "C" void kgf_test_surface_release(IOSurfaceRef surface) { if (surface) CFRelease(surface); }
