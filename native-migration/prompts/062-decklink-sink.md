# Prompt 062 — DeckLink output

**Phase:** 4. Output engine  
**Tracker ID:** `062-decklink-sink`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Connect the existing helper to the new engine, and leave it otherwise unchanged.

## 2. Prerequisites

- Prompt [005](./005-spike-sinks.md) is marked `done`.
- Prompt [051](./051-compositor-core.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/native/decklink/helper.cpp` — unchanged
- `../kairo-presenter/native/decklink/README.md` — qualification rules
- `../kairo-presenter/native/decklink/compatibility.h` — unchanged
- `../kairo-presenter/native/decklink/sdk-compatibility.h` — unchanged
- `../kairo-presenter/native/decklink/frame-input.h` — unchanged
- `../kairo-presenter/native/decklink/build.mjs` — build
- `../kairo-presenter/native/decklink/service-probe.ts` — probe
- `../kairo-presenter/native/decklink/stream-probe.ts` — probe
- `../kairo-presenter/native/decklink/tests/` — keep passing
- `../kairo-presenter/src/main/services/video-devices/index.ts` — service
- `../kairo-presenter/src/main/services/video-devices/provider.ts` — interface
- `../kairo-presenter/src/main/services/video-devices/provider-loader.ts` — loading
- `../kairo-presenter/src/main/services/video-devices/providers/decklink/client.ts` — CLI
- `../kairo-presenter/src/main/services/video-devices/providers/decklink/index.ts` — provider
- `../kairo-presenter/src/main/services/video-devices/providers/decklink/paths.ts` — paths
- `../kairo-presenter/src/main/services/video-devices/providers/decklink/playback-worker.ts` — retired worker
- `../kairo-presenter/src/main/services/video-devices/providers/decklink/worker-protocol.ts` — retired worker
- `../kairo-presenter/src/main/services/video-devices/providers/decklink/worker-sink.ts` — retired worker
- `../kairo-presenter/src/main/services/video-devices/__tests__/` — behaviours
- `../kairo-presenter/tests/video-device-ui.test.ts` — expectations
- `../kairo-presenter/tests/video-driver-notice.integration.cjs` — expectations
- `../kairo-presenter/docs/decisions/007-hardware-video-outputs.md` — open issues

## 4. Implementation requirements

1. Port the service, port ownership, label matching rules, status states, driver notice, and test patterns. Spawn the helper from the engine.
2. Render at the mode raster to UYVY. Write to the ring. Keep stdin BGRA as the fallback.
3. Close and quit rules: 2 s bounded release, unlink semantics, SIGKILL escalation.
4. Key/fill stays disabled, as today.
5. Test patterns are `bars`, `text` and `motion`. Keep `KAIRO_ENABLE_DECKLINK` until hardware qualification, and the driver-downloads link.
6. Copy `native/decklink` from the Electron repo into `native/decklink` here, including `prebuilt/` and its checksum, and record the commit in SYNC.md. Keep the helper's source and protocol the same as the copy Electron ships.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- Do not change helper protocol unless a defect forces it; log any change.
- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- Device service and sink, tests.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- All fake-provider tests ported. Real card: 10-minute run with zero late or dropped frames, if hardware is available.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.
- Run `kairo-engine` for real and confirm output visually. Headless tests alone do not finish this prompt.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Measure completed, late and dropped counts as decision 007 does.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Tests pass, with hardware results recorded or flagged.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 065, 110.
