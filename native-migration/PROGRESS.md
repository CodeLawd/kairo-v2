# Migration progress tracker

**Go / no-go:** GO (owner, 2026-11-09; prompts 006 and 007 waived, see DECISIONS.md)
**Planning baseline commit (kairo-presenter):** `de3bb54`
**Current prompt:** 013

Status values: `todo`, `in-progress`, `blocked`, `done`.

Rules:

- One agent owns a row at a time. Set `in-progress` before changing anything.
- `done` means every gate passed. If something could not be verified, say so in Notes.
- `blocked` needs a reason in Notes, what was tried, and what would unblock it.
- Notes hold measured numbers and anything the next session must know.

## Session log

Append a line per session: date, prompt, outcome, next step.

| Date | Prompt | Outcome | Next step |
|---|---|---|---|
| 2026-11-09 | 000 | Local gates passed, CI unverified | Push the native workflow and verify all three jobs |

| 2026-11-09 | 000 | Hosted CI passed on all three platforms | Bootstrap complete |
| 2026-11-09 | 001 | Baseline recorded with unmeasured cases and test failures | Use the same fixtures in prompt 107 |
| 2026-11-09 | 002 | Spike report written; macOS single-display results pass, hardware checks open | Second display checked afterwards; see the 002 row |
| 2026-11-09 | 005 | Done on macOS arm64; hardware and other platforms recorded as unverified | Test on an NDI receiver, a DeckLink card and other platforms before release |

| 2026-11-09 | 004 | in-progress | Prototype FFmpeg decode and wgpu upload |

| 2026-11-09 | 003 | Parley recommended; 99.75% exact breaks; raster p95 misses 16 ms | Decide the raster approach at prompt 008 |
| 2026-11-09 | 007 | Spike report written; macOS interactions pass, performance and platforms open | Rerun off Low Power Mode, then test other platforms |
| 2026-11-09 | 006 | Spike report written; local proofs pass, Slido sign-in and header decision open | Sign in to Slido or waive at prompt 008 |
| 2026-11-09 | 008 | GO recorded by the owner | Run 009, then phase 1 from 010 |
| 2026-11-09 | 009 | Sync tool built; first queue written | Review the open overlay.html row, then run 010 |
| 2026-11-09 | 010 | API inventory generated and checked | Run 011 |
| 2026-11-09 | 011 | Oracle harness, cache and overlay-boxes demo done | Run 012 |
| 2026-11-09 | 012 | Domain types, fixtures and oracle comparisons done | Run 013 |

## Prompts

### 0. Foundations and gating spikes

