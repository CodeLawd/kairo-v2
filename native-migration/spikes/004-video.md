# 004 — Video decode into wgpu

The standalone Rust spike in [spikes/004-video](../../spikes/004-video)
decodes video with FFmpeg and converts uploaded YUV planes to RGBA in WGSL.
The test host was an Apple M2 Pro on macOS 27.2. The GPU backend was Metal.
The release build used ffmpeg-next 8.1.0, dynamically linked FFmpeg 8.0.1,
and wgpu 30.0.1. The FFmpeg configuration reported LGPL 2.1-or-later, and
`otool -L` showed shared libavcodec, libavformat, libavutil and libswscale.

The spike measures an offscreen texture after GPU completion. It does not
include audio, a compositor layer stack, a window surface or display
presentation. The [runbook](../../spikes/004-video/README.md) contains the
FFmpeg configure flags, fixture recipes and commands. The source checkout
was read at `de3bb54` plus its existing working-tree changes. The only drift
in the four reference files was an uncommitted update to the overlay.

## Format results

| Extension | Fixture | Decoder | macOS | Windows | Linux |
|---|---|---|---|---|---|
| .mp4 | H.264, 1080p30 and 4K30, 8-bit | VideoToolbox and software at 1080p30 | PASS | Unverified | Unverified |
| .m4v | H.264, 1080p60, 8-bit | VideoToolbox | PASS | Unverified | Unverified |
| .mov | HEVC Main 10, 1080p30 | VideoToolbox | PASS, P010 | Unverified | Unverified |
| .mov | ProRes 4444 with alpha | FFmpeg software | PASS, alpha 80 at center pixel | Unverified | Unverified |
| .webm | VP9 with alpha | libvpx software | PASS, alpha 80 at center pixel | Unverified | Unverified |
| .ogv | Theora, 640 × 360 at 30 fps | FFmpeg software | PASS, 60 frames then EOF | Unverified | Unverified |

The video formats above cover every video extension in `pa-media-range.ts`.
The remaining MIME mappings are images handled by the image layer. VP9 alpha
requires the libvpx decoder because the native VP9 path does not combine
WebM's alpha payload. The spike uses software for ProRes to preserve possible
alpha. The alpha path currently reduces ProRes 4:4:4 color to 8-bit 4:2:0,
while preserving a separate 8-bit alpha plane.

The decoder uses VideoToolbox on macOS, D3D11VA on Windows and VAAPI on Linux
when FFmpeg advertises a matching device configuration. On a hardware
failure it reopens the stream in software and seeks past already submitted
timestamps. Windows and Linux builds and devices are unverified. FFmpeg's
hardware transfer produced ordinary NV12 or P010 planes. The shader reads
strided luma and chroma uploads as integer textures, unpacks P010's top ten
bits and applies luma and chroma scaling for limited or full range. It uses
the decoded frame's coefficients for BT.601, BT.709, SMPTE 240M and BT.2020
non-constant luminance, with an SD or HD heuristic when metadata is missing.
Other matrices, HDR transfer functions, gamut conversion and tone mapping
are outside this proof. The RGBA output is eight bits per channel.

## Throughput and transport

The 10-second H.264 trials used a four-second synthetic test pattern, with
looping enabled and keyframes at one-second intervals. The HEVC trial ran for
eight seconds. A submission interval is time between completed GPU conversions.
The GPU column includes plane upload, dispatch and a wait for completion.
The hardware download column times `av_hwframe_transfer_data` alone.

| Workload | Completed frames | Process CPU | Frame interval p99 | Upload + compute p99 | Hardware download p99 |
|---|---:|---:|---:|---:|---:|
| 1080p30 H.264, VideoToolbox | 300 | 8.55% | 41.7 ms | 10.2 ms | 0.5 ms |
| 1080p60 H.264, VideoToolbox | 600 | 14.86% | 24.8 ms | 9.4 ms | 0.7 ms |
| 4K30 H.264, VideoToolbox | 300 | 16.36% | 45.4 ms | 13.1 ms | 1.7 ms |
| 3 × 1080p30 H.264, VideoToolbox | 300 per stream | 18.93% total | 42.0, 44.3, 48.8 ms | 8.5, 9.1, 9.2 ms | 0.7, 0.8, 0.5 ms |
| 1080p30 HEVC Main 10, VideoToolbox | 240 | 11.81% | 44.8 ms | 12.7 ms | 1.4 ms |
| 1080p30 H.264, software | 300 | 23.64% | 41.2 ms | 8.1 ms | n/a |

All short-run frame interval p99 values were below 1.5 times nominal, including
24.8 ms against the 25 ms target at 60 fps. The widest untrimmed wrap gap was
46.2 ms at 4K30. The 1080p30 wrap gap was 33.8 ms, close to its 33.3 ms frame
period. These are frame completion timings. Visual smoothness at the display
and A/V sync remain unverified. Electron's baseline has no comparable
1080p playback CPU sample, so these figures do not establish a CPU win over
Electron.

- The 0.50–2.75 s trim loop at 2× wrapped four times, held the last texture
  during a 0.501 s pause, and sought to 1.300 s for a 1.273 s request. The
  27 ms error was within one source frame. This trial hit a **67.8 ms p99**
  interval and an **86.0 ms maximum wrap gap**, missing the smoothness target
  at 2×. Synchronous keyframe seek and decode pre-roll are on the playback
  path. The next implementation should prepare the next trim start before
  the wrap and verify the same fixture.
- The 0.50–1.50 s trim at 0.25× produced all 30 frames and reported ended.
  It completed in 3.876 s from the first schedule through the final visible
  sample, with a 140.0 ms p99 interval against a nominal 133.3 ms.
