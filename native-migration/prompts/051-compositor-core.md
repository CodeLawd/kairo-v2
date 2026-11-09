# Prompt 051 — Compositor core and layer stack

**Phase:** 4. Output engine  
**Tracker ID:** `051-compositor-core`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Build the GPU compositor which draws the program layer stack.

## 2. Prerequisites

- Prompt [050](./050-engine-scaffold.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/main/services/output/overlay.html` — layer order, swap and retire logic, camera layouts, fit-display letterbox
- `../kairo-presenter/src/lib/live-output.ts` — live output rules
- `native-migration/spikes/003-text-fidelity.md` — texture format decisions

## 4. Implementation requirements

1. Implement `kairo-compositor`: one thread owning the device; layer slots in the exact bottom-to-top order from the audit; per-layer content textures; a crossfade per layer, matching `swapLayer` (new enters, old retires over `transitionMs` with linear opacity, and a leaving node can no longer be targeted).
2. Render-on-change scheduling: idle draws nothing. Animation and live sources mark the output dirty.
3. Offscreen render targets per output at the frame size (1920 × `frameHeight`, 360 to 4320), transparent and premultiplied.
4. Implement the three camera layouts: full, pip with free box and radius, split with `splitPct` and side; ported from `applyCameraLayout`.
5. Emit `FrameCommitted` once GPU work for a frame's content is complete and the sink has it.
6. Clamp `transitionMs` to 0 to 5000. Instant pushes skip the transition.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- Blending is premultiplied everywhere. One colour pipeline.
- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- Compositor, headless tests.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Headless golden images using `wgpu` on a software adapter in CI (WARP or lavapipe): transitions at known progress, layouts, letterbox. Add tests for layer independence: changing the logo does not re-fade text.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.
- Run `kairo-engine` for real and confirm output visually. Headless tests alone do not finish this prompt.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Idle CPU and GPU near zero. Static slide on four outputs must use well under one renderer's cost in baseline.md.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Golden suite is green on CI.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 052 through 060.
