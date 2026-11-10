# Plan 002: Stop the orchestrator holding its state lock across network awaits

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat a2628bd..HEAD -- crates/kairo-orchestrator/src/orchestrator.rs crates/kairo-orchestrator/tests/orchestrator.rs`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MED
- **Depends on**: none
- **Category**: bug
- **Planned at**: commit `a2628bd`, 2026-11-10

## Why this matters

`Orchestrator` keeps all session state behind one `tokio::sync::Mutex` (`inner.state`).
Two paths hold that guard across slow network awaits: `start()` awaits STT start
(Deepgram socket connect) and `handle_detection()` awaits Bible lookups, one per
requested translation, sequentially. While held, `stop()`, `status()`, approvals,
auto timers and the transcript pump all block. On bad venue Wi-Fi the operator cannot
stop a session. The code already anticipates release — `handle_detection` re-checks
"A lookup can finish after stop" — but that check can never fire while the lock is held.
(A 10 s connect timeout was added to Deepgram in `a2628bd`, which bounds but does not
remove the stall.)

## Current state

- `crates/kairo-orchestrator/src/orchestrator.rs`
  - `start` (~line 464):

    ```rust
    pub async fn start(&self, config: OrchestratorConfig) {
        let mut state = self.inner.state.lock().await;
        if state.running { self.stop_locked(&mut state); }
        ...
        self.configure_stt(&config);
        self.start_stt(&mut state).await;      // awaits network while locked
        self.attach_audio(&mut state);
        state.session = Some(Session { ... });
        state.running = true;
        ...
    }

    async fn start_stt(&self, state: &mut State) {
        match self.inner.services.stt.start().await {
            Ok(()) => state.update_health(ServiceName::Stt, HealthStatus::Ok, None),
            Err(error) => { ...; state.update_health(ServiceName::Stt, HealthStatus::Error, Some(error)); }
        }
    }
    ```

  - `handle_detection` (~line 849): takes `let mut state = self.inner.state.lock().await;`
    at the top, loops `for (reference, requested) in requests`, and inside awaits
    `kairo_scripture::api_bible::detection::lookup_with_translation_fallback(...)`,
    whose closure calls `scripture.search(query, candidate).await`. After the await:

    ```rust
    // A lookup can finish after stop or after another service started.
    if !state.running || state.session.as_ref().map(|s| s.session_id.clone()) != session_id {
        return;
    }
    ```

- Tests: `crates/kairo-orchestrator/tests/orchestrator.rs` (uses `#[tokio::test(start_paused = true)]`
  and fake services). Match its fake-service style for new tests.
- Behaviour parity: results must be processed in the original request order (TS ran
  them sequentially), so keep ordering when processing even if lookups run concurrently.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Format check | `cargo fmt --all -- --check` | exit 0 |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |
| Tests (crate) | `cargo test -p kairo-orchestrator` | all pass |
| Tests (all) | `cargo test --workspace` | 0 failed |

## Scope

**In scope**: `crates/kairo-orchestrator/src/orchestrator.rs`, `crates/kairo-orchestrator/tests/orchestrator.rs`.

**Out of scope**: `kairo-stt` and `kairo-scripture` APIs; the detector (`detector.rs`);
the resilience duplication (separate finding). Do not change emitted events or their order.

## Git workflow

- Branch: `advisor/002-orchestrator-lock`
- Commit message e.g. `Release orchestrator state lock across STT start and lookups`.
- Do NOT push.

## Steps

### Step 1: `start` — release the lock around `stt.start()`

Restructure: under the lock do everything up to `configure_stt`, set a "starting" marker
(e.g. a `start_generation: u64` counter incremented here and stored in a local),
drop the guard, `await self.inner.services.stt.start()`, re-lock, and if the
generation changed or `stop()` ran meanwhile, return without marking running (call
`stt.stop()` if start succeeded). Otherwise apply health, `attach_audio`, session,
`running = true`, emit status as today. Keep `start_stt` as a helper that takes the
result rather than awaiting under the lock.

**Verify**: `cargo test -p kairo-orchestrator` → all pass.

### Step 2: `handle_detection` — lookups outside the lock

Under the lock, compute `cfg`, `session_id`, `requests`, and per-request inputs that
read state (plan entries, translation choice). Drop the guard. Run the lookups
(may stay sequential first; optionally `futures_util::future::join_all` preserving
order). Re-lock, then process each result in original order with the existing
session/running check (which now does real work). Anything after the lookup that
touches `state` stays under the re-acquired lock.

**Verify**: `cargo test -p kairo-orchestrator` → all pass.

## Test plan

In `crates/kairo-orchestrator/tests/orchestrator.rs`, using the existing fakes:
1. Fake STT whose `start()` never resolves (pending future): `status()` and `stop()`
   complete within a paused-time timeout of 1 s.
2. Fake scripture search that awaits a `Notify`: while pending, `stop()` completes; after
   release, no card is emitted (session check fires).
3. Two translations requested, lookups resolve in reverse order: cards emitted in
   request order.

## Done criteria

- [ ] fmt, clippy (`-D warnings`) exit 0
- [ ] `cargo test --workspace` 0 failed; 3 new tests pass
- [ ] `grep -n "start_stt(&mut state).await" crates/kairo-orchestrator/src/orchestrator.rs` → no match
- [ ] Only in-scope files modified
- [ ] `plans/README.md` row updated

## STOP conditions

- An existing orchestrator test changes event order after the change.
- The fakes cannot model a pending `stt.start()` without changing `kairo-stt` types.
- You find another `.await` under the state lock that needs restructuring beyond these two paths — list it, don't fix it.

## Maintenance notes

- Rule for this file going forward: never `.await` a service call while holding `inner.state`.
- Reviewer: check every early `return` after re-locking leaves STT/audio consistent.
