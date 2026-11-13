# 060 display sinks implementation

Spec: ../prompts/060-display-sinks.md. Electron reference de3bb54..af978c5,
installed Electron 33.4.11 (Chromium 130). No drift in the six listed paths.
Prerequisites 051 and 054 are done. Preserve pending 059 changes.

1. Display identity and lifecycle: native Electron-compatible Mac/Windows ids,
   friendly names, logical sizes for saved bindings, control-display reporting,
   immediate removal safety and 250 ms quiet-period reconcile. Persist rematches
   through supervisor state and a narrowly scoped store helper (065 owns routing).
   Tests: scaled/ambiguous/claimed bindings, debounce bursts, control moves,
   rematch/replay and platform hash vectors.
2. Stage surfaces: SetStage payload through protocol/reducer/replay into existing
   stage renderer; same lifecycle/placement as screens. Tests: payload/replay,
   dedup, clock/countdown, rebuild and command-family acceptance.
3. Physical windows, diagnostics and power: platform window policy, native display
   notifications, display-sleep assertion tied to actual windows; transient
   identify (3 s) and shape test (5 s), newest request replaces same display.
   Tests: geometry/content, expiry/replacement, failed/missing displays, resource
   release; compare raster plans to extracted Electron HTML and run real windows.
4. GPUI control reporter: report native display on create/move/metrics; supervisor
   retains it for replay; bootstrap composes this without taking feature ownership.
   Verify against pinned GPUI Kit source; test reporting and real two-process flow.
5. Verification: formatter, workspace Clippy/tests, native visual run, timing;
   fresh-context final review and one tested correction pass. Owner can connect
   second display; record plug/unplug, sleep/wake, resolution and control moves in
   qualification.md. Do not claim physical actions or numbers not observed.

Review focus: stale/ambiguous ids, immediate unplug safety, mixed scale factors,
controller-not-yet-known safety, transient callback ownership, stage rebuild,
assertion lifecycle, replay ordering, genuine vsync pacing.

Execution ledger
- Started: current feature branch native-056-audio-engine carries the preceding
  task's pending changes. Continue this owner-selected workspace, preserving
  those changes; do not stage or commit unrelated work.
- Graphify snapshot e263079 predates most engine code; current local sources govern.
- Native control reporting must translate GPUI's Windows HMONITOR to Chromium's
  adapter/target PersistentHash id; Mac GPUI id already is CGDirectDisplayID.
