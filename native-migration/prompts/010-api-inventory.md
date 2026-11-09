# Prompt 010 — Freeze the application API inventory

**Phase:** 1. Domain model  
**Tracker ID:** `010-api-inventory`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Convert the Electron IPC surface into a checked list so no call can be forgotten. Every later service and UI prompt ticks items off here.

## 2. Prerequisites

- Prompt [008](./008-go-no-go.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/lib/ipc.ts` — `IPC` channel registry and API interfaces
- `../kairo-presenter/src/lib/program.ts` — `PROGRAM` channels and ProgramAPI
- `../kairo-presenter/src/preload/index.ts` — the `KairoAPI` object as actually exposed
- `../kairo-presenter/src/preload/program.ts` — program-window bridge
- `../kairo-presenter/src/main/ipc/index.ts` — all handlers and which service each calls
- `../kairo-presenter/src/renderer/src/env.d.ts` — renderer typing of the API
- `../kairo-presenter/src/lib/ipc-error.ts` — how errors cross the boundary

## 4. Implementation requirements

1. Write a script under `tools/ts-oracle/` that parses these files and generates `native-migration/api-inventory.md`, listing each call: namespace, method, channel, direction (invoke, send, push), argument and result type names, handler location, and calling renderer components.
2. Add a `native status` column with values `todo`, `done` and the owning prompt. Add `tools/check-api-coverage`, which fails if a call is unassigned.
3. Identify channels defined but never used, and handlers never reached; list them for review, and do not port dead code silently.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- Generated output must be reproducible. Do not hand-edit the generated columns.
- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- `api-inventory.md`, the generator and checker.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Generator count must reconcile with the audit: 122 `ipcMain.handle` calls in `src/main/ipc/index.ts` plus those registered elsewhere. Explain any difference.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Not applicable.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Every call is listed and has an owning prompt number.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 025 and every service and UI prompt update this file.

## Amendment from prompt 009

`electron-sync --apply-generated` runs `cargo run --package check-api-coverage -- --regenerate`. Build `tools/check-api-coverage` to accept that flag and add it to the workspace.
