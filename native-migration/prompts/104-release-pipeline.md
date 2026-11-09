# Prompt 104 — Release pipeline

**Phase:** 6. Hardening and cutover  
**Tracker ID:** `104-release-pipeline`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Move release automation to the native build, keeping the same safety properties.

## 2. Prerequisites

- Prompt [101](./101-package-macos.md) is marked `done`.
- Prompt [102](./102-package-windows.md) is marked `done`.
- Prompt [103](./103-package-linux.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/.github/workflows/release.yml` — coordinated publish
- `../kairo-presenter/.github/workflows/ci.yml` — checks
- `../kairo-presenter/scripts/release.mjs` — release flow
- `../kairo-presenter/scripts/draft-changelog.mjs` — changelog drafting
- `../kairo-presenter/scripts/dev-processes.mjs` — dev tooling
- `../kairo-presenter/docs/release-checklist.md` — checklist
- `../kairo-presenter/website/src/lib/changelog.ts` — changelog consumed by the site and app
- `../kairo-presenter/tests/dev-processes.test.ts` — expectations
- `../kairo-presenter/website/src/app/api/download/[platform]/route.ts` — asset names the site matches
- `../kairo-presenter/server/src/admin/admin.service.ts` — installer filename check

## 4. Implementation requirements

1. Matrix builds on native runners, signing when secrets exist, publication only after all jobs succeed. Same asset names and `latest*.yml` shape. CHANGELOG requirement stays.
2. Build-time API URLs come from the same variables.
3. DeckLink helper prebuilt verification stays.
4. Asset names must keep matching: `Kairo-<v>-arm64.dmg`, `Kairo-<v>.dmg`, `Kairo-Setup-<v>.exe`, `Kairo-<v>.AppImage`, `-mac.zip`. `KAIRO_ELECTRON_ARCHIVE` retires with Electron.
5. This workflow lives here, but deployed apps and the website read releases from `CodeLawd/kairo-presenter`. Publish there using a scoped token. Until prompt 112 is approved, native builds go only to a private feed or draft release, never as a latest release, and Electron keeps publishing normally. Agree one version sequence, so native versions sort above Electron's at cutover.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- Native release workflow, gated on a manual trigger until prompt 112.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- A dry run to a draft release on a test tag. Deliberately fail one job and prove nothing publishes.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Cache to keep builds practical; record times.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Dry run passes.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 105.
