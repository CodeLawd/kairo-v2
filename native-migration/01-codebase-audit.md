# 01 — Codebase and architecture audit

Audited at branch `ci/release-packaging-retries`, commit `de3bb54`, app version 1.0.33.
Every statement here comes from the repository. Where behaviour was not read line by
line, the owning prompt tells the agent to extract it from source before porting.

## 1. Size and shape

| Area | Files | Lines | Migration treatment |
|---|---:|---:|---|
| `src/main/` (excluding tests) | 135 | 31,725 | Port to Rust services |
| `src/main/**/__tests__/` | 57 | 8,145 | Port as Rust tests, or use as a differential oracle |
| `src/lib/` (pure shared logic) | 85 | 14,094 | Port to `kairo-domain`, verified against the TypeScript originals |
| `src/preload/` | 3 | 891 | Disappears; the typed `AppCore` API replaces it |
| `src/renderer/` | 195 | 47,686 | Rebuild in GPUI Kit |
| `native/` (Metal, Syphon, recorder, DeckLink) | 33 | 3,911 | Reuse; replace the N-API shims with a C ABI |
| `tests/` | 144 | 14,645 | Port or adapt; several `.integration.cjs` drive Electron directly |
| `scripts/` | 17 | 3,080 | Keep the Bible tools; rewrite the release tools |
| `server/`, `website/` | — | — | **Unchanged.** The desktop app remains a client of them |

## 2. Runtime today

| Process or thread | Created by | Job |
|---|---|---|
| Electron main | `src/main/index.ts` | Every service, the single JS event loop, frame hand-off |
| Main renderer | `createWindow` | React operator UI, **and microphone capture and PDF rasterisation** |
| Display surface renderers | `display-window.ts` | One visible `BrowserWindow` per physical screen output |
| Offscreen surface renderers | `program-surface.ts` | One hidden window per NDI, Syphon or DeckLink output |
| Stage renderers | `stage-window.ts` | Confidence monitors using `stage.html` |
| Slido renderer | `audience/sources/slido.ts` | Hidden scraper window, visible for moderator sign-in |
| Temporary PPTX window | `documents/pptx-builtin.ts` | Lays out generated HTML and calls `printToPDF` |
| Identify and test-pattern windows | `surface-manager.ts` | Short-lived display overlays |
| Worker thread: quote search | `scripture/quote-worker.ts` | FTS and fuzzy verse-quote matching |
| Worker thread: DeckLink pump | `decklink/playback-worker.ts` | Moves frames to the helper |
| `decklink-helper` (C++) | `playback-worker.ts` | Owns SDK scheduling, separate for crash isolation |
| GPU, utility, network | Chromium | Implicit; reported through `child-process-gone` |

### 2.1 Contract surface

- 122 `ipcMain.handle` calls and 6 `ipcMain.on` calls in `src/main/ipc/index.ts`, plus
  lifecycle handlers in `src/main/index.ts` and `audience:slido:read`.
- About 218 `ipcRenderer` call sites in `src/preload/index.ts`, grouped into 29 API
  namespaces: `audience, transfer, passages, libraries, setlist, services, documents, app,
  audio, scripture, transcription, workspace, lyrics, settings, orchestrator, resilience,
  ndi, capture, displays, videoDevices, output, program, songUsage, media, tracks,
  onboarding, account, feedback, updates`.
- The channel registry is `IPC` in `src/lib/ipc.ts:1722` and `PROGRAM` in
  `src/lib/program.ts:1140`. Prompt 010 turns these into a checked API inventory.

## 3. Critical flows

### 3.1 Sermon to verse on screen

```
Mic → getUserMedia + AudioWorklet (renderer, 16 kHz PCM16)
    → audio:pcmChunk IPC → AudioService relay (5 s cap, drops newest)
    → DeepgramSTTService (nova-3, keyterms, 30 s replay on reconnect)
    → STT final/interim → TranscriptionBuffer
    → ScriptureDetector (regex fast path, LLM, global quote worker, quote recovery)
    → Orchestrator → suggestion or timed auto-present (default 1 s delay)
    → dispatchToOutputs → ProgramSurface.showSlide per output
    → renderOverlayHTML → executeJavaScript(__setContent) → paint → sink
```

Latency tracing already exists (`scripture/trace.ts`, `scripture-trace.ts`,
`ScriptureLatencyPanel.tsx`) and must survive, because it is the regression instrument.

### 3.2 Program output

`SurfaceManager.reconcile` is the only place outputs open or close. It runs on launch,
after each overlay or presentation settings write, and on display or device changes.
It is serialized, debounced by 250 ms for display events, and keeps state through window
rebuilds.

