# Spike 005: NDI, DeckLink ring and macOS native code from Rust

**Date:** 2026-11-09. **Machine:** Apple Silicon Mac, macOS 27.2, NDI SDK 6.3.2.0, no DeckLink card, no NDI Studio Monitor or OBS.
**Code:** `spikes/sinks`, `spikes/sinks-grafton`. **Log:** `evidence/005/probe.log`.

**Overall: PASS on macOS arm64 for everything testable here.** NDI works from Rust with no Node runtime, the ring and golden colours pass, and the `gpu_frames` converter runs through a C ABI. **HARDWARE-UNVERIFIED:** a real DeckLink card and a third-party NDI receiver. Only macOS arm64 was run.

## Chosen NDI approach: thin `libloading` binding

| Requirement | `libloading` binding | `grafton-ndi` 2.0.0 |
|---|---|---|
| Load from the app folder, no global install | PASS: explicit absolute path | **FAIL as built:** links `@rpath/libndi.dylib`, no rpath, and dyld stopped the process at launch |
| Missing library handling | PASS: returns an error; unit tested | **FAIL:** the process cannot start, against decision 001's rule that a broken NDI disables only NDI |
| Build needs | SDK headers read once, by a person | SDK and libclang on every build machine |
| UYVA, BGRA, stride, FLTp, discovery | PASS, exercised | Types present in source; send not exercised |
| Explicit destroy, 10 cycles | PASS, exercised | Not exercised |

`grafton-ndi` could gain an rpath through build settings, which would solve app-folder loading when the file is present, but not the launch failure when it is absent. Its send path was not run, because the launch failure decided the comparison.

The binding resolves exported C symbols instead of mirroring the `NDIlib_v6` function table. Struct layouts come from the 6.3.2 headers, and offsets are unit tested for 64-bit targets. `libndi` is copied to a gitignored `app-folder/`.

## Results

| # | Path | Result |
|---|---|---|
| 1 | Load from app folder, version | PASS: 6.3.2.0. The SDK is also installed globally on this Mac, but loading used the absolute path |
| 1 | UYVA and BGRA with stride, FLTp audio, explicit destroy, 10 load-send-destroy cycles | PASS |
| 1 | Malformed video and audio rejected before the SDK | PASS |
| 1 | Discovery | PASS: found itself and one other LAN source |
| 2 | UYVA image received | PASS by **in-process NDI SDK loopback only**: 1280×720, delivered as BGRA at stride 5120, centre pixel `[2,0,255,255]` for red. **HARDWARE-UNVERIFIED with NDI Studio Monitor or OBS**, neither is installed. The receiver shares the sender's library, and the alpha plane was not compared |
| 3 | Ring, exact `ring.h` layout | PASS: size and offsets asserted against the C++ header. Rust wrote 600 1080p frames, and a separate process using `ring.h` read 600 with 0 torn |
| 3 | Ring failure paths | PASS: bad names and rasters, stale ring replaced, wrong raster refused, ring gone after close, no frames yet |
| 3 | `decklink-helper --play-shm` | **HARDWARE-UNVERIFIED.** Started by Rust with a real ring; it exited 1 with `Device disconnected`. `--list` reports no ports, so it never read a frame |
| 4 | `gpu_frames` C ABI built with `cc`, Metal section unmodified | PASS for this addon: `available`, `open`, asynchronous IOSurface `convert`, `close`, `stats`, `convertPixels`, ring create, close and read are all outside N-API. Built with `cc` in `spikes/sinks`, because spike code stays out of `kairo-native-sys`. `syphon.mm` and `recorder.mm` were not touched, as the prompt asks for one addon |
| 4 | Converter behaviour, mirroring the Electron tests | PASS: 18 library tests, and the 8 converter tests repeated 8 times. Three slots with no queueing, the newest ring slot never overwritten, ring publishing, closed ring drops quietly, close waits for in-flight work, invalid inputs rejected |
| 4 | Golden: black, white, Rec.709 red | PASS, byte for byte against `uyvy.h`: `[128,16,128,16]`, `[127,235,128,235]`, `[102,63,240,63]`. The IOSurface path matches too. A 1080p gradient at equal size and scaled to 720p also matches, and the alpha plane is checked |
| 5 | IOSurface | Written below, not prototyped |

White's U value is 127 because of integer rounding in `uyvy.h`. The first hand-written expected value of 128 was wrong; the header decided it.

## 1080p UYVA conversion latency

Decision 008 records p50 0.6 ms and p99 1.5 to 2.2 ms for IOSurface input, submit to callback. The comparable path here submits an IOSurface and copies the result out in the completion, with no JS thread hop. Three batches of 120 frames each, in two runs:

| Run | Load average | p50 | p99 |
|---|---|---|---|
| Earlier run (not kept in the log) | about 3.4 | 0.60, 0.70, 0.78 ms | 2.58, 2.58, 3.16 ms |
| Final run, in `probe.log` | about 4.7 | 0.97, 0.97, 1.04 ms | 4.80, 4.81, 4.46 ms |

The median reaches decision 008's figure at its best. p99 is above decision 008's range in every batch. Other work was running on this Mac, and decision 008's own load conditions are not known, so an idle-machine rerun is needed to tell load from a real difference. The earlier run used the build before the alignment guard.

The CPU-upload path, for reference, n=300: 3.5 to 4.5 ms p50 in total.


## wgpu Metal textures and IOSurface

Read from `wgpu` and `wgpu-hal` 30, not prototyped. The route exists: create an IOSurface, make an `MTLTexture` with `newTextureWithDescriptor:iosurface:plane:` on the device from `Device::as_hal::<Metal>()`, wrap it with `hal::metal::Device::texture_from_raw`, then import it with `Device::create_texture_from_hal`. The render target then has a shared IOSurface, so Syphon and the converter could read it without a CPU readback. Risks: it is unsafe and tied to the Metal backend, the texture must use a format IOSurface and wgpu both accept, and GPU completion must be sequenced before another consumer reads. Readback remains the fallback and the only cross-platform route.

## Finding: misaligned IOSurface aborts the process

Metal asserts and aborts on an IOSurface whose row size is not aligned to the device's linear alignment. The port rejects this with an error. `gpu_frames.mm` has no such check. Chromium's surfaces may always be aligned, which was not checked. Native capture sources should keep the guard.

The C ABI also differs in one way that matters: with no JS thread to drain completions, `kgf_close` waits on a condition variable, because Metal may run handlers after `waitUntilCompleted` returns.

## Failure paths handled

From the source: invalid ring names and rasters; a stale ring; `shm_open`, `ftruncate` and `mmap` failures with unlink; incompatible rasters; a reader during a write; an already-unlinked ring; missing, too-small or incompatible helper rings in the stub; Metal unavailable; bad conversion dimensions; GPU errors; a busy converter; a closed converter or ring; a null, non-BGRA or misaligned surface; a wrong ring raster or alpha on a ring; unavailable NDI; a null sender, finder or receiver; a null source URL; receive errors. Not exercised by fault injection: `ftruncate` and `mmap` failure, Metal unavailable, GPU error.

## Remaining before release

1. View the UYVA stream in NDI Studio Monitor or OBS, including alpha.
2. Run `--play-shm` with a DeckLink card attached.
3. Run decision 001's probes on Intel macOS, Windows and Linux. Decision 001 makes these a release requirement.
4. Rerun the IOSurface latency on an idle machine.

## Gates

Spike fmt, Clippy and tests pass, with 18 library tests in release. `sinks-grafton` fmt and Clippy pass. Root `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` pass. No NDI SDK file is in the untracked set, and the listed Electron native folders are unchanged.
