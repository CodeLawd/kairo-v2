# Prompt 005 — Spike: NDI, DeckLink ring and macOS native code from Rust

**Phase:** 0. Foundations and gating spikes  
**Tracker ID:** `005-spike-sinks`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Prove that the expensive native integrations Kairo already owns can be reached from Rust with no Node runtime.

## 2. Prerequisites

- Prompt [000](./000-workspace-bootstrap.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/docs/decisions/001-cross-platform-ndi.md` — acceptance contract and redistribution duties
- `../kairo-presenter/src/main/services/ndi/provider.ts` — NDI operations in use
- `../kairo-presenter/src/main/services/ndi/provider-loader.ts` — load order and failure handling
- `../kairo-presenter/native/gpu-frames/ring.h` — shared memory layout and seqlock
- `../kairo-presenter/native/gpu-frames/gpu_frames.mm` — Metal converter
- `../kairo-presenter/native/decklink/helper.cpp` — `--play-shm` protocol
- `../kairo-presenter/native/decklink/uyvy.h` — colour reference
- `../kairo-presenter/native/syphon/syphon.mm` — Syphon server
- `../kairo-presenter/native/recorder/recorder.mm` — AVAssetWriter recorder
- `../kairo-presenter/src/main/services/video-devices/providers/decklink/client.ts` — helper CLI protocol

## 4. Implementation requirements

1. NDI: compare a thin `libloading` binding over the official NDI 6 SDK headers against `grafton-ndi`. Requirements: dynamic load from the app folder with no global install, UYVA and BGRA video with line stride, FLTp audio, source discovery, explicit destroy, and 10 start-stop cycles. Choose one.
2. Send a UYVA test image, verified with NDI Studio Monitor or OBS if available. If no receiver is available, state that.
3. DeckLink: from Rust, create the POSIX ring with the exact `ring.h` layout, start `decklink-helper --play-shm`, and write frames. If no card is attached, test against a stub reader built from `ring.h`, and say that hardware is unverified.
4. Native: prove the strategy for `gpu_frames.mm`, `syphon.mm` and `recorder.mm`: split out the `napi_*` shim into a C ABI, built by `cc` in `kairo-native-sys`, leaving the Metal and AV logic untouched. Do this for one addon (`gpu_frames`) in a copy under `spikes/`, and run its golden comparison against `uyvy.h`.
5. Report how wgpu Metal textures can be exposed as IOSurfaces (IOSurface-backed render target created through Metal, then wrapped for wgpu) or why readback must be used instead.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- Do not modify files under `native/` in this prompt. Copy into the spike.
- Respect decision 001: proprietary SDK payloads must never be committed.
- Reused native sources are copied into this repository under `native/`, each with the Electron commit it came from recorded in `native-migration/SYNC.md`. The release build must not depend on the sibling checkout.
- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.
- Spike code never merges into app crates. Its value is the written report.

## 6. Expected deliverables

- Spike and `native-migration/spikes/005-sinks.md` with one chosen NDI approach.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Golden colour vectors for black, white and Rec.709 red must match `uyvy.h` byte for byte.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Record 1080p UYVA conversion latency against the p50 0.6 ms in decision 008.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Each path has PASS, FAIL or a clearly labelled hardware-unverified status.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 008, 061, 062, 063, 064.
