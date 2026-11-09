# Prompt 039 — Orchestrator, voice navigation and recovery

**Phase:** 3. Services  
**Tracker ID:** `039-orchestrator`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Port the conductor that connects transcripts, detection and presentation.

## 2. Prerequisites

- Prompt [036](./036-detector-llm.md) is marked `done`.
- Prompt [037](./037-quote-index.md) is marked `done`.
- Prompt [038](./038-stt.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/main/orchestrator.ts` — all of it
- `../kairo-presenter/src/main/services/scripture/voice-navigation.ts` — commands
- `../kairo-presenter/src/main/services/scripture/live-plan.ts` — live plan
- `../kairo-presenter/src/main/services/scripture/sermon-plans.ts` — plans
- `../kairo-presenter/src/main/services/resilience.ts` — health and recovery
- `../kairo-presenter/src/main/services/output/dispatch.ts` — dispatch rules
- `../kairo-presenter/tests/scripture-voice-navigation.test.ts` — expectations
- `../kairo-presenter/tests/overlay-dispatch.test.ts` — expectations
- `../kairo-presenter/src/main/services/scripture/__tests__/sermon-plans.test.ts` — behaviour
- `../kairo-presenter/src/main/services/scripture/__tests__/live-detection-wiring.test.ts` — behaviour

## 4. Implementation requirements

1. Port as a single actor. Auto-present timers, dedupe release on dismiss, session stats, cost accounting and fallback destinations must behave identically.
2. Dispatch goes through an `EngineClient` trait with a fake for tests until phase 4 exists.
3. Session recovery keeps `session-recovery.json` format and 30 s cadence.
4. Starting needs a licence and an active service (`Create a service in Operator before starting transcription.`). Credentials from any caller are ignored. Restore also needs an active service.
5. Port `presentLyricSlide` with its next-slide data, `presentDocumentPage`, `presentLiveBackground`, `testOverlay`, `clearText`, `clearOverlay` and `retranslateSuggestion`. Port the resilience poll (10 s) and fallback state.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- Orchestrator, tests.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- End-to-end on a virtual clock: replay corpus into a fake STT, assert the sequence of EngineClient calls matches the TypeScript version.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Speech-final to `ShowSlide` command, excluding the operator-chosen delay, measured with traces.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Inventory `orchestrator` and `resilience` items done.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 065, 077.
