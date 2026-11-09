# Prompt 060 — Display outputs, stage windows and test patterns

**Phase:** 4. Output engine  
**Tracker ID:** `060-display-sinks`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Drive physical displays, with the same behaviour as today's windows.

## 2. Prerequisites

- Prompt [051](./051-compositor-core.md) is marked `done`.
- Prompt [054](./054-scene-program-layers.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/main/services/output/display-window.ts` — bounds, rehearsal mode
- `../kairo-presenter/src/main/services/output/stage-window.ts` — stage output
- `../kairo-presenter/src/main/services/output/surface-manager.ts` — identify, test patterns, power blocker
- `../kairo-presenter/src/renderer/src/components/screens/displays.tsx` — display naming
- `../kairo-presenter/tests/window-background.test.mjs` — expectations
- `../kairo-presenter/tests/window-drag-region.test.mjs` — expectations

## 4. Implementation requirements

1. Open borderless output windows per the rules: full screen on a non-control display, half-size rehearsal window on the control display, letterbox on black, hidden cursor, no focus theft, excluded from window lists and Cmd-Tab.
2. Identify overlays for 3 s and test patterns for 5 s, one per display. Screen-saver window level via winit plus native hooks.
3. Prevent display sleep while outputs are active, as `powerSaveBlocker` does.
4. Debounce display changes 250 ms. Match displays by id, with `resolveDisplay` fallback rules and persisted re-matching.
5. `hostsMainWindow` depends on where the control window is, in another process. `kairo` must report the control window's display, including on move, to drive rehearsal mode.
6. Reproduce `identifyHtml` and `testPatternHtml` content, including the numbering, "Kairo's controls are here", the yellow grid card and its three lines of text. Display identity must map to and from stored Electron display ids.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- Display sink, tests.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Manual matrix on 2+ displays: plug, unplug, sleep, wake, resolution change, moving the control window between displays.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.
- Run `kairo-engine` for real and confirm output visually. Headless tests alone do not finish this prompt.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Vsync-paced presentation; no frame pacing jitter above one interval at p99.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Manual matrix recorded in `native-migration/qualification.md`.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 065, 075.
