# Prompt 014 — Port lyrics logic

**Phase:** 1. Domain model  
**Tracker ID:** `014-domain-lyrics`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Port all pure song logic, which encodes years of operator-driven detail.

## 2. Prerequisites

- Prompt [012](./012-domain-types.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/lib/lyrics-clipboard.ts` — clipboard parsing
- `../kairo-presenter/src/lib/lyrics-duplicate.ts` — duplicate detection
- `../kairo-presenter/src/lib/lyrics-hotkeys.ts` — hotkeys
- `../kairo-presenter/src/lib/lyrics-online-error.ts` — errors
- `../kairo-presenter/src/lib/lyrics-query.ts` — search query
- `../kairo-presenter/src/lib/lyrics-reflow.ts` — reflow
- `../kairo-presenter/src/lib/lyrics-reorder.ts` — reordering
- `../kairo-presenter/src/lib/lyrics-section-edit.ts` — section edits
- `../kairo-presenter/src/lib/lyrics-section-json.ts` — section JSON
- `../kairo-presenter/src/lib/lyrics-slides.ts` — slide splitting
- `../kairo-presenter/src/lib/lyrics-style.ts` — style
- `../kairo-presenter/src/lib/lyrics-translate.ts` — translation helpers
- `../kairo-presenter/src/lib/rtf-text.ts` — RTF
- `../kairo-presenter/tests/lyrics-clipboard.test.ts` — expectations
- `../kairo-presenter/tests/lyrics-duplicate.test.ts` — expectations
- `../kairo-presenter/tests/lyrics-hotkeys.test.ts` — expectations
- `../kairo-presenter/tests/lyrics-reflow.test.ts` — expectations
- `../kairo-presenter/tests/lyrics-reorder.test.ts` — expectations
- `../kairo-presenter/tests/lyrics-section-edit.test.ts` — expectations
- `../kairo-presenter/tests/lyrics-slides.test.ts` — expectations
- `../kairo-presenter/tests/lyrics-style.test.ts` — expectations
- `../kairo-presenter/tests/lyrics-query.test.ts` — expectations
- `../kairo-presenter/tests/lyrics-translate.test.ts` — expectations
- `../kairo-presenter/tests/lyrics-usr-title.test.ts` — expectations
- `../kairo-presenter/tests/lyrics-multilingual-search.test.ts` — expectations
- `../kairo-presenter/tests/rtf-text.test.ts` — expectations
- `../kairo-presenter/tests/workspace-song-file.test.ts` — expectations

## 4. Implementation requirements

1. Port each module and its tests into `kairo-domain::lyrics`.
2. Text handling must operate as JavaScript does where offsets are exposed. Identify every use of string index or `.length` and determine whether UTF-16 semantics matter. Use tests with Yoruba combining marks and emoji.
3. Regexes: JavaScript and Rust regex engines differ on lookaround and backreferences. Use `fancy-regex` only where required, and record which patterns need it.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- `kairo-domain::lyrics`, tests.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Differential tests on generated lyric text, including Unicode edge cases.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Splitting a 200-line song into slides under 1 ms.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] No differential failures and all test files ported.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 040, 081, 082.
