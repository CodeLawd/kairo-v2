# 03 — Proposed native architecture

The reference every prompt assumes. A change to anything here is recorded in
`DECISIONS.md` first.

## 1. Goals, in priority order

1. **The picture never goes wrong during a service.** A UI hang or crash must not disturb
   what is on the wall.
2. Every existing feature and every existing user's data survives.
3. Low, measured latency from spoken word to verse, and from clicker to page.
4. Low idle CPU and memory, including with several outputs.
5. Code an agent or engineer can change safely.

## 2. Process model

```
┌─────────────────────────────┐   commands / state      ┌───────────────────────────┐
│ kairo (UI + domain)         │ ─────────────────────▶  │ kairo-engine (output)     │
│  GPUI main thread           │ ◀─────────────────────  │  winit event loop         │
│  tokio runtime: kairo-core  │   acks, health, levels  │  compositor thread        │
│  audio capture thread       │                         │  video / audio threads    │
│  SQLite worker threads      │ ◀══ preview shm ══════  │  one thread per sink      │
└──────────────┬──────────────┘                         └───┬─────────┬─────────┬───┘
               │ local socket                               │ shm ring│         │
        ┌──────▼────────────┐                     ┌─────────▼───┐  ┌──▼───┐ ┌───▼────┐
        │ OS web views      │ ══ captured frames▶ │decklink-    │  │ NDI  │ │Syphon, │
        │ (WebKit/WebView2) │                     │helper (C++) │  │      │ │recorder│
        └───────────────────┘                     └─────────────┘  └──────┘ └────────┘
```

| Process | Why separate |
|---|---|
| `kairo` | Owns GPUI's single-threaded app, all business logic and persistence |
| `kairo-engine` | Output keeps running through a UI stall or crash. `winit` and GPUI cannot share the macOS event loop. Native decoder or GPU faults stay contained |
| OS web views | Not a Kairo binary. WKWebView, WebView2 and WebKitGTK run their own processes, managed by the OS |
| `decklink-helper` | Existing and qualified. Its reasons are documented in decision 007 |

**Engine lifecycle.** `kairo` launches and supervises the engine. If the engine dies,
`kairo` relaunches it and replays the current program state, with the same failure budget
as `ProgramSurface.allowFailure`. If the UI dies, the engine holds its last program state
for a grace period so the projector does not go black, then exits if no UI reconnects.
Deliberately closing the main window still closes screens, as today.

## 3. Workspace layout

```
kairo-gpui-kit/            this repository; Electron stays in ../kairo-presenter
  native-migration/        this playbook
  native/                  helpers copied from Electron, each with its source commit
  rust-toolchain.toml      Cargo.toml (workspace; exact pins)
  apps/
    kairo/                 UI binary (thin composition root)
    kairo-engine/          output engine binary
  crates/
    kairo-domain/          pure types, normalizers, rules (port of src/lib)
    kairo-store/           paths, settings, secrets, SQLite, workspace files
    kairo-net/             HTTP with failover, retries, WebSocket helpers
    kairo-core/            AppCore, actors, event topics, orchestrator
    kairo-core-gpui/       bridge from core topics into GPUI entities
    kairo-cloud/           session, licence, usage, crash reports, feedback
    kairo-scripture/       Bible data, detector, quote index, plans
    kairo-stt/             capture, relay, Deepgram, transcript buffer
    kairo-lyrics/          library, online sources, translation
    kairo-media/           media and tracks libraries, thumbnails
    kairo-documents/       PDF, PPTX, converters
    kairo-audience/        rooms, Engage, Slido bridge
    kairo-transfer/        .kairo, setlist, libraries, passages, usage logs
    kairo-updater/         feed parsing, staging, swap
    kairo-engine-proto/    versioned wire protocol, shm ring
    kairo-scene/           SlideScene + CPU layer raster (used by UI and engine)
    kairo-compositor/      wgpu layer compositor
    kairo-video/           decode, clocking
    kairo-audio/           mixer, device routing, NDI tap
    kairo-camera/          capture devices
    kairo-sinks/           display, NDI, DeckLink, Syphon, recorder
    kairo-native-sys/      C ABI wrappers over native/*.mm
    kairo-ui-kit/          tokens, fonts, icons, Kairo-styled components
    ui-shell/ ui-operator/ ui-scripture/ ui-lyrics/ ui-media/ ui-documents/
    ui-theme/ ui-screens/ ui-settings/ ui-audience/ ui-account/ ui-web/
  tools/
    ts-oracle/             drives TypeScript originals to produce golden JSON
    golden-capture/        captures Chromium reference frames and layout
    soak/                  long-run harness
```

