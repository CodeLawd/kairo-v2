# Prompt 056 — Program audio, routing and tracks

**Phase:** 4. Output engine  
**Tracker ID:** `056-audio-engine`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Rebuild everything `<audio>`, `setSinkId` and Web Audio did.

## 2. Prerequisites

- Prompt [055](./055-video-engine.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/main/services/output/overlay.html` — audio graph, NDI tap, sink routing, camera audio
- `../kairo-presenter/src/preload/program.ts` — NDI audio bridge
- `../kairo-presenter/src/main/services/output/surface-manager.ts` — one audio surface rule
- `../kairo-presenter/src/renderer/src/components/tracks/TracksPlayer.tsx` — house audio playback
- `../kairo-presenter/src/renderer/src/components/media/ProgramAudioFallback.tsx` — fallback playback rules
- `../kairo-presenter/src/renderer/src/stores/useTracksPlaybackStore.ts` — state
- `../kairo-presenter/src/main/services/ndi/index.ts` — sendAudio

## 4. Implementation requirements

1. `kairo-audio`: mixer of video, document video, camera, web and tracks sources, with per-source gain. Output to the device matching `outputLabel`; match by label as today, with default fallback; hot-plug handling.
2. Only the configured audio surface makes sound. Preserve the rule: no echoes from multiple outputs. Program fallback plays when only hardware outputs exist.
3. NDI audio tap at 48 kHz planar float.
4. Tracks: play, pause, stop, progress.
5. NDI audio is 1024-sample stereo blocks. Keep the validation in `handleNdiAudio` (1 to 8 channels, up to 16384 samples, 8 to 192 kHz) wherever data crosses a boundary. The tap does no work when NDI audio is off.
6. The audio surface is the first live screen, else the primary NDI surface, and lobby screens are never it.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- Realtime callbacks do not lock or allocate.
- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- `kairo-audio`, tests.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Null-sink tests with known tones; device-change simulation; sum levels and sync.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.
- Run `kairo-engine` for real and confirm output visually. Headless tests alone do not finish this prompt.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Audio glitches zero over a 60-minute soak.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Inventory `tracks` playback and program audio items done.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 057, 061, 084.
