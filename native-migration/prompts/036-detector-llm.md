# Prompt 036 — Scripture detector, LLM path and fallback

**Phase:** 3. Services  
**Tracker ID:** `036-detector-llm`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Port LLM detection, fallback mode and cost accounting.

## 2. Prerequisites

- Prompt [035](./035-detector-rules.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/main/services/scripture/detector.ts` — LLM section
- `../kairo-presenter/src/main/services/scripture/__tests__/fast-path-trace.test.ts` — trace behaviour
- `../kairo-presenter/src/main/services/scripture/__tests__/detection-translation-fallback.test.ts` — behaviour
- `../kairo-presenter/src/main/services/scripture/transcript-recording.ts` — recordings
- `../kairo-presenter/src/main/services/scripture/trace.ts` — tracing
- `../kairo-presenter/src/main/services/cloud/managed.ts` — managed key routing

## 4. Implementation requirements

1. Replace the OpenAI and Anthropic SDKs with `reqwest` calls through the managed gateway. There is no direct provider connection and no key: port `managedFetch`, which sends the path and JSON body to `/v1/managed/request`, deletes `stream`, carries `purpose`, honours abort, and turns gateway failures into the same user-visible errors (not activated, minutes used up, Kairo unreachable). Capture real gateway request bodies from Electron for comparison.
2. Prompts and model selection stay byte-identical, including `llmModel` semantics. Cancellation and timeouts must match.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- LLM path, mocks, tests.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Replay with recorded LLM responses, confirming identical suggestion sequences.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- An in-flight LLM call must never delay the regex fast path.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Replay differential passes with mocks; a live smoke test passes if keys are available.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 039.
