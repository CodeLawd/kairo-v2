# Prompt 071 — Application shell, menus and quit flow

**Phase:** 5. Operator interface  
**Tracker ID:** `071-app-shell`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Build the window that everything lives in.

## 2. Prerequisites

- Prompt [070](./070-design-system.md) is marked `done`.
- Prompt [065](./065-surface-manager.md) is marked `done`.
- Prompt [067](./067-application-wiring.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/renderer/src/App.tsx` — routing, global state, keyboard handling
- `../kairo-presenter/src/renderer/src/main.tsx` — entry
- `../kairo-presenter/src/renderer/src/components/layout/` — chrome pieces
- `../kairo-presenter/src/renderer/src/stores/useAppStore.ts` — runtime state
- `../kairo-presenter/src/renderer/src/stores/useBoothToolboxStore.ts` — state
- `../kairo-presenter/src/renderer/src/hooks/useSettings.ts` — settings access
- `../kairo-presenter/src/renderer/src/hooks/useImportRequest.ts` — import flow
- `../kairo-presenter/src/renderer/src/hooks/useUpdates.ts` — updates
- `../kairo-presenter/src/renderer/src/lib/whats-new.ts` — release sheet
- `../kairo-presenter/src/renderer/src/lib/clear-live-output.ts` — clear behaviour
- `../kairo-presenter/src/renderer/src/lib/output-settings.ts` — output settings
- `../kairo-presenter/src/main/index.ts` — menu template, window events, file opens, focus actions
- `../kairo-presenter/tests/keyboard-shortcuts.test.ts` — expectations
- `../kairo-presenter/tests/pane-sizing.test.ts` — expectations
- `../kairo-presenter/tests/packaged-locales.test.ts` — locale packaging expectations

## 4. Implementation requirements

1. Build the rail, header, routed workspaces, resizable sidebars, popovers, tooltips, update pill and toast, feedback button, activation chip, What's New sheet (reads `website/src/lib/changelog.ts` data embedded at build time), and keychain notice.
2. Native menus matching `setupApplicationMenu`, including Import and Help items, and drop items that only make sense in a browser, such as Reload and DevTools. Log this.
3. `.kairo` and Import requests delivered once the UI is ready. Quit confirmation for every path. On focus: pull org secrets (throttled), wake uploaders. Full-screen state in header.
4. Global shortcut handling must respect customised bindings and never fire while a text field has focus unless intended.
5. Crash fallback view replaces `ErrorBoundary.tsx`: a panic in a feature must not take down the app where containment is possible.
6. Window: 1280×800 default, 960×600 minimum, per-OS chrome (Mac traffic lights and drag region; logo elsewhere, or in full screen). External links open in the system browser. Set the Dock icon and `AppUserModelID`.
7. Menus must include About, Services, Hide, Hide Others, Show All, Check for Updates, What's New, the Edit and Window roles, and Zoom In, Zoom Out, Actual Size (implement as a persisted UI scale) and Toggle Full Screen.
8. Double-clicking the custom title area zooms the window (`FrameZoomContext`). `app.isFullScreen` state drives the header. Log renderer-equivalent panics to crash reports, with a retry limit of three in 60 s as the renderer reload has.
9. On macOS, closing the window leaves the app running and the Dock icon reopens it; on other platforms it quits.
10. Generate an embedded changelog from `website/src/lib/changelog.ts` during sync (prompt 009), and commit it. Do not read the sibling repo at build time.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.
- Before UI work, read the GPUI Kit `gpui-kit` skill's Coding Guides and `gpui-kit-design-guides`, then `DESIGN.md`. Kairo's `DESIGN.md` wins on appearance. No raw colours outside `kairo-ui-kit`.

## 6. Expected deliverables

- `ui-shell`, `apps/kairo` wiring.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- UI integration tests for navigation, shortcuts, quit paths.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.
- Run the app (`cargo run -p kairo`), exercise the feature by hand, compare it with the Electron app side by side, and save screenshots to `native-migration/evidence/<prompt>/`.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Launch to first paint target from baseline.md. Idle at 0 fps with no animation running.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Every route shows, and a placeholder exists only where a later prompt is named.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 072 and all later UI prompts.

## Amendment from prompt 009

`electron-sync --apply-generated` runs `cargo run --package embed-changelog -- --regenerate`. Build `tools/embed-changelog` to accept that flag and add it to the workspace.