`ProgramSurface` holds `lastRender`, `layers`, `documentSlide` and `config` so that a
crashed or replugged surface repaints the same picture. Failures count against a retry
budget and planned rebuilds never do. Page turns burst capture to 60 fps and wait for the
**actual frame** (`nextPaint`, 400 ms cap).

The overlay shell has a fixed layer stack, bottom to top:

```
split backdrop → background → camera → web → web pointer → elements below → text
→ elements above → props → timer → message → poll → question → info bar → logo
```

(A camera in picture-in-picture moves above the content. Layer changes are sent one by one
so an unrelated update never re-fades a logo.)

Its full control API is `window.__configure, __videoEnded, __nextFrame, __setContent,
__markDocument, __patchBackground, __setVideoPaused, __swapDocumentPage,
__preloadDocumentPage, __setDocumentVideos, __documentPlayback, __controlDocumentVideo,
__clearText, __seekVideo, __setPoll, __setQuestion, __setMessage, __setProps,
__setTimerDesign, __setLogo, __setInfo, __setWebPointer, __setWeb, __setCamera`.

### 3.3 Frame delivery

| Sink | Fast path | Fallback |
|---|---|---|
| Display | Chromium compositor presents directly | None needed |
| NDI | IOSurface → Metal UYVA (`gpu-frames`) → `grandi` send | BGRA bitmap |
| Syphon | IOSurface publish | BGRA publish |
| DeckLink | IOSurface → Metal UYVY → POSIX shm ring → helper | BGRA over stdin |
| Recorder | IOSurface blit → AVAssetWriter | `pushPixels`, frame subscription on displays |

Decisions already paid for (`docs/decisions/001`, `007`, `008`) and carried forward:

- UYVA for NDI, because feeds are keyed overlays.
- Metal conversion that matches `uyvy.h` byte for byte.
- Textures are dropped rather than queued.
- The native hardware clock owns frame repetition.
- The DeckLink helper stays out of process because a native crash cannot be caught.

The DeckLink EULA reading and NDI redistribution checklist also still apply.

### 3.4 Startup

`main/index.ts` initialises settings, registers `pa-media`, ensures the workspace, opens
lyrics and scripture stores, runs migrations, registers IPC, starts NDI, starts the
updater, creates the window, then reconciles surfaces. The renderer runs `app:bootstrap`:
nine resources load concurrently with an 8 second ceiling, and failures yield neutral
values (`src/main/bootstrap.ts`).

### 3.5 Shutdown

Quit is guarded for every path: red button, Cmd+W, Cmd+Q and updater. An active recording
is finished first. Hardware release has a 2 second timeout. Closing the main window closes
screens on macOS so a controller-less program never stays on the wall.

## 4. Persistence

| Data | Location | Format | Native plan |
|---|---|---|---|
| Settings | userData `kairo-settings.json` | electron-store JSON, renamed from `proautomate-*` | Same file and schema |
| Other JSON stores | userData `kairo-onboarding`, `kairo-sermon-plans`, `kairo-services`, `kairo-setlist`, `kairo-libraries`, `kairo-passages`, `kairo-usage`, `kairo-crash-reports`, `song-usage` | electron-store JSON | Same files |
| View preferences | Chromium `Local Storage` | Panel widths, zoom, recent lookups, legacy nuggets, engagement cache | New `ui-state.json`, imported once (prompt 100) |
| Writable Bible copy | userData `bible.db`, copied once from `resources/bible.db`; packs install into it | SQLite | Same |
| Documents | userData `documents/<id>/document.json` and pages | Files | Same |
| Slide converter | userData `slide-converter/` | Downloaded LibreOffice | Same |
| Device identity | userData `cloud/device.json` | JSON | Same |
| One-shot migrations | userData `kairo-migrations.json` | JSON booleans | Same file |
| Songs (source of truth) | `<workspace>/Songs/<slug>__<id>.song.json` | `src/lib/workspace.ts` | Unchanged |
| Song index | userData `lyrics.db` | SQLite WAL + FTS5 | Rebuildable cache, same schema |
| Media | `<workspace>/Media`, `<folder>/.kairo/playlists.json` | Files + JSON | Unchanged |
| Bibles | bundled `bible.db` (43 MB), external packs | SQLite + FTS5 | Unchanged |
| API.Bible cache | SQLite, AES-256-GCM values | **Fails closed** when no OS encryption | Same crypto, migrated key |
| Refresh token | `SecureStore` | AES-GCM + `safeStorage` key. **Fails open** | Same semantics, migrated key |
| Session recovery | userData `session-recovery.json` | JSON, saved every 30 s | Same file |
| Logs | electron-log main log | Text | Same folder via `tracing` |
| Crash dumps | Electron `crashReporter`, local only | Minidump | Local minidumps, see prompt 020 |

