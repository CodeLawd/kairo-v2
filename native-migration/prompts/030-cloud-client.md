# Prompt 030 — Cloud API client and session

**Phase:** 3. Services  
**Tracker ID:** `030-cloud-client`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Port sign-in and the failover HTTP client against the unchanged Kairo server.

## 2. Prerequisites

- Prompt [022](./022-secure-storage.md) is marked `done`.
- Prompt [025](./025-core-runtime.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/main/services/cloud/api-client.ts` — timeouts, failover, single-flight refresh
- `../kairo-presenter/src/main/services/cloud/session.ts` — session state machine, secrets hydration
- `../kairo-presenter/src/main/services/cloud/managed.ts` — managed provider routing
- `../kairo-presenter/tests/cloud-api-failover.test.ts` — expectations
- `../kairo-presenter/tests/cloud-pairing-session.test.ts` — expectations
- `../kairo-presenter/tests/managed-providers.test.ts` — expectations
- `../kairo-presenter/server/src/auth` — server behaviour

## 4. Implementation requirements

1. Port with `reqwest` (rustls). Keep header `X-PA-Client: desktop`, the 8 s default timeout, primary and fallback host selection, and refresh deduplication.
2. Port sign-up, sign-in, sign-out, verification, password reset, device pairing and org-secret pull.
3. Verify against a local server: `cd server && bun run start:dev`.
4. Port `managedRequest`, which every paid provider call uses (`POST /v1/managed/request`). The desktop holds no provider keys.
5. Port `syncOrgProfile`, `engageRequest`, `pullOrgSecrets` throttling, the stable device id in `<userData>/cloud/device.json`, reconnect on OS resume, and the `{ ok, data, error }` result that delivers server messages to form fields.
6. Confirm proxy and certificate behaviour against today's axios and `electron.net` paths, and record any difference.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- Do not change the server or its contract.
- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- `kairo-net`, `kairo-cloud` session, tests with a mock server, including failure injection.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Refresh rotation and reuse detection flows against the real local server e2e.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- No cloud call may block operator actions.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] All account API items in the inventory are done.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 031, 073.
