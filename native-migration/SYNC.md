# Electron sync

The Electron app in `../kairo-presenter` keeps changing during the migration. This file
records what has been carried over. `tools/electron-sync` (prompt 009) maintains it.

**Last-synced commit:** `af978c5` (advanced by 109, 2026-11-16; uncommitted Electron changes then were only in `website/` and `plans/`, outside the native app)
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
| `af978c5159` | `src/main/services/output/overlay.html` | 003, 004, 051, 053, 054, 055, 056, 057, 058, 059 | no impact | done (109): the engine already decodes the page before showing it, ignores stale turns, and keeps the page and document videos when a decode fails (`program.rs` `swap_page`, `app.rs` swap answer; `apps/kairo-engine/tests/documents.rs`) | — |
| `af978c5159` | `src/renderer/src/App.tsx` | 071 | port needed | done (109): already in `ui-shell` (header "Lyrics Search" opens song search) | — |
| `af978c5159` | `src/renderer/src/components/layout/AppShell.tsx` | 007, 071 | port needed | done (109): `crates/ui-shell/src/view.rs` "Lyrics Search" / "Search lyrics" | — |
| `af978c5159` | `src/renderer/src/components/media/MediaDock.tsx` | 083 | port needed | ported (083, from the source at `af978c5`) |  |
| `af978c5159` | `src/renderer/src/components/screens/ScreenConfiguration.tsx` | 075 | port needed | done (109): detail header delete button removed; removal stays in the list menu, as Electron's list keeps `removeScreen` (native had no header on/off switch) | 109, uncommitted |
| `af978c5159` | `tests/document-page-swap.test.ts` | 045, 058, 109 | test mapping | done (109): mapped to `page_turns_answer_and_never_end_on_a_stale_page` (stale turns, undecodable and missing pages) | — |
| owner (113) | `src/renderer/src/components/documents/Documents.tsx`, `scripture/VerseThemePreview`, `screens/ScreenConfiguration.tsx` | 113 | deliberate deviation | keep: no Documents preview, themed square verse cards, "External display" name (DECISIONS 113). Do not port back from Electron | 113, uncommitted |

## Copied native sources

| Path here | Electron path | Source commit | Notes |
|---|---|---|---|
| `spikes/sinks/native/ring.h` | `native/gpu-frames/ring.h` | `de3bb54` | Spike copy, unmodified |
| `spikes/sinks/native/uyvy.h` | `native/decklink/uyvy.h` | `de3bb54` | Spike copy, unmodified |
| `spikes/sinks/native/metal_core.inc` | `native/gpu-frames/gpu_frames.mm` | `de3bb54` | Shader and Metal section only, unmodified |
| `native/decklink/` | `native/decklink/` (without `build/`) | `1067495` (tree checked at `af978c5`) | Unmodified, including `prebuilt/decklink-helper` and `prebuilt/SOURCE_SHA256`; the hash matches the copied sources (test `the_prebuilt_helper_matches_its_recorded_sources`). Prompt 062 |
| `native/gpu-frames/ring.h` | `native/gpu-frames/ring.h` | `8754529` | Unmodified; included by `native/decklink/helper.cpp`. Prompt 062 |

## Running it

```bash
cargo run -p electron-sync -- --write
```

Add `--advance` to move the last-synced commit once items are resolved, and `--apply-generated` to regenerate derived files. Queue status values: `open`, `not-started`, `no-impact`, `done`. Mark an item `done` with the native commit after porting it.
