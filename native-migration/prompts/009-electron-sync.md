# Prompt 009 — Electron-to-native sync process

**Phase:** 0. Foundations and gating spikes  
**Tracker ID:** `009-electron-sync`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

The Electron app keeps changing while this repository is built. A prompt marked done can go stale the next day. Build the process that finds each Electron change and queues it for porting. Run this prompt once to build the tool, then re-run the tool on the cadence in `native-migration/SYNC.md`.

## 2. Prerequisites

- Prompt [000](./000-workspace-bootstrap.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `native-migration/SYNC.md` — last-synced commit and queue format
- `native-migration/prompts/` — section 3 of each prompt maps Electron files to the prompt that owns them

## 4. Implementation requirements

1. Build `tools/electron-sync`. It reads the last-synced commit from SYNC.md, then lists commits and changed files in `../kairo-presenter` since then, plus uncommitted changes, shown separately and never recorded as synced.
2. Map each changed file to its owning prompts using their section 3 lists, and flag files with no owner. New files are always flagged.
3. Classify each change: `not started` (the prompt will read current source; nothing to do), `port needed` (owning prompt is done), or `no native impact` (server, website, marketing, or Electron-only plumbing, with a reason).
4. Mark high-risk changes for priority: `src/lib/ipc.ts`, `src/lib/program.ts`, `src/main/db/`, `src/lib/workspace.ts`, `src/lib/kairo-bundle.ts`, `overlay-template.ts`, `overlay.html`, `detector.ts`, and anything altering a stored format.
5. Add `--apply-generated`: regenerate `api-inventory.md`, lib coverage, the oracle cache, the embedded changelog, and report which goldens need recapturing.
6. Check copied native sources against their originals, and report drift.
7. Porting a queued change is normal work: read the Electron diff, change the Rust, add a test, then mark the item done with the Electron commit. Advance last-synced only when every item up to that commit is done or marked no-impact.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- This tool only reads the Electron repo. Never commit, stash, pull or switch branches there.
- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.
- Spike code never merges into app crates. Its value is the written report.

## 6. Expected deliverables

- `tools/electron-sync`, tests, first SYNC.md update.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Run against a scratch clone with a staged commit of each class, and an uncommitted edit.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- A sync check should take under 10 seconds, excluding oracle regeneration.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] The tool produces a correct queue on the real Electron repo.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- Run before each phase starts, and before 100, 109 and 112.