Migration cost to note: `electron-store` shallow-merges defaults, so `db/index.ts` carries
heal-on-launch logic that must be ported as written. `safeStorage` blobs are encrypted by
Chromium's OS crypt, so moving away from it needs an explicit migration (prompt 022).

## 5. External systems

| System | Used by | Transport |
|---|---|---|
| Kairo API (Nest, Workers fallback host) | `cloud/*` | HTTPS with `X-PA-Client: desktop`, 8 s timeout, automatic failover |
| Audience and Engage rooms (Cloudflare Durable Objects) | `audience/sources/kairo.ts` | WebSocket; protocol in `audience-protocol.ts`, `engage.ts` |
| Managed gateway, `POST /v1/managed/request` | LLM, API.Bible, Brave, translation (`cloud/managed.ts`) | HTTPS through the Kairo server. The desktop holds no provider keys |
| Deepgram | `stt/deepgram.ts` | WebSocket with a short-lived server grant (get, renew, release) |
| API.Bible | `scripture/api-bible-*` | Via the gateway, licence-gated offline policy |
| LRCLIB, Genius, Brave, DuckDuckGo, Mojeek, gospel lyric sites | `lyrics/*` | HTTP plus HTML scraping (`cheerio`) |
| Google Translate | `lyrics/translate.ts` | HTTPS |
| Slido | `sources/slido.ts` | Real browser, selector scraping |
| GitHub Releases | `updater/*`, Bible packs | HTTPS |
| LibreOffice download | `documents/slide-converter.ts` | HTTPS, installed on request |
| PowerPoint, WPS, Keynote, LibreOffice | `documents/converters.ts` | AppleScript or child process |
| NDI, DeckLink, Syphon | output services | Native SDKs |

## 6. Dependencies on Chromium that are easy to miss

These are the items a straight UI port would silently lose.

1. **HTML and CSS are the slide format.** `renderOverlayHTML`, `renderPropHTML`,
   `renderTimerHTML`, `renderMessageHTML`, `renderPollHTML`, `renderQuestionHTML`,
   `renderLogoHTML` and `outputTestPatternHTML` all return HTML.
2. **CSS features in slides:** `-webkit-text-stroke` with `paint-order`, blurred
   `text-shadow`, `filter: hue-rotate/saturate/brightness/contrast/blur`, linear gradients,
   `object-fit`, rotation and mirroring, SVG shapes with `non-scaling-stroke`, marquee
   ticker keyframes, and opacity transitions.
3. **The DOM is the measuring tool.** Auto-fit runs a binary search against real layout
   (`overlay-fit.ts`, duplicated in `overlay.html`).
4. **`<video>` is the media engine:** decode, loop, trim range, rate, seek, pause, cues,
   per-device audio with `setSinkId`, and Web Audio tapping for NDI sound.
5. **`getUserMedia` is the camera and mic layer,** matching devices by label because IDs
   are unstable (`audio/devices.ts`, `overlay.html:1070`).
6. **iframes are the web layer,** with `X-Frame-Options` and `frame-ancestors` stripped,
   scripted mute, and scroll and pointer mirroring (`services/web/index.ts`).
7. **`pdfjs-dist` in the renderer** rasterises each PDF page to 4K.
8. **`printToPDF`** produces built-in PPTX conversion.
9. **Chromium codecs,** including H.264 and AAC, ship with Electron and will need their
   own licensing review once replaced.
10. **Tiptap** is the rich-text editor in `SermonNotesReviewModal.tsx`.
11. **Radix, react-resizable-panels and input-otp** supply focus, menu, split-pane and
    OTP behaviour.
12. **Chromium's `safeStorage`, `powerSaveBlocker`, `crashReporter`,
    `systemPreferences` and `electron-updater`** cover platform duties.

## 7. Renderer inventory

| Workspace | Key files (lines) | Notes |
|---|---|---|
| Shell | `App.tsx` 731, `AppShell.tsx` 567 | Routes: operator, scripture, lyrics, theme, documents, web, audience |
| Operator | `Operator.tsx` 2312, `ProgramPanels.tsx` 1478, `LiveOutputPreview.tsx` 550 | Booth control surface |
| Lyrics | `Lyrics.tsx` 4061, `SongReflowEditor.tsx` 526, `SongQuickOpen.tsx` 495 | Largest single view |
| Scripture | `Scripture.tsx` 1679, `PlaylistSidebar.tsx` 683, `SermonNotesReviewModal.tsx` 473 | |
| Media | `MediaDock.tsx` 1903, `MediaInspector.tsx` 287 | Marquee select, drag reorder |
| Theme | `ThemeEditor.tsx` 1212, `OverlayCanvas.tsx` 479, panels | Direct-manipulation canvas with undo |
| Props | `PropsEditor.tsx` 976 | Second canvas |
| Screens | `ScreenConfiguration.tsx` 1569 | System-Settings-style modal |
| Settings | `Settings.tsx` 1846 | |
| Documents | `Documents.tsx` 923 | Import, render, slideshow |
| Audience | 10 components, about 2,900 lines | Q&A, polls, engagements |
| Transcription | `Transcription.tsx` 1099 | |
| Onboarding and account | about 2,100 lines | Wizard, coach tour, gate |

