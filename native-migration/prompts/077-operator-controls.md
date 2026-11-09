# Prompt 077 — Operator queue, suggestions and program panels

**Phase:** 5. Operator interface  
**Tracker ID:** `077-operator-controls`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Finish the booth: suggestions, queue, messages, timers, cameras.

## 2. Prerequisites

- Prompt [076](./076-operator-layout.md) is marked `done`.
- Prompt [054](./054-scene-program-layers.md) is marked `done`.
- Prompt [057](./057-camera-layer.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/renderer/src/components/operator/Operator.tsx` — queue and suggestions (second half)
- `../kairo-presenter/src/renderer/src/components/operator/OperatorQueueSearch.tsx` — search
- `../kairo-presenter/src/renderer/src/components/operator/ProgramPanels.tsx` — all panels
- `../kairo-presenter/src/renderer/src/components/operator/ServicePanel.tsx` — service records
- `../kairo-presenter/src/renderer/src/components/operator/BoothToolbox.tsx` — toolbox
- `../kairo-presenter/src/renderer/src/components/operator/DeepgramKeyDialog.tsx` — key prompt
- `../kairo-presenter/src/renderer/src/components/operator/VideoLayoutCanvas.tsx` — camera layout
- `../kairo-presenter/src/renderer/src/lib/markSuggestionRendered.ts` — trace mark
- `../kairo-presenter/tests/operator-api-bible-wiring.test.ts` — expectations

## 4. Implementation requirements

1. Port suggestion cards, auto-present countdown and dismiss, keyboard commands (clicker PageUp and PageDown), retranslation, recovery prompts. Call `markRendered` at the point a card is actually painted, using a frame callback, so traces stay meaningful.
2. Migrate the legacy `kairo-message-nuggets` list as Electron does. Include the `detect-*` and `listening` animations, and nugget copy.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.
- Before UI work, read the GPUI Kit `gpui-kit` skill's Coding Guides and `gpui-kit-design-guides`, then `DESIGN.md`. Kairo's `DESIGN.md` wins on appearance. No raw colours outside `kairo-ui-kit`.

## 6. Expected deliverables

- `ui-operator` complete.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- UI flows driven by a scripted core, including rapid clicker input.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.
- Run the app (`cargo run -p kairo`), exercise the feature by hand, compare it with the Electron app side by side, and save screenshots to `native-migration/evidence/<prompt>/`.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Suggestion arrival to paint under one frame after the event reaches the UI.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Inventory `orchestrator`, `program` UI items done.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 078.
