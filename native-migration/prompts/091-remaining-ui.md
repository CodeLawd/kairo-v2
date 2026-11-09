# Prompt 091 — Transcription, transfer, capture popover and leftovers

**Phase:** 5. Operator interface  
**Tracker ID:** `091-remaining-ui`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Close the UI inventory. Whatever remains must be finished here.

## 2. Prerequisites

- Prompt [090](./090-audience-ui.md) is marked `done`.
- Prompt [044](./044-transfer-libraries.md) is marked `done`.
- Prompt [063](./063-syphon-sink.md) is marked `done`.
- Prompt [064](./064-recorder-sink.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/renderer/src/components/transcription/Transcription.tsx` — transcription view
- `../kairo-presenter/src/renderer/src/components/transfer/` — bundle import and export
- `../kairo-presenter/src/renderer/src/stores/useTransfer.ts` — state
- `../kairo-presenter/src/renderer/src/stores/useCaptureStore.ts` — state
- `../kairo-presenter/src/renderer/src/components/layout/RailPopovers.tsx` — Live popover
- `../kairo-presenter/src/renderer/index.html` — entry; retired
- `../kairo-presenter/src/renderer/src/components/layout/useSidebarWidth.ts` — `kairo.page-sidebar-width`

## 4. Implementation requirements

1. Build the Transcription view, Export and Import review modals, Live popover (Capture, Syphon switches), and any item `check-api-coverage` still lists.
2. Run a script listing each `src/renderer` file with its native owner; none may be unassigned.
3. `Transcription.tsx` is not imported anywhere in the renderer at the audit commit, so it appears to be dead code; the live transcript is in `Operator.tsx`. Re-verify. Unless the owner asks for it, do not port it. Ensure its one unique capability, if any, exists in the Operator. Run the same import check for every other renderer file.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.
- Before UI work, read the GPUI Kit `gpui-kit` skill's Coding Guides and `gpui-kit-design-guides`, then `DESIGN.md`. Kairo's `DESIGN.md` wins on appearance. No raw colours outside `kairo-ui-kit`.

## 6. Expected deliverables

- Final UI, coverage report.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- `check-api-coverage` passes with no `todo`.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.
- Run the app (`cargo run -p kairo`), exercise the feature by hand, compare it with the Electron app side by side, and save screenshots to `native-migration/evidence/<prompt>/`.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- The transcript view handles a 2-hour service.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] The whole inventory is done.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 100.
