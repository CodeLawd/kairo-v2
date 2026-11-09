# Prompt 040 — Lyrics library and search

**Phase:** 3. Services  
**Tracker ID:** `040-lyrics-service`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Port the local song library.

## 2. Prerequisites

- Prompt [014](./014-domain-lyrics.md) is marked `done`.
- Prompt [023](./023-sqlite-layer.md) is marked `done`.
- Prompt [024](./024-workspace.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/main/services/lyrics/index.ts` — service, FTS, import paths
- `../kairo-presenter/src/main/services/lyrics/normalize.ts` — normalisation
- `../kairo-presenter/src/main/services/lyrics/review.ts` — review markers
- `../kairo-presenter/src/main/services/lyrics/section-label.ts` — labels
- `../kairo-presenter/src/main/services/lyrics/page-title.ts` — titles
- `../kairo-presenter/src/main/services/lyrics/provider-types.ts` — provider types
- `../kairo-presenter/src/main/services/lyrics/song-files.ts` — files
- `../kairo-presenter/tests/lyrics-review.test.ts` — expectations
- `../kairo-presenter/tests/lyrics-section-label.test.ts` — expectations
- `../kairo-presenter/tests/lyrics-preview-speed.test.ts` — expectations

## 4. Implementation requirements

1. Port library CRUD, FTS with the same ranking, `syncFromFolder` semantics (deleted file removes the song; dropped-in file adds one), favourites, and OpenLyrics and text import.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- Service, tests.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Search result differential for 1,000 queries on a generated 5,000-song library.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Open library at 5,000 songs under baseline.md. Search under 20 ms.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Inventory items done.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 041, 080.
