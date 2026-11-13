# 059 — live web layer

Implementation: shared Wry pages and captured compositor outputs. The owner
explicitly deferred Windows runtime/performance qualification after the original
both-platform gate; see the latest 059 amendments in DECISIONS.md. No CEF or
GPUI pin change.

## WebKit capture visibility fix (2026-11-13)

A fully covered source window froze WebKit's requestAnimationFrame callbacks.
The default offscreen-output rig and standalone visible probe did not catch this.
The fullscreen regression first failed with
`fully covered web source must keep requestAnimationFrame advancing`.

The fix disables window occlusion detection **only on Kairo's source WKWebView**,
using `_setWindowOcclusionDetectionEnabled:NO`. This is private WebKit SPI,
declared in WebKit's [own header](https://github.com/WebKit/WebKit/blob/main/Source/WebKit/UIProcess/API/Cocoa/WKWebViewPrivate.h).
The normal inactive scheduling policy does not control this separate visibility
check. Before loading content, the backend checks both setter and getter selectors,
sets the policy and reads it back. An incompatible runtime reports an error rather
than silently streaming frozen frames. No global swizzle, JavaScript animation
replacement, extra visible source overlay, or new browser dependency is used.

This preserves one page/shared playback and compositor outputs. The explicit
tradeoff is reliance on private SPI: macOS updates may change its availability or
behaviour. The runtime check handles missing/ignored selectors; the native
fullscreen regression must be rerun when qualifying an OS update. There is no
claim of a supported public offscreen WKWebView API. Replacing the backend with
a browser supporting offscreen rendering would be a separate architecture change.

```sh
cargo run -p kairo-engine --example verify_web -- --occluded --measure
```

The fixture reports every 30 animation callbacks. It records a baseline after
the cover settles, then requires at least two new reports; with `--measure`, the
baseline is taken during the final 60 of 330 captures, so early animation followed
by a freeze cannot pass. `--measure` records 300 timings after 30 warm-up frames
and asserts capture p99 below 50 ms. The guarded default fix passed
the fullscreen regression and existing native checks. A real fullscreen engine
capture after link navigation visibly showed **frame 381**, compared with frame 0
before the fix. See `evidence/059/` for before/after frames, logs and timings.

An earlier experimental own-window ScreenCaptureKit stream (1920×1080 BGRA,
30 Hz, no cursor/shadows/audio, borderless source) delivered captures but still
captured frozen frame 0. Its software delivery+copy p99 was 2.663–4.450 ms in
two-second batches and capture→display p99 11.792–16.933 ms. Fast frozen pixels
were not accepted. That unsuccessful experiment and dependency were removed.
Setting public WKInactiveSchedulingPolicy::None directly also did not resolve it.

Covered-source capture measurement on the same M2 Pro/macOS 27.2 debug build:
1920×1080 at 30 Hz, 30 warm-up + 300 measured frames; p50 **3.495 ms**,
p99 **6.813 ms**, max **10.297 ms**. Capture→offscreen GPU-complete p50
6.454 ms/p99 10.605 ms. An earlier 300-frame run measured capture p99
8.276 ms; both passed. The separate real fullscreen display run logged warm
two-second-batch software latency around 19–21 ms median, with final five
batch p99s 21.400–22.393 ms. That run also recorded earlier load/permission
outliers (capture p99 54.529 and 111.189 ms; cold first batch 239.883 ms), so
those warm batches are not a whole-run percentile. All software figures exclude
capture sampling age and physical scanout. No latency number claims physical
monitor response.

Windows qualification remains explicitly deferred by the owner.

## Try it

See [the native verification and live demo commands](../../tools/web-layer-site/README.md).
The automatic rig is `cargo run -p kairo-engine --example verify_web`. Build the
engine binary before using the `drive --web URL` example. The operator workspace
itself is prompt 089; app IPC/output routing remains 065.

## Implemented

- One native page per normalized requested URL, shared by consumers and actual
  navigated URL. Page identity is independent of the original address: adopting
  A→B keeps media/scroll alive, and asking for A again can create A alongside B.
  Up to eight pages, unused owners released before capacity checks, freed slots
  retried. Zero pages/source windows without eligible consumers. Last GPU pixels
  survive at most 5100 ms for the longest layer fade after the browser closes.
- Native navigation restrictions, permissions, reload/crash recovery and clean
  teardown; bounded, coalesced scroll-only IPC. Native location is polled so
  content cannot fabricate navigation commands through IPC. Input strings are
  inserted as text, not executed. Pointer hover, clicks, keys and scrolling go
  directly to the shared page. Same URL/reload is a no-op.
