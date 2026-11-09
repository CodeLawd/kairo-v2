# Prompt 080 — Lyrics library and quick open

**Phase:** 5. Operator interface  
**Tracker ID:** `080-lyrics-library-ui`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

First of three prompts rebuilding the 4,061-line lyrics view. Begin by separating model from view.

## 2. Prerequisites

- Prompt [078](./078-scripture-ui.md) is marked `done`.
- Prompt [040](./040-lyrics-service.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/renderer/src/components/lyrics/Lyrics.tsx` — read all; extract state map first
- `../kairo-presenter/src/renderer/src/components/lyrics/LibraryRail.tsx` — libraries
- `../kairo-presenter/src/renderer/src/components/lyrics/SongQuickOpen.tsx` — quick open
- `../kairo-presenter/src/renderer/src/components/lyrics/SongUsageModal.tsx` — usage
- `../kairo-presenter/src/renderer/src/components/lyrics/LyricsReviewNotice.tsx` — notice
- `../kairo-presenter/src/renderer/src/components/lyrics/section-colors.ts` — section colours
- `../kairo-presenter/src/renderer/src/stores/useLibraries.ts` — state
- `../kairo-presenter/src/renderer/src/stores/useSetlist.ts` — state

## 4. Implementation requirements

1. Write the state and event map of `Lyrics.tsx` to `native-migration/notes/lyrics-state-map.md`, then build `LyricsModel` entities.
2. Build library rail, song list, search, favourites, setlists, quick open, online-fallback entry point.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.
- Before UI work, read the GPUI Kit `gpui-kit` skill's Coding Guides and `gpui-kit-design-guides`, then `DESIGN.md`. Kairo's `DESIGN.md` wins on appearance. No raw colours outside `kairo-ui-kit`.

## 6. Expected deliverables

- `ui-lyrics` library portion and the state map.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Virtualisation at 5,000 songs; keyboard flows.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.
- Run the app (`cargo run -p kairo`), exercise the feature by hand, compare it with the Electron app side by side, and save screenshots to `native-migration/evidence/<prompt>/`.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Compare to baseline.md.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] State map reviewed against source, and library flows pass.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 081.