Dependencies point one way: `ui-* → kairo-core → service crates → kairo-domain`. `ui-*`
crates never import each other. Existing C, C++ and Objective-C++ is copied from
`../kairo-presenter/native` into this repository's `native/`, and `kairo-native-sys` builds it
with `cc`. Electron keeps its own copy while it ships, and `SYNC.md` tracks drift between them.

## 4. Concurrency model

| Work | Runs on | Mechanism | Rule |
|---|---|---|---|
| UI layout and paint | GPUI main thread | `Entity`, `cx.notify` | Never block. No file, DB or network calls |
| UI background work | GPUI `background_spawn` | `Task` | Dropped tasks cancel, so views hold them |
| Business logic | tokio multi-thread runtime | One actor per stateful service | Serial per actor, replacing the single JS loop |
| SQLite | Dedicated reader pool and one writer thread | Bounded channel to callers | Never on tokio worker threads |
| Blocking I/O, parsing | `spawn_blocking` | | Bounded |
| Mic capture callback | cpal realtime thread | `rtrb` SPSC ring | No locks, no allocation |
| Compositor | One thread owning `wgpu::Device` | Latest-state mailbox | Renders on change or when a sink clock needs it |
| Decode | One thread per stream | Bounded frame queue (3–4) | Backpressure, drop for sync |
| Audio out | cpal callback | SPSC rings per source | No locks, no allocation |
| Sinks | One thread per sink | Latest-frame slot | **Frames are dropped, never queued** (decision 008) |
| Quote search | Dedicated worker | Request/response channel | Port of `quote-worker.ts` |

Ordering guarantees that TypeScript got from one event loop, such as `SurfaceManager`'s
serialized reconcile, must be made explicit through the owning actor's mailbox.

### 4.1 UI to core

```rust
// Shape only. Prompt 025 defines it.
core.call(lyrics::Search { query }).await?;          // request / response
core.send(program::SetLogo { on: true });            // fire and forget
let rx = core.watch::<ProgramState>();               // latest-value topic
let rx = core.stream::<TranscriptEvent>();           // ordered, bounded topic
```

- State snapshots (`ProgramState`, libraries, status) use latest-value topics, so a slow
  view skips stale values instead of building a backlog.
- Event streams (transcripts, suggestions) use bounded ordered channels.
- High-rate values (audio level, timers) are coalesced to display refresh at the bridge.

## 5. The output engine

### 5.1 Pipeline

```
kairo-core: ProgramState + OutputConfig
     │ EngineCommand (bincode over local socket)
     ▼
Scene builder (kairo-scene): typed SlideScene for each output
     │  text, shapes, gradients → CPU raster on a worker → RGBA layer textures
     │  video, camera, web → live texture sources
     ▼
Compositor (wgpu): layer stack, opacity transitions, filters, blur, camera layouts
     ▼
Render targets, one per distinct (scene, resolution)
     ├─▶ Display swapchain (vsync)
     ├─▶ NDI: GPU conversion to UYVA → async readback → send
     ├─▶ DeckLink: UYVY → existing shm ring → helper
     ├─▶ Syphon: IOSurface publish (macOS)
     ├─▶ Recorder: AVAssetWriter (macOS)
     └─▶ Preview: downscaled BGRA → shm → GPUI RenderImage
```

Outputs showing identical scenes at the same resolution share one render. Electron paid
for a separate renderer each time.

### 5.2 Why text is rasterised on the CPU

A slide's text changes only when the slide changes. Rendering it once with `tiny-skia`
takes milliseconds off the render path, gives stroke and blur control matching CSS, and
lets the **same code draw every thumbnail in the UI** without a GPU. The GPU only does
what must happen every frame: compositing, filters and video.

### 5.3 Engine commands

These map one to one with the Electron shell, so porting can be checked line by line.

| Today | `EngineCommand` |
|---|---|
| `__configure` | `Configure { output, transition, audio, video_layout }` |
| `__setContent(html, instant)` | `ShowSlide { output, scene, instant }` |
| `__clearText` / `__setContent("")` | `ClearText` / `Clear` |
| `__patchBackground` | `PatchBackground { filter, loop }` |
| `__setVideoPaused`, `__seekVideo`, `__videoEnded` | `VideoTransport`, `VideoState` query |
| `__markDocument`, `__swapDocumentPage`, `__preloadDocumentPage` | `ShowDocument`, `SwapPage`, `PreloadPage` |
| `__setDocumentVideos`, `__documentPlayback`, `__controlDocumentVideo` | `DocumentVideos`, `DocumentPlayback`, `ControlDocumentVideo` |
| `__setPoll`, `__setQuestion`, `__setMessage`, `__setProps`, `__setTimerDesign`, `__setLogo`, `__setInfo` | `SetLayer { kind, layer }` |
| `__setCamera`, `__setWeb`, `__setWebPointer`, `scrollWeb` | `SetCamera`, `SetWeb`, `WebPointer`, `WebScroll` |
| `__nextFrame` / `paintWaiters` | `FrameCommitted { output, seq }` event |
| `webContents.setFrameRate(...)` | Removed: pacing is internal |
| Test patterns | `SetTestPattern` |

