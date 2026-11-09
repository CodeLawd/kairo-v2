# Spike 007: operator UI on GPUI Kit

**Date:** 2026-11-09. **Machine:** Apple Silicon Mac, macOS 27.2, built-in ProMotion display, on battery in **Low Power Mode**.
**Code:** `spikes/ui-platform`. **Logs and screenshots:** `evidence/007/`.

**Overall: PARTIAL.** The shell, grid, interactions and file open work on macOS. Frame-time targets are not proven, and Windows, Linux, older macOS and a real IME were not tested.

| # | Requirement | Result | Evidence |
|---|---|---|---|
| 1 | Coding and Design Guides read | Done | |
| 2 | Traffic lights at {18,24}, transparent title bar | PASS | `native-dark.png` |
| 2 | Under-window material on header and rail | PASS by source: `Blurred` is an `NSVisualEffectView` with `UnderWindowBackground`, behind-window blending. It sets state Active where Electron uses followWindow. Translucency over a contrasting desktop not compared by eye | GPUI source |
| 2 | Opaque inset workspace, 16 px top-left radius, 0.5 px hairlines | PASS | `native-dark.png` |
| 2 | Pinned light or dark independent of OS | PASS by screenshots of both modes, on one OS appearance | `native-dark.png`, `native-light.png` |
| 3 | 5,000 cards virtualised, changing columns | PASS: a bounded visible set is drawn, thumbnails LRU-capped at 400 with `drop_image` | UI tests |
| 3 | Marquee, drag reorder, zoom, keyboard | PASS in UI integration tests, through real pointer and key events. Edge autoscroll and hand use not tested | `tests/ui.rs` |
| 3 | Scrolled with the spike 002 preview | Ran; see frame times | logs |
| 8 | 120 Hz, no frame over 16 ms | **NOT MET in the one valid full-power run, under load;** not proven on an idle machine | below |
| 8 | Open grid under 100 ms | **MIXED:** 52.57, 64.01 (20 cards) and 73.87 ms in the bundle, but 438.72 and 934.64 ms in runs started beside heavy load | logs |
| 4 | Yoruba composition through the IME handler; undo | PASS at handler level. **Real Yoruba keyboard or IME UNVERIFIED** | below |
| 4 | Rich text | **Kit does not cover it.** Recommendation below | |
| 5 | Minimum macOS | **Declared 11.0, runtime UNVERIFIED** | below |
| 6 | Windows, Linux, Linux without Vulkan | **NOT RUN** | |
| 7 | `.kairo` open | PASS on macOS, cold and warm | `open-file.log` |
| Deliverable | Side-by-side with Electron | **PARTIAL:** an isolated Electron profile stops at onboarding (`electron-isolated.png`), so its shell was not captured. A signed-in profile was not used, to keep personal data out of the evidence | |

## Frame times

GPUI caps every window at 33.3 ms in Low Power Mode by default (`low_power_frame_interval`), and inactive windows too. All capped runs are therefore about 30 fps by design, and say nothing about 120 Hz. Scrolling was programmatic, through the same offset the wheel moves, at about 2,400 px/s, using GPUI's frame snapshot, the source `gpui-fps` reads. The HUD itself was not shown.

| Run, 20 s, 5,000 cards, active window | Draw p50 / p99 / max (ms) | Present p50 / p99 / max (ms) |
|---|---|---|
| Preview, default cap | 2.70 / 5.13 / 9.18 | 33.23 / 36.54 / 37.03 |
| No preview, default cap | 2.78 / 5.05 / 7.30 | 33.31 / 36.18 / 41.62 |
| Preview, cap removed | 2.01 / 4.74 / 10.42 | 8.33 / 10.71 / 33.88 |

In the uncapped run, 2 of 736 presented intervals exceeded 16 ms. But 416 of 1,151 `on_next_frame` callbacks were over 16 ms, at about 33 ms, so a large share of frames were not on the 120 Hz cadence. The cause was not isolated; the Mac stayed in Low Power Mode, and load average was 8 to 10. `Window::draw` stayed well inside budget, with a 10.42 ms maximum.

### Rerun on AC power with Low Power Mode off

`pmset` confirmed the power state before and after. Load average was 9.6 to 14, with other work running on this Mac.

