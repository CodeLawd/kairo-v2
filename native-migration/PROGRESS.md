# Migration progress tracker

**Go / no-go:** not yet decided (set by prompt 008)
**Planning baseline commit (kairo-presenter):** `de3bb54`
**Current prompt:** 000

Status values: `todo`, `in-progress`, `blocked`, `done`.

Rules:

- One agent owns a row at a time. Set `in-progress` before changing anything.
- `done` means every gate passed. If something could not be verified, say so in Notes.
- `blocked` needs a reason in Notes, what was tried, and what would unblock it.
- Notes hold measured numbers and anything the next session must know.

## Session log

Append a line per session: date, prompt, outcome, next step.

| Date | Prompt | Outcome | Next step |
|---|---|---|---|
| 2026-11-09 | 000 | Local gates passed, CI unverified | Push the native workflow and verify all three jobs |

## Prompts

### 0. Foundations and gating spikes

| ID | Prompt | Status | Date | Branch | Notes |
|---|---|---|---|---|---|
| 000 | [Create the native workspace and agent tooling](./prompts/000-workspace-bootstrap.md) | blocked | 2026-11-09 | main | All 40 members resolved after removal of the dedicated web host. Local fmt, build, Clippy and test gates passed on macOS. Clean workspace build 153.02 s including downloads, unchanged app rebuild 1.02 s. Cargo launch and [window capture](./evidence/000/hello-world.png) verified using the built executable in a temporary app bundle for capture. Lockfile committed as `be7133a`. Electron status matched the initial snapshot. GitHub origin is configured. All three hosted CI results are pending verification. |
| 001 | [Measure the Electron baseline](./prompts/001-electron-baseline.md) | todo | | | |
| 002 | [Spike: separate engine process with output windows and preview](./prompts/002-spike-engine-process.md) | todo | | | |
| 003 | [Spike: match Chromium slide text](./prompts/003-spike-text-fidelity.md) | todo | | | |
| 004 | [Spike: video decode into wgpu](./prompts/004-spike-video.md) | todo | | | |
| 005 | [Spike: NDI, DeckLink ring and macOS native code from Rust](./prompts/005-spike-sinks.md) | todo | | | |
| 006 | [Spike: CEF web host for all browser needs](./prompts/006-spike-web-host.md) | todo | | | |
| 007 | [Spike: operator UI on GPUI Kit](./prompts/007-spike-ui-platform.md) | todo | | | |
| 008 | [Human checkpoint: go or no-go](./prompts/008-go-no-go.md) | todo | | | |
| 009 | [Electron-to-native sync process](./prompts/009-electron-sync.md) | todo | | | |

### 1. Domain model

| ID | Prompt | Status | Date | Branch | Notes |
|---|---|---|---|---|---|
| 010 | [Freeze the application API inventory](./prompts/010-api-inventory.md) | todo | | | |
| 011 | [Differential test harness against the TypeScript originals](./prompts/011-ts-oracle.md) | todo | | | |
| 012 | [Port the data types with on-disk compatibility](./prompts/012-domain-types.md) | todo | | | |
| 013 | [Port settings and theme normalizers](./prompts/013-domain-normalizers.md) | todo | | | |
| 014 | [Port lyrics logic](./prompts/014-domain-lyrics.md) | todo | | | |
| 015 | [Port scripture logic](./prompts/015-domain-scripture.md) | todo | | | |
| 016 | [Port program, media, transfer and remaining shared logic](./prompts/016-domain-program.md) | todo | | | |

### 2. Persistence and runtime

| ID | Prompt | Status | Date | Branch | Notes |
|---|---|---|---|---|---|
| 020 | [Paths, logging, crash handling and single instance](./prompts/020-paths-logging.md) | todo | | | |
| 021 | [Settings store and migrations](./prompts/021-settings-store.md) | todo | | | |
| 022 | [Secure storage and Electron secret migration](./prompts/022-secure-storage.md) | todo | | | |
| 023 | [SQLite access layer](./prompts/023-sqlite-layer.md) | todo | | | |
| 024 | [Workspace folders and song files](./prompts/024-workspace.md) | todo | | | |
| 025 | [AppCore, actors and event topics](./prompts/025-core-runtime.md) | todo | | | |

