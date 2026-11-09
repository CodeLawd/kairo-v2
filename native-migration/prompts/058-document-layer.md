# Prompt 058 — Document pages and embedded video

**Phase:** 4. Output engine  
**Tracker ID:** `058-document-layer`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Reproduce fast page turns and document video.

## 2. Prerequisites

- Prompt [055](./055-video-engine.md) is marked `done`.
- Prompt [045](./045-documents-service.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/main/services/output/program-surface.ts` — showDocument, swapDocumentPage, preloadDocumentPage
- `../kairo-presenter/src/main/services/output/overlay.html` — swap and preload, setDocumentVideos, documentPlayback, control
- `../kairo-presenter/src/lib/documents.ts` — slide and playback types
- `../kairo-presenter/tests/document-page-swap.test.ts` — behaviour; untracked in the owner's tree, read only
- `../kairo-presenter/tests/documents-slideshow.test.ts` — expectations

## 4. Implementation requirements

1. Decode next and previous pages ahead, in GPU memory, bounded by an LRU. Page swap is one texture change plus `FrameCommitted`, without a full paint.
2. Non-matching output or wrong deck returns false, so the caller performs a full push, as today.
3. Embedded videos at their slide geometry, with commands and status.
4. Opaque, unblurred document frame rule.
5. Decode before swapping; a newer click supersedes an older request and returns false. Black opaque backing, contain fit. Embedded videos start hidden and paused at the clip start, are letterboxed to the page, stop at clip end, and report the two existing error strings. Commands are play, pause, restart, seek.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- Document layer, tests.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Replicate the swap tests. Rapid clicker input of 20 turns per second never shows a stale page last.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.
- Run `kairo-engine` for real and confirm output visually. Headless tests alone do not finish this prompt.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Clicker press to `FrameCommitted`: target under one frame at 60 Hz with preloaded page. Report against baseline.md.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Tests pass.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 065, 085.
