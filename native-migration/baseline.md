# Electron performance baseline

Measured November 9, 2026 on an Apple silicon Mac14,9 running macOS 27.2
(build 26B123), with the built-in 3024 × 1964 Retina display as the main display.
The packaged measurements use the existing arm64 Kairo 1.0.27 release bundle and
its matching DMG. Source tests use a writable copy of Electron commit `de3bb54`
with the operator's working tree edits, including the untracked document page
swap test. These are different revisions. The installed 1.0.33 app was excluded
so package, startup, and disk measurements all refer to the same bundle.

Other desktop apps remained open. Treat the timing as a development-machine
sample. The account gate measurement covers startup to the first usable
onboarding control. A signed-in operator shell and multiple outputs require
separate runs.

| Metric | Result | Repeatable method |
|---|---|---|
| Process start to first interactive onboarding screen | Median **3,661 ms**, range **3,609–7,338 ms** | Run the packaged executable with a new isolated profile per trial using `tools/baseline/startup.mjs`. Use a monotonic clock from spawn to the first visible renderer with an enabled onboarding or sign-in button. Poll loopback DevTools every 50 ms. OS caches remain warm across launches. |
| Idle RSS and CPU, 0 outputs | Median **411,264 KiB RSS** (401.6 MiB), range **394,192–419,280 KiB** across trial medians. CPU median **11.3%**, range **9.4–13.7%** across trial medians. | Wait 15 s after the startup milestone. Sample `ps` once per second for five seconds. Sum RSS and reported CPU for the main process and all descendants per sample. Take the median for each trial, then the median and range across trials. RSS includes shared pages counted by each process. macOS `ps` CPU is a recent average expressed relative to one logical core. |
| Idle RSS and CPU, 1, 2, 4 outputs | not measured. The isolated profile stopped at onboarding and had no configured output. | Use an activated benchmark profile. Repeat the five-sample protocol for each output count, with a static 1080p slide at 60 Hz per output. Record physical display, NDI and DeckLink counts separately. |
| Document deck page turn | not measured. A packaged operator session and reference deck were unavailable. | Open a fixed 1080p PDF deck with at least 20 pages. Advance from page 1 through page 20, with a one-second pause between inputs. Compare input timestamps to the first output frame showing the new page number. Report median, p95, and stalled or missing frames. Record cold import separately. |
| Slide push to NDI frame | not measured. No configured NDI receiver or DeckLink qualification setup was available. | Use 1080p60 NDI and a receiver on the same wired LAN. Push 30 slides with unique frame markers one second apart. Record command and first matching received frame on a common clock. Repeat with CPU capture forced by `KAIRO_GPU_CAPTURE=0` and with default capture. Record loss, latency and receiver configuration. |
| Lyrics library open, 1,000 and 5,000 songs | not measured. No activated test workspace or fixtures at either size. | Seed an isolated workspace with synthetic song files and let indexing finish. For each size, restart the app, open Lyrics, and time input to the first rendered and selectable row. Repeat five times per size and report median and range. Record first index build separately. |
| Scripture search | Warm SQLite FTS phase measured below. Operator UI search latency not measured. | Run `tools/baseline/scripture-search.py` on the packaged `bible.db`. For end-to-end timing, type the same three queries in the KJV scripture workspace and measure input to the first completed result paint. Keep remote translation results separate. |
| Verse grid scrolling | Synthetic renderer sample: **8.3 ms median**, **10.2 ms p95**, and **2 of 193** intervals over 17 ms. Packaged foreground scrolling not measured. | Run `tools/baseline/verse-grid.py` on the snapshot. It uses the 54-item, 360-card fixture at 1600 × 900 and scrolls 2,000 pixels over two seconds. It records animation frame deltas in a hidden Electron window with background throttling disabled. |
| Media dock scrolling | not measured. A reference library and activated operator session were unavailable. | Use an isolated library with 1,000 generated 1280 × 720 images. Wait for thumbnails, scroll 2,000 pixels over two seconds, and report the same frame statistics as the verse grid. Record cold-thumbnail results separately. |
| 1080p video background CPU | not measured. No configured output and reference video session. | Loop a fixed H.264 1080p30 asset on one 1080p60 output. After 15 seconds, sample the full process tree once per second for 60 seconds. Record codec, hardware decode status, dropped frames, CPU median and p95. |
| Installer and installed bundle | DMG **116,643,433 bytes**. Unpacked app bundle **301,828 KiB** allocated (294.75 MiB). | Use `stat -f %z` for the 1.0.27 arm64 DMG and `du -sk` for the matching `Kairo.app`. Application support data is outside the bundle. |
| Speech to screen | not measured. No configured speech input and active output. | Replay fixed audio and pair `scripture.detected`, request, completion and confirmation events by correlation ID. Use the trace's wall-clock phase timestamps. For audio-to-photon timing, record the audio cue and first matching display frame on one clock. Report cancelled, superseded, failed and unconfirmed results separately. |

### Startup samples

| Trial | Startup, ms | Median idle RSS, KiB | Median idle CPU |
|---|---:|---:|---:|
| 1 | 7,338 | 419,280 | 13.7% |
| 2 | 3,609 | 394,192 | 11.3% |
| 3 | 3,661 | 411,264 | 9.4% |

Each profile contained only isolated workspace and telemetry settings before
launch. The probe recorded four to five processes per sample. Other local work,
including Electron integration setup, was present on the machine.

### Warm KJV FTS samples

Read the same full-text SQL shape as `bible-db.ts` against the release bundle's
database in read-only mode. One warmup and 30 timed executions per normalized
query include reading the result rows. The app also sanitizes input, ranks and
deduplicates results, and can fall back to LIKE. This sample covers the FTS query.

