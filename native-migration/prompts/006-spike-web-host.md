# Prompt 006 — Spike: CEF web host for all browser needs

**Phase:** 0. Foundations and gating spikes  
**Tracker ID:** `006-spike-web-host`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Kairo needs a real browser for four things: live web layer, web preview, Slido and PPTX to PDF. Confirm `kairo-web-host` built on CEF can do them all, and measure its cost.

## 2. Prerequisites

- Prompt [000](./000-workspace-bootstrap.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/main/services/web/index.ts` — framing header stripping, mute script, scroll mirror
- `../kairo-presenter/src/lib/web.ts` — frames, addresses, messages
- `../kairo-presenter/src/renderer/src/components/operator/ScaledWebFrame.tsx` — preview scaling
- `../kairo-presenter/src/main/services/audience/sources/slido.ts` — navigation rules, persistent partition, window behaviour
- `../kairo-presenter/src/main/services/audience/sources/slido-selectors.ts` — scraper
- `../kairo-presenter/src/preload/audience-slido.ts` — page bridge to port as an injected script
- `../kairo-presenter/src/main/services/documents/pptx-builtin.ts` — printToPDF options

## 4. Implementation requirements

1. Using the `cef` crate (tauri-apps/cef-rs; verify the current version), build a host with windowless rendering at 1920×1080 that streams frames to a separate wgpu process. Use accelerated shared textures where the crate exposes them, and CPU paint buffers if not. Report which.
2. Prove: remove `X-Frame-Options` and `frame-ancestors` for the program page only; forward mouse wheel, pointer and click input; capture page audio; reload; navigation events; report crashes.
3. Prove a visible window with persistent profile, injected init script, navigation allowlist and permission denial, sufficient for Slido sign-in.
4. Prove `PrintToPDF` with the options from `pptx-builtin.ts`, on HTML from that file's generator.
5. Measure the host's on-disk size per platform, RSS idle and with a page playing video. Report macOS bundle and signing structure, including the helper apps that CEF requires.
6. Recommend bundled, or on-demand signed component (mirroring `documents/slide-converter.ts`). Describe the degraded experience when absent.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- Throwaway code under `spikes/`.
- Do not use `gpui-webview`; on Linux it forces X11 for the entire application.
- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.
- Spike code never merges into app crates. Its value is the written report.

## 6. Expected deliverables

- Spike and `native-migration/spikes/006-web-host.md`.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- 10-minute page with video playing (owner limit: no run longer than 10 minutes); log frame cadence, RSS and clean shutdown of all helpers.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Target: web-layer frames reach the compositor at 30 fps at 1080p with under 50 ms added latency.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] All four uses marked PASS or FAIL on at least macOS. Other platforms stated.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 008, 047, 045, 059.
