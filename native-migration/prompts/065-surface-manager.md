# Prompt 065 — Reconcile, routing, lobby playlists and program service

**Phase:** 4. Output engine  
**Tracker ID:** `065-surface-manager`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Port the brain that decides what each output shows.

## 2. Prerequisites

- Prompt [039](./039-orchestrator.md) is marked `done`.
- Prompt [058](./058-document-layer.md) is marked `done`.
- Prompt [060](./060-display-sinks.md) is marked `done`.
- Prompt [061](./061-ndi-sink.md) is marked `done`.
- Prompt [062](./062-decklink-sink.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/main/services/output/surface-manager.ts` — reconcile and all state
- `../kairo-presenter/src/main/services/output/program-surface.ts` — restore and recover flows
- `../kairo-presenter/src/main/services/output/program-service.ts` — program state
- `../kairo-presenter/src/main/services/output/lobby-player.ts` — per-screen playlists
- `../kairo-presenter/src/main/services/output/dispatch.ts` — dispatch
- `../kairo-presenter/src/main/services/output/gpu-capture.ts` — retired capture logic
- `../kairo-presenter/src/main/services/video-devices/__tests__/surface-manager.test.ts` — behaviours
- `../kairo-presenter/src/main/services/video-devices/__tests__/surface.test.ts` — behaviours
- `../kairo-presenter/tests/program-preview-sync.test.ts` — expectations
- `../kairo-presenter/tests/overlay-dispatch.test.ts` — expectations

## 4. Implementation requirements

1. Port as a core actor. Serialized reconcile; stable routing via `followsProgram`; lobbies isolated from the service; show filters per output; live state persistence during rebuild; the stage status topic with duplicate suppression.
2. Use `FrameCommitted` where code awaited `nextPaint`.
3. Every lobby and routing case in the listed tests must pass against a fake engine.
4. Display, device and stage reconciles behave as written: a slow card driver must not delay screens; a program screen outranks a stage display for the same display; every status reason string is kept. A re-matched display id is persisted and re-broadcast.
5. Lobby: images hold for `slideSec`, videos play once with loop off, poll 500 ms after a 1 s grace, advance after 30 minutes at most, retry every 5 s when empty or failing, and a repeated single video rewinds. Lobbies never receive service pushes, camera or web, and are never the audio surface.
6. Closing the main window on macOS closes screens and stages but keeps their content; reopening reconciles. Keep the presentation cache so pushes do not re-read settings.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- Actor, tests.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Port `surface-manager.test.ts` and `surface.test.ts`, with crash and replug scenarios on a fake engine, then on the real engine.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.
- Run `kairo-engine` for real and confirm output visually. Headless tests alone do not finish this prompt.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Reconcile with no change does no engine work.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Inventory `output`, `displays`, `videoDevices`, `program` items done.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 066, 075, 076.
