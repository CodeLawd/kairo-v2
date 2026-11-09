# Prompt 034 — API.Bible, offline policy and download manager

**Phase:** 3. Services  
**Tracker ID:** `034-api-bible`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Port licensed online Bible access and its strict offline policy.

## 2. Prerequisites

- Prompt [022](./022-secure-storage.md) is marked `done`.
- Prompt [033](./033-scripture-service.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/main/services/scripture/api-bible.ts` — API
- `../kairo-presenter/src/main/services/scripture/api-bible-client.ts` — client
- `../kairo-presenter/src/main/services/scripture/api-bible-cache.ts` — encrypted cache
- `../kairo-presenter/src/main/services/scripture/api-bible-lookup.ts` — lookup routing
- `../kairo-presenter/src/main/services/scripture/api-bible-download-manager.ts` — queue, pause, resume
- `../kairo-presenter/src/main/services/scripture/api-cache-policy.ts` — policy
- `../kairo-presenter/src/main/services/scripture/offline-download-policy.ts` — licence gate
- `../kairo-presenter/src/main/services/scripture/detection-lookup.ts` — fallback lookup

## 4. Implementation requirements

1. Keep the rule: an API key alone never permits whole-translation download.
2. Downloads can pause, resume, survive restart and respect rate limits, as today.
3. Requests use the managed gateway. Every id is checked with `ensureAuthorizedBibleId`, except removal, which validates against the cache so it still works after access is revoked. Keep `bible_download` tracking.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- Cached text is never plaintext. Fail closed.
- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- Service and tests, ported from the matching `__tests__` files.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Mock API for pagination, 429 and key change (see scripture-api-key-change test).

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Downloads must not affect live detection; throttle while a service is live, if Electron does.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Inventory items done.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 036.
