// ─── Shader ───────────────────────────────────────────────────────────────────

static const char* kShaderSource = R"METAL(
#include <metal_stdlib>
using namespace metal;

struct Params { uint srcW; uint srcH; uint dstW; uint dstH; uint alpha; uint pad; };

inline int clamp8(int x) { return clamp(x, 0, 255); }
inline int luma(int r, int g, int b) { return clamp8(((47 * r + 157 * g + 16 * b + 128) >> 8) + 16); }
inline int4 bytesOf(float4 c) { return int4(rint(c * 255.0)); }

// One thread per UYVY group (two output pixels). Nearest-neighbour scaling uses
// the helper's integer mapping, so equal-size input is copied pixel for pixel.
kernel void bgra_to_uyvy(texture2d<float, access::read> src [[texture(0)]],
                         device uchar* dst [[buffer(0)]],
                         constant Params& p [[buffer(1)]],
                         uint2 gid [[thread_position_in_grid]]) {
  const uint x = gid.x * 2, y = gid.y;
  if (x >= p.dstW || y >= p.dstH) return;
  const uint sy = y * p.srcH / p.dstH;
  const int4 a = bytesOf(src.read(uint2(x * p.srcW / p.dstW, sy)));
  const int4 c = bytesOf(src.read(uint2((x + 1) * p.srcW / p.dstW, sy)));
  // Metal reads BGRA8 as (r, g, b, a). Premultiplied RGB is the colour
  // composited on black, as the CPU path sends it.
  const int r = (a.r + c.r) / 2, g = (a.g + c.g) / 2, b = (a.b + c.b) / 2;
  const uint o = (y * p.dstW + x) * 2;
  dst[o + 0] = uchar(clamp8(((-26 * r - 87 * g + 112 * b + 128) >> 8) + 128));
  dst[o + 1] = uchar(luma(a.r, a.g, a.b));
  dst[o + 2] = uchar(clamp8(((112 * r - 102 * g - 10 * b + 128) >> 8) + 128));
  dst[o + 3] = uchar(luma(c.r, c.g, c.b));
  if (p.alpha != 0) {
    // NDI UYVA: the alpha plane follows the UYVY plane, one byte per pixel.
    const uint ao = p.dstW * p.dstH * 2 + y * p.dstW + x;
    dst[ao] = uchar(a.a);
    dst[ao + 1] = uchar(c.a);
  }
}
)METAL";

struct Params { uint32_t srcW, srcH, dstW, dstH, alpha, pad; };

// ─── Metal singleton ──────────────────────────────────────────────────────────

struct Gpu {
  id<MTLDevice> device = nil;
  id<MTLCommandQueue> queue = nil;
  id<MTLComputePipelineState> pipeline = nil;
  std::string error;
};

static Gpu& gpu() {
  static Gpu instance;
  static std::once_flag once;
  std::call_once(once, [] {
    @autoreleasepool {
      instance.device = MTLCreateSystemDefaultDevice();
      if (!instance.device) { instance.error = "No Metal device"; return; }
      NSError* err = nil;
      id<MTLLibrary> library = [instance.device newLibraryWithSource:@(kShaderSource) options:nil error:&err];
      if (!library) { instance.error = std::string("Shader compile failed: ") + (err ? err.localizedDescription.UTF8String : "unknown"); return; }
      id<MTLFunction> fn = [library newFunctionWithName:@"bgra_to_uyvy"];
      instance.pipeline = [instance.device newComputePipelineStateWithFunction:fn error:&err];
      if (!instance.pipeline) { instance.error = std::string("Pipeline failed: ") + (err ? err.localizedDescription.UTF8String : "unknown"); return; }
      instance.queue = [instance.device newCommandQueue];
      if (!instance.queue) instance.error = "No command queue";
    }
  });
  return instance;
}

static void encode(id<MTLCommandBuffer> cb, id<MTLTexture> src, id<MTLBuffer> dst, const Params& params) {
  id<MTLComputeCommandEncoder> enc = [cb computeCommandEncoder];
  [enc setComputePipelineState:gpu().pipeline];
  [enc setTexture:src atIndex:0];
  [enc setBuffer:dst offset:0 atIndex:0];
  [enc setBytes:&params length:sizeof(params) atIndex:1];
  const MTLSize group = MTLSizeMake(16, 16, 1);
  const MTLSize groups = MTLSizeMake((params.dstW / 2 + 15) / 16, (params.dstH + 15) / 16, 1);
  [enc dispatchThreadgroups:groups threadsPerThreadgroup:group];
  [enc endEncoding];
}

