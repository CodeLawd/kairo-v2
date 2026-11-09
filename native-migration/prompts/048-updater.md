# Prompt 048 — Auto-update

**Phase:** 3. Services  
**Tracker ID:** `048-updater`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Port updates with the safety of today's flow, and read the same GitHub feed shape.

## 2. Prerequisites

- Prompt [025](./025-core-runtime.md) is marked `done`.
- Prompt [020](./020-paths-logging.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/main/services/updater/index.ts` — state machine
- `../kairo-presenter/src/main/services/updater/mac-self-update.ts` — zip, sha512, swap
- `../kairo-presenter/src/main/services/updater/errors.ts` — errors
- `../kairo-presenter/tests/mac-self-update.test.ts` — expectations
- `../kairo-presenter/tests/updater-errors.test.ts` — expectations
- `../kairo-presenter/docs/release-checklist.md` — release rules

## 4. Implementation requirements

1. Parse `latest-mac.yml`, `latest.yml` and `latest-linux.yml`. Never start a download without the operator asking, as today.
2. macOS: bundle-aware verified swap on quit. Windows: run the installer. Linux AppImage: staged replace; deb: direct the user to the package.
3. Release hardware and finish recording before install, mirroring `setBeforeInstall`.
4. Include a dev simulation mode, like `KAIRO_UPDATE_SIM`.
5. Unsigned macOS builds use the self-update path; signed builds use Squirrel in Electron, which requires a matching designated requirement. Decide the native equivalent. Keep `bundleFromExecutable` checks, and download through a client that honours the OS proxy, as `electron.net` does.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- Verify sha512 before installation. Never install while live output is running without confirmation.
- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- `kairo-updater`, tests.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Simulated feeds: valid, tampered checksum, wrong arch, network loss, interrupted download.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Background; throttled.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Update path proven on macOS from N to N+1 using local builds.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 105.
