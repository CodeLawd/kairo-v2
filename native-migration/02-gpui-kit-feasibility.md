# 02 — GPUI Kit feasibility and compatibility assessment

**Evidence.** Read from source, not from marketing text, on 2026-11-09:

- `longbridge/gpui-kit` at commit `ca479f3` (workspace version 0.9.0).
- The crates it pins: `gpui-pre`, `gpui-pre-macos`, `gpui-pre-apple`, all `=0.3.11`.
- The site docs under `website/docs` in that commit.

**Limit.** Nothing here has been compiled or run yet. The spikes in phase 0 exist to prove
the items marked *Spike*. Crate versions below were read from crates.io the same day.

## 1. Verdict

GPUI Kit is a strong fit for the **operator interface**. It is not, by itself, able to
produce **program output**, and it should not be asked to.

Kairo therefore needs two rendering systems: GPUI Kit for controls, and a purpose-built
compositor for everything the congregation sees. That split is the central decision in
this plan.

## 2. What GPUI Kit provides

| Kairo need | GPUI Kit answer | Confidence |
|---|---|---|
| Buttons, inputs, selects, switches, sliders, tabs, tooltips, toggles | `gpui_kit::component` | High |
| Dialogs, sheets, popovers, menus, toasts | `dialog`, `sheet`, `popover`, `menu`, `notification` | High |
| Split panes (`react-resizable-panels`) | `Resizable` | High |
| Long lists and grids | `VirtualList`, `List`, `DataTable`, `uniform_list` | High |
| OTP entry | `otp-input` | High |
| Colour picker | `color-picker` | High, needs alpha check |
| Trees and sidebars | `tree`, `sidebar` | High |
| Settings-style grouped rows | `settings` | High |
| Rich rendered text | `TextView` (Markdown and HTML) | High for display |
| Dark and light tokens | Semantic theme system | High |
| Accessibility | AccessKit integration | High for stock components; custom canvases need work |
| UI integration tests | `#[gpui_kit::test]`, `gpui_kit::test::TestWindowExt` | High |
| Pixel tests | `test-support` headless renderer | Test builds only |
| Native menus | `set_menus`, `on_app_menu_action` | High |
| Dock menu, recent documents | Platform trait | High |
| Multiple windows, per-display placement | `WindowOptions.display_id`, `cx.displays()` | High |
| Display hotplug | `on_displays_changed` | High; verify debounce |
| File association, URL open | `on_open_urls` | *Spike* on all platforms |
| Quit interception | `on_quit`, `on_should_close` | High |
| Sleep and wake, thermal state | `on_system_sleep`, `on_system_wake`, `thermal_state` | High |
| Prevent idle sleep | `prevent_idle_sleep` → `ActivityGuard` | *Spike*: must cover display sleep |
| Clipboard text and images | `read_from_clipboard` | High. File lists need native code |
| Keychain | `read_credentials`, `write_credentials` | High. Kairo will use `keyring` to stay process independent |
| Native file dialogs | `prompt_for_paths`, `prompt_for_new_path` | High |
| OS file drop | `FileDropEvent`, `ExternalPaths` | High |
| Internal drag | `on_drag`, `on_drop` | High |
| Vibrancy | `WindowBackgroundAppearance` (Opaque, Transparent, Blurred variants) | *Spike*: compare with `under-window` |
| Frame budget control | `inactive_frame_interval`, `low_power_frame_interval` | High |
| FPS and memory overlay | `gpui-fps` crate | High, dev only |
| Agent guidance | `npx skills add longbridge/gpui-kit` | Mandatory, see prompt 000 |

## 3. Gaps, verified in source

