# Prompt 033 — Bible lookup and search

**Phase:** 3. Services  
**Tracker ID:** `033-scripture-service`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Port offline Bible access and search.

## 2. Prerequisites

- Prompt [015](./015-domain-scripture.md) is marked `done`.
- Prompt [023](./023-sqlite-layer.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/main/services/scripture/index.ts` — service API
- `../kairo-presenter/src/main/services/scripture/bible-db.ts` — queries
- `../kairo-presenter/src/main/services/scripture/reference.ts` — parsing
- `../kairo-presenter/src/main/services/scripture/reference-search.ts` — search
- `../kairo-presenter/src/main/services/scripture/verse-bounds.ts` — bounds
- `../kairo-presenter/src/main/services/scripture/bible-verse-counts.ts` — counts
- `../kairo-presenter/src/main/services/scripture/bible-pack-versification.ts` — versification
- `../kairo-presenter/src/main/services/scripture/offline-bibles.ts` — startup
- `../kairo-presenter/src/main/services/scripture/local-bible-pack.ts` — packs
- `../kairo-presenter/src/main/services/scripture/local-bible-installer.ts` — installer
- `../kairo-presenter/src/main/services/scripture/__tests__/` — behaviours to port

## 4. Implementation requirements

1. Port the service. Results must match, including ordering, for a corpus of queries through the oracle.
2. Port pack download, verification and removal for NKJV, NLT and AMPC from `src/lib/bible-translations.ts`.
3. `setDefaultTranslation`, `initOfflineBibles` and `getTranslationOptions('')` (no network at bootstrap) must keep their startup behaviour.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- `kairo-scripture` lookup, tests.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Port test files relevant to these modules in `__tests__/`.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Search p95 under 10 ms. Report against baseline.md.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Inventory `scripture` lookup items done.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 034, 035, 078.
