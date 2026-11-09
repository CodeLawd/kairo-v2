# Prompt 000 — Create the native workspace and agent tooling

**Phase:** 0. Foundations and gating spikes  
**Tracker ID:** `000-workspace-bootstrap`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Turn this repository (`kairo-gpui-kit`) into the Cargo workspace for the native app, so all later prompts have a place to build, test and record results. The Electron app stays in `../kairo-presenter`. Nothing here renders Kairo yet.

## 2. Prerequisites

- None. This is the first prompt.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `native-migration/03-native-architecture.md` — section 3 is the exact workspace layout
- `../kairo-presenter/CLAUDE.md` — repository rules that remain in force while Electron still ships
- `../kairo-presenter/.github/workflows/ci.yml` — see how CI is organised before adding a job

## 4. Implementation requirements

1. If this folder is not yet a git repository, run `git init`. It currently holds a single-package GPUI Kit counter starter (`Cargo.toml`, `src/main.rs`, `README.md`, `.gitignore`); convert it, moving the window code into `apps/kairo`. Make the root `Cargo.toml` a resolver 2 workspace, edition 2024, with an empty member for every crate and app listed in architecture section 3. Each crate gets `src/lib.rs` (or `main.rs`) with a one-line doc comment only.
2. Pin `gpui-kit = "=0.9.0"` exactly, with `[workspace.dependencies]` as the only place versions appear. Commit `Cargo.lock`.
3. Add `rust-toolchain.toml` using the channel GPUI Kit's installation guide requires (at audit time, Rust 1.92 or later). Read `Cargo.toml` of gpui-kit 0.9.0 to confirm.
4. Install the GPUI Kit agent skills with `npx skills add longbridge/gpui-kit` so `gpui-kit` and `gpui-kit-design-guides` are available. If the command needs approval, ask the operator. Record where they landed in `native-migration/DECISIONS.md`.
5. Rewrite `README.md` with build and test commands and the two-repo layout. Keep `/target/` in `.gitignore`.
6. Add `.github/workflows/ci.yml`: macOS, Windows and Ubuntu jobs running fmt, clippy `-D warnings`, and tests. Install GPUI's Linux packages per its installation guide. CI must not need the Electron repo: it uses cached oracle results and committed goldens.
7. Open a hello-world window in `apps/kairo` with `gpui_kit::application()`, `gpui_kit::init(cx)` and `gpui_kit::open_window`, to prove the toolchain.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- No Kairo features. Do not add dependencies a later prompt will choose.
- Do not edit anything in `../kairo-presenter`.
- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- Workspace skeleton, `ci.yml`, updated PROGRESS.md and DECISIONS.md.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Run `cargo build --workspace` and `cargo run -p kairo`; capture a screenshot of the window.
- Confirm `git -C ../kairo-presenter status --short` is unchanged by this work.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Record clean and incremental build time for `apps/kairo` in DECISIONS.md, so later slowdowns are visible.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] The window opens on the development machine, and CI is green on all three operating systems or the failure is logged as a blocker.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 001, and every other prompt.
