# Prompt 035 — Scripture detector, deterministic path

**Phase:** 3. Services  
**Tracker ID:** `035-detector-rules`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Port the rule-based portion of the 2,188-line detector, proven against the corpus.

## 2. Prerequisites

- Prompt [011](./011-ts-oracle.md) is marked `done`.
- Prompt [015](./015-domain-scripture.md) is marked `done`.
- Prompt [033](./033-scripture-service.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/main/services/scripture/detector.ts` — the source
- `../kairo-presenter/src/main/services/scripture/quote-recovery.ts` — recovery
- `../kairo-presenter/src/main/services/scripture/scripture-signal.ts` — signals
- `../kairo-presenter/src/main/services/scripture/live-detection-wiring.ts` — wiring
- `../kairo-presenter/src/main/services/scripture/__tests__/detector-live.test.ts` — behaviours
- `../kairo-presenter/src/main/services/scripture/__tests__/detection-gaps.test.ts` — behaviours
- `../kairo-presenter/src/main/services/scripture/__tests__/transcript-replay.test.ts` — replay
- `../kairo-presenter/tests/scripture-direct-detection.test.ts` — expectations
- `../kairo-presenter/tests/scripture-signal.test.ts` — expectations
- `../kairo-presenter/tests/fixtures/scripture-replay/quotation-corpus.json` — corpus
- `../kairo-presenter/scripts/replay-scripture.ts` — replay tool
- `../kairo-presenter/src/main/services/scripture/replay-report.ts` — reports
- `../kairo-presenter/tests/scripture-replay-report.test.ts` — expectations

## 4. Implementation requirements

1. Start by writing a section map of `detector.ts` into `DECISIONS.md`: state, regex tables, fast path, debounce, dedupe, LLM boundary.
2. Port the deterministic path. Keep the same event sequence shape: interim and final inputs, with timing.
3. Create a differential replay: drive TypeScript and Rust with each corpus case plus generated transcripts on a virtual clock, and compare emitted references, order and timing buckets.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- Regex translation needs a review pass. Record any pattern changed.
- Do not tune or improve detection during the port. Differences are bugs.
- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- Rules in `kairo-scripture`, replay comparison tool.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- The whole corpus, plus the ported `__tests__` for these modules.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- p99 processing per transcript event under 2 ms. Compare the fast-path trace.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Zero differences on the corpus and generated transcripts, or each one is listed and approved in DECISIONS.md.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 036, 037.
