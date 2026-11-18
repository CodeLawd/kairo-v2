# 108 — Scripture detection latency plan

Baseline: `eef3611`. Source review: Codex latency audit (2026-11-17), claims
re-verified against the code before this plan was written.

The local parser is already fast (detector p99 ≈ 1 ms debug, 0.25 ms
release). The work below removes blocking dependencies, stops stale results
from reaching projection, and measures the real path. No regex rewrite, no
SQLite replacement, no vector store, no added model concurrency.

## Verified findings

| #   | Finding                                                                                                                                                                                         | Location                                                                                                |
| --- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| F1  | The detector event pump awaits `handle_detection`, which awaits each Bible lookup in turn; a slow lookup blocks newer detections and retractions.                                               | `crates/kairo-orchestrator/src/orchestrator.rs` (`create_detector`, `handle_detection`)                 |
| F2  | A queued quote search displaced by a newer one resolves `Ok(vec![])`, indistinguishable from "no match"; a final displaced by an interim skips local quote search and goes straight to AI.      | `crates/kairo-scripture/src/quote/worker.rs` (`search`), `apps/kairo/src/quote_detector.rs` (`publish`) |
| F3  | STT fan-out awaits `records.transcript` before forwarding a final to detection.                                                                                                                 | `apps/kairo/src/services.rs` (STT fan-out task)                                                         |
| F4  | Reconnect replay drains buffered audio with an unbounded `socket.send().await` loop and replays audio of any age.                                                                               | `crates/kairo-stt/src/deepgram.rs` (socket open handler)                                                |
| F5  | `ScriptureTraces` is constructed but never started in production; `show_slide` reports acceptance, not frame commit.                                                                            | `apps/kairo/src/services.rs`, `crates/kairo-orchestrator/src/engine.rs`                                 |
| F6  | Local evidence providers run `block_in_place` + `block_on` while the detector holds its state mutex.                                                                                            | `apps/kairo/src/services.rs` (`Evidence`)                                                               |
| F7  | Explicit references from a single interim are authoritative immediately (confidence 0.9) and can auto-project.                                                                                  | `crates/kairo-scripture/src/live_detector.rs`, `crates/kairo-orchestrator/src/detector.rs`              |
| F8  | AI results are published with an empty quote-evidence slice, so AI `Quote` results can never be accepted; explicit results are checked against model-supplied `sourceText`, not the transcript. | `crates/kairo-orchestrator/src/detector.rs`, `crates/kairo-scripture/src/live_detector.rs`              |

## Stages

Each stage is independently shippable. Stages 1 and 2 have no dependency on
each other. Stages 4–6 wait for Stage 2 data to confirm they are still the
largest remaining cost.

### Stage 1 — Quick wins (F2, F3, F4)

- Quote worker `search` returns `QuoteSearchOutcome::{Found, Superseded}`;
  a corpus version change mid-search is also `Superseded`.
- Quote detector: a superseded provisional search is dropped silently (a
  newer search is already queued); a superseded final search is retried once
  before it falls back to the managed detector. Errors keep the existing
  fallback.
- STT fan-out forwards finals to detection before archiving; archiving moves
  to a dedicated ordered task.
- Deepgram replay: each send is bounded by the normal send timeout.
- Replay audio is kept, not aged out: dropping it would also drop it from the
  service transcript. Marking replayed results as not-live moved to Stage 4.

Validation: worker and detector unit tests for superseded handling; existing
quote, STT and orchestrator suites pass.

### Stage 2 — Latency traces (F5)

- Start a `ScriptureTraces` trace per detection at STT receipt; carry its id
  through suggestion and engine command.
- Stamped stages: lookup start/end, suggestion published, suggestion
  rendered (existing UI hook), countdown start/end, presenter request
  start/end. Completion: `presented` with `request-accepted`, `failed`
  (verse not found, presenter rejected), `cancelled` (stopped, already on
  screen) or `superseded` (quote withdrawn). Read in Settings → Scripture.
- Deferred: correlating engine `FrameCommitted` back across the engine
  process; capture-sample timing; cancelling traces of countdowns that a
  newer countdown or a dismissal replaced (they stay `active`).

### Stage 3 — Decouple lookup from the event pump (F1)

- The pump spawns detection handling, bounded by a two-permit semaphore, and
  keeps receiving events.
- A detection revision is recorded in orchestrator state. A lookup that
  completes after a newer explicit detection publishes as suggestion-only and
  never starts an auto countdown.
- Validation: injected slow lookup does not delay a later detection;
  obsolete lookup cannot auto-project.

### Stage 4 — Interim gating (F7)

- An explicit reference seen on an interim auto-projects only after a second
  agreeing interim or a final; lookup still starts on the first interim.
  The live detector's dedup cache suppresses the confirming emission today,
  so this needs a confirmation signal from detector to orchestrator that
  arms the existing card's countdown instead of adding a second card.
- Speech context is cleared when audio reconnects after a discontinuity.
- Results transcribed from replayed audio are marked not-live and never
  auto-project.

### Stage 5 — Evidence off the detector lock (F6)

- Parsing stays synchronous; chapter and quote evidence fetches move to async
  tasks applied only if the detector revision is unchanged.
- Prefetch chapter evidence when book and chapter become known.

### Stage 6 — Narrow the AI contract (F8)

- Prompt targets unresolved citation repair only.
- Validate returned references against the actual transcript text.
- One overall deadline per work item; cancel superseded work.

### Deferred

20 ms audio chunks, semantic retrieval, extra model concurrency, reducing the
one-second auto countdown. Revisit only with Stage 2 data.

## Status

| Stage | Status                       |
| ----- | ---------------------------- |
| 1     | Done                         |
| 2     | Done (frame commit deferred) |
| 3     | Done                         |
| 4     | Not started                  |
| 5     | Not started                  |
| 6     | Not started                  |