- Captured RGBA feeds the compositor source texture; ordinary layers, fades,
  camera layouts and the actual arrow stay above it. Canonical 1920×1080 viewport
  contained on white for other frame heights; pointer uses identical fit.
- macOS WKWebView snapshot, explicit main-thread delegates denying camera/mic,
  popups, downloads and subframe top navigation. Renderer reloads up to three
  times per minute, manual Reload retries. Outstanding callbacks are invalidated
  on reload, timeout, renderer loss or teardown. Snapshot pixels are explicitly
  converted into premultiplied RGBA, dimensions validated. A hidden view stopped
  animation; the source is mapped behind other windows, non-interactive and out
  of window cycling. The guarded visibility control above sustains animation
  under full occlusion. Public fullscreen preference is enabled; Wry enables PiP
  and autoplay. Original framing headers are untouched.
- Windows WGC source capture, two-frame pool, D3D11 BGRA→RGBA staging copy,
  WebView2 scale 1, restricted native permissions and input to the owned renderer
  HWND. Response-stage CDP interception installs a real CSP sandbox and strict
  referrer policy before loading documents; failures stop loading. Original
  headers and response body are retained. Empty polls after a static frame are
  normal; capture failures are reported once and retried. Source windows remain
  behind other windows, non-activating and outside the taskbar. **Not run on Windows.**
- Linux reports explicit unsupported capture instead of claiming a live page.
- Protocol 3 adds WebInput/WebNavigated and eligibility; native navigation updates
  both the engine and supervisor's intended state, so crash replay uses the actual
  page. Eligibility is the conjunction of background visibility and program follow,
  supplied by 065. Parked, closed and ineligible surfaces never acquire a browser.

## Measurements and evidence

Original standalone macOS probe (M2 Pro, 16 GB, macOS 27.2, debug, 1920×1080,
30 Hz, 3 s load + 30 warm-up + 300 measured frames, CPU copy included):
p50 6.274 ms, p99 9.216 ms, max 12.772 ms, zero missed ticks/errors. Earlier
correctly sized run p99 9.014 ms. Saved in `evidence/059/capture-macos-debug.txt`
and `snapshot.png`. The first attempt used Wry's default square child bounds and
was rejected for incorrect size; explicit view bounds fixed it. A sandboxed run
could not access the window server and produced no measurement.

Real output measurements and final checks are recorded in PROGRESS.md and
`evidence/059/verification.txt`. Native warm-up can be much slower (one cold first
snapshot took 449 ms during concurrent build activity); that is not hidden in
steady-state numbers. Software capture→display timing excludes sampling age and
physical scanout; the separate capture and compositor stages are logged.

The initial implementation delayed CPU pixels until the next engine tick, adding
about one tick; completed callbacks now send frames directly to the GPU. PNG
readback/encoding runs off the window thread. A native sample identified a GPU
shutdown deadlock (main joined the GPU while GPU-side Window destruction needed
main); asynchronous shutdown fixed it without extending the supervisor timeout.

## Verification scope

The real Wry/GPU rig passed: two outputs/one page and equal pixels, framing bypass,
click/text/scroll, denied camera/mic, user-activated cross-origin frame top navigation,
malformed/oversized IPC, close/reopen/reload during capture, URL adoption/return to
original, and replacement at capacity. The local manual site additionally observed
popup attempts without loading /popup, no download, HTML audio `playing`, and link
navigation. SIGTERM of the test engine's sole WebKit WebContent helper caused the
native termination event, one reload and a new successful /next load.

Actual `kairo-engine` output windows were run; composed frames were read and
visually inspected (page, animation, pointer). Native computer-use inspection
was attempted but its service timed out. No claim of physical scanout measurement
or sound heard by ear. Window-server access is required for the native tests; a
workspace run hit the existing no-display timeout when the display slept. The
unchanged display test passed after `caffeinate -u`; no assertion was weakened.

## Deferred or unavailable

- Owner-deferred Windows runtime/security/latency and full Windows app build.
  The actual WGC/WebView2 source was Windows-target checked/linted in a small
  harness using the real compositor/domain/protocol crates, avoiding the unrelated
  cross-build FFmpeg/AV1 toolchain requirements. This is **compile evidence only**.
  The native rig also accepts Windows for later on-machine testing.
- Windows renderer HWND hierarchy compatibility, Windows/browser hot updates,
  multiple physical displays and unusual GPU adapters; Linux native backend.
- Physical scanout latency, DRM/video snapshot coverage, fullscreen/PiP behaviour
  on captured outputs, and long-run resource soak. ScreenCaptureKit is not used:
  the snapshot fallback remains the current macOS path, with the guarded
  private visibility control above.
