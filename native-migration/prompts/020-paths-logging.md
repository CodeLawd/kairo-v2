# Prompt 020 — Paths, logging, crash handling and single instance

**Phase:** 2. Persistence and runtime  
**Tracker ID:** `020-paths-logging`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Establish where the app reads and writes, how it logs, and what happens when it fails.

## 2. Prerequisites

- Prompt [016](./016-domain-program.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/main/dev-profile.ts` — separate dev data folder
- `../kairo-presenter/src/main/db/legacy-store.ts` — rename from proautomate
- `../kairo-presenter/src/main/index.ts` — single instance, crash reporter, uncaught handlers
- `../kairo-presenter/src/main/env.d.ts` — build-time environment variables
- `../kairo-presenter/src/main/main-window.ts` — dialog parent window logic
- `../kairo-presenter/scripts/brand-dev-electron.mjs` — dev branding and its data-path effect
- `../kairo-presenter/.env.example` — build-time URL configuration
- `../kairo-presenter/src/main/services/workspace/index.ts` — documents-folder fallback to home

## 4. Implementation requirements

1. Compute the userData directory so the result is byte-for-byte the folder Electron uses for `app.setName("Kairo")` on each OS, and honour the dev profile rule. Write tests with expected paths per OS.
2. Set up `tracing` writing to the same logs folder as electron-log, with size rotation. Levels: info to file, and warn to console in release builds.
3. Install a panic hook that logs, serializes recovery state (see `resilience.ts`), and increments the crash ledger. Record crashes locally. Evaluate a minidump writer; the existing behaviour is local-only, and uploads go through `crash-reports` in prompt 032.
4. Single instance: use a lock plus a local socket so a second launch forwards `.kairo` paths. Dev builds are exempt, as today.
5. Replace `MAIN_VITE_API_URL`, `MAIN_VITE_WEB_URL`, `MAIN_VITE_AUDIENCE_API_URL` with build-time `option_env!` values, set by the release job. A release build with localhost URLs must fail to compile.
6. Map every `app.getPath` in use: userData, appData, logs, crashDumps, documents, home, videos, temp, exe. Honour `KAIRO_SHARED_PROFILE` from `dev-profile.ts`.
7. Handle `.kairo` paths from `process.argv` at launch on Windows and Linux, and `open-file` on macOS before the window exists. Also expose the OS locale, which usage reports send.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- Never log secrets. Reuse any existing redaction.
- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- `kairo-store::paths`, logging, panic hook, instance lock.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Unit tests for paths. An integration test launching two instances and verifying forward.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Logging must not block; use a non-blocking writer.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] A packaged-style build locates the user's real Electron data folder without writing to it.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 021, 022, 023.
