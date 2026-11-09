# Decisions log

Append-only and newest last. This file **overrides** any prompt or planning document.
When a prompt must deviate from its text, record why here first.

Format:

```
## YYYY-MM-DD — <prompt id> — <short title>
Decision:
Why:
Alternatives rejected:
Affects prompts:
```

## 2026-11-09 — plan — Planning baseline

Decision: This plan was written against commit `de3bb54` on `ci/release-packaging-retries`.
The working tree also held uncommitted owner edits to `overlay.html`, `App.tsx`,
`AppShell.tsx`, `MediaDock.tsx`, `ScreenConfiguration.tsx`, and a new
`tests/document-page-swap.test.ts`. The audit read those files as they stood.
Why: Prompts compare against this commit when checking for drift.

## 2026-11-09 — plan — Directory location

Decision: Planning files live in `native-migration/`, and the new code in `kairo-native/`.
Why: `.gitignore` ignores `docs/*` except named files, and `native/` already holds C++ and
Objective-C++ helpers that the new app reuses.

## 2026-11-09 — plan — Proposed decisions awaiting prompt 008

These are recommendations, not settled facts, until the owner confirms at prompt 008:
two-process design, `wgpu` compositor, CPU text raster, FFmpeg, CEF web host as an
on-demand component, no `gpui-webview`.

## 2026-11-09 — plan — Final review corrections

Decision: A review against the code corrected these planning errors: provider keys are not stored in settings (they go through the managed gateway); `bible.db` is copied to writable userData; the theme font list is fixed; `Transcription.tsx` appears to be dead code. Handler-level logic gained an owner in prompt 067, and Electron `localStorage` gained a migration.
Why: Each was checked against source at the planning baseline.

## 2026-11-09 — plan — Separate repository for the native app

Decision: The new app lives in `kairo-gpui-kit`, at the workspace root, and this playbook moved there. `../kairo-presenter` stays the reference and keeps shipping. This supersedes the earlier `kairo-native/` location.
Why: The owner wants to keep updating the Electron app during the migration.
Consequences: prompt 009 adds a sync process; native helpers are copied with recorded source commits; CI works without the Electron checkout; releases still publish to `CodeLawd/kairo-presenter`, which the updater and website read.

## 2026-11-09 — 000 — Bootstrap scope and toolchain

Decision: Use stable Rust with rustfmt and Clippy, following the installation
guide's Rust 1.92-or-newer baseline. The published GPUI Kit 0.9.0 manifest
declares edition 2024 and does not declare a separate minimum Rust version.
Why: The installation guide ties the minimum to the Linux dependency graph.

Decision: The UI binary is the hello-world exception to empty workspace
members. Other application and tool binaries have a doc comment and an empty
entry point so Cargo can build them. Preserve the starter's GPUI initialization
and replace its sample counter with the requested hello-world view.
Why: The prompt requires a window smoke test and buildable binary members.

Decision: Install project-scoped GPUI Kit skills for Codex using
`npx skills add longbridge/gpui-kit --skill gpui-kit gpui-kit-design-guides --agent codex --yes`.
The installer placed them at `.agents/skills/gpui-kit/` and
`.agents/skills/gpui-kit-design-guides/` and wrote `skills-lock.json`.
Why: Both requested skills are available in the native checkout.

Decision: Keep the website separate from the desktop workspace. The operator can
continue using Next.js in a separate project.
Why: The operator confirmed this during bootstrap.

GPUI Kit 0.9.0 enables the platform's `runtime_shaders` feature. The bootstrap
CI follows the documented platform setup and adds no standalone Metal compiler step.

## 2026-11-09 — 000 — Local build and window evidence

Decision: Record the clean build of the workspace, including `apps/kairo`, at
153.02 seconds wall time, and the immediate unchanged `cargo build --locked -p kairo`
at 1.02 seconds. These were measured on macOS with Rust 1.99.1. The clean
measurement includes the first dependency downloads and compilation.

Decision: Capture the hello-world window by placing the same built executable
in a temporary macOS app bundle. `cargo run --locked -p kairo` launched the
executable, but the UI tool could not target the bare executable by path.
The app bundle was only for the screenshot and lives outside the workspace.

Decision: Keep the cross-platform CI gate open. The new native repository
has no Git remote, so there is no GitHub Actions run to verify.

## 2026-11-09 — 000 — Remove the dedicated web host

Decision: Remove the `kairo-web-host` binary from the native workspace. The
website remains a separate Next.js project. This supersedes the proposed CEF
host and any instruction to add that binary. Prompts that depend on the host
need a revised approach before implementation.

## 2026-11-09 — 001 — Begin baseline during hosted CI

Decision: The operator authorized starting the Electron baseline while the
remaining bootstrap CI jobs finish.

## 2026-11-09 — 002 — Engine spike structure and API differences

Decision: `spikes/engine-window` declares its own `[workspace]`, so its throwaway `winit` and `interprocess` dependencies stay out of the app lockfile. The root workspace gates do not cover it, so its gates are run separately.
Decision: Shared memory is a memory-mapped temporary file with three seqlocked frames. A SIGKILLed writer leaves no reader state to repair.
Decision: The 15 and 30 fps soaks ran concurrently to fit one 30-minute window. CPU was measured in separate 60-second runs.
API differences: `wgpu` 30 presents with `queue.present(frame)` and returns a `Result` from `get_mapped_range`. `interprocess` 2.4 deprecates `split`, so the stream is shared through `Arc` and `&Stream`. winit 0.30 has no non-focusable window, so the spike replaces `canBecomeKeyWindow` and `canBecomeMainWindow` for borderless windows through the Objective-C runtime.
Why: Each was read from the pinned crate source. Prompts 008 and 050 must carry the focus override forward, or find a supported alternative.

