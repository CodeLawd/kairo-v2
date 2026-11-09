# Prompt 003 — Spike: match Chromium slide text

**Phase:** 0. Foundations and gating spikes  
**Tracker ID:** `003-spike-text-fidelity`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Lyrics and verses must wrap the same as today, or operators will see changed slides on Sunday. Determine which Rust text stack comes closest to Chromium and how close that is.

## 2. Prerequisites

- Prompt [000](./000-workspace-bootstrap.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/lib/overlay-template.ts` — every CSS property text uses
- `../kairo-presenter/src/lib/overlay-fit.ts` — auto-fit semantics to match
- `../kairo-presenter/src/lib/overlay-boxes.ts` — box geometry
- `../kairo-presenter/src/main/services/output/overlay.html` — duplicated fit logic and layout rules
- `../kairo-presenter/tests/overlay-content.test.ts` — existing expectations
- `../kairo-presenter/tests/typography-system.test.mjs` — font rules

## 4. Implementation requirements

1. Build `tools/golden-capture`: an Electron-hosted script (separate from the app, reading the app's `overlay-template.ts`) that renders a corpus and dumps for each case the PNG at 1920×1080, the final fitted font size, and each line's text and rectangle using DOM Range client rects.
2. Corpus: at least 300 cases. Include short and very long verses, all bundled-translation styles, lyrics with gloss lines, Yoruba text with tone marks (see `tests/lyrics-yoruba-hymn.test.ts`), RTL if any path allows it, all alignments, letter spacing, text-transform, each default theme from `src/lib/overlay-defaults.ts` and `src/lib/theme-library.ts`, with and without auto-fit.
3. Implement the same layout with `parley` + `swash`, and with `cosmic-text`. Rasterise with `tiny-skia`, including stroke with paint-order stroke-fill, and blurred shadow (CSS sigma = blur radius / 2).
4. Report per stack: exact line-break match percentage, fitted-size match within 1 px, mean and worst SSIM against Chromium, and raster time per slide.
5. Investigate each mismatch category and state whether it can be fixed. Recommend one stack.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- Golden files live in `tests/golden/`; keep them small using a representative PNG subset, with JSON for all layout data.
- The tool must not modify anything in `src/`.
- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.
- Spike code never merges into app crates. Its value is the written report.

## 6. Expected deliverables

- golden-capture tool, corpus and results in `native-migration/spikes/003-text-fidelity.md`.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Re-run capture twice and confirm the golden output is deterministic before comparing.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Target: full 1080p slide text raster under 16 ms p95 on the baseline machine.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] A recommendation with measured numbers. Target for go: at least 99% exact line breaks on the corpus and every remaining difference explained.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 008, 052.
