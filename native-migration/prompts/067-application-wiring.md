# Prompt 067 — Application wiring formerly in the IPC layer and main entry

**Phase:** 4. Output engine  
**Tracker ID:** `067-application-wiring`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

`src/main/ipc/index.ts` and `src/main/index.ts` are not thin. They hold real product rules: side effects after a setting changes, cross-service clean-up, startup order and every native dialog. Without this prompt, those rules have no owner.

## 2. Prerequisites

- Prompt [032](./032-telemetry-services.md) is marked `done`.
- Prompt [039](./039-orchestrator.md) is marked `done`.
- Prompt [043](./043-media-service.md) is marked `done`.
- Prompt [044](./044-transfer-libraries.md) is marked `done`.
- Prompt [045](./045-documents-service.md) is marked `done`.
- Prompt [046](./046-audience-rooms.md) is marked `done`.
- Prompt [048](./048-updater.md) is marked `done`.
- Prompt [065](./065-surface-manager.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/main/ipc/index.ts` — read every handler; this is the specification
- `../kairo-presenter/src/main/index.ts` — startup order, lifecycle, media allowlist
- `../kairo-presenter/src/lib/cloud/org-secrets.ts` — redaction and secret merge
- `../kairo-presenter/src/main/services/output/program-service.ts` — `pruneProps`, `setSlide`

## 4. Implementation requirements

1. Settings side effects: `overlay` merges with stored, normalises, then reconciles; `presentation` normalises, prunes props, reconciles; `stt` and `lyrics` always store blank keys; `capture`, `web`, `documents`, `themeLibrary` normalise; `church` syncs the org profile; `scripture` updates the service and orchestrator. Retired ProPresenter keys are rejected. Reads are redacted and include `secretsConfigured`. Broadcast settings changes in the same cases as today.
2. Cross-service effects: sermon plan save and delete refresh the live plan; `setAutoMode` and `setConfidence` update services and persist; workspace change resyncs songs and rescans media; `lyrics.pushSlide` builds slides the same way as the grid, records song usage only after a successful push; document push clears the live media item, sets the program slide and preloads the next page without awaiting it; media push, clear, delete, set playback, pause and seek behave as written, including `backgroundTargetProblem` wording.
3. Startup order must match: settings, workspace, default media folder, lyrics and scripture open, NKJV migration, default translation, offline Bibles, live plan init, onboarding migration, licence and session start without awaiting, NDI start, updater, window, surface init and reconcile. Media and tracks scans run in the background.
4. OS events: on resume reconnect the session; on focus pull org secrets, wake uploaders and usage.
5. Port every native dialog with its title, button label, filters and default path, and create the media or tracks folder when an import needs one, as the handlers do.
6. Keep the picked-media allowlist and folder realpath rules from `isUnderAllowedFolder` for any path that reaches the engine from outside `kairo`.
7. Mark each handler in `api-inventory.md` with the Rust function that replaced it.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- Do not change the order or conditions of these effects. Differences are bugs.
- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- Wiring in `kairo-core`, tests, updated `api-inventory.md`.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- One test per side effect above, against fakes. Assert order for startup.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.
- Run `kairo-engine` for real and confirm output visually. Headless tests alone do not finish this prompt.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Startup must not await network. Bootstrap remains disk only.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Every handler in `src/main/ipc/index.ts` is accounted for.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 071 and all UI prompts.