### 3. Services

| ID | Prompt | Status | Date | Branch | Notes |
|---|---|---|---|---|---|
| 030 | [Cloud API client and session](./prompts/030-cloud-client.md) | todo | | | |
| 031 | [Licence and onboarding](./prompts/031-cloud-licensing.md) | todo | | | |
| 032 | [Usage, crash reports, feedback and sermon records](./prompts/032-telemetry-services.md) | todo | | | |
| 033 | [Bible lookup and search](./prompts/033-scripture-service.md) | todo | | | |
| 034 | [API.Bible, offline policy and download manager](./prompts/034-api-bible.md) | todo | | | |
| 035 | [Scripture detector, deterministic path](./prompts/035-detector-rules.md) | todo | | | |
| 036 | [Scripture detector, LLM path and fallback](./prompts/036-detector-llm.md) | todo | | | |
| 037 | [Global quote search worker](./prompts/037-quote-index.md) | todo | | | |
| 038 | [Audio capture and Deepgram transcription](./prompts/038-stt.md) | todo | | | |
| 039 | [Orchestrator, voice navigation and recovery](./prompts/039-orchestrator.md) | todo | | | |
| 040 | [Lyrics library and search](./prompts/040-lyrics-service.md) | todo | | | |
| 041 | [Online lyric search and translation](./prompts/041-lyrics-online.md) | todo | | | |
| 042 | [Sermon notes import](./prompts/042-sermon-import.md) | todo | | | |
| 043 | [Media and tracks libraries](./prompts/043-media-service.md) | todo | | | |
| 044 | [Setlist, libraries, passages, usage and transfer](./prompts/044-transfer-libraries.md) | todo | | | |
| 045 | [Document import pipeline](./prompts/045-documents-service.md) | todo | | | |
| 046 | [Audience rooms and Engage](./prompts/046-audience-rooms.md) | todo | | | |
| 047 | [Slido source](./prompts/047-slido.md) | todo | | | |
| 048 | [Auto-update](./prompts/048-updater.md) | todo | | | |

### 4. Output engine

| ID | Prompt | Status | Date | Branch | Notes |
|---|---|---|---|---|---|
| 050 | [Engine process, protocol and supervision](./prompts/050-engine-scaffold.md) | todo | | | |
| 051 | [Compositor core and layer stack](./prompts/051-compositor-core.md) | todo | | | |
| 052 | [Scene model and slide text](./prompts/052-scene-text.md) | todo | | | |
| 053 | [Backgrounds, elements, shapes and filters](./prompts/053-scene-visuals.md) | todo | | | |
| 054 | [Messages, props, timers, polls, questions, info, logo](./prompts/054-scene-program-layers.md) | todo | | | |
| 055 | [Video playback layer](./prompts/055-video-engine.md) | todo | | | |
| 056 | [Program audio, routing and tracks](./prompts/056-audio-engine.md) | todo | | | |
| 057 | [Live camera layer](./prompts/057-camera-layer.md) | todo | | | |
| 058 | [Document pages and embedded video](./prompts/058-document-layer.md) | todo | | | |
| 059 | [Live web layer](./prompts/059-web-layer.md) | todo | | | |
| 060 | [Display outputs, stage windows and test patterns](./prompts/060-display-sinks.md) | todo | | | |
| 061 | [NDI output](./prompts/061-ndi-sink.md) | todo | | | |
| 062 | [DeckLink output](./prompts/062-decklink-sink.md) | todo | | | |
| 063 | [Syphon output](./prompts/063-syphon-sink.md) | todo | | | |
| 064 | [Capture to file](./prompts/064-recorder-sink.md) | todo | | | |
| 065 | [Reconcile, routing, lobby playlists and program service](./prompts/065-surface-manager.md) | todo | | | |
| 066 | [Operator preview transport](./prompts/066-preview-transport.md) | todo | | | |
| 067 | [Application wiring formerly in the IPC layer and main entry](./prompts/067-application-wiring.md) | todo | | | |

