# Prompt 061 — NDI output

**Phase:** 4. Output engine  
**Tracker ID:** `061-ndi-sink`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Send NDI with parity to decisions 001 and 008.

## 2. Prerequisites

- Prompt [005](./005-spike-sinks.md) is marked `done`.
- Prompt [051](./051-compositor-core.md) is marked `done`.
- Prompt [056](./056-audio-engine.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/main/services/ndi/index.ts` — scheduling, heartbeat frames, retries, status
- `../kairo-presenter/src/main/services/ndi/provider.ts` — interface
- `../kairo-presenter/src/main/services/ndi/provider-loader.ts` — loading
- `../kairo-presenter/src/main/services/ndi/__tests__/ndi-service.test.ts` — behaviours
- `../kairo-presenter/src/main/services/ndi/__tests__/provider-loader.test.ts` — behaviours
- `../kairo-presenter/src/main/services/output/gpu-capture.ts` — frame flow
- `../kairo-presenter/src/main/services/gpu-frames/index.ts` — converter service
- `../kairo-presenter/src/main/services/gpu-frames/__tests__/gpu-capture.test.ts` — behaviours
- `../kairo-presenter/native/gpu-frames/tests/` — golden colour and ring tests to carry over
- `../kairo-presenter/native/gpu-frames/build.mjs` — build rules for the retained Metal code
- `../kairo-presenter/docs/decisions/001-cross-platform-ndi.md` — licensing list

## 4. Implementation requirements

1. Implement WGSL conversion to UYVA matching `uyvy.h` byte for byte. Use async readback with a ring of staging buffers, dropping frames rather than queueing.
2. One sender per output; keep the primary NDI source name, name-clash retry with cooldown, transparent clear frames, and repeat frames at the fixed cadence.
3. NDI audio. No SDK files in the repository. Runtime placement and attribution per decision 001.
4. The first NDI output uses `NDI_SENDER_NAME`, so existing receivers still find it, and is the app-lifetime sender started at launch. Other outputs are `Kairo <name>` and restart when renamed. Report per-output status with the existing reason text.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- NDI sink, tests.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Golden colour vectors. Ten start-stop cycles. A real receiver if available.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.
- Run `kairo-engine` for real and confirm output visually. Headless tests alone do not finish this prompt.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Measure the same p50 and p99 as decision 008, against 0.6 ms and 2.2 ms.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Sender appears on the network, or its absence is recorded as an unverified item.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 065, 110.

## Amendment from prompt 008

Use libloading over the official NDI 6 SDK. Verify with a real receiver.
