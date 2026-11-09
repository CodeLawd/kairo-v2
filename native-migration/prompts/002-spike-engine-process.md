# Prompt 002 — Spike: separate engine process with output windows and preview

**Phase:** 0. Foundations and gating spikes  
**Tracker ID:** `002-spike-engine-process`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Prove the riskiest architectural claim: a child process using `winit` and `wgpu` can drive borderless output windows on chosen displays while GPUI runs the controls in another process, and frames can return to GPUI as a preview.

## 2. Prerequisites

- Prompt [000](./000-workspace-bootstrap.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `native-migration/03-native-architecture.md` — sections 2, 5 and 6
- `../kairo-presenter/src/main/services/output/display-window.ts` — exact window behaviour to reproduce: rehearsal window versus full-screen, bounds, focus
- `../kairo-presenter/src/main/services/output/managed-window.ts` — lifecycle and retry budget
- `../kairo-presenter/src/lib/displays.ts` — display description and resolution logic

## 4. Implementation requirements

1. Build `spikes/engine-window` in this repository: a GPUI controller app that launches a child `winit` process. The child opens one borderless window per selected display and renders an animated test scene with `wgpu`.
2. Prove on macOS: no second Dock icon or Cmd-Tab entry (accessory activation policy), output windows never take focus from the GPUI window, the windows sit above the desktop and over the menu bar on secondary displays, the cursor hides over them, and display unplug and replug is detected.
3. Send control messages over `interprocess` local sockets; measure round-trip latency over 10,000 messages.
4. Return a 640×360 BGRA preview at 15 and 30 fps over shared memory. Show it in GPUI with `img()` over `Arc<RenderImage>`, calling `window.drop_image` for each replaced frame. Run for 30 minutes and log RSS and atlas behaviour. Compare with `surface()` using an NV12 `CVPixelBuffer` on macOS.
5. Kill the child with SIGKILL and show the parent relaunching it within 1 second. Kill the parent and show the child holding its picture for a configurable grace period.
6. Repeat the core checks on Windows and Linux (X11 and Wayland) if machines or CI runners allow; otherwise state exactly what remains unproven.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- This is throwaway code under `spikes/`. Keep it out of the app crates.
- Read the pinned GPUI source for the APIs you use. Do not assume an API exists because another version had it.
- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.
- Spike code never merges into app crates. Its value is the written report.

## 6. Expected deliverables

- Spike code and `native-migration/spikes/002-engine-process.md` containing method, results, screenshots and a PASS or FAIL for each requirement.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Run the 30-minute preview soak and report RSS at start, 10 minutes and 30 minutes.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Targets: control round trip p99 under 2 ms. Preview must add under 5% of one core at 15 fps. RSS must plateau.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Report written with explicit PASS or FAIL. A FAIL on process separation or focus behaviour stops the migration at prompt 008.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 008, 050, 066.