## 2026-11-09 — 005 — NDI binding and spike scope

Decision: Recommend a thin `libloading` binding for NDI, loading the runtime from the app folder. This is a recommendation for prompt 061 and is subject to prompt 008.
Why: `grafton-ndi` 2.0.0 links `libndi` at build time, so a missing library stops the process at launch. Decision 001 requires a broken NDI to disable only NDI.
Alternatives rejected: `grandi`, which requires a Node runtime.
Decision: The spike builds its C ABI in `spikes/sinks` and not in `kairo-native-sys`, because spike code stays out of app crates. The binding resolves exported symbols rather than the `NDIlib_v6` table.
API difference: wgpu 30 exposes the import route as `Device::create_texture_from_hal` with `hal::metal::Device::texture_from_raw`.
Affects prompts: 008, 061, 064.

## 2026-11-09 — 003 — Text spike structure and results

Decision: `spikes/003-text` declares its own `[workspace]`, so its comparison dependencies stay out of the app lockfile. Its gates are run separately.
Decision: Recommend `parley` + `swash` for slide text. `cosmic-text` tied on line breaks but needed a spike-side family and nearest-weight lookup through `fontdb`.
Decision: The corpus adds empty and blank-verse cases beyond the prompt's list, and capture runs with software compositing so output is repeatable.
Open for 008: full-slide raster p95 is 25.28 ms against the 16 ms target.
Affects prompts: 008, 052.

## 2026-11-09 — 006 — Restore the browser component for spike 006

Decision: The operator confirmed the native app needs an embedded browser for the live web layer, web preview, Slido and PPTX to PDF, and that CEF is acceptable. This corrects the 2026-11-09 prompt 000 decision "Remove the dedicated web host", which is superseded for the CEF host. That decision's statement that the website remains a separate Next.js project still stands.
Decision: Spike 006 lives under `spikes/` with its own `[workspace]`. Adding `kairo-web-host` to the app workspace is left to prompts after the go/no-go at 008.
Affects prompts: 006, 008, 045, 047, 059, 073, 089, 101.

## 2026-11-09 — 007 — UI spike findings

Decision: Measurements used GPUI's `frame_duration_snapshot`, the source `gpui-fps` reads, rather than its on-screen HUD.
API difference: `KeyBinding::new` rejects `secondary-`, which only OS-aware parsing accepts.
Decision: The side-by-side uses an isolated Electron profile, to keep personal data out of committed evidence. It only reaches onboarding.
Recommendation for prompt 070: a focused rich text editor on `EntityInputHandler`, not Kit's `Editor` or `TextView`.
Open question for the owner: whether Low Power Mode should keep GPUI's 30 fps default.
Affects prompts: 008, 070.

API differences (006): `cef` 156.4.0 names getters without `get_` (`view_rect`, `audio_parameters`). `PdfPrintSettings` takes inches where Electron takes microns. `execute_java_script` has no isolated world.

## 2026-11-09 — 008 — Waive open spike items for the go/no-go

Decision: The owner chose to start prompt 008 with prompts 006 and 007 still `blocked`, accepting their open items as known risks. Their tracker status is unchanged, because the work is not complete.
Accepted for 006: no real Slido sign-in yet; subframe header removal not implemented; Windows and Linux not run.
Accepted for 007: 120 Hz not proven; the Low Power Mode frame-rate question remains open.
Carry forward: prompt 047 proves real sign-in; prompts 070 and 107 revisit UI frame pacing.

## 2026-11-09 — 008 — Go decision

Decision: **GO**, given by the owner, who accepted every recommendation below as presented. Prompts 006 and 007 remain `blocked` under the waiver above.

1. Minimum macOS: target 11, and test on real macOS 11 before release. Why: the binary declares 11.0, but no run on it exists.
2. Text stack: `parley` + `swash`. Why: 399/400 exact breaks and no font-matching workaround. Raster p95 of 25.28 ms misses 16 ms, so slide text is cached and rasterised on change.
3. Video: FFmpeg, dynamically linked LGPL build. H.264 and HEVC need legal review before release. Why: the soak passed; licensing is unsettled.
4. NDI: `libloading` over the official NDI 6 SDK. Why: 10 cycles passed. A real receiver is still unverified.
5. Web host: CEF as an on-demand signed component, offered during onboarding. Why: 148 MB zipped on macOS arm64, with its own security updates.
6. Zero-copy video: later, only if profiling shows a need.
7. Rich text: a focused editor on GPUI `EntityInputHandler` with marks as domain ranges. Not prototyped.
8. Linux: best-effort, Vulkan required, after macOS and Windows. Why: not run in any spike.
9. Intel Macs: remain supported until tested. Why: dropping them could strand existing churches.

Sync: run `tools/electron-sync` before each phase and before prompts 100, 109 and 112, as `SYNC.md` states.
Low Power Mode keeps GPUI's 30 fps default.
Conditions: confirm whether H.264 in the web layer matters to churches, since standard CEF cannot play it and an unplayable required format is a stop condition. Windows is untested across the spikes and is the largest remaining risk.
