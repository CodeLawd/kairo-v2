# Prompt 050 — Engine process, protocol and supervision

**Phase:** 4. Output engine  
**Tracker ID:** `050-engine-scaffold`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Create the production `kairo-engine` process and the versioned protocol between it and `kairo`.

## 2. Prerequisites

- Prompt [008](./008-go-no-go.md) is marked `done`.
- Prompt [025](./025-core-runtime.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `native-migration/03-native-architecture.md` — sections 2 and 5
- `native-migration/spikes/002-engine-process.md` — proven approach
- `../kairo-presenter/src/main/services/output/managed-window.ts` — failure budget and lifecycle
- `../kairo-presenter/src/main/services/output/program-surface.ts` — state that must survive a rebuild

## 4. Implementation requirements

1. Define `kairo-engine-proto` with postcard or bincode framing, a version handshake with a token, and every command in architecture section 5.3. Include `FrameCommitted`, health, stats, errors.
2. Build `EngineClient` in `kairo-core`, implementing the trait prompt 039 introduced.
3. Supervision: launch, heartbeats, relaunch with state replay, failure budget equal to `allowFailure`, and UI-death grace behaviour.
4. The engine runs `winit` on its main thread, with no business logic. It may not read settings, the network or the database.
5. Implement a shm ring in `kairo-engine-proto` compatible in layout with `native/gpu-frames/ring.h`.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- Protocol structs are the only shared code between processes, apart from `kairo-scene`.
- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- Engine binary that renders black, protocol crate, client, supervisor.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Fuzz the decoder with malformed frames. Kill the engine during a command storm; confirm state replay. Version mismatch is rejected with a clear log entry.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.
- Run `kairo-engine` for real and confirm output visually. Headless tests alone do not finish this prompt.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Command to engine under 1 ms p99.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Supervision tests pass.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 051 and all engine prompts.