| Query | Rows | Median | Range |
|---|---:|---:|---:|
| lord shepherd | 10 | 0.834 ms | 0.779–0.948 ms |
| love patient | 1 | 0.082 ms | 0.080–0.112 ms |
| beginning god created | 3 | 0.545 ms | 0.524–1.135 ms |

### Failure and empty cases

- Startup exits or an absent interactive marker cause a failed trial. A timed
  out startup contributes no latency. Process sampling also fails if the main
  process disappears. Use an empty profile and isolated workspace for every
  launch to avoid single-instance forwarding to an existing session.
- Scripture traces can lack a transcript timestamp, correlation ID or a
  completed phase. Traces without a transcript start at recognition. Ignore
  absent phases when deriving timing and keep unconfirmed presenter requests
  separate from presented verses. Count unresolved references, missing verses,
  unavailable translations, presenter disconnects, timeouts, rejections and
  unknown failures separately from cancellation and supersession.
- Empty FTS queries return no rows. A missing database or a fixture returning
  zero rows makes the timing script fail. The app can continue through the
  LIKE fallback on empty or failed FTS results.
- Benchmark and integration process timeouts are recorded as failures. Missing
  renderer CSS, missing native modules or assets, and esbuild errors must be
  recorded before comparing runtime performance.
- Output tests must count rejected conversions, full-ring frame drops, torn
  or stale frames, receiver failures and device disconnects. GPU and CPU paths
  are separate runs because a surface can switch to CPU for the rest of its
  session after a GPU error.

### Source test method

Copy tracked files at their current working-tree contents, the explicitly
included untracked test, built assets, and dependencies into a disposable
writable directory. Preserve relative symlinks and copy root and server
`node_modules` directories. Run `python3 tools/baseline/suites.py SNAPSHOT LOGS`
from the native workspace. The suite runner records each subprocess exit and
test-runner totals where available. For standalone integration scripts,
one invocation counts as one pass or failure. Keep raw logs outside Git.
The initial sandbox run denied local IPC, Metal and shared memory access.
The results below use the retry with local IPC and GUI access. The shipping
checkout was read-only throughout.

| Suite | Pass | Fail | Skip | Notes |
|---|---:|---:|---:|---|
| `bun run test` | 1,040 | 0 | 0 | Retry after including tracked website and server sources. The incomplete copy initially reported 1,030 pass and 5 fail. |
| `bun run test:main` | 653 | 0 | 0 | Retry after adding the checkout's `.gitignore`. The incomplete copy initially reported 652 pass and 1 fail. |
| `bun run test:scripture-replay` | 90 | 0 | 0 | Local replay and detection wiring fixtures. |
| `bun run test:gpu-frames` | 7 | 0 | 0 | Metal and shared memory available on retry. |
| `bun run test:syphon` | 6 | 0 | 0 | Local addon. |
| `bun run test:recorder` | 9 | 0 | 0 | Local addon. |
| `bun run test:decklink` | 13 | 0 | 1 | SDK compatibility test skipped. |
| `bun run test:audience-local` | 1 | 0 | 0 | Local Worker and account-server fixture. |
| `bun run test:audience-room` | 0 | 1 | 0 | The connection-count assertion reported 0 where it expected 4. |
| `bun run test:audience-slido` | 0 | 0 | 1 | Needs a confirmed live event fixture. |
| `audio-worklet.integration.cjs` | 1 | 0 | 0 | |
| `capture.integration.cjs` | 1 | 0 | 0 | Existing recorder build in the snapshot. |
| `document-video-playback.integration.cjs` | 0 | 1 | 0 | Test fixture omits three converter exports. After the esbuild error, cleanup hit ENOTEMPTY and the harness timed out. |
| `documents.integration.cjs` | 0 | 1 | 0 | Documents did not mount. |
| `live-ui-rendering.integration.cjs` | 1 | 0 | 0 | |
| `pinned-scroll.integration.cjs` | 1 | 0 | 0 | |
| `pptx-rendering.integration.cjs` | 0 | 0 | 1 | Optional test needs a specific local 13-slide PPTX. The no-argument probe exited 1 because it could not open a deck. |
| `screen-configuration.integration.cjs` | 1 | 0 | 0 | |
| `scripture-lazy-cards.integration.cjs` | 1 | 0 | 0 | |
| `theme-picker.integration.cjs` | 1 | 0 | 0 | |
| `theme-shape-editing.integration.cjs` | 0 | 1 | 0 | Expected UI target was absent when the test called `dispatchEvent`. |
| `theme-text-size.integration.cjs` | 1 | 0 | 0 | |
| `video-driver-notice.integration.cjs` | 1 | 0 | 0 | |
| `scripture-grid.bench.cjs` | 0 | 1 | 0 | Exit 0 with zero mounted cards in every case. The harness omitted the current selection props. |

| Adapted verse-grid benchmark | 1 | 0 | 0 | All cases mounted cards. The 360-card cases took 23.8–27.9 ms median to mount across four theme combinations. |

The adapter generates a temporary copy of the benchmark with the current
selection props, rejects zero mounted cards, and measures the first synthetic
360-card scroll. Renderer callback cadence does not establish compositor frame
delivery.

### Native verification

`cargo fmt --all -- --check`, `cargo clippy --offline --locked --workspace --all-targets -- -D warnings`,
and `cargo test --offline --locked --workspace` passed. The shipping checkout's
Git status matched the initial snapshot. Prompt 001 introduced no application API.
