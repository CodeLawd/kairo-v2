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
