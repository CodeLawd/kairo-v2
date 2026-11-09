# Prompt 012 — Port the data types with on-disk compatibility

**Phase:** 1. Domain model  
**Tracker ID:** `012-domain-types`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Create Rust types for all persisted and exchanged data, able to read today's real files without loss. Everything else builds on them.

## 2. Prerequisites

- Prompt [011](./011-ts-oracle.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/lib/ipc.ts` — AppSettings, OverlayTheme, OverlayOutput and about 1,700 lines of API types
- `../kairo-presenter/src/lib/program.ts` — presentation and program state types
- `../kairo-presenter/src/lib/capture.ts` — capture settings
- `../kairo-presenter/src/lib/documents.ts` — document types
- `../kairo-presenter/src/lib/web.ts` — web settings
- `../kairo-presenter/src/lib/workspace.ts` — song file format
- `../kairo-presenter/src/lib/video-devices.ts` — device types
- `../kairo-presenter/src/lib/audience.ts` — question types
- `../kairo-presenter/src/lib/engage.ts` — engagements
- `../kairo-presenter/src/lib/audience-protocol.ts` — wire protocol
- `../kairo-presenter/src/lib/audience-connection.ts` — connection rules
- `../kairo-presenter/src/lib/libraries.ts` — libraries
- `../kairo-presenter/src/lib/setlist.ts` — setlists
- `../kairo-presenter/src/lib/passages.ts` — passages
- `../kairo-presenter/src/lib/tracks.ts` — tracks
- `../kairo-presenter/src/lib/song-usage.ts` — song usage
- `../kairo-presenter/src/lib/usage-ledger.ts` — usage ledger
- `../kairo-presenter/src/lib/service-records.ts` — service records
- `../kairo-presenter/src/lib/service-archive.ts` — archive
- `../kairo-presenter/src/lib/sermon-upload.ts` — sermon upload
- `../kairo-presenter/src/lib/cloud/contracts.ts` — server contracts
- `../kairo-presenter/src/lib/cloud/` — remaining cloud types
- `../kairo-presenter/src/lib/live-output.ts` — live output
- `../kairo-presenter/src/lib/brand.ts` — product strings
- `../kairo-presenter/src/lib/import-menu.ts` — import kinds
- `../kairo-presenter/server/src` — server DTOs the types must stay aligned with

## 4. Implementation requirements

1. Implement the types in `kairo-domain` with `serde`. Optional fields must round-trip as absent, not null. Preserve JSON field names exactly using `rename_all` and explicit renames.
2. Unknown fields must survive a load-and-save (`#[serde(flatten)]` extras or equivalent) on `AppSettings` and every nested settings object. An older or newer version must never lose data.
3. Collect real-shaped fixtures: defaults from `db/index.ts` through the oracle, the owner's settings shape sanitized of secrets, and hand-built old-format files including `proautomate-settings.json` shapes.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- Discriminated unions must match the TypeScript tagging. Do not normalise on deserialise; normalisers come in prompt 013.
- Never commit real API keys or personal data in fixtures.
- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- Types in `kairo-domain`, fixtures, round-trip tests.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Byte-semantic round trip: JSON in equals JSON out, for every fixture.
- Oracle comparison of defaults for each settings section.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Parsing a 5 MB settings file must take under 20 ms.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] All types compile, all fixtures round trip, none use `serde_json::Value` without a recorded reason.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 013 and all later prompts.
