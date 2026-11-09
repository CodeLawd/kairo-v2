# Prompt 022 — Secure storage and Electron secret migration

**Phase:** 2. Persistence and runtime  
**Tracker ID:** `022-secure-storage`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Preserve sign-in and licensed-text cache encryption while leaving Chromium's `safeStorage`.

## 2. Prerequisites

- Prompt [020](./020-paths-logging.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/main/services/cloud/secure-store.ts` — fails open
- `../kairo-presenter/src/main/services/cloud/__tests__/secure-store.test.ts` — behaviours
- `../kairo-presenter/src/main/services/scripture/api-cache-crypto.ts` — fails closed
- `../kairo-presenter/src/main/services/scripture/__tests__/api-cache-crypto.test.ts` — behaviours
- `../kairo-presenter/src/renderer/src/lib/credentials-notice.ts` — user notice
- `../kairo-presenter/src/renderer/src/components/layout/KeychainNotice.tsx` — notice UI

## 4. Implementation requirements

1. Implement a `KeyProtector` over `keyring`, and the AES-256-GCM format, byte-compatible with both existing stores.
2. Migration. Research and document Chromium's OS-crypt format for the Electron version in `package.json` per platform: macOS uses the "Safe Storage" keychain item; Windows uses DPAPI with a key in Local State; Linux depends on the backend. On first run, try to unwrap the old data key and re-wrap it with the new protector.
3. If migration fails, the behaviour must be the existing one: refresh token forgotten, so the operator signs in again, and API.Bible cache discarded and rebuilt. No crash, and show the existing keychain notice.
4. Test keychain-access behaviour with a differently signed binary. A prompt asking the user to allow access must be expected and documented.
5. Provider keys are already retired: `cloudSession.start()` blanks any left in settings, and `settings:set` always writes them empty. Port that retirement. Do not reintroduce stored provider keys anywhere.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- Keep fail-open and fail-closed exactly as they are today.
- Never print key material.
- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- `kairo-store::secrets`, tests, migration note in DECISIONS.md.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Encrypt in TypeScript, decrypt in Rust, and reverse.
- Run migration against a real macOS Electron profile created for the test, not the owner's.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Not on hot paths.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] The token and cache survive the move on macOS, with a stated result on Windows and Linux.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 030, 034.
