# Electron sync

The Electron app in `../kairo-presenter` keeps changing during the migration. This file
records what has been carried over. `tools/electron-sync` (prompt 009) maintains it.

**Last-synced commit:** `de3bb54`
**Note:** when planning started, the tree also held uncommitted edits to `overlay.html`,
`App.tsx`, `AppShell.tsx`, `MediaDock.tsx`, `ScreenConfiguration.tsx`, and a new
`tests/document-page-swap.test.ts`. Treat those files as changed.

## When to run

Before each phase starts, and before prompts 100, 109 and 112. More often if Electron is
changing quickly. Prompt 008 confirms the cadence.

## Rules

- Advance the last-synced commit only when every change up to it is ported or marked no-impact.
- Uncommitted Electron changes are reported but never count as synced.
- A change to stored data must be readable by both apps, so rollback keeps working.
- Files with no owning prompt need a decision: assign one or add a new prompt.

## Queue

| Electron commit | File | Owning prompt | Class | Status | Native commit |
|---|---|---|---|---|---|
| `af978c5159` | `src/main/services/output/overlay.html` | 003, 004, 051, 053, 054, 055, 056, 057, 058, 059 | port needed | open |  |
| `af978c5159` | `src/renderer/src/App.tsx` | 071 | not started | not-started |  |
| `af978c5159` | `src/renderer/src/components/layout/AppShell.tsx` | 007, 071 | not started | not-started |  |
| `af978c5159` | `src/renderer/src/components/media/MediaDock.tsx` | 083 | not started | not-started |  |
| `af978c5159` | `src/renderer/src/components/screens/ScreenConfiguration.tsx` | 075 | not started | not-started |  |
| `af978c5159` | `tests/document-page-swap.test.ts` | 045, 058, 109 | not started | not-started |  |

## Copied native sources

| Path here | Electron path | Source commit | Notes |
|---|---|---|---|
| `spikes/sinks/native/ring.h` | `native/gpu-frames/ring.h` | `de3bb54` | Spike copy, unmodified |
| `spikes/sinks/native/uyvy.h` | `native/decklink/uyvy.h` | `de3bb54` | Spike copy, unmodified |
| `spikes/sinks/native/metal_core.inc` | `native/gpu-frames/gpu_frames.mm` | `de3bb54` | Shader and Metal section only, unmodified |

## Running it

```bash
cargo run -p electron-sync -- --write
```

Add `--advance` to move the last-synced commit once items are resolved, and `--apply-generated` to regenerate derived files. Queue status values: `open`, `not-started`, `no-impact`, `done`. Mark an item `done` with the native commit after porting it.
