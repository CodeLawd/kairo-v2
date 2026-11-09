# Spike 006: CEF web host

> **Superseded (2026-11-09):** the owner chose `gpui-webview` and the OS web views over CEF. See DECISIONS.md, "Replace CEF with `gpui-webview`". This report is kept for reference.

**Result: PARTIAL, leaning GO.** All four browser uses work locally on macOS arm64 with `cef` 156.4.0. Open items: a real Slido sign-in needs a person, subframe header removal is not implemented, cross-process shared textures are not built, and this CEF build cannot play H.264 or HEVC.

Measured 2026-11-09 on an Apple M2 Pro with other agent sessions running. Windows and Linux were not run.

## The four uses

| Use | macOS | Evidence |
|---|---|---|
| Live web layer | **PASS** (CPU paint) | 20-minute soak below |
| Web preview | **PASS** for input and events | Move, click and wheel reached the page at the sent coordinates; reload, address changes and load events reported |
| Slido | **PASS against a local fixture; real sign-in not verified** | See below |
| PPTX to PDF | **PASS** | App's `parseSlide` and `slideHtml` on a hand-written two-slide fixture: `%PDF-`, 2 pages, MediaBox 1440×810 pt (20 × 11.25 in) |

## Requirement detail

**Frames.** Windowless 1920×1080 at 30 fps, published by CPU `on_paint` to a seqlocked shared-memory file and uploaded to a `wgpu` texture by a separate process. `shared_texture_enabled` also works in the host (167 accelerated paints in 6 s, each with an IOSurface), but those frames were only counted. Sending an IOSurface to another process needs a mach port and was not built. CPU paint already meets the target, so shared textures are an optimisation.

**Framing headers.** A page sending `X-Frame-Options: DENY` and `frame-ancestors 'none'` loads as a top-level page. Control: the same page in an iframe fails with `ERR_BLOCKED_BY_RESPONSE`. The native design renders the program page top-level, so the headers never apply. Actual removal is **not implemented**. CEF documents the response as read-only in `OnResourceResponse`, so removal would need a proxying `ResourceHandler`, and only matters if something frames a page.

**Audio.** The audio handler received 55,941 packets (57,283,584 frames at 48 kHz stereo) in the soak. Routing it onward was not built.

**Crashes.** `chrome://crash` produced `on_render_process_terminated`, and loading the page again recovered.

**Slido window.** Tested with the sandbox on and off:

- Visible window; profile persisted a cookie across restarts.
- The navigation rule is ported with unit tests. A top-level navigation and a popup to a disallowed host were refused.
- Microphone and geolocation denied through the permission handler.
- Init script ran before page scripts, once moved to the renderer's `on_context_created`. Running it at `on_load_start` was too late.

Difference from Electron: `execute_java_script` runs in the page's main world, so there is no `contextIsolation`. The page can see the script's globals and could forge console-based results. Prompt 047 needs a V8 isolated-world route or must treat every read as untrusted. The script is passed as a child-process switch because the sandboxed renderer cannot read files.

**Bugs found while building.** Holding the browser lock across `close_browser` deadlocked, since it can call `on_before_close` synchronously. AppKit's `terminate:` must be overridden so it cannot exit before `CefShutdown`. Prompt 050 onward should carry both.

## Measurements

| Metric | Result | Target |
|---|---:|---:|
| Soak frames, 20 min | 34,452 (28.71 fps average, including a 6 s fixture recording phase with no frames) | 30 fps |
| Late 60 s windows | 29.0, 28.5 fps | |
| Paint to `wgpu` upload, p50 / p99 | 3.00 / 5.46 ms | < 50 ms added |
| Frame gap p50 / p99 | 33.37 / 70.94 ms | |
| Torn reads skipped | 5 | |
| Host process CPU, average of samples | 3.5% (browser process only) | |
| RSS all processes, idle `about:blank` | 456–558 MB | |
| RSS all processes, video, about 7 min in | about 700 MB | |
| RSS all processes, soak peak | 816 MB | |
| Shutdown | Exit 0, `SHUTDOWN clean`, 0 helpers left | |

The latency figure starts at CEF's paint callback, so Chromium's own pipeline is not included. The video was a VP8/Opus 1080p clip recorded by Chromium in the fixture. Logs: [evidence/006](../evidence/006/).

Size, macOS arm64: 329 MB installed, 148 MB zipped. The framework is 326 MB of that, its binary 226 MB. Windows and Linux sizes were not measured.

## macOS bundle and signing

```
KairoWeb.app/Contents/
  MacOS/host
  Frameworks/Chromium Embedded Framework.framework
  Frameworks/host Helper.app
  Frameworks/host Helper (GPU).app
  Frameworks/host Helper (Renderer).app
  Frameworks/host Helper (Plugin).app
  Frameworks/host Helper (Alerts).app
```

Helper names are derived from the main executable. Each helper, the framework and the app are signed inside-out. Only ad-hoc signing was tested, and the sandbox worked with it. Developer ID, hardened-runtime entitlements and notarization are not verified (prompt 101). The app needs an `NSApplication` subclass implementing `CefAppProtocol`.

## H.264 and HEVC

`canPlayType` answered "no" for H.264 and HEVC, "probably" for VP8 and VP9. Standard CEF builds exclude proprietary codecs. Sites offering VP9 work; H.264-only pages and streams will not. Electron's behaviour should be compared at 008. Enabling them needs a custom CEF build, which carries licensing questions that belong with the FFmpeg decision.

## Recommendation: on-demand signed component

Mirror `slide-converter.ts`: pinned version, checksum, installed in Kairo's own folder, with an option to pre-install during onboarding. At 148 MB compressed per architecture it is too large to impose on operators who never use web content, and CEF needs regular security updates independent of app releases.

When absent: the Web tab and web layer offer the install; Slido sources cannot connect and say why; PPTX conversion uses PowerPoint, Keynote or LibreOffice if present, or asks for a PDF. Scripture, lyrics, media and all outputs are unaffected. Because the first service may be offline, onboarding should offer the install.

## Failure and empty paths from the source

| Source path | Status |
|---|---|
| Non-http(s), empty or malformed navigation | Rule ported with tests; `@` userinfo also refused |
| Popups: denied unless allowed | Done |
| `will-navigate`, `will-redirect` | `on_before_browse`, which also reports redirects. A real redirect was not exercised |
| Permission request and check denied | Prompts and media denied; a synchronous check equivalent was not examined |
| `did-fail-load` (ignoring -3) | `on_load_error` logs code and text; a subframe failure was observed |
| Renderer gone | Reported, recovery shown |
| Closing the sign-in window hides it | **Not implemented** (`do_close`) |
| PDF magic check; PDF failure | Magic logged; failure path not forced |
| `.ppt`, invalid or over 500-slide files | Happen before printing, stay in the generator; not exercised |
| Images waited for before printing | **Not implemented**; fixture has no images |
| Mute, scroll and follow scripts | Unneeded for one shared instance per architecture 5.6; not ported |
| Slido watchdog and scraper | Prompt 047 |

## Not verified

Real Slido and Google sign-in; Windows and Linux; hardware video decode; Developer ID signing; idle-machine timing; a live site or long streaming page; multiple simultaneous browsers.
