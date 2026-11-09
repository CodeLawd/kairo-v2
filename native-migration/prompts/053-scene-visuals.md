# Prompt 053 — Backgrounds, elements, shapes and filters

**Phase:** 4. Output engine  
**Tracker ID:** `053-scene-visuals`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Complete the slide visuals.

## 2. Prerequisites

- Prompt [052](./052-scene-text.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/lib/overlay-template.ts` — backgroundLayerHTML and elementHTML
- `../kairo-presenter/src/lib/overlay-shapes.ts` — shapes
- `../kairo-presenter/src/lib/media-playback.ts` — mediaFilterCss
- `../kairo-presenter/src/main/services/output/overlay.html` — element layers kept across slides
- `../kairo-presenter/tests/overlay-elements.test.ts` — expectations

## 4. Implementation requirements

1. Implement colour, gradient (CSS angle semantics), transparent, and image backgrounds with cover, contain, fill; opacity.
2. Filters as WGSL matching the CSS definitions for hue-rotate, saturate, brightness and contrast, in the same order as `mediaFilterCss`. Blur equals CSS sigma, including the edge-scaling behaviour `1 + blur*4/1080`.
3. Elements: rectangle, ellipse, shape (SVG path, preserveAspectRatio none, non-scaling stroke), image, video; rotation, flips, opacity, below or above text; up to 40 elements. Inside stroke as `box-shadow: inset`. Elements persist across slides as in `setElements`.
4. Background identity is file plus cue: the same key keeps playing and only takes the new opacity, filter, fit and loop. Identical element sets keep their nodes, so element videos continue across verses. Element videos are always muted and ignore transport.
5. Decode gif (animated, as Chromium plays it), webp, avif, bmp and svg images.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- Scene visuals, golden tests.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Reference frames captured by `golden-capture` for each background, filter, shape and transform. SSIM thresholds in DECISIONS.md.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.
- Run `kairo-engine` for real and confirm output visually. Headless tests alone do not finish this prompt.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Filtered 1080p video at target frame rate on baseline hardware.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Golden suite passes.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 086, 088.