| Gap | Source evidence | Consequence | Resolution |
|---|---|---|---|
| **No production offscreen render.** `render_to_image` and `PlatformHeadlessRenderer` are `cfg(any(test, feature = "test-support", ...))` | `gpui-pre/src/platform.rs:1456-1475` | Cannot feed NDI, DeckLink, Syphon or the recorder | `kairo-compositor` on `wgpu` |
| **`surface()` is Apple only** and asserts NV12 full-range | `elements/surface.rs`; `metal_renderer.rs:1413` | No cross-platform video element | Preview through frames from the engine |
| **No video element or decoder** | Absent from the source tree | `<video>` must be replaced | FFmpeg, `kairo-video` |
| **No text stroke, blurred text shadow, CSS filter or arbitrary div rotation.** `with_transformation` exists only on SVG | `style.rs`, `elements/svg.rs:64` | Cannot draw slides in GPUI | Slides use the compositor |
| **Window level and collection behaviour are not exposed.** `kind` maps to fixed levels | `gpui-pre-macos/src/window.rs:1238-1263` | Screen-saver-level identify windows need native code | Created in the engine via `winit` |
| **`WindowKind` has no non-activating full-screen output mode** | `platform.rs:2839` | Output windows in GPUI risk stealing focus | Output windows live in the engine |
| **WebView is labelled experimental.** Wayland is rejected, and it forces X11 for **all** app windows on Linux | `website/docs/webview.md` | Would change Kairo's whole Linux window path | Do not use `gpui-webview` |
| **WebView covers GPUI overlays on the default backend** | Same | Menus and dialogs would go behind the page | Not used |
| **No microphone or camera API suitable for the service.** The `speech` component is OS dictation UI | `crates/component/src/speech` | Not usable for 16 kHz raw PCM | `cpal`, platform camera |
| **File-list clipboard** is not in `ClipboardItem` | `platform.rs:587-609` | `media/clipboard.ts` needs equivalents | Small platform shims |
| **Auto-update is left to the application** | `website/docs/auto-update.md` | Kairo must own it | Prompt 048 |
| **GPUI API churn.** `gpui-pre` is a snapshot and "any snapshot may change GPUI's API" | Kit `Cargo.toml` comment | Upgrades can break builds | Pin exact versions; upgrade as a dedicated task |
| **Minimum OS.** Docs list macOS 15 for building; Kairo ships to macOS 11. Runtime floor unproven | `installation.md`, `package.json` | Possible loss of older Macs | *Spike* prompt 007, owner decision |
| **Linux needs Vulkan** | `packaging.md` | Old church PCs may fail where Electron fell back to software | *Spike*, documented requirement |

`gpui-fast` is an optional alternate backend. Treat it as a later evaluation; the plan does
not depend on it, since its main benefit here would be WebView overlays.

## 4. Why not render slides with GPUI anyway

Three approaches were considered for program output.

| Option | Verdict | Reason |
|---|---|---|
| Paint slides as GPUI elements and display them in GPUI windows | Rejected | Fails text stroke, shadow blur, filters and rotation. No path to NDI or cards. Breaks "one renderer, every sink" |
| Enable `test-support` in production for headless capture | Rejected | Unsupported, pulls in test code, no shared-texture path |
| Keep Chromium for all program output through CEF | Rejected as the default | Keeps the largest cost Kairo is trying to remove: a full renderer per output. Kept only for genuine web content |
| **Dedicated `wgpu` compositor** | **Chosen** | One renderer and many sinks, runs on every OS, deterministic and testable |

## 5. Replacement technology

Versions are the latest stable on crates.io on 2026-11-09. Prompts must re-check them.

