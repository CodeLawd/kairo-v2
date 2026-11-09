# Prompt 046 — Audience rooms and Engage

**Phase:** 3. Services  
**Tracker ID:** `046-audience-rooms`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Port live Q&A and polls against Cloudflare rooms.

## 2. Prerequisites

- Prompt [030](./030-cloud-client.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/main/services/audience/index.ts` — exports
- `../kairo-presenter/src/main/services/audience/service.ts` — state
- `../kairo-presenter/src/main/services/audience/source.ts` — source interface
- `../kairo-presenter/src/main/services/audience/sources/kairo.ts` — WebSocket, heartbeat
- `../kairo-presenter/src/main/services/audience/sources/sample.ts` — demo source
- `../kairo-presenter/server/cloudflare/audience-room.ts` — server behaviour
- `../kairo-presenter/server/cloudflare/engage-routes.ts` — server behaviour
- `../kairo-presenter/tests/audience-service.test.ts` — expectations
- `../kairo-presenter/tests/engage-socket-recovery.test.ts` — expectations
- `../kairo-presenter/scripts/test-audience-local.mjs` — local harness
- `../kairo-presenter/scripts/test-audience-room.mjs` — local harness
- `../kairo-presenter/scripts/test-engage-room.mjs` — local harness

## 4. Implementation requirements

1. Port over `tokio-tungstenite` with the same reconnect, heartbeat and request-ack rules. Exercise against the local Worker.
2. QR codes via the `qrcode` crate, matching current scan behaviour.
3. Port the Engage REST flows in `src/main/ipc/index.ts`: list, create, resume (optional reopen), reopen, delete, invite, revoke, with the code pattern check and 90 s timeouts. The sample source runs only in development with `KAIRO_AUDIENCE_SAMPLE=1`.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- `kairo-audience`, tests.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Run against `server` audience dev Worker: create, join, ask, vote, moderate, poll, reconnect.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Snapshot up to `AUDIENCE_MAX_SNAPSHOT_BYTES` parsed off the actor.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Inventory `audience` items done, except Slido.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 047, 054, 090.
