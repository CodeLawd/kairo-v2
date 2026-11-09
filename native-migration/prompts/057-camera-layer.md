# Prompt 057 — Live camera layer

**Phase:** 4. Output engine  
**Tracker ID:** `057-camera-layer`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Put cameras and capture cards live.

## 2. Prerequisites

- Prompt [051](./051-compositor-core.md) is marked `done`.
- Prompt [056](./056-audio-engine.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/main/services/output/overlay.html` — setCamera and constraint fallback at line 1070 onward
- `../kairo-presenter/src/renderer/src/components/operator/ProgramPanels.tsx` — enumeration, permission, labels
- `../kairo-presenter/src/renderer/src/components/operator/VideoLayoutCanvas.tsx` — layout editing
- `../kairo-presenter/src/main/services/output/program-surface.ts` — CameraLayer

## 4. Implementation requirements

1. Evaluate `nokhwa` against direct AVFoundation, Media Foundation and V4L2. Must expose stable human-readable labels that match those Chromium reported, because they are saved in settings, and support common capture-card resolutions. Choose and record.
2. Convert to textures at low latency. Handle unplug, busy device and denied permission with the same operator-facing states. Pair optional audio input.
3. Request 1920×1080 at 30 fps ideal, cover fit on black. Camera sound has echo cancellation, noise suppression and gain control off. A camera change during open must discard the stale stream.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- `kairo-camera`, tests with a synthetic source.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Real webcam on macOS. Capture card if present, else state unverified.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.
- Run `kairo-engine` for real and confirm output visually. Headless tests alone do not finish this prompt.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Glass-to-output latency measured and compared with Electron, using a timecode display and a second camera.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Saved camera labels from Electron resolve correctly.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 077.
