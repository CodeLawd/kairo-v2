# Prompt 070 — Design system

**Phase:** 5. Operator interface  
**Tracker ID:** `070-design-system`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Create `kairo-ui-kit`, so every screen uses the same tokens and controls.

## 2. Prerequisites

- Prompt [007](./007-spike-ui-platform.md) is marked `done`.
- Prompt [025](./025-core-runtime.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/DESIGN.md` — interface contract
- `../kairo-presenter/tailwind.config.js` — token mapping
- `../kairo-presenter/components.json` — shadcn setup; informational
- `../kairo-presenter/src/renderer/src/index.css` — tokens, component classes, utilities
- `../kairo-presenter/src/renderer/src/lib/hue.ts` — tool hues
- `../kairo-presenter/src/renderer/src/lib/appTheme.ts` — theme
- `../kairo-presenter/src/renderer/src/lib/utils.ts` — helpers
- `../kairo-presenter/src/renderer/src/lib/paneSizing.ts` — pane rules
- `../kairo-presenter/src/renderer/src/lib/drag.ts` — drag helpers
- `../kairo-presenter/src/renderer/src/icons/index.tsx` — icon set
- `../kairo-presenter/src/renderer/src/components/ui/` — primitives
- `../kairo-presenter/src/renderer/src/components/shared/` — shared controls
- `../kairo-presenter/src/renderer/src/components/brand/` — logos
- `../kairo-presenter/src/renderer/src/components/theme/NumberField.tsx` — field behaviour
- `../kairo-presenter/src/renderer/src/components/theme/fields.tsx` — form fields
- `../kairo-presenter/src/renderer/src/assets/` — brand images
- `../kairo-presenter/tests/app-theme.test.mjs` — expectations
- `../kairo-presenter/tests/operator-visual-system.test.mjs` — expectations
- `../kairo-presenter/tests/typography-system.test.mjs` — expectations

## 4. Implementation requirements

1. Read the GPUI Kit Coding Guides and Design Guides. Where they conflict with DESIGN.md on appearance, DESIGN.md wins. The project memory also applies: flat, restrained radius, few borders, purposeful motion, minimal copy.
2. Map every CSS token, dark and light, into a Kit theme plus Kairo tokens: surfaces, line, accent, live, tints, tool hues. No raw hex outside the token module.
3. Embed Source Sans 3 via the fonts API. `src/renderer/src/icons/index.tsx` maps to Hugeicons as well as Phosphor: inventory both, check licences, and bundle only the icons used.
4. Components: button variants, input, number field with shift nudging, select, slider, switch, segmented control, tabs, tooltip, context menu, `.pref-group` settings boxes, frosted modal, movable dialog, macOS traffic lights, zoom control, selection bar, marquee select, library section, secret key field, OTP.
5. Build a gallery binary with every component in dark and light.
6. Provide styled replacements for `window.confirm` and `window.alert`, keeping each existing message (Lyrics, Settings, Tracks, Screens, Theme, Playlists, Media, Song usage, Program panels). Provide a text-field context menu with Cut, Copy, Paste and Select All, and the keyframe animations in `index.css` and `onboarding.css`.
7. Before porting, check each primitive is still imported. At the audit commit these had no importers: `badge`, `field`, `input-group`, `resizable`, `scroll-area`, `secret-key-field`, `skeleton`, `tabs`, `toggle-group`, plus `StepApiKeys`, `StepChurchProfile`, `StepDone`, `VerseBlock`, `paneSizing`, `offline-bible-view-model`. Do not build what nothing uses; confirm each by reading, because the check was a text search.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.
- Before UI work, read the GPUI Kit `gpui-kit` skill's Coding Guides and `gpui-kit-design-guides`, then `DESIGN.md`. Kairo's `DESIGN.md` wins on appearance. No raw colours outside `kairo-ui-kit`.

## 6. Expected deliverables

- `kairo-ui-kit`, gallery, screenshots beside Electron counterparts.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- UI integration tests for each interactive component, covering keyboard and focus. Accessibility assertions.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.
- Run the app (`cargo run -p kairo`), exercise the feature by hand, compare it with the Electron app side by side, and save screenshots to `native-migration/evidence/<prompt>/`.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Components allocate little in render; use `RenderOnce` where no state is needed.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] The gallery has been reviewed against DESIGN.md and differences logged.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- All UI prompts.
- 067 does not depend on this prompt; all UI prompts do.