| Run, 20 s, 5,000 cards | Window active at end | Draw p50 / p99 / max (ms) | Present p50 / p99 / max (ms) | Presents over 16 ms |
|---|---|---|---|---|
| `ac-preview-1`, with preview | Yes | 1.63 / 4.01 / 9.49 | 8.40 / 17.87 / 34.67 | 31 of 1,382 |
| `ac-preview-2`, with preview | **No: invalid** | 2.16 / 4.96 / 5.32 | 8.50 / 24.92 / 45.88 | 19 of 161 |
| `ac-nopreview` | **No: invalid** | 2.09 / 4.48 / 5.80 | no presents recorded | 0 of 0 |

Only `ac-preview-1` is usable. It runs at 120 Hz at the median but **misses "no frame over 16 ms"**: 31 of 1,382 presented intervals exceeded it (2.2%), and 271 of 1,624 `on_next_frame` callbacks did. The window may have lost focus briefly, since the check only samples the end of the run, and the machine was under load, so this does not separate GPUI from the environment. GPUI's own draw never exceeded 9.49 ms, so the draw work was within the 16 ms budget; the long intervals were outside `Window::draw`. The cause was not isolated. Open to first frame in these runs: 77.75, 66.94 and 95.47 ms.

**To close:** repeat on AC power with no other work running and the window kept in front, sampling focus throughout.

**Product note:** decide whether an operator's Mac in Low Power Mode should keep the 30 fps default.

## IME and undo

Marked text `e`, then `ẹ`, then `ẹ\u{301}` composes in place and commits, leaving no marked range. After more text the value is U+1EB9 U+0301 U+006B U+1ECD U+0300. Backspace removed only U+0300, one code point rather than a grapheme. Undo restored it. Whether that deletion matches what Yoruba typists expect needs a person to judge. The placeholder rendered `ẹ̀kọ́` with the default font, visible in the screenshot.

This calls the handler macOS uses; no Yoruba layout was installed or typed on.

## Rich text recommendation

The Tiptap editor in `SermonNotesReviewModal.tsx` needs editable paragraphs, ranged highlight marks with `reference` and `active` attributes, rebuilding marks after edits, text extraction with a block separator, and on-screen coordinates for a position.

- Kit's `Editor` is documented in source as a source-code editor. Colouring comes from language highlighting.
- A search of `InputState`'s public functions found no API for arbitrary ranged highlights with attributes.
- `TextView` renders Markdown and HTML and is not an editing surface.

**Recommendation:** do not use `Editor` or `TextView` for this. Build a focused editor on GPUI's `EntityInputHandler` and styled text runs, with marks stored as domain ranges. It was not prototyped here, so its cost is an estimate for prompt 070 to confirm.

## Minimum macOS

The release binary's `LC_BUILD_VERSION` is minos 11.0, SDK 27.2, matching Kairo's `LSMinimumSystemVersion` of 11.0. It links `UniformTypeIdentifiers`, itself macOS 11. GPUI guards several newer calls with version or selector checks. Those are static signs only. **Only macOS 27.2 was reachable**, so no older OS has run this, and the full Metal and AppKit usage was not audited. Treat 11.0 as declared, not proven.

## `.kairo` file open

`Application::on_open_urls` must be registered before `run`. A temporary signed bundle with a `CFBundleDocumentTypes` entry received a percent-encoded `file://` URL on cold launch, before the first frame, and again while running. The spike decodes, filters by extension and existence, and queues paths. The temporary bundle was unregistered afterwards. Windows and Linux command-line delivery and single-instance forwarding have no GPUI equivalent in this spike, and need their own design.

## Failure paths handled

Empty grid; narrow width, always one column; marquee over gaps, past the end and while scrolling; plain press on a card left to click or drag; ⌘ press starting a marquee; drop onto a moved card; zoom limits; navigation at edges; missing or non-`.kairo` files; encoded paths; engine or preview unavailable.

## Findings for the app

- `secondary-` works in test key presses but panics in `KeyBinding::new`. Use `cmd-` and `ctrl-`, or the OS-aware parser.
- Launch through a bundle: a bare binary was not brought to the front, so GPUI throttled it as inactive.

## Gates

Spike fmt and Clippy pass, with 8 model and 6 UI integration tests. Root fmt, Clippy and tests pass.
