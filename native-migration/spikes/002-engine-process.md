# Spike 002: separate engine process with output windows and preview

**Date:** 2026-11-09. **Machine:** Apple Silicon Mac, macOS 27.2, built-in display, plus a Sceptre Z27 over HDMI for the second-display checks.
**Code:** `spikes/engine-window` (`controller` is GPUI, `engine` is winit and wgpu). **Raw logs:** `evidence/002/`.

**Overall: PASS on macOS, including a real second display, with one finding on unplug.** Process separation, IPC, preview, crash recovery, focus, placement, cursor hiding and replug pass. Windows and Linux were not run, as no machines are available, and remain unproven.

## Method

Release binaries. Control messages are length-prefixed bincode over `interprocess` local sockets. Pings are answered on the engine's socket thread, so latency covers the transport and not the winit loop. Preview is a 640×360 BGRA offscreen render, read back and published to a memory-mapped file holding three seqlocked frames. CPU comes from `ps` CPU-time deltas between the 10 s and 50 s samples of separate 60-second runs. Each sample logs load average.

**Load caveat:** other work ran on this Mac during testing. Load average was about 20 during latency and relaunch, and fell to about 2 to 8 during the CPU runs. Treat CPU numbers as indicative.

## Results

| # | Requirement | Result | Evidence |
|---|---|---|---|
| 1 | GPUI controller launches winit child, one window per display, animated wgpu scene | PASS on one display | `smoke.log`, `controller-img.png` |
| 2a | No second Dock or Cmd-Tab entry | PASS: accessory policy reported and confirmed by eye | `second-display.log` |
| 2b | Output windows never take focus | PASS: `isKey=false`, `canBecomeKey=false`, and a physical click left focus on the controller, confirmed by eye. Needs a runtime override of `canBecomeKeyWindow` | `second-display.log` |
| 2c | Above desktop and over the menu bar | PASS on the secondary display: level 1000, full 3008×1692 pt frame, menu bar covered, confirmed by eye | `second-display.log` |
| 2d | Cursor hides over outputs | PASS: confirmed by eye on the secondary display | |
| 2e | Display unplug and replug detected | PASS with a finding: parked at 253.06 s, restored on the same display at 274.36 s, 16 ms after detection. A quick disconnect and reconnect at 282 to 283 s was also handled. See the finding below | `second-display.log` |
| 3 | Round trip over 10,000 messages, p99 under 2 ms | PASS: p50 0.0053, p95 0.0149, p99 0.0305, max 0.0778 ms | `smoke.log` |
| 4a | Preview at 15 and 30 fps through shared memory, shown with `img()` and `drop_image` | PASS | `controller-img.png`, soak logs |
| 4b | Preview under 5% of one core at 15 fps | PASS: 3.95% (controller +3.16, engine +0.79 over idle) | table below |
| 4c | 30-minute soak, RSS plateaus | PASS, with the concurrency and occlusion notes below | soak logs |
| 4d | Compare `surface()` NV12 | Done: `surface()` costs more CPU here, because of CPU BGRA-to-NV12 conversion. Its picture was not inspected | table below |
| 5a | SIGKILL child, relaunch within 1 s | PASS: 217.1 ms from kill to first frame, state replayed | `smoke.log` |
| 5b | Kill parent, child holds for configurable grace | PASS: held, exited 3.02 s after loss with a 3 s grace | `parent-kill.log` |
| 6 | Windows, Linux X11 and Wayland | **NOT RUN.** All of sections 2 to 5 remain unproven there. The non-macOS focus, level and cursor paths are not implemented in this spike | |

## CPU and memory, 60-second runs

| Mode | Controller % | Engine % | Footprint 10 s → 50 s (MB) |
|---|---|---|---|
| No preview | 0.07 | 9.18 | 37.6 → 37.3 |
| `img()` 15 fps | 3.23 | 9.97 | 57.3 → 63.7 |
| `img()` 30 fps | 6.45 | 11.70 | 58.2 → 59.0 |
| `surface()` 15 fps | 5.95 | 10.77 | 54.5 → 54.6 |
| `surface()` 30 fps | 11.03 | 11.25 | 59.5 → 64.5 |
| `img()` 30 fps, no `drop_image` | 7.70 | 8.90 | **635.9 → 3031.9** |

The last row is the control. Each replaced preview must call `drop_image`.

## 30-minute soak

RSS in KiB. Footprint is the controller's physical footprint, which includes GPU atlas memory.

| Run | Point | Controller RSS | Footprint MB | Engine RSS |
|---|---|---|---|---|
| 15 fps | ~10 s | 110,160 | 57.3 | 88,448 |
| 15 fps | ~10 min | 50,752 | 58.6 | 40,704 |
| 15 fps | ~30 min | 43,264 | 43.6 | 35,824 |
| 30 fps | ~10 s | 100,896 | 59.3 | 85,568 |
| 30 fps | ~10 min | 60,176 | 57.8 | 42,112 |
| 30 fps | ~30 min | 45,248 | 47.9 | 44,848 |

Footprint range after start: 15 fps 43.6 to 61.2 MB, 30 fps 47.0 to 67.2 MB. No relaunch or `drop_image` failure was logged. The engine stayed alive throughout.

Notes: the two soaks overlapped. The last sample is near 1,785 s. CPU per minute fell late in the runs and `frontmost_is_controller` was false at the end, so the windows were probably in the background or occluded for part of the soak, and painting may have slowed. RSS falling also suggests memory compression. The soak shows no growth, but it is not proof of behaviour under continuous foreground painting. There is no public atlas counter in this GPUI version, so atlas behaviour is inferred from footprint and the control run.

## Failure paths handled

From the Electron source: rehearsal versus borderless windows; close requests ignored, so settings and windows agree; id then unique label and size matching, refusing to guess between identical displays and refusing claimed displays; a null binding; parking on loss and no placement on another screen; three failures per rolling minute, after which relaunching stops.

Added for this design: engine spawn failure; connect timeout; truncated and oversized messages; invalid shared-memory mapping; a writer killed mid-frame; a stale socket name; no GPU adapter; outdated or lost surfaces; zero-size resizes; `drop_image` errors; controller reconnection cancelling the grace period (implemented, not exercised).

Not reproduced: Electron's load-generation guards, because this spike builds windows synchronously.

## Finding: output flashes on the remaining screen at unplug

When the HDMI cable was pulled, the gradient briefly appeared on the laptop screen before going away. macOS moves the window when its display disappears, and this spike only polls displays once a second, so the screen-saver-level window can sit over the operator's controls for up to about a second. The duration was seen by a person and not measured. Electron's `ManagedWindow` exists so that a window is never placed on whatever display is left, so the native engine must not ship this behaviour.

Prompt 050 should react to the display-change notification instead of polling, and hide or close the window at once. That fix is untested: whether the notification arrives before macOS moves the window needs checking.

## To close before release

1. Run the core checks on Windows and Linux X11 and Wayland, implementing their window behaviour first.
2. Optionally repeat the soak with the controller kept visible and alone.

## Gates

`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` passed in the spike, with 8 library tests. `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` passed at the root. The prompt's Electron source files showed no changes.
