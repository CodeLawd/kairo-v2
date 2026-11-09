# Prompt 059 — Live web layer

**Phase:** 4. Output engine  
**Tracker ID:** `059-web-layer`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Put live web pages on outputs from a single shared browser instance.

## 2. Prerequisites

- Prompt [006](./006-spike-web-host.md) is marked `done`.
- Prompt [051](./051-compositor-core.md) is marked `done`.
- Prompt [056](./056-audio-engine.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/main/services/web/index.ts` — prepareScreenWebFrame, watchWebPreview, scroll, mute
- `../kairo-presenter/src/main/services/output/overlay.html` — setWeb, reload, pointer
- `../kairo-presenter/src/renderer/src/components/web/WebPage.tsx` — operator expectations
- `../kairo-presenter/src/renderer/src/components/operator/ScaledWebFrame.tsx` — scale
- `../kairo-presenter/src/lib/web.ts` — address rules

## 4. Implementation requirements

1. Production `kairo-web-host`: one page per URL, frames in shm or shared texture, input forwarding, navigation events, audio routed to the audio engine, crash recovery, and clean helper shutdown.
2. Remove mirroring: scroll and pointer become direct input to the single page; draw the operator's pointer as an overlay above the page on outputs, as today.
3. Only the program page has framing headers removed.
4. Enforce the iframe's sandbox: scripts, same-origin, forms and presentation only, so no popups, downloads, top navigation, camera or microphone. Allow autoplay, fullscreen, encrypted media and picture in picture; referrer policy strict-origin-when-cross-origin; white backdrop. Port `rememberWebScroll`, the 56 px pointer with its tip offset, and clamped coordinates. Web shows only on outputs that show backgrounds and follow the program.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- Treat all messages from the web host as untrusted; validate sizes and bounds.
- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- `kairo-web-host` and layer integration, tests.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Local test site verifying framing bypass, scroll, pointer and audio. Kill the helper during live; verify recovery.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.
- Run `kairo-engine` for real and confirm output visually. Headless tests alone do not finish this prompt.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Web host absent when no page is live.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Inventory `program.setWeb` family done.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 089.
