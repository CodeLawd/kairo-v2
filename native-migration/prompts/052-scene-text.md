# Prompt 052 — Scene model and slide text

**Phase:** 4. Output engine  
**Tracker ID:** `052-scene-text`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Replace `renderOverlayHTML` with a typed `SlideScene` and render text to match Chromium.

## 2. Prerequisites

- Prompt [013](./013-domain-normalizers.md) is marked `done`.
- Prompt [051](./051-compositor-core.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/lib/overlay-template.ts` — behaviour to reproduce
- `../kairo-presenter/src/lib/overlay-fit.ts` — fit logic
- `../kairo-presenter/src/lib/overlay-content.ts` — formatting
- `../kairo-presenter/src/main/services/output/program-surface.ts` — paint and clearText semantics
- `native-migration/spikes/003-text-fidelity.md` — chosen stack and known differences
- `../kairo-presenter/tests/overlay-content.test.ts` — expectations
- `../kairo-presenter/src/renderer/src/components/theme/ThemeTypePanel.tsx` — FONT_STACKS

## 4. Implementation requirements

1. `kairo-scene`: build a `SlideScene` from (theme, reference, text, frame, coloured lines). Cover: boxes, padding, vertical align, backdrop box, alignment, line height, letter spacing, decoration, transform, colours with opacity.
2. One auto-fit implementation: largest size that fits, between 12 and the smart-fill cap (`SMART_FILL_REFERENCE` logic), using real shaping. Must match Chromium's final size per spike tolerance. Retire the estimate and the duplicate.
3. Stroke per `outlineCss` (inside uses 0.55 × width, minimum 0.5; dashed and dotted fall back to solid; stroke then fill). Shadow per `shadowCss`.
4. Font resolution: themes choose from the fixed `FONT_STACKS` list in `ThemeTypePanel.tsx` (older themes may hold other CSS stacks). Parse the stack, find system fonts, bundle Source Sans 3 (the variable package the renderer uses), and define the fallback chain, including for Yoruba diacritics.
5. Escape semantics no longer apply; assert untrusted text cannot affect styling. Keep `sanitizeOverlayColor` rules.
6. Raster on a worker pool, cached by content hash. Same code must work in the UI process for thumbnails.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- `kairo-scene` text, golden tests.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- The full 300-case corpus from prompt 003: line breaks, fitted size and SSIM thresholds set in DECISIONS.md.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.
- Run `kairo-engine` for real and confirm output visually. Headless tests alone do not finish this prompt.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Slide raster under 16 ms p95. Push-to-commit measured with traces.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Corpus gates pass. Theme changes made in Electron render the same here.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 053, 054, 076, 086.
