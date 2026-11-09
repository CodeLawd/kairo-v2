# Prompt 025 — AppCore, actors and event topics

**Phase:** 2. Persistence and runtime  
**Tracker ID:** `025-core-runtime`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Build the replacement for IPC: the typed API between UI and services.

## 2. Prerequisites

- Prompt [010](./010-api-inventory.md) is marked `done`.
- Prompt [021](./021-settings-store.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/main/ipc/index.ts` — handler behaviours, push fan-out, input validation
- `../kairo-presenter/src/main/bootstrap.ts` — startup resource loading
- `../kairo-presenter/src/main/__tests__/bootstrap.test.ts` — behaviours
- `../kairo-presenter/src/main/quit-confirmation.ts` — quit state machine
- `../kairo-presenter/src/main/__tests__/standalone-dispatch.test.ts` — behaviours
- `../kairo-presenter/src/lib/ipc-error.ts` — user-visible errors
- `../kairo-presenter/tests/bootstrap-contract.test.ts` — expectations
- `../kairo-presenter/tests/bootstrap-state.test.ts` — expectations
- `../kairo-presenter/tests/bootstrap-store.test.ts` — expectations
- `../kairo-presenter/tests/bootstrap-render.test.ts` — expectations
- `../kairo-presenter/tests/quit-confirmation.test.ts` — expectations
- `native-migration/03-native-architecture.md` — section 4

## 4. Implementation requirements

1. Implement `AppCore` in `kairo-core`, with a tokio runtime, `call`, `send`, `watch` and `stream` as in the architecture. Each stateful service is an actor with a bounded mailbox.
2. Define the shutdown sequence replicating `src/main/index.ts`: capture finish, audience shutdown, stop quote worker, NDI stop, output release with 2 s timeout.
3. Port `runBootstrap` with the same 8 s ceiling, neutral values and progress events.
4. Provide a GPUI bridge in `kairo-core-gpui`, converting topics into `Entity` updates, coalescing high-rate streams to display refresh, and cancelling when dropped.
5. Port the `resilience.ts` state machine and recovery file (see also prompt 039).

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- UI code must never see a raw channel or `tokio` type.
- Mailboxes are bounded. Document the overflow policy of each topic.
- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- `kairo-core` runtime, bridge, tests.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Actor cancellation and shutdown under load, with a stalled service. Bootstrap timeout tests matching the TypeScript tests.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Call round trip under 50 µs in-process. Idle CPU near zero with no polling timers.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] A sample service proves call, send, watch and stream from a GPUI view.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- All phase 3 and 5 prompts.