### 5. Operator interface

| ID | Prompt | Status | Date | Branch | Notes |
|---|---|---|---|---|---|
| 070 | [Design system](./prompts/070-design-system.md) | todo | | | |
| 071 | [Application shell, menus and quit flow](./prompts/071-app-shell.md) | todo | | | |
| 072 | [Launch screen](./prompts/072-bootstrap-ui.md) | todo | | | |
| 073 | [Account, onboarding, coach tour](./prompts/073-account-onboarding.md) | todo | | | |
| 074 | [Settings](./prompts/074-settings-ui.md) | todo | | | |
| 075 | [Screens configuration](./prompts/075-screens-ui.md) | todo | | | |
| 076 | [Operator workspace, live rail and monitor](./prompts/076-operator-layout.md) | todo | | | |
| 077 | [Operator queue, suggestions and program panels](./prompts/077-operator-controls.md) | todo | | | |
| 078 | [Scripture workspace](./prompts/078-scripture-ui.md) | todo | | | |
| 079 | [Sermon notes review](./prompts/079-sermon-review.md) | todo | | | |
| 080 | [Lyrics library and quick open](./prompts/080-lyrics-library-ui.md) | todo | | | |
| 081 | [Lyrics slide grid](./prompts/081-lyrics-slides-ui.md) | todo | | | |
| 082 | [Lyrics editing and import](./prompts/082-lyrics-editors-ui.md) | todo | | | |
| 083 | [Media dock and inspector](./prompts/083-media-ui.md) | todo | | | |
| 084 | [Tracks](./prompts/084-tracks-ui.md) | todo | | | |
| 085 | [Documents](./prompts/085-documents-ui.md) | todo | | | |
| 086 | [Theme editor canvas](./prompts/086-theme-canvas-ui.md) | todo | | | |
| 087 | [Theme editor panels](./prompts/087-theme-panels-ui.md) | todo | | | |
| 088 | [Props editor](./prompts/088-props-ui.md) | todo | | | |
| 089 | [Web workspace](./prompts/089-web-ui.md) | todo | | | |
| 090 | [Audience workspace](./prompts/090-audience-ui.md) | todo | | | |
| 091 | [Transcription, transfer, capture popover and leftovers](./prompts/091-remaining-ui.md) | todo | | | |

### 6. Hardening and cutover

| ID | Prompt | Status | Date | Branch | Notes |
|---|---|---|---|---|---|
| 100 | [First-run migration and rollback](./prompts/100-data-migration.md) | todo | | | |
| 101 | [macOS packaging](./prompts/101-package-macos.md) | todo | | | |
| 102 | [Windows packaging](./prompts/102-package-windows.md) | todo | | | |
| 103 | [Linux packaging](./prompts/103-package-linux.md) | todo | | | |
| 104 | [Release pipeline](./prompts/104-release-pipeline.md) | todo | | | |
| 105 | [Bridge from Electron to native](./prompts/105-bridge-release.md) | todo | | | |
| 106 | [Accessibility and keyboard pass](./prompts/106-accessibility-keyboard.md) | todo | | | |
| 107 | [Performance validation and tuning](./prompts/107-performance-hardening.md) | todo | | | |
| 108 | [Soak and failure testing](./prompts/108-soak-chaos.md) | todo | | | |
| 109 | [Full parity audit](./prompts/109-parity-audit.md) | todo | | | |
| 110 | [Hardware qualification](./prompts/110-hardware-qualification.md) | todo | | | |
| 111 | [Pilot with real services](./prompts/111-pilot.md) | todo | | | |
| 112 | [General release and Electron retirement](./prompts/112-cutover.md) | todo | | | |
