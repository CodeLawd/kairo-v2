# Prompt 021 — Settings store and migrations

**Phase:** 2. Persistence and runtime  
**Tracker ID:** `021-settings-store`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Read and write `kairo-settings.json` compatibly, and reproduce every launch-time repair.

## 2. Prerequisites

- Prompt [013](./013-domain-normalizers.md) is marked `done`.
- Prompt [020](./020-paths-logging.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/main/db/index.ts` — schema defaults, migrations, heal-on-launch code, in order
- `../kairo-presenter/src/main/db/legacy-store.ts` — legacy names
- `../kairo-presenter/src/renderer/src/lib/defaultSettings.ts` — renderer defaults; check for differences
- `../kairo-presenter/src/lib/cloud/org-secrets.ts` — secret field handling
- `../kairo-presenter/src/main/services/song-usage.ts` — store `song-usage`
- `../kairo-presenter/src/main/services/crash-reports/index.ts` — store `kairo-crash-reports` queue
- `../kairo-presenter/src/main/services/libraries/index.ts` — store `kairo-libraries`
- `../kairo-presenter/src/main/services/usage/index.ts` — store `kairo-usage`
- `../kairo-presenter/src/main/services/setlist/index.ts` — store `kairo-setlist`
- `../kairo-presenter/src/main/services/service-records/index.ts` — store `kairo-services`
- `../kairo-presenter/src/main/services/cloud/onboarding.ts` — store renamed from `proautomate-onboarding`
- `../kairo-presenter/src/main/services/scripture/sermon-plans.ts` — store renamed from `proautomate-sermon-plans`
- `../kairo-presenter/src/main/services/passages/index.ts` — store `kairo-passages`
- `../kairo-presenter/src/main/index.ts` — `migrateUnreadableNkjvDefault`, a migration that lives outside `db/index.ts`

## 4. Implementation requirements

1. Implement `SettingsStore` with typed section get and set, atomic writes (temp file, fsync, rename), and a change topic with per-section notification matching `store.onDidChange`.
2. Port every block in `db/index.ts` in the same order, including migration flags in `kairo-migrations.json`, the shallow-merge repairs, `upgradedFromBeforeWhatsNew`, and the DeepSeek, KJV, gloss-colour and built-in-theme migrations.
3. `settings:set` semantics include `clearKeys` and secrets reporting (`SettingsWithSecretsStatus`). Reproduce what `src/main/ipc/index.ts` does, including rejecting retired ProPresenter settings.
4. Provide one generic JSON store compatible with electron-store for every store file listed in section 3, including the `renamedStore` copy-once behaviour for each legacy name.
5. Port `kjvDefaultV1` from `src/main/index.ts`. It needs the scripture service, so expose a post-open migration hook and run it at the same point in startup (see prompt 067).
6. Add a `ui-state.json` store for per-machine view preferences that Electron keeps in `localStorage`. Prompt 100 imports the old values.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- Writes must be crash-safe. A failed write must not corrupt the file. Coalesce rapid writes without losing the last one.
- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- `kairo-store::settings`, tests.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Fixture matrix: fresh install, 1.0.28 upgrade, `proautomate` era, corrupt file, partial file, read-only file. Compare final JSON against the Electron result via oracle or captured fixtures.
- Kill the process during write in a loop 500 times. The file must always parse.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Set under 5 ms on the actor; disk write off the actor.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] All migration fixtures match Electron's output.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 025.