### 5.4 Pacing

No more capture-rate tuning. The compositor renders when state changes, and while
something animates or plays. Each sink has its own clock:

- Display: vsync.
- NDI: fixed cadence, repeating the latest frame.
- DeckLink: the helper's hardware clock.
- Page turns are submitted immediately and acknowledged by `FrameCommitted`.

### 5.5 Colour

Compositing is premultiplied alpha. BGRA → UYVY/UYVA matches `native/decklink/uyvy.h`,
which remains the reference. The golden tests already written against it are carried over.

### 5.6 Web content

Web content uses the OS's engine: WKWebView, WebView2 or WebKitGTK. Nothing is downloaded
or bundled. See DECISIONS.md, "Replace CEF with `gpui-webview`".

- Web tab preview: `gpui-webview` in the operator window. GPUI overlays above the page
  need the `gpui-fast` backend, on macOS and Linux only.
- Slido: a separate GPUI window holding a `gpui-webview`, with the scraper as an init script.
- Built-in PPTX conversion: keeps the existing HTML generator, printed by the platform's
  PDF call through Wry's raw handle.
- Live web layer: a Wry web view owned by `kairo-engine`. Display outputs show it as a
  native child view. Capture-only sinks (NDI, DeckLink, Syphon, recorder) need platform
  frame capture, which is unproven and is checked first in prompt 059.

Linux requires X11 or XWayland for web content.

## 6. Previews in the operator UI

| Preview | Source |
|---|---|
| Live monitor, confidence preview, web preview | Engine shm frames at a capped rate |
| Verse cards, lyric slide cards, theme and look thumbnails | `kairo-scene` CPU raster, LRU cached |
| Theme and props editor canvas | `kairo-scene` raster with GPUI handles on top |
| Media thumbnails | FFmpeg poster frame, cached on disk |

Replacing images in a GPUI window requires calling `window.drop_image`, or the sprite atlas
grows without bound. Spike 002 validates this at the preview frame rate.

## 7. Data compatibility

The native app uses the **same** userData directory, settings files, databases, workspace
and bundle formats. On a first run it detects Electron-era secrets and re-wraps them.
Rollback to the last Electron release stays possible because formats are unchanged. See
prompt 100.

## 8. Security

| Electron control | Native equivalent |
|---|---|
| `contextIsolation`, no node integration | Business logic is not reachable from web content. Only the OS web views run remote code, and Wry IPC messages are untrusted |
| `pa-media` allowlist | The engine accepts only paths in commands from `kairo`, which keep the same allowlist and realpath logic for anything originating outside Kairo |
| Frame-ancestor stripping, scoped to subframes | Not needed: the program page loads top-level |
| Slido navigation allowlist, permission denial, sandbox | Same rules through Wry's navigation and new-window handlers. The OS web view process sandbox applies |
| API cache fails closed; session store fails open | Same |
| `.kairo` 50 MB cap, safe parsing | Same |

The local socket is per-user, uses a random name passed to the child, and requires a
handshake token. Web content is treated as untrusted input.

## 9. Observability

`tracing` writes to the existing log folder, and panics are logged and reported through the
existing crash-report path. Scripture traces keep their format. The engine emits per-sink
stats: frames produced, dropped and late. A hidden diagnostics panel shows them.

## 10. Improvements taken, and not taken

**Taken because they serve the goals:** engine isolation, shared renders, GPU compositing on
every platform, capture independent of the UI, a single auto-fit implementation, one
instance per web page.

**Deliberately not taken during migration:** changing file formats, redesigning screens,
adding features, rewriting the DeckLink helper.

## Amendments from prompt 008 (2026-11-09)

- Slide text uses `parley` + `swash`. Text is rasterised on change and cached, not per frame.
- ~~`kairo-web-host` is restored as an on-demand CEF component.~~ Superseded: web content uses `gpui-webview` and the OS web views (DECISIONS.md, "Replace CEF with `gpui-webview`").
- Zero-copy video is deferred. Rich text is a custom editor on `EntityInputHandler`.
