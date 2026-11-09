# Prompt 112 — General release and Electron retirement

**Phase:** 6. Hardening and cutover  
**Tracker ID:** `112-cutover`

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

Release the native app and retire Electron in a recoverable order.

## 2. Prerequisites

- Prompt [105](./105-bridge-release.md) is marked `done`.
- Prompt [111](./111-pilot.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `native-migration/08-production-readiness-checklist.md` — must be fully checked
- `../kairo-presenter/CLAUDE.md` — update for the native architecture
- `../kairo-presenter/AGENTS.md` — update for the native architecture
- `../kairo-presenter/README.md` — update
- `../kairo-presenter/src/renderer/src/env.d.ts` — retired with the renderer
- `../kairo-presenter/electron.vite.config.ts` — retired
- `../kairo-presenter/tsconfig.json` — keep only as needed by remaining TS tools and server
- `../kairo-presenter/tsconfig.node.json` — review
- `../kairo-presenter/tsconfig.web.json` — review
- `../kairo-presenter/postcss.config.js` — retired
- `../kairo-presenter/src/index.css` — leftover scaffold stylesheet; confirm unused, then retire
- `../kairo-presenter/.eslintrc.json` — review
- `../kairo-presenter/.eslintignore` — review
- `../kairo-presenter/premium-audit.json` — review
- `../kairo-presenter/plans/` — historic plans; keep
- `../kairo-presenter/design/` — design reference; keep
- `../kairo-presenter/video/` — marketing project; unrelated
- `../kairo-presenter/scripts/test-setup.cjs` — retired with TS tests
- `../kairo-presenter/scripts/seed-bible.ts` — tool that needs a new runtime
- `../kairo-presenter/scripts/build-bible-pack.ts` — tool that needs a new runtime
- `../kairo-presenter/scripts/import-translation.ts` — tool that needs a new runtime

## 4. Implementation requirements

1. Every step requires explicit owner approval: enable the bridge feed, stage rollout, monitor crash reports, keep Electron build reproducible on a tag for at least one release cycle.
2. After stabilising, retire Electron from `../kairo-presenter` on a separate branch, in reviewable commits. `server/`, `website/` and the Bible tooling stay there unless the owner decides to move them. Editing that repository is allowed in this prompt only, with approval for each step.
3. `seed:bible`, `build:bible-pack` and `import:translation` run on Electron-as-Node today. Before removing Electron, move them to a plain Node or Bun SQLite runtime, and confirm identical database output.
4. Before the final build, run prompt 009's sync and agree a short Electron feature freeze with the owner.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- Do not delete anything until the owner says so.
- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- Release, updated repository docs.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Post-release checks; rollback rehearsal before enabling.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Field telemetry within targets.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Owner confirms the migration is complete.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- None. Migration complete.
