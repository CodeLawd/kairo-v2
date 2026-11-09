# Prompt 083 — Media dock and inspector

**Phase:** 5. Operator interface  
**Tracker ID:** `083-media-ui`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Build the media dock.

## 2. Prerequisites

- Prompt [082](./082-lyrics-editors-ui.md) is marked `done`.
- Prompt [043](./043-media-service.md) is marked `done`.
- Prompt [055](./055-video-engine.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/renderer/src/components/media/MediaDock.tsx` — dock
- `../kairo-presenter/src/renderer/src/components/media/MediaInspector.tsx` — inspector
- `../kairo-presenter/src/renderer/src/components/media/TrimTimeline.tsx` — trim
- `../kairo-presenter/src/renderer/src/components/media/ProgramAudioFallback.tsx` — fallback
- `../kairo-presenter/src/renderer/src/stores/useMediaDockStore.ts` — state

## 4. Implementation requirements

1. Grid, playlists, marquee, drag reorder and import from the OS, cut, copy, paste, rename, reveal, push, hover or selected preview playback via engine frames or an in-process decoder (choose and log), inspector, trim timeline, colour sliders that do not restart the video.
2. Persist `media-dock-height`, `media-dock-zoom`, `kairo.mediaDock.sidebarHidden`, `kairo.mediaDock.sidebarWidth`. Deleting the live item clears it from screens. Failed pushes show `backgroundTargetProblem` text.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.
- Before UI work, read the GPUI Kit `gpui-kit` skill's Coding Guides and `gpui-kit-design-guides`, then `DESIGN.md`. Kairo's `DESIGN.md` wins on appearance. No raw colours outside `kairo-ui-kit`.

## 6. Expected deliverables

- `ui-media`.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Port behaviours from `media-*` tests at the UI level.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.
- Run the app (`cargo run -p kairo`), exercise the feature by hand, compare it with the Electron app side by side, and save screenshots to `native-migration/evidence/<prompt>/`.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- 2,000 items smooth. Thumbnails off-thread.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Inventory `media` UI items done.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 084.
