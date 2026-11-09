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

## Copied native sources

| Path here | Electron path | Source commit | Notes |
|---|---|---|---|