- Missing files and a start beyond EOF returned explicit errors. Invalid
  timing arguments are rejected before opening a decoder. A clip without
  video or without a frame in the requested range is an error. Clean EOF
  drains buffered frames and sets ended for a non-looping stream. The spike
  skips non-video packets, synthesizes PTS only when no timestamp is
  available, and returns demux, decode, transfer and GPU failures to the
  caller.

The Electron background transport ignores invalid seeks, clamps valid seeks to
the clip, restarts ended clips on resume, and considers absent or failed video
ended for lobby progression. It preserves pause across media readiness and
suppresses repeat play attempts while seeking or paused. Document clips use
time relative to their trim and hide a clip that fails to start. Image changes
wait for the latest request to decode and paint before exposing it to a sink.
The standalone spike has one scheduled seek and pause and reports errors to
its caller. Matching Electron's event, document and lobby policies belongs in
the production engine. The stored Electron rate clamps to 0.5×–2×. The spike
exercises the requested 0.25×–2×. Fit, hue, saturation, brightness and contrast
remain compositor concerns. HTTP range syntax and MIME routing remain outside
the local-file decoder.

## External texture assessment

No external texture was imported during the trials. The baseline quantifies
the transfer and upload costs that make each path worth investigating. Tight
8-bit 4:2:0 planes contain 3.11 MB per 1080p frame and 12.44 MB per 4K frame.
This is about 93 MB/s at 1080p30, 187 MB/s at 1080p60, 373 MB/s at 4K30 and
280 MB/s for three 1080p30 streams, before stride padding. P010 doubles those
plane sizes. A zero-copy path could remove the CPU transfer and upload, but it
would still run the shader. The upload-plus-compute measurement is a combined
cost, so it does not predict the size of a zero-copy improvement.

| Path | Integration to qualify | Recommendation |
|---|---|---|
| macOS IOSurface | Create Metal plane textures from the VideoToolbox pixel buffer with `CVMetalTextureCache` on wgpu's device. Wrap the retained textures through Metal HAL `texture_from_raw` and wgpu's `create_texture_from_hal`. Retain the decoder surface until the GPU completes and use normalized plane formats. | Start with upload. Prototype import first on macOS if 4K or P010 leaves too little compositor budget. Observed hardware transfer p99 was 0.5 ms at 1080p30, 1.7 ms at 4K30 and 1.4 ms for 1080p30 P010. |
| Windows D3D11 shared texture | FFmpeg's D3D11VA decoder emits a D3D11 resource and array slice. wgpu's DX12 HAL import takes an ID3D12Resource. Qualify a compatible shareable resource or interop copy, matching adapter and explicit fence ownership. | Defer pending measurements on Windows. There is no D3D11-to-DX12 import measurement. |
| Linux DMA-BUF | Export VAAPI planes as DRM PRIME and import the file descriptors as Vulkan external memory. Match format modifier, pitch, offset and plane bindings, and synchronize ownership before wrapping a VkImage with the Vulkan HAL. | Defer until supported driver and modifier combinations are known. No VAAPI or DMA-BUF timing was measured. |

The HAL entry points are present in the pinned wgpu source. Native handle
creation, synchronization and lifetime are separate requirements. [CoreVideo
Metal texture cache](https://developer.apple.com/documentation/corevideo/cvmetaltexturecache),
[D3D12 shared handles](https://learn.microsoft.com/en-us/windows/win32/api/d3d12/nf-d3d12-id3d12device-opensharedhandle),
[Vulkan DMA-BUF external memory](https://docs.vulkan.org/refpages/latest/refpages/source/VK_EXT_external_memory_dma_buf.html),
[wgpu HAL import](https://docs.rs/wgpu/30.0.1/wgpu/struct.Device.html#method.create_texture_from_hal).

## Distribution and codec review

Electron currently supplies H.264 and AAC via its bundled FFmpeg. The native
application would distribute and track its own FFmpeg binaries. The spike's
configuration disables GPL, nonfree and version-3 components. The distribution
build needs a reproducible shared-library configuration, exact corresponding
source and patches, build instructions, license texts and notices in the
installer, application and download material. Review how signed packages and
the EULA let recipients replace the LGPL library, and verify transitive codec
library notices and the actual binaries shipped on each OS. AAC was outside
this video-only build and needs its own playback and distribution verification.

Have the owner and counsel review H.264, AAC and HEVC patent or royalty
questions by intended market, codecs and distribution model. Review VP9,
Theora and ProRes obligations and platform SDK terms as well. Dynamic linking
and the FFmpeg license label do not settle those questions. The local source
build and fixtures are engineering evidence. [FFmpeg license and legal
considerations](https://ffmpeg.org/legal.html).

## Soak and gates

The 30-minute 1080p30 VideoToolbox loop completed in **1,800.002 s** with
**54,000 frames** and **450 wraps**. Process CPU averaged **10.66%**. Frame
interval p99 was **43.6 ms** against the 50 ms target. The longest wrap gap
was **61.7 ms**. Sampled RSS started at 34,416 KiB and ended at 35,232 KiB.
After the first minute, samples ranged from 26,704 to 44,288 KiB. This run
showed bounded RSS variation with a net increase of 816 KiB. It does not
establish a zero-growth guarantee. Raw logs are under
`/tmp/kairo-004-ffmpeg.88tsh7/results/`.

Formatting, Clippy with `-D warnings`, and Cargo tests passed for the native
workspace and the standalone crate. The Electron checkout status matched
the initial snapshot. The spike introduces no application API.
