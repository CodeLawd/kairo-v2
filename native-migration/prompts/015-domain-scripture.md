# Prompt 015 — Port scripture logic

**Phase:** 1. Domain model  
**Tracker ID:** `015-domain-scripture`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Port reference parsing, query logic, sermon-plan logic and the live progress model.

## 2. Prerequisites

- Prompt [012](./012-domain-types.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/lib/bible-translations.ts` — translation registry
- `../kairo-presenter/src/lib/bible-versification.ts` — versification
- `../kairo-presenter/src/lib/scripture-query.ts` — search parsing
- `../kairo-presenter/src/lib/scripture-live-progress.ts` — live progress
- `../kairo-presenter/src/lib/scripture-offline-state.ts` — offline state
- `../kairo-presenter/src/lib/scripture-tab-presentation.ts` — tab presentation
- `../kairo-presenter/src/lib/scripture-trace.ts` — trace format
- `../kairo-presenter/src/lib/scripture-trace-store.ts` — trace store
- `../kairo-presenter/src/lib/sermon-notes-review.ts` — notes review
- `../kairo-presenter/src/lib/sermon-plan-match.ts` — plan matching
- `../kairo-presenter/src/lib/sermon-plan-resolve.ts` — plan resolving
- `../kairo-presenter/src/lib/single-verse-presentation.ts` — presentation rule
- `../kairo-presenter/src/lib/spoken-translations.ts` — spoken translation names
- `../kairo-presenter/src/lib/verse-text.ts` — verse text
- `../kairo-presenter/src/lib/detector-recovery.ts` — recovery state machine
- `../kairo-presenter/tests/scripture-query.test.ts` — expectations
- `../kairo-presenter/tests/scripture-live-progress.test.ts` — expectations
- `../kairo-presenter/tests/sermon-notes-review.test.ts` — expectations
- `../kairo-presenter/tests/sermon-plan-match.test.ts` — expectations
- `../kairo-presenter/tests/sermon-plan-resolve.test.ts` — expectations
- `../kairo-presenter/tests/spoken-translations.test.ts` — expectations
- `../kairo-presenter/tests/single-verse-presentation.test.ts` — expectations
- `../kairo-presenter/tests/scripture-trace.test.ts` — expectations
- `../kairo-presenter/tests/detector-recovery.test.ts` — expectations
- `../kairo-presenter/tests/scripture-tab-presentation.test.ts` — expectations
- `../kairo-presenter/tests/scripture-initial-state.test.ts` — expectations
- `../kairo-presenter/tests/api-bible-offline-state.test.ts` — expectations
- `../kairo-presenter/tests/offline-bible-view-model.test.ts` — expectations

## 4. Implementation requirements

1. Port each module and test to `kairo-domain::scripture`.
2. The trace format must remain compatible, so the existing scripture latency analysis still applies.
3. `sermon-notes-review.ts` references the Tiptap document shape. Keep the data model separate from any editor; prompt 079 decides the view.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- `kairo-domain::scripture`, tests.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Differential tests over every book name, abbreviation and malformed references.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Reference parse under 20 µs.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] All listed modules ported with zero differential failures.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 033, 035.
