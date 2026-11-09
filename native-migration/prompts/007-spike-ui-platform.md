# Prompt 007 — Spike: operator UI on GPUI Kit

**Phase:** 0. Foundations and gating spikes  
**Tracker ID:** `007-spike-ui-platform`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Build a slice of the hardest screens to confirm that GPUI Kit can look and behave like Kairo, on the operating systems Kairo supports.

## 2. Prerequisites

- Prompt [000](./000-workspace-bootstrap.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/DESIGN.md` — interface contract
- `../kairo-presenter/src/renderer/src/index.css` — tokens
- `../kairo-presenter/src/renderer/src/components/layout/AppShell.tsx` — chrome
- `../kairo-presenter/src/renderer/src/components/scripture/VerseCardGrid.tsx` — responsive card grid
- `../kairo-presenter/src/renderer/src/components/shared/MarqueeSelect.tsx` — marquee selection
- `../kairo-presenter/src/renderer/src/components/lyrics/Lyrics.tsx` — section grid to approximate
- `../kairo-presenter/src/main/index.ts` — window options, traffic light position, vibrancy

## 4. Implementation requirements

1. Read the GPUI Kit Coding Guides and Design Guides first.
2. Reproduce the shell: traffic lights at `{18,24}`, under-window style vibrancy on the header and rail with an opaque inset rounded workspace panel, 0.5 px hairlines, and pinned light or dark appearance independent of the OS.
3. Build a virtualised responsive grid of 5,000 slide cards with a changing column count, marquee select, drag reorder, zoom, keyboard navigation and `RenderImage` thumbnails. Scroll it while the preview from spike 002 runs.
4. Evaluate IME and dead-key input with Yoruba diacritics in `Input`, and undo. Evaluate the rich text options for the Tiptap editor in `SermonNotesReviewModal.tsx`. Determine whether Kit's `Editor` and TextView cover highlight-style editing.
5. Determine the actual runtime minimum macOS version. Build a release binary, inspect `LC_BUILD_VERSION`, and search for Metal or AppKit calls that need newer OS versions. Compare with Kairo's `LSMinimumSystemVersion` of 11.0. Test on the oldest OS you can reach.
6. Run on Windows and Linux. Note Linux without Vulkan behaviour.
7. Evaluate `.kairo` file open via `on_open_urls` and macOS document-type handling.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- Throwaway code under `spikes/`.
- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.
- Spike code never merges into app crates. Its value is the written report.

## 6. Expected deliverables

- Spike, screenshots side by side with Electron, and `native-migration/spikes/007-ui-platform.md`.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- `gpui-fps` frame times while scrolling; IME sequence recorded; UI integration test using `#[gpui_kit::test]` for one interaction.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Targets: 120 Hz scrolling with no frame over 16 ms on the baseline machine; opening the grid under 100 ms.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Written PASS or FAIL per item, including the actual minimum OS and the rich text recommendation.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 008, 070.