| ID | Prompt | Status | Date | Branch | Notes |
|---|---|---|---|---|---|
| 000 | [Create the native workspace and agent tooling](./prompts/000-workspace-bootstrap.md) | done | 2026-11-09 | main | All 40 members resolved after removal of the dedicated web host. Local fmt, build, Clippy and test gates passed on macOS. Clean workspace build 153.02 s including downloads, unchanged app rebuild 1.02 s. Cargo launch and [window capture](./evidence/000/hello-world.png) verified using the built executable in a temporary app bundle for capture. Lockfile committed as `be7133a`. Electron status matched the initial snapshot. [Hosted CI](https://github.com/CodeLawd/kairo-v2/actions/runs/42571222400) passed on macOS, Windows and Ubuntu. |
| 001 | [Measure the Electron baseline](./prompts/001-electron-baseline.md) | done | 2026-11-09 | main | [Baseline](./baseline.md): packaged startup median 3,661 ms (3,609–7,338), idle RSS 411,264 KiB (394,192–419,280), idle CPU 11.3% (9.4–13.7%) at onboarding with zero outputs. Measured bundle size, warm KJV SQL and synthetic verse scroll. Packaged operator and hardware metrics remain unverified. Source suite failures and skips are recorded. Native fmt, Clippy and tests passed. Electron Git status matched the initial snapshot. |
| 002 | [Spike: separate engine process with output windows and preview](./prompts/002-spike-engine-process.md) | done | 2026-11-09 | main | [Report](./spikes/002-engine-process.md): macOS, including a real second display. Measured: control p99 0.0305 ms, kill-to-first-frame 217.1 ms, parent grace 3.02 s of 3 s, preview +3.95% of a core at 15 fps. 30-minute soak footprint 43.6 to 67.2 MB across overlapping 15 and 30 fps runs; without `drop_image`, 3,031.9 MB in 50 s. Fmt, Clippy and tests passed. Focus, menu-bar cover, cursor hiding and replug confirmed by hand. Finding: output flashes on the remaining screen for up to about 1 s at unplug; prompt 050 must fix it. Windows and Linux not run, no machines available. |
| 003 | [Spike: match Chromium slide text](./prompts/003-spike-text-fidelity.md) | done | 2026-11-09 | main | [Report](./spikes/003-text-fidelity.md): parley + swash recommended. On 400 text cases (480 with empty cases), exact line breaks 399/400 (99.75%), fitted size 200/200 within 1 px. All 480: SSIM mean 0.98627, worst 0.81611, median raster 5.15 ms. **Raster p95 25.28 ms misses the 16 ms target** on a loaded M2 Pro. Every mismatch category explained in the report. Layout JSON and golden PNGs byte-identical across two captures; 3 of 480 working PNGs vary (117 channels at ±1 in the one quantified). Root fmt, Clippy and tests passed; spike fmt, Clippy and tests passed. Electron status unchanged. Not verified: Windows, Linux, idle-machine timing, themed backgrounds, custom fonts. |
| 004 | [Spike: video decode into wgpu](./prompts/004-spike-video.md) | done | 2026-11-09 | main | macOS codec matrix and native gates passed. 30-minute soak completed 54,000 frames, 43.6 ms p99, 10.66% process CPU. Sampled RSS after warm-up 26,704–44,288 KiB. The 2× trimmed wrap hit 86.0 ms. Windows, Linux and native external texture imports remain unverified. |
| 005 | [Spike: NDI, DeckLink ring and macOS native code from Rust](./prompts/005-spike-sinks.md) | done | 2026-11-09 | main | [Report](./spikes/005-sinks.md): macOS arm64 only. NDI 6.3.2 from Rust without Node passed 10 cycles, discovery and SDK loopback; `libloading` chosen over `grafton-ndi`. Ring: 600 of 600 frames, 0 torn. Golden colours match `uyvy.h` on both paths. `gpu_frames` runs through a C ABI. IOSurface 1080p UYVA p50 0.60 to 1.04 ms, p99 2.58 to 4.81 ms, under load 3.4 to 4.7; decision 008: 0.6, 1.5 to 2.2. Fmt, Clippy and tests passed. HARDWARE-UNVERIFIED: DeckLink card, third-party NDI receiver. Not run: Intel macOS, Windows, Linux. |
| 006 | [Spike: CEF web host for all browser needs](./prompts/006-spike-web-host.md) | blocked | 2026-11-09 | main | [Report](./spikes/006-web-host.md): PARTIAL, on-demand component recommended. macOS arm64: web layer and preview PASS, PDF PASS, Slido PASS against a local fixture only. 20-minute soak 28.71 fps average, paint-to-upload p99 5.46 ms, all-process RSS idle 456–558 MB and peak 816 MB, exit 0 with no helpers left. Size 329 MB, 148 MB zipped. CPU paint streamed; shared textures fire but are not sent across processes. H.264 and HEVC unavailable in this CEF build. Root and spike fmt, Clippy and tests passed; Electron status unchanged. Blocked on a person for real Slido sign-in, and a decision that subframe header removal is unnecessary. Windows and Linux not run. |
| 007 | [Spike: operator UI on GPUI Kit](./prompts/007-spike-ui-platform.md) | blocked | 2026-11-09 | main | [Report](./spikes/007-ui-platform.md): PARTIAL. macOS shell, 5,000-card grid, marquee, reorder, zoom, keyboard and `.kairo` open work; 14 tests and all gates pass. Draw p50 2.01 to 2.78 ms, max 10.42 ms. Low Power Mode capped frames at 33 ms; uncapped present p50 8.33 ms, but 416 of 1,151 frame callbacks over 16 ms, so 120 Hz is not proven. On AC power, one valid run at load 13: present p50 8.40 ms, 31 of 1,382 presents over 16 ms, draw max 9.49 ms; target not met under load. Minimum macOS declared 11.0, runtime unverified. Rich text: Kit does not cover it. Blocked on: idle-machine rerun, real IME, older macOS, Windows, Linux, Electron shell screenshot. |
| 008 | [Human checkpoint: go or no-go](./prompts/008-go-no-go.md) | done | 2026-11-09 | native-phase-0-spikes | GO by the owner; nine decisions in DECISIONS.md, [summary](./spikes/008-summary.md). 006 and 007 waived. Root fmt, Clippy and tests passed. Not verified: Windows, Linux, macOS 11, Intel, real NDI and DeckLink hardware. Whole-app gain over the baseline is unproven until 107. |
| 009 | [Electron-to-native sync process](./prompts/009-electron-sync.md) | done | 2026-11-09 | native-phase-0-spikes | `tools/electron-sync` built. Real check took about 0.1 s against the 10 s target, and queued 6 files from Electron `af978c5`; copied native sources in sync. Scratch-repo test covers each class, an uncommitted edit, drift, idempotent writes and refusal to advance. Root fmt, Clippy and tests passed; Electron not edited. Not verified: `--apply-generated` running a real generator (none exists until 010, 011, 016, 071). Stored-format detection is by path only. |

### 1. Domain model

| ID | Prompt | Status | Date | Branch | Notes |
|---|---|---|---|---|---|
| 010 | [Freeze the application API inventory](./prompts/010-api-inventory.md) | done | 2026-11-09 | native-phase-0-spikes | [Inventory](./api-inventory.md): 257 calls generated from the TypeScript AST, all with owning prompts; `check-api-coverage` passes and two runs are byte-identical. Reconciled: 209 `ipcMain.handle` registrations in `src/main/ipc/index.ts` (120 direct, 89 through its `handle()` wrapper) against the audit's text count of 122; none elsewhere; 8 `on` or `once`. Review lists: 2 pushes with no detected sender, 23 exposed methods with no detected caller. Root fmt, Clippy and tests passed; Electron not edited. Not verified: the owner mapping is by namespace with method overrides, so some rows may move when the service prompts run; the 23 unused methods need a human decision before porting. |
| 011 | [Differential test harness against the TypeScript originals](./prompts/011-ts-oracle.md) | done | 2026-11-09 | native-phase-0-spikes | `tools/ts-oracle/runner.mjs` (Node + Electron `tsx`), `crates/kairo-oracle` (dev-only: `assert_matches_oracle`, `assert_all_match`, seeded proptest `generate`, cache/live/refresh modes) and `cargo run -p ts-oracle -- --regenerate`; see [README](../tools/ts-oracle/README.md) and the comparison rules in DECISIONS.md. Demo: 12,000 generated inputs across six `overlay-boxes.ts` exports all match, including NaN propagation and the omitted default minimum; a deliberately wrong port fails. Cache: 6 files, 1.1 MB, stamped with Electron `af978c5159`, clean `src/lib`; two regenerations byte-identical. `cargo test --workspace` passes in cache mode with Node removed from PATH. Measured throughput (release, macOS arm64, Node 24.21.0): 185,282 to 223,181 calls/s over 3 batches of 50,000; Node start and module load 97 ms. Edge cases covered by `tests/runner.rs`: undefined return, field and array element; null; NaN and infinities; -0; async resolve and reject; throw of Error and non-Error; console output kept off the protocol; tagged Date, Map, Set, bytes both ways; omitted and undefined default arguments; missing module, missing or non-function export, path escape, malformed name, non-array args, bigint, circular value; a failed batch leaving no stale replies; empty batch; closed pipe at exit; missing Electron checkout; cache miss and corrupt cache. Gates: fmt, Clippy `-D warnings` and `cargo test --workspace` passed; live mode also passed. Electron not edited. Not verified: hosted CI (branch not pushed); Windows and Linux; `runner.rs` tests skip in cache mode, so CI does not exercise the runner; `applyLayoutPreset` and `overlayBoxStyle` not compared (the latter needs JavaScript number formatting, for prompt 013). No API inventory rows are owned by 011. |
| 012 | [Port the data types with on-disk compatibility](./prompts/012-domain-types.md) | done | 2026-11-09 | native-phase-0-spikes | `kairo-domain`: 24 modules, about 5,900 lines, porting every data type in the listed sources plus `displays`, `keyboard-shortcuts`, `scripture-trace` and `lyrics-duplicate`; mapping rules in DECISIONS.md. `AppSettings` is canonical; `StoredSettings` keeps each of 18 sections typed or raw, so old files load losslessly; other store files load as `Stored<T>`. Unknown keys survive at every level of persisted objects. Fixtures: 24 of the owner's real store files (current and backup profiles, including `proautomate-settings.json`), sanitized by `tools/ts-oracle/sanitize-fixture.mjs` (only TypeScript literals and colours kept; checked: no other original string remains); 3 hand-built old or hostile settings files; fresh-install settings and 200 generated song files from the oracle. Every fixture round trips to identical JSON values and number text. Every current-profile file is canonical, including the 598 KB services file; 3 backup files stay raw where expected. Oracle: `db/index.ts` defaults match section by section (overlay section round-tripped only, as prompt 013 owns its default); 18 exported defaults and 11 enum lists match exactly. The oracle caught two wrong values (`idleUpload().retryable`, tagline spacing), both fixed. Cache regenerated twice, byte-identical, stamped `af978c5159`. Measured (release, M2 Pro on battery, 21 runs): 5.01 MB settings parse median 19.0–19.2 ms as `StoredSettings` and 12.6–14.0 ms as `AppSettings`, under the 20 ms target with about 5% margin; the cost is serde `flatten` on every persisted struct. Gates: fmt, Clippy `-D warnings` and `cargo test --workspace` passed, with Node removed from PATH (cache mode). Electron not edited. Not verified: hosted CI (not pushed); Windows and Linux; a slower machine may miss 20 ms, in which case replace derived `flatten` with generated visitors; `server/src` DTOs were not compared field by field beyond the shared `cloud/contracts.ts`; types reached only by `.kairo` bundles, the media manifest file and SQLite rows are typed but have no real-file fixture. No API inventory rows are owned by 012. |
| 013 | [Port settings and theme normalizers](./prompts/013-domain-normalizers.md) | todo | | | |
| 014 | [Port lyrics logic](./prompts/014-domain-lyrics.md) | todo | | | |
| 015 | [Port scripture logic](./prompts/015-domain-scripture.md) | todo | | | |
| 016 | [Port program, media, transfer and remaining shared logic](./prompts/016-domain-program.md) | todo | | | |

### 2. Persistence and runtime

| ID | Prompt | Status | Date | Branch | Notes |
|---|---|---|---|---|---|
| 020 | [Paths, logging, crash handling and single instance](./prompts/020-paths-logging.md) | todo | | | |
| 021 | [Settings store and migrations](./prompts/021-settings-store.md) | todo | | | |
| 022 | [Secure storage and Electron secret migration](./prompts/022-secure-storage.md) | todo | | | |
| 023 | [SQLite access layer](./prompts/023-sqlite-layer.md) | todo | | | |
| 024 | [Workspace folders and song files](./prompts/024-workspace.md) | todo | | | |
| 025 | [AppCore, actors and event topics](./prompts/025-core-runtime.md) | todo | | | |

### 3. Services

| ID | Prompt | Status | Date | Branch | Notes |
|---|---|---|---|---|---|
| 030 | [Cloud API client and session](./prompts/030-cloud-client.md) | todo | | | |
| 031 | [Licence and onboarding](./prompts/031-cloud-licensing.md) | todo | | | |
| 032 | [Usage, crash reports, feedback and sermon records](./prompts/032-telemetry-services.md) | todo | | | |
| 033 | [Bible lookup and search](./prompts/033-scripture-service.md) | todo | | | |
| 034 | [API.Bible, offline policy and download manager](./prompts/034-api-bible.md) | todo | | | |
| 035 | [Scripture detector, deterministic path](./prompts/035-detector-rules.md) | todo | | | |
| 036 | [Scripture detector, LLM path and fallback](./prompts/036-detector-llm.md) | todo | | | |
| 037 | [Global quote search worker](./prompts/037-quote-index.md) | todo | | | |
| 038 | [Audio capture and Deepgram transcription](./prompts/038-stt.md) | todo | | | |
| 039 | [Orchestrator, voice navigation and recovery](./prompts/039-orchestrator.md) | todo | | | |
| 040 | [Lyrics library and search](./prompts/040-lyrics-service.md) | todo | | | |
| 041 | [Online lyric search and translation](./prompts/041-lyrics-online.md) | todo | | | |
| 042 | [Sermon notes import](./prompts/042-sermon-import.md) | todo | | | |
| 043 | [Media and tracks libraries](./prompts/043-media-service.md) | todo | | | |
| 044 | [Setlist, libraries, passages, usage and transfer](./prompts/044-transfer-libraries.md) | todo | | | |
| 045 | [Document import pipeline](./prompts/045-documents-service.md) | todo | | | |
| 046 | [Audience rooms and Engage](./prompts/046-audience-rooms.md) | todo | | | |
| 047 | [Slido source](./prompts/047-slido.md) | todo | | | |
| 048 | [Auto-update](./prompts/048-updater.md) | todo | | | |

### 4. Output engine

| ID | Prompt | Status | Date | Branch | Notes |
|---|---|---|---|---|---|
| 050 | [Engine process, protocol and supervision](./prompts/050-engine-scaffold.md) | todo | | | |
| 051 | [Compositor core and layer stack](./prompts/051-compositor-core.md) | todo | | | |
| 052 | [Scene model and slide text](./prompts/052-scene-text.md) | todo | | | |
| 053 | [Backgrounds, elements, shapes and filters](./prompts/053-scene-visuals.md) | todo | | | |
| 054 | [Messages, props, timers, polls, questions, info, logo](./prompts/054-scene-program-layers.md) | todo | | | |
| 055 | [Video playback layer](./prompts/055-video-engine.md) | todo | | | |
| 056 | [Program audio, routing and tracks](./prompts/056-audio-engine.md) | todo | | | |
| 057 | [Live camera layer](./prompts/057-camera-layer.md) | todo | | | |
| 058 | [Document pages and embedded video](./prompts/058-document-layer.md) | todo | | | |
| 059 | [Live web layer](./prompts/059-web-layer.md) | todo | | | |
| 060 | [Display outputs, stage windows and test patterns](./prompts/060-display-sinks.md) | todo | | | |
| 061 | [NDI output](./prompts/061-ndi-sink.md) | todo | | | |
| 062 | [DeckLink output](./prompts/062-decklink-sink.md) | todo | | | |
| 063 | [Syphon output](./prompts/063-syphon-sink.md) | todo | | | |
| 064 | [Capture to file](./prompts/064-recorder-sink.md) | todo | | | |
| 065 | [Reconcile, routing, lobby playlists and program service](./prompts/065-surface-manager.md) | todo | | | |
| 066 | [Operator preview transport](./prompts/066-preview-transport.md) | todo | | | |
| 067 | [Application wiring formerly in the IPC layer and main entry](./prompts/067-application-wiring.md) | todo | | | |

### 5. Operator interface

| ID | Prompt | Status | Date | Branch | Notes |
|---|---|---|---|---|---|
| 070 | [Design system](./prompts/070-design-system.md) | todo | | | |
| 071 | [Application shell, menus and quit flow](./prompts/071-app-shell.md) | todo | | | |
| 072 | [Launch screen](./prompts/072-bootstrap-ui.md) | todo | | | |
| 073 | [Account, onboarding, coach tour](./prompts/073-account-onboarding.md) | todo | | | |
| 074 | [Settings](./prompts/074-settings-ui.md) | todo | | | |
| 075 | [Screens configuration](./prompts/075-screens-ui.md) | todo | | | |
| 076 | [Operator workspace, live rail and monitor](./prompts/076-operator-layout.md) | todo | | | |
| 077 | [Operator queue, suggestions and program panels](./prompts/077-operator-controls.md) | todo | | | |
| 078 | [Scripture workspace](./prompts/078-scripture-ui.md) | todo | | | |
| 079 | [Sermon notes review](./prompts/079-sermon-review.md) | todo | | | |
| 080 | [Lyrics library and quick open](./prompts/080-lyrics-library-ui.md) | todo | | | |
| 081 | [Lyrics slide grid](./prompts/081-lyrics-slides-ui.md) | todo | | | |
| 082 | [Lyrics editing and import](./prompts/082-lyrics-editors-ui.md) | todo | | | |
| 083 | [Media dock and inspector](./prompts/083-media-ui.md) | todo | | | |
| 084 | [Tracks](./prompts/084-tracks-ui.md) | todo | | | |
| 085 | [Documents](./prompts/085-documents-ui.md) | todo | | | |
| 086 | [Theme editor canvas](./prompts/086-theme-canvas-ui.md) | todo | | | |
| 087 | [Theme editor panels](./prompts/087-theme-panels-ui.md) | todo | | | |
| 088 | [Props editor](./prompts/088-props-ui.md) | todo | | | |
| 089 | [Web workspace](./prompts/089-web-ui.md) | todo | | | |
| 090 | [Audience workspace](./prompts/090-audience-ui.md) | todo | | | |
| 091 | [Transcription, transfer, capture popover and leftovers](./prompts/091-remaining-ui.md) | todo | | | |

### 6. Hardening and cutover

| ID | Prompt | Status | Date | Branch | Notes |
|---|---|---|---|---|---|
| 100 | [First-run migration and rollback](./prompts/100-data-migration.md) | todo | | | |
| 101 | [macOS packaging](./prompts/101-package-macos.md) | todo | | | |
| 102 | [Windows packaging](./prompts/102-package-windows.md) | todo | | | |
| 103 | [Linux packaging](./prompts/103-package-linux.md) | todo | | | |
| 104 | [Release pipeline](./prompts/104-release-pipeline.md) | todo | | | |
| 105 | [Bridge from Electron to native](./prompts/105-bridge-release.md) | todo | | | |
| 106 | [Accessibility and keyboard pass](./prompts/106-accessibility-keyboard.md) | todo | | | |
| 107 | [Performance validation and tuning](./prompts/107-performance-hardening.md) | todo | | | |
| 108 | [Soak and failure testing](./prompts/108-soak-chaos.md) | todo | | | |
| 109 | [Full parity audit](./prompts/109-parity-audit.md) | todo | | | |
| 110 | [Hardware qualification](./prompts/110-hardware-qualification.md) | todo | | | |
| 111 | [Pilot with real services](./prompts/111-pilot.md) | todo | | | |
| 112 | [General release and Electron retirement](./prompts/112-cutover.md) | todo | | | |
