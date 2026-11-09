# Prompt 016 — Port program, media, transfer and remaining shared logic

**Phase:** 1. Domain model  
**Tracker ID:** `016-domain-program`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Finish `kairo-domain`: timers, program state, playback rules, bundle format, and everything still left in `src/lib`.

## 2. Prerequisites

- Prompt [012](./012-domain-types.md) is marked `done`.
- Prompt [013](./013-domain-normalizers.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/lib/program.ts` — timers, normalisation, show filters, confidence, layers. HTML functions go to prompt 054
- `../kairo-presenter/src/lib/media-playback.ts` — playback rules and themeForPush
- `../kairo-presenter/src/lib/media-order.ts` — ordering
- `../kairo-presenter/src/lib/pa-media-range.ts` — extension map; HTTP range code retires
- `../kairo-presenter/src/lib/kairo-bundle.ts` — `.kairo` format
- `../kairo-presenter/src/lib/keyboard-shortcuts.ts` — shortcuts
- `../kairo-presenter/src/lib/operator-layout.ts` — layout rules
- `../kairo-presenter/src/lib/capture.ts` — capture naming and sizing
- `../kairo-presenter/src/lib/detector-recovery.ts` — shared state
- `../kairo-presenter/tests/program.test.ts` — expectations
- `../kairo-presenter/tests/media-playback.test.ts` — expectations
- `../kairo-presenter/tests/media-order.test.ts` — expectations
- `../kairo-presenter/tests/kairo-bundle.test.ts` — expectations
- `../kairo-presenter/tests/keyboard-shortcuts.test.ts` — expectations
- `../kairo-presenter/tests/operator-layout.test.ts` — expectations
- `../kairo-presenter/tests/capture.test.ts` — expectations
- `../kairo-presenter/tests/setlist.test.ts` — expectations
- `../kairo-presenter/tests/libraries.test.ts` — expectations
- `../kairo-presenter/tests/song-usage.test.ts` — expectations
- `../kairo-presenter/tests/usage-ledger.test.ts` — expectations
- `../kairo-presenter/tests/service-archive.test.ts` — expectations
- `../kairo-presenter/tests/audience.test.ts` — expectations
- `../kairo-presenter/tests/audience-protocol.test.ts` — expectations
- `../kairo-presenter/tests/audience-connection.test.ts` — expectations
- `../kairo-presenter/tests/engage.test.ts` — expectations
- `../kairo-presenter/tests/engage-program.test.ts` — expectations
- `../kairo-presenter/tests/web.test.ts` — expectations
- `../kairo-presenter/tests/document-videos.test.ts` — expectations
- `../kairo-presenter/tests/documents-slideshow.test.ts` — expectations
- `../kairo-presenter/tests/sermon-upload.test.ts` — expectations
- `../kairo-presenter/tests/sermon-summary-compat.test.ts` — expectations
- `../kairo-presenter/tests/cloud-auth-state.test.ts` — expectations
- `../kairo-presenter/tests/cloud-device-code.test.ts` — expectations
- `../kairo-presenter/tests/cloud-onboarding.test.ts` — expectations
- `../kairo-presenter/tests/cloud-return-path.test.ts` — expectations
- `../kairo-presenter/tests/cloud-timezone-search.test.ts` — expectations
- `../kairo-presenter/tests/org-secrets.test.ts` — expectations
- `../kairo-presenter/tests/live-output.test.ts` — expectations
- `../kairo-presenter/tests/video-device-routing.test.ts` — expectations
- `../kairo-presenter/tests/import-menu.test.ts` — expectations
- `../kairo-presenter/tests/ipc-error.test.ts` — expectations
- `../kairo-presenter/tests/pa-media-range.test.ts` — expectations

## 4. Implementation requirements

1. Port what remains. Use a script that lists each export from `src/lib/**` and shows its Rust counterpart; `tools/check-lib-coverage` must pass with an explicit, justified exclusion list (for example HTML renderers moved to `kairo-scene`).
2. Timers must use an injectable clock so tests are deterministic.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- Complete `kairo-domain`, coverage checker.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Differential tests for timers across time-of-day and overrun cases.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- ProgramState rebuild under 50 µs.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] `check-lib-coverage` passes. The domain crate has no platform, network or GUI dependency.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- All phase 2 prompts.
