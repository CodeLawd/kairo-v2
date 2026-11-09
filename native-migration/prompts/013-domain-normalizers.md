# Prompt 013 — Port settings and theme normalizers

**Phase:** 1. Domain model  
**Tracker ID:** `013-domain-normalizers`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Normalizers silently heal old data on every launch. Port them precisely, including their clamps and fallbacks.

## 2. Prerequisites

- Prompt [012](./012-domain-types.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/lib/normalize.ts` — primitives
- `../kairo-presenter/src/lib/overlay-defaults.ts` — overlay defaults and normalisation
- `../kairo-presenter/src/lib/overlay-elements.ts` — theme elements
- `../kairo-presenter/src/lib/overlay-boxes.ts` — box maths
- `../kairo-presenter/src/lib/overlay-shapes.ts` — shape catalog and SVG paths
- `../kairo-presenter/src/lib/overlay-outputs.ts` — output routing helpers
- `../kairo-presenter/src/lib/overlay-content.ts` — verse and reference formatting
- `../kairo-presenter/src/lib/theme-library.ts` — built-in themes and library rules
- `../kairo-presenter/src/lib/pp-retirement.ts` — ProPresenter data removal
- `../kairo-presenter/src/lib/output-settings.ts` — output settings
- `../kairo-presenter/src/lib/output-health.ts` — dispatch health
- `../kairo-presenter/src/lib/output-test-pattern.ts` — test pattern spec
- `../kairo-presenter/src/lib/displays.ts` — display logic
- `../kairo-presenter/src/lib/prop-geometry.ts` — prop geometry
- `../kairo-presenter/tests/overlay-boxes.test.ts` — expectations
- `../kairo-presenter/tests/overlay-elements.test.ts` — expectations
- `../kairo-presenter/tests/overlay-content.test.ts` — expectations
- `../kairo-presenter/tests/overlay-outputs.test.ts` — expectations
- `../kairo-presenter/tests/theme-library.test.ts` — expectations
- `../kairo-presenter/tests/pp-retirement.test.ts` — expectations
- `../kairo-presenter/tests/displays.test.ts` — expectations
- `../kairo-presenter/tests/output-health.test.ts` — expectations
- `../kairo-presenter/tests/output-test-pattern.test.ts` — expectations
- `../kairo-presenter/tests/prop-geometry.test.ts` — expectations
- `../kairo-presenter/tests/screen-settings.test.ts` — expectations

## 4. Implementation requirements

1. Port every export. Non-pure helpers should be flagged and handled in the owning service prompt.
2. Port the matching test files to Rust. Add `proptest` differential tests through the oracle for each normalizer with hostile inputs: wrong types, NaN strings, nulls, huge numbers, unknown enum values.
3. The output must be idempotent: normalising twice equals normalising once. Assert it.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- Replicate JavaScript numeric semantics where they affect results (`Math.round`, `Number('')`, `parseInt` prefix behaviour). Add helpers in `kairo-domain::js`, documented.
- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- Normalizers and tests.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Run 100,000 differential cases per normalizer.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Normalising a full settings file must take under 5 ms.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] No differential failure. Every export is ported or explicitly reassigned.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 021, 051.