| Capability | Choice | Version seen | Notes |
|---|---|---|---|
| GPU | `wgpu` | 30.0.1 | Metal, D3D12, Vulkan |
| Output windows | `winit` | 0.30.13 | In the engine process, so it never shares an event loop with GPUI |
| Text shaping and layout | `parley` with `swash`; alternative `cosmic-text` 0.19 | 0.12.0 / 0.2.10 | Prompt 003 selects by measured line-break parity |
| CPU rasterisation | `tiny-skia` | 0.12.0 | Text strokes, shapes, gradients |
| Video | `ffmpeg-next` | 9.0.0 | Hardware decode. LGPL dynamic linking |
| Audio I/O | `cpal` | 0.18.2 | Input and per-device output |
| Camera | `nokhwa` or direct AVFoundation, Media Foundation, V4L2 | 0.10.11 | Must expose labels; prompt 057 decides |
| Local IPC | `interprocess` | 2.4.8 | Sockets and named pipes |
| Shared memory | `memmap2` + `shm_open` / file mapping | — | `shared_memory` crate is stale (2022). Mirrors `ring.h` |
| SQLite | `rusqlite` (bundled, FTS5) | 0.41.1 | Same databases |
| PDF | `pdfium-render` | 0.9.5 | Replaces pdf.js. Ships PDFium |
| Web engine | `cef` (tauri-apps) | 156.4.0 | Off-screen capable. Only when web content is used |
| Secrets | `keyring` | 4.2.0 | OS credential stores |
| WebSocket | `tokio-tungstenite` | 0.30.0 | Deepgram, Audience |
| NDI | Own thin FFI, or `grafton-ndi` | 2.0.0 | Prompt 005 decides on UYVA, FLTp and bundling needs |
| DeckLink | **Existing helper** | — | Unchanged process and protocol |
| Syphon, recorder, Metal | **Existing Objective-C++** | — | N-API shim swapped for a C ABI |

### Disposition of every npm dependency

| Package | Fate |
|---|---|
| `electron`, `electron-builder`, `electron-vite`, `electron-log`, `electron-store`, `electron-updater`, `@electron/*`, `@electron-toolkit/*` | Replaced: native packaging, `tracing`, `kairo-store`, `kairo-updater` |
| `better-sqlite3` | `rusqlite` |
| `axios` | `reqwest` |
| `ws`, `@deepgram/sdk` | `tokio-tungstenite` |
| `openai`, `@anthropic-ai/sdk` | Direct JSON through the managed gateway |
| `cheerio` | `scraper`; XML reader for PPTX |
| `jszip` | `zip` |
| `mammoth`, `pdf-parse`, `pdfjs-dist` | docx XML reader, `pdfium-render` |
| `qrcode` | `qrcode` crate |
| `grandi` | NDI binding from prompt 005 |
| `@tiptap/*` | Decided by prompts 007, 008 and 079 |
| `radix-ui`, `react-resizable-panels`, `input-otp`, `class-variance-authority`, `tailwindcss`, `zustand`, `react`, `react-dom` | GPUI Kit, `kairo-ui-kit`, entities |
| `@phosphor-icons/react`, `@hugeicons/*` | Embedded SVG subset; check licences |
| `@fontsource-variable/source-sans-3` | Embedded font |
| TypeScript, ESLint, `tsx`, Vite tools | Remain only for `server/`, `website/`, Bible tools and the oracle |

## 6. Mapping React and Electron concepts

| Today | Native |
|---|---|
| React component with hooks | `Entity<T>` + `Render`, or `RenderOnce` for stateless pieces |
| Zustand stores (12, including bootstrap) | `Entity<Model>` per feature |
| `window.api.x.y()` | Typed `AppCore` call |
| `ipcRenderer.on` pushes | Topic subscription that updates an entity |
| Tailwind classes | Tokens in `kairo-ui-kit` |
| Radix primitives | `gpui-component`, or `gpui-base` where Kairo owns the look |
| `setInterval` for clocks | `cx.spawn` with timers, one ticking source per concern |
| CSS transitions | `animation`, `motion` |
| `dangerouslySetInnerHTML` previews | `kairo-scene` CPU raster → `RenderImage` |
| `<video>` previews | Frames from the engine |
| DevTools and `console-message` | `tracing`, `gpui-fps`, the Kit inspector feature |

## 7. Component policy

Use `gpui-component` wherever its look can be themed into Kairo's system. Drop to
`gpui-base` only where the design requires it: the rail, the live monitor frame, slide
cards, and canvas handles. Every deviation is recorded in `DECISIONS.md`.

The agent must read the Kit's **Coding Guides and Design Guides** before UI work. They
specify rules this plan relies on: feature-oriented crates, one `Root` per window, and
`Entity` state ownership. Kairo's `DESIGN.md` overrides the Kit's Design Guides whenever
the two disagree about appearance.

## 8. Upgrade rule

`gpui-kit` is pinned with `=0.9.0`. Never bump it inside a feature prompt. Upgrades are
their own task: run all gates, then re-run spikes 002 and 007.
