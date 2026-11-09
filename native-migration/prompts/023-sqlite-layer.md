# Prompt 023 — SQLite access layer

**Phase:** 2. Persistence and runtime  
**Tracker ID:** `023-sqlite-layer`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Create a concurrency-safe database layer for the existing schemas.

## 2. Prerequisites

- Prompt [020](./020-paths-logging.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/main/services/lyrics/index.ts` — schema constants and pragmas
- `../kairo-presenter/src/main/services/scripture/bible-db.ts` — schema, FTS and read patterns
- `../kairo-presenter/src/main/services/scripture/api-bible-cache.ts` — cache schema
- `../kairo-presenter/src/main/services/scripture/cache-quote-index.ts` — in-memory quote FTS
- `../kairo-presenter/src/main/services/scripture/local-bible-pack.ts` — read-only external packs
- `../kairo-presenter/scripts/seed-bible.ts` — how `resources/bible.db` is produced
- `../kairo-presenter/scripts/build-bible-pack.ts` — pack production
- `../kairo-presenter/scripts/import-translation.ts` — import tool
- `../kairo-presenter/resources/BIBLE_SOURCES.md` — data provenance

## 4. Implementation requirements

1. Use `rusqlite` with bundled SQLite and FTS5. The tokenizer configuration must match the FTS tables already built; verify with the bundled `bible.db`.
2. Provide a writer thread and reader pool per database, reached through bounded async calls.
3. Handle: first-run copy of bundled `resources/bible.db` to writable `<userData>/bible.db` (an existing copy is never replaced, because local packs install into it; see `resolveBibleDbPath` in `orchestrator.ts` and `local-bible-pack.ts`), read-only external packs without WAL side files, pack integrity validation, WAL recovery after a crash.
4. Keep the seed and build tools in TypeScript for now. They run in development only.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- No SQL on the UI thread or tokio workers.
- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- `kairo-store::db`, tests.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Open a real `resources/bible.db` and compare 1,000 FTS queries to better-sqlite3 results through the oracle.
- Concurrent readers during a writer, for 60 seconds, without `SQLITE_BUSY` surfacing.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Point verse lookup p99 under 1 ms.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Existing database files open and return identical results.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 033, 040.
