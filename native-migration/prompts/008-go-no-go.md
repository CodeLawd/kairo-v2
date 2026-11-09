# Prompt 008 — Human checkpoint: go or no-go

**Phase:** 0. Foundations and gating spikes  
**Tracker ID:** `008-go-no-go`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

> **Human checkpoint.** This prompt needs the owner's decision, approval or physical observation. An agent may prepare, but must not complete it alone.

## 1. Objective and context

Review the spike results with the owner and lock the decisions the rest of the migration depends on. This prompt must not be skipped, and an agent must not answer it alone.

## 2. Prerequisites

- Prompt [001](./001-electron-baseline.md) is marked `done`.
- Prompt [002](./002-spike-engine-process.md) is marked `done`.
- Prompt [003](./003-spike-text-fidelity.md) is marked `done`.
- Prompt [004](./004-spike-video.md) is marked `done`.
- Prompt [005](./005-spike-sinks.md) is marked `done`.
- Prompt [006](./006-spike-web-host.md) is marked `done`.
- Prompt [007](./007-spike-ui-platform.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `native-migration/spikes/` — all spike reports
- `native-migration/07-risks.md` — update each risk with what was learned
- `native-migration/03-native-architecture.md` — amend if a spike changed the design

## 4. Implementation requirements

1. Summarise each spike in one page: PASS, FAIL, unknowns, cost.
2. Present these decisions to the owner, with a recommendation for each: minimum supported macOS, if higher than 11; text stack; video stack and codec licensing path; NDI binding; web host packaged or on demand; zero-copy video now or later; rich text approach; Linux support scope, including the Vulkan requirement; whether Intel Macs remain supported.
3. Record every answer in `DECISIONS.md` with date and reasoning, and revise affected prompts in `native-migration/prompts/`.
4. Set the sync cadence and the rules for Electron changes during migration, per SYNC.md.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- Stop and wait for the owner. Do not proceed on assumption.
- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- Updated DECISIONS.md, 03-native-architecture.md, 07-risks.md and any changed prompts. A `GO` or `NO-GO` line at the top of PROGRESS.md.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Confirm there is no remaining `TBD` in the decision list.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Compare spike measurements against baseline.md and state the expected gain honestly.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] The owner has answered and PROGRESS.md records GO.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- All phase 1 prompts.
