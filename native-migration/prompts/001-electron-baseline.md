# Prompt 001 — Measure the Electron baseline

**Phase:** 0. Foundations and gating spikes  
**Tracker ID:** `001-electron-baseline`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Numbers from the shipping Electron app become the bar the native app must beat. Without them every performance claim later is opinion, and decision 008 shows the CPU baseline was never taken.

## 2. Prerequisites

- Prompt [000](./000-workspace-bootstrap.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/docs/decisions/008-gpu-frame-capture.md` — the unmeasured baseline matrix to fill where hardware allows
- `../kairo-presenter/src/main/services/scripture/trace.ts` — existing latency trace to reuse for speech-to-screen timing
- `../kairo-presenter/src/lib/scripture-trace.ts` — trace phases and their meaning
- `../kairo-presenter/tests/scripture-grid.bench.cjs` — existing UI benchmark
- `../kairo-presenter/package.json` — scripts for tests and builds

## 4. Implementation requirements

1. Run every existing suite and record pass and fail counts per suite: `bun run test`, `bun run test:main`, `bun run test:scripture-replay`, the native addon tests, and the `.integration.cjs` files you can run. Known environment failures must be listed, not hidden. Note that the working tree may contain the owner's uncommitted edits: never stash or alter them.
2. Write `native-migration/baseline.md` with a table of measurements for a packaged build where possible: cold start to interactive, idle RSS and CPU summed across all Kairo processes (with 0, 1, 2 and 4 outputs), page turn latency on document decks, slide push to frame on NDI, time to open the Lyrics library at 1,000 and 5,000 songs, scripture search latency, scrolling smoothness in the verse grid and media dock, video background CPU at 1080p, and installer plus installed size.
3. For each metric, record the method precisely so the native measurement in prompt 107 can repeat it. Add small measurement scripts under `tools/baseline/` only.
4. Metrics you cannot measure (no card attached, for example) must be written as `not measured` with the reason.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- Measurement only. Do not change Electron application code. Temporary logging must be reverted.
- Never invent a number or copy one from documentation.
- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- `native-migration/baseline.md` and scripts under `tools/baseline/`.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Repeat at least the startup and idle memory numbers three times and record median and spread.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- This prompt defines the numbers. Close other applications and record machine model, OS and display configuration.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] baseline.md covers every row above with a value or an explicit reason.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 107 compares against this file.