`src/renderer/src/index.css` (880 lines) holds the dark and light tokens. `DESIGN.md`
and the auto-memory style note (flat, restrained radius, no gratuitous shadows or
translucency, minimal copy) define the visual contract.

## 8. Packaging and release

`package.json#build`, `.github/workflows/release.yml`, `scripts/release.mjs`,
`scripts/after-pack.cjs`, `scripts/verify-package.mjs`, `scripts/prepare-mac-natives.mjs`.
Targets: macOS arm64 and x64 (DMG and zip, hardened runtime), Windows x64 (NSIS), Linux x64
(AppImage and deb). Publishing goes to GitHub Releases with `latest*.yml` feeds. macOS
minimum is 11.0. The `.kairo` file association, single-instance lock, and NDI Bonjour and
local network declarations also ship from here.

## 9. Findings to challenge

| # | Finding | Evidence | Native direction |
|---|---|---|---|
| A1 | HTML strings are the output API. Behaviour lives in strings parsed by another process | `overlay-template.ts`, `program.ts` | Typed `SlideScene` |
| A2 | One Chromium renderer per output. Each NDI or card output costs a full renderer | `surface-manager.ts` | One compositor, N render targets, N sinks |
| A3 | Single event loop carries STT, detector, IPC and frames | `orchestrator.ts`, `program-surface.ts` | Actors plus a dedicated engine process |
| A4 | Frames are capture-driven with hand-tuned rates (10, 24, 30, 60 fps) | `program-surface.ts:26-33` | Render on change, one clock per sink |
| A5 | Live web pages run once per output and mirror scroll by message | `services/web/index.ts` | One page instance, shared frames |
| A6 | Auto-fit exists three times (estimate, shared DOM fit, copy in `overlay.html`) | `overlay-template.ts`, `overlay-fit.ts` | One implementation against real shaping |
| A7 | Microphone capture depends on the UI being alive | `audio/capture.ts`, `App.tsx` | Capture in the core; the UI cannot stall it |
| A8 | Mac GPU capture only. Windows and Linux use CPU paths | `native/gpu-frames/README.md` | The compositor is GPU on every platform |
| A9 | `lyrics.db` and JSON both describe songs | `CLAUDE.md` workspace section | Keep. It is a good design; port as is |
| A10 | `Lyrics.tsx` at 4,061 lines mixes state and view | File size | Split model and view by feature |

Things that should **not** be redesigned: the workspace folder model, `.kairo` bundles,
the reconcile pattern, the detector's replay corpus, the dropped-not-queued frame rule,
and the out-of-process DeckLink helper.

## 10. Logic outside the service layer

`src/main/ipc/index.ts` and `src/main/index.ts` carry product rules: settings side effects,
cross-service clean-up, startup order and all native dialogs. Prompt 067 owns them.

`Transcription.tsx` (1,099 lines) is imported nowhere and appears to be dead. The live
transcript is in `Operator.tsx`.

## 11. Development and build flags

| Flag | Purpose | Prompt |
|---|---|---|
| `MAIN_VITE_API_URL`, `_WEB_URL`, `_AUDIENCE_API_URL`, `_LICENSE_PUBLIC_KEY` | Build-time endpoints and licence key | 020, 031 |
| `KAIRO_SHARED_PROFILE` | Dev data profile | 020 |
| `KAIRO_UPDATE_SIM` | Simulated update flow | 048 |
| `KAIRO_STT_REPLAY_PATH` | Replay audio into STT | 038 |
| `KAIRO_AUDIENCE_SAMPLE` | Sample audience source, development only | 046 |
| `KAIRO_ENABLE_DECKLINK`, `DECKLINK_SDK_PATH` | DeckLink preview and build | 062 |
| `KAIRO_GPU_CAPTURE=0` | Forces CPU capture in Electron; no native equivalent needed unless a readback fallback is added | 061 |
| `KAIRO_ADHOC_SIGN` | Unsigned macOS builds | 101 |
| `VITE_WHATS_NEW_PREVIEW`, `CHANGELOG_AGENT`, `CHANGELOG_SINCE` | Changelog tooling | 071, 104 |

## 12. Known hardware gaps inherited from Electron

`docs/decisions/007` and `008` list unfinished physical qualification: UYVA receiver
acceptance, key/fill, interlaced motion, 4 cards plus NDI soak. The native app cannot claim
parity on points that were never qualified. Prompt 111 records those as a shared backlog.
