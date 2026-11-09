# 05 — Dependency-aware migration roadmap

The complete prompt list, with dependencies, is in `START_HERE.md`. This page explains
the order.

## Phase map

```
Phase 0  Spikes and go/no-go ──▶ human gate 008
              │
Phase 1  kairo-domain (pure logic, verified against TypeScript)
              │
Phase 2  Persistence and AppCore runtime
              │
      ┌───────┴───────┐
Phase 3  Services    Phase 4  Output engine         (can overlap)
      └───────┬───────┘
Phase 5  Operator interface
              │
Phase 6  Migration, packaging, soak, qualification, pilot, cutover
```

| Phase | Prompts | Exit criterion |
|---|---|---|
| 0. Foundations and spikes | 000–008 | Owner records GO in PROGRESS.md |
| 1. Domain model | 010–016 | `check-lib-coverage` passes; no differential failures |
| 2. Persistence and runtime | 020–025 | Real Electron data loads; a sample service reaches a GPUI view |
| 3. Services | 030–048 | All non-UI API inventory items done; corpus replay shows no differences |
| 4. Output engine | 050–067 | A scripted service runs on real outputs; goldens green |
| 5. Interface | 070–091 | `check-api-coverage` reports nothing left |
| 6. Hardening and cutover | 100–112 | Readiness checklist complete; owner approves |

## Why this order

- **Spikes first.** Four assumptions could change the whole design: process separation,
  text fidelity, video decode and the web host. Finding out after porting 30,000 lines
  would be too late.
- **Domain before services.** The differential oracle gives every later port a trusted
  foundation.
- **Persistence before services.** Every service reads settings. Compatibility with real
  user data is the migration's hardest promise, so it is proven early.
- **Engine independent of UI.** It has a fake-driven test harness and can start as soon
  as prompt 025 and the spikes are done.
- **UI last, in workflow order.** Shell → launch → settings → Screens → Operator, since
  each one needs the last to be usable. A UI prompt never starts without its service.
- **Lyrics after Scripture.** Scripture proves the grid and thumbnail patterns on a simpler
  screen before the largest view is attempted.

## Critical path

```
000 → 002 → 008 → 012 → 021 → 025 → 050 → 051 → 052 → 065 → 066 → 076 → … → 091
→ 100 → 104 → 105 → 109 → 110 → 111 → 112
```

Detector parity (035–037) and Electron-to-native bridging (105) are the highest-risk items
outside that path.

## Parallel work

Prompts may run at the same time if each one's prerequisites are done **and** they do not
touch the same crate. Good pairs: phase 3 service prompts on different crates, 061/062/063
sinks, UI workspaces after 076. Do not run two prompts on the same `ui-*` crate, or on
`kairo-domain`, at once. Parallel agents must use separate git worktrees, and each updates
only its own PROGRESS.md row.

## Effort

No calendar estimate is given, because it would not be evidence-based. After phase 0, use
the actual time per prompt recorded in PROGRESS.md to project the remainder.

## Keeping Electron alive

Electron, in `../kairo-presenter`, stays the shipping product until prompt 112 and keeps
receiving fixes and features. Prompt 009 builds the sync tool. It queues each Electron change
against the prompt that owns it, including prompts already finished. See `SYNC.md`.

## Milestones worth demonstrating

1. After 016: the Rust domain matches TypeScript on generated data.
2. After 039: a recorded sermon drives the Rust detector, with a fake output.
3. After 052: a Chromium slide and a native slide shown side by side.
4. After 065: a full program runs on real screens from a test script, with no UI.
5. After 077: an operator can run a scripture-only service.
6. After 091: complete app, unpackaged.
7. After 111: real Sundays.