- Public WKWebView/WebView2/WebKitGTK APIs do not expose page PCM to kairo-audio.
  One page can sound through the OS default route; Kairo mixer gain/device selection
  and per-output NDI audio do not control it. macOS HTML media playback was
  observed through its play promise, not by ear. Other OS audio unverified.
- Actual NDI/DeckLink/Syphon/recorder sinks are prompts 061–064; this prompt feeds
  compositor textures that those outputs can use. App routing/IPC is 065; preview
  input and bookmarks/history UI is 089. No Electron files were edited.

## Source audit and edge/error paths to carry into implementation

Graphify repository `CodeLawd/kairo-v2` is indexed at `e263079` (2026-11-11);
the initial call omitted the required repository selection. The indexed engine
structure was checked against the newer, uncommitted local engine files.

Reference: Electron `af978c5`; drift since SYNC.md's `de3bb54` is that commit's
`overlay.html` change. The five listed source paths had no uncommitted changes
when inspected, and the Electron checkout was not edited.

- `web/index.ts`: destroyed WebContents yields no frames; only direct child
  frames with the correct name are followed (ignore main page, nested ads and
  players). Missing frames and exceptions from frame lookup log warnings;
  script-evaluation failures are swallowed during load or teardown. Scroll
  coordinates are rounded; only the last URL's position is retained and restored
  when normalized URLs match. Before the follower loads, scroll instantly;
  otherwise ease toward the target on a 16 ms timer that runs when covered.
  With a shared page, remove follower/mirroring rather than retaining that timer.
- Preview mute: catches existing, new, playing and unmuted media; required only
  for independent previews. The shared source must not have multiple audio
  playbacks. Public WKWebView/WebView2/WebKitGTK APIs do not expose a decoded
  audio feed into kairo-audio. No web-to-mixer or per-NDI audio is implemented.
- `overlay.html`: unchanged URL + reload is a no-op; a reload change makes a new
  page; clearing removes it with the configured fade; pointer is hidden on
  navigation/clear. Before paint the backdrop is white. Pointer absent, malformed
  or without a live page hides it; finite coordinates clamp to the frame; the
  arrow is 56 px with left/top `max(0, x - 10)` and `max(0, y - 5)`.
- `WebPage.tsx`: empty initial state gives the address/search prompt, no page;
  invalid/empty address gives an error; failed setWeb surfaces its error message;
  unavailable history steps do nothing. The first navigation/redirect after
  operator load replaces that history entry; later links and SPA routes append;
  unchanged URL does nothing. Following carries navigation/scroll only while
  the current page matches the live URL. Hide the pointer when disabled, inactive,
  not following, on a different page, leaving the page or outside 1920×1080.
  Ignore messages not from the expected frame. Shared-page IPC must additionally
  bound message sizes, numbers and URL lengths before allocation/evaluation.
- Adaptive preview detection samples up to 4000 elements; no body or height
  below 200 is non-adaptive. Full-height or large sticky blocks select exact
  16:9 layout; resizes debounce 250 ms. ScaledWebFrame hides at zero scale;
  fill-height divides only for positive scale; passive previews reject pointer
  events and tab focus. A frame resize must not reload the page.
- `web.ts`: trim input; empty or more than 2048 UTF-16 units is invalid; bare
  hosts get HTTPS; only HTTP(S) loads; whitespace, unsupported schemes and invalid
  URLs search only when search is enabled. A scheme-like host:port is allowed.
  Already ported as `kairo_domain::web::web_address`; reuse it. Header stripping
  was limited to subframes and left unrelated CSP directives intact; a top-level
  page needs no stripping at all. Bookmarks: ignore bad entries/empty ids, trim
  and cap names at 80, use display address for missing names, maximum 50 entries;
  showPointer is true only for literal true (operator workspace prompt 089).
- Native failures additionally need bounded capture timeouts/retries, stale
  callback generation checks, no page without consumers, navigation/load errors,
  renderer termination reload, native permission denial and teardown with an
  outstanding snapshot. The pinned Wry macOS UIDelegate explicitly **grants**
  media capture permissions; replace it with a denying delegate before loading
  any untrusted page. Wry navigation/new-window/download hooks alone do not
  recreate the iframe sandbox or enforce referrer policy. These are now handled by the native delegates and Windows CSP response enforcement above.
- The original shell had a white placeholder and invisible pointer geometry. A
  failing regression assertion proved it; the slot now has captured source pixels
  and the exact rasterized arrow SVG from overlay.html, with passing tests.
- Follow/show-background eligibility belongs to the surface manager (065);
  SurfaceConfig now carries the explicit aggregate `web_enabled`. It is not inferred
  from speaker enablement. An ineligible output acquires no web consumer.

