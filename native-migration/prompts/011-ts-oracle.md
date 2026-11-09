# Prompt 011 — Differential test harness against the TypeScript originals

**Phase:** 1. Domain model  
**Tracker ID:** `011-ts-oracle`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

The most reliable specification of 14,000 lines of pure logic is the code itself. Build a harness that runs the original TypeScript and the new Rust on the same inputs and compares the results.

## 2. Prerequisites

- Prompt [010](./010-api-inventory.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/lib/` — pure logic to be called by the oracle
- `../kairo-presenter/tests/fixtures.ts` — existing fixtures
- `../kairo-presenter/tests/helpers` — test helpers
- `../kairo-presenter/scripts/test-setup.cjs` — stubbing used by existing tests
- `../kairo-presenter/scripts/test-main.mjs` — how main-process tests run

## 4. Implementation requirements

1. In `tools/ts-oracle`, create a Bun or `tsx` runner which accepts JSON lines `{fn, args}` on stdin, calls the named export from `src/lib`, and prints JSON results, with `undefined` kept distinct from `null`, and thrown errors captured.
2. In Rust add a `kairo-oracle` dev-only helper: `assert_matches_oracle("module.fn", input, rust_output)` plus a seeded generator framework using `proptest`, and a mode that caches oracle outputs into `tests/oracle-cache/` so CI needs no Node.
3. Define how Rust JSON output is compared: key order independent, numeric tolerance only where stated, and `undefined` handling per serde rules agreed in this prompt.
4. Locate the Electron repo through `KAIRO_ELECTRON_DIR`, defaulting to `../kairo-presenter`, and use its installed dependencies. Stamp each cached oracle result with the Electron commit it came from.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- Node or Bun is a dev and CI tool only. The shipped app must never depend on it.
- Do not change `src/lib`.
- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- Oracle runner, Rust helper, documented cache refresh command.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Demonstrate on `overlay-boxes.ts` with 10,000 generated inputs.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Oracle round trip must allow at least 5,000 calls per second through batching.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Harness works locally and in CI from the cache.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 012 through 016, 035, 036.

## Amendment from prompt 009

`electron-sync --apply-generated` runs `cargo run --package ts-oracle -- --regenerate`. Build `tools/ts-oracle` to accept that flag and add it to the workspace.
