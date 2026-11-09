# Prompt 032 — Usage, crash reports, feedback and sermon records

**Phase:** 3. Services  
**Tracker ID:** `032-telemetry-services`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Port the reporting services, respecting the privacy promises in Settings.

## 2. Prerequisites

- Prompt [030](./030-cloud-client.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/main/services/usage/index.ts` — anonymous stats
- `../kairo-presenter/src/main/services/crash-reports/index.ts` — ledger and upload
- `../kairo-presenter/src/main/services/crash-reports/reporter.ts` — report shape
- `../kairo-presenter/src/main/services/cloud/sermon-upload.ts` — retrying uploader
- `../kairo-presenter/src/main/services/service-records/index.ts` — service summaries with LLM
- `../kairo-presenter/src/lib/cloud/usage.ts` — shape
- `../kairo-presenter/src/lib/cloud/crashes.ts` — shape
- `../kairo-presenter/src/lib/cloud/feedback.ts` — shape
- `../kairo-presenter/tests/crash-reports.test.ts` — expectations
- `../kairo-presenter/tests/usage-ledger.test.ts` — expectations
- `../kairo-presenter/tests/sermon-upload.test.ts` — expectations
- `../kairo-presenter/src/lib/service-archive.ts` — ServiceRecords: create, end, discard, nuggets, analysis, upload queue
- `../kairo-presenter/src/lib/service-records.ts` — service commands and talk time
- `../kairo-presenter/website/src/lib/admin.ts` — admin labels for crash and error kinds
- `../kairo-presenter/server/src/admin/admin.service.ts` — admin consumers

## 4. Implementation requirements

1. Honour `usage.shareStats` and `sendCrashReports`. Keep `installId` stable using the existing stored value.
2. Map Electron-only error kinds such as `renderer_gone` onto native equivalents: engine crash, web host crash, UI panic. Tell the admin console's consumers about any new kind in DECISIONS.md.
3. Port ServiceRecords and the `services:command` actions (list, start, end, discard, nugget, analyze, removeNugget, upload), including the single-command guard, the nugget prompt text and model names via the managed gateway, and fan-out from transcripts, suggestions, live plan and orchestrator status.
4. Keep every usage event name: bible_download, document_page, listening_minutes, lyrics_slide, media_live, message_shown, output_clear, recap_uploaded, scripture_auto, scripture_manual, scripture_search, service_ended, service_started, song_import, song_online_search, timer_start, uncaught, unhandled_rejection, renderer_gone, child_process_gone.
5. Port `noteError` context on crash reports, which Electron fills from renderer console errors.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- Reports never include content.
- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- Services, tests.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Assert opt-out sends nothing, with a mock endpoint.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Batched, background only.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Inventory items done.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 109.
