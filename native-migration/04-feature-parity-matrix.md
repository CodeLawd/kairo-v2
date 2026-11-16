# 04 — Feature inventory and parity matrix

Each row maps an existing behaviour to its native home and how it is proven. `Prompts`
are implementation prompts; prompt 109 walks this whole table against the running app.
**Status starts at `todo`.** Only prompt 109 may set `PASS`. 109 (2026-11-16) set the rows marked below. Every row still `todo` was waived by the owner without being walked (DECISIONS 109), and 113 walks them before cutover.

Finer-grained checklists live in `api-inventory.md` (every API call, generated in prompt
010), and in the `check-lib-coverage` tool (every `src/lib` export, prompt 016).

## A. Live output

| ID | Feature | Existing source | Native home | Prompts | Verification | Status |
|---|---|---|---|---|---|---|
| A01 | Screen output: full-screen on chosen display | `display-window.ts` | `kairo-sinks::display` | 060 | Manual display matrix | PASS (109, `evidence/109/displays.md`) |
| A02 | Rehearsal window when output shares control display | `display-window.ts`, `displays.ts` | same | 060 | Move window across displays | PASS (109, `evidence/109/displays.md`) |
| A03 | Custom aspect, fill, letterbox | `displays.ts`, `overlay.html` | compositor | 051, 060 | Goldens | todo |
| A04 | NDI output, UYVA with alpha, per-output sender | `ndi/`, decision 008 | `kairo-sinks::ndi` | 005, 061 | Colour vectors, real receiver | todo |
| A05 | NDI source name retry and clear frames | `ndi/index.ts` | same | 061 | Start-stop tests | todo |
| A06 | NDI audio | `overlay.html`, `preload/program.ts` | `kairo-audio` | 056, 061 | Tone test at receiver | todo |
| A07 | DeckLink output, modes, ports, statuses, test patterns, driver notice | `video-devices/`, helper | `kairo-sinks::decklink` | 005, 062 | Fake provider, Duo 2 | todo |
| A08 | Syphon server (macOS) | `syphon/`, `native/syphon` | `kairo-sinks::syphon` | 063 | Local client | todo |
| A09 | Capture to file, codecs, disk guards (macOS) | `capture/`, `native/recorder` | `kairo-sinks::recorder` | 064 | ffprobe, low-disk | todo |
| A10 | Stage and confidence displays | `stage-window.ts`, `stage.html` | scene and display | 054, 060 | Goldens and manual | todo |
| A11 | Lobby output with its own playlist | `lobby-player.ts` | core | 065 | Tests | todo |
| A12 | Show filters per output and playlist | `program.ts` | domain and core | 016, 065 | Tests | todo |
| A13 | Fallback destinations | `dispatch.ts`, `orchestrator.ts` | core | 039, 065 | Dispatch tests | todo |
| A14 | Identify displays, test pattern | `surface-manager.ts` | engine | 060 | Manual | PASS (109, `evidence/109/displays.md`) |
| A15 | Prevent display sleep while live | `surface-manager.ts` | engine | 060 | Manual | PASS (109, `evidence/109/displays.md`) |
| A16 | Recovery from crash, replug, sleep, retaining picture | `program-surface.ts` | supervisor | 050, 065, 108 | Chaos tests | Replug PASS, crash PASS (108); sleep/wake not verified — waived (DECISIONS 109) |
| A17 | Clear text / Clear all | `program-surface.ts` | engine | 051, 065, 076 | Tests | todo |

## B. Slide rendering

| ID | Feature | Existing source | Native home | Prompts | Verification | Status |
|---|---|---|---|---|---|---|
| B01 | Verse and reference text, full type options | `overlay-template.ts` | `kairo-scene` | 003, 052 | 300-case corpus | todo |
| B02 | Auto-fit with smart fill | `overlay-fit.ts` | `kairo-scene` | 052 | Fitted size tolerance | todo |
| B03 | Outline and shadow | `overlay-template.ts` | `kairo-scene` | 052 | SSIM | todo |
| B04 | Coloured lyric gloss lines | `overlay-template.ts` | `kairo-scene` | 052 | Goldens | todo |
| B05 | Backgrounds: colour, gradient, image, video, transparent | `overlay-template.ts` | scene and compositor | 053, 055 | Goldens | todo |
| B06 | Filters and blur | `media-playback.ts` | compositor | 053 | Goldens | todo |
| B07 | Theme elements (40 max): shapes, media, rotation, flip | `overlay-elements.ts`, `overlay-shapes.ts` | `kairo-scene` | 053 | Goldens | todo |
| B08 | Cut and fade per layer | `overlay.html` | compositor | 051 | Goldens at progress points | todo |
| B09 | Independent layer updates | `program-surface.ts` | compositor | 051 | Test | todo |
| B10 | Messages and ticker | `program.ts`, `overlay.html` | scene | 054 | Goldens | todo |
| B11 | Props, timers, logo | `program.ts` | scene | 054 | Goldens | todo |
| B12 | Poll, question, QR join code | `program.ts` | scene | 054 | Goldens | todo |
| B13 | Countdown, clock, stage message | `overlay.html`, `program.ts` | scene | 054 | Tests | todo |

## C. Media and devices

| ID | Feature | Existing source | Native home | Prompts | Verification | Status |
|---|---|---|---|---|---|---|
| C01 | Video backgrounds, all accepted formats | `overlay.html`, `pa-media-range.ts` | `kairo-video` | 004, 055 | Format table | todo |
| C02 | Loop, trim, rate, seek, pause, cue | `overlay-fit.ts` | `kairo-video` | 055 | Burned-in frames | todo |
| C03 | Sliders update without restarting video | `program-surface.ts` | engine | 055, 083 | Test | todo |
| C04 | Program audio to selected device; one audio surface | `overlay.html`, `surface-manager.ts` | `kairo-audio` | 056 | Null sink, manual | todo |
| C05 | Audio fallback when hardware-only | `ProgramAudioFallback.tsx` | `kairo-audio` | 056 | Test | todo |
| C06 | Camera live, three layouts, audio pairing | `overlay.html`, `VideoLayoutCanvas.tsx` | `kairo-camera` | 057, 077 | Real device | todo |
| C07 | Media library, playlists, order, copy/cut/paste | `media/`, `MediaDock.tsx` | `kairo-media` | 043, 083 | Tests | todo |
| C08 | Tracks house audio | `tracks/`, `TracksPlayer.tsx` | `kairo-media`, `kairo-audio` | 043, 056, 084 | Tests | todo |
| C09 | Live web page on outputs | `web/`, `overlay.html` | web host | 006, 059 | Local site | todo |
| C10 | Web tab: bookmarks, follow navigation, scroll, pointer | `WebPage.tsx` | `ui-web` | 089 | Manual | todo |

## D. Scripture

| ID | Feature | Existing source | Native home | Prompts | Verification | Status |
|---|---|---|---|---|---|---|
| D01 | Offline Bibles, search, references | `bible-db.ts`, `index.ts` | `kairo-scripture` | 033 | Oracle queries | todo |
| D02 | Local pack install (NKJV, NLT, AMPC) | `local-bible-*` | same | 033 | Tests | todo |
| D03 | API.Bible, encrypted cache, licence rules | `api-bible-*` | same | 034 | Mock API | todo |
| D04 | Live detection: regex fast path | `detector.ts` | same | 035 | Corpus replay, zero differences | todo |
| D05 | LLM detection, fallback, cost stats | `detector.ts` | same | 036 | Recorded replay | todo |
| D06 | Global quote detection | `global-quote.ts`, workers | same | 037 | Corpus | todo |
| D07 | Voice navigation | `voice-navigation.ts` | `kairo-core` | 039 | Tests | todo |
| D08 | Auto-present delay, approve, dismiss | `orchestrator.ts` | same | 039, 077 | Virtual clock | todo |
| D09 | Latency trace and panel | `trace.ts`, `ScriptureLatencyPanel.tsx` | same | 036, 074 | Trace format test | todo |
| D10 | Sermon notes import, review | `index.ts`, `SermonNotesReviewModal.tsx` | service, `ui-scripture` | 042, 079 | Reference differential | todo |
| D11 | Playlists and live plan | `sermon-plans.ts`, `live-plan.ts` | core | 039, 078 | Tests | todo |
| D12 | Translations and retranslate row | `index.ts`, `Operator.tsx` | core | 033, 077 | Tests | todo |
| D13 | Workspace UI: grid, search, keyboard | `Scripture.tsx` | `ui-scripture` | 078 | Bench | todo |

## E. Speech

| ID | Feature | Existing source | Native home | Prompts | Verification | Status |
|---|---|---|---|---|---|---|
| E01 | Mic capture, device select, level, hotplug | `audio/`, `Settings.tsx` | `kairo-stt` | 038, 074 | Sine test, unplug | todo |
| E02 | Deepgram stream, keyterms, replay on reconnect | `stt/deepgram.ts` | same | 038 | Scripted server | todo |
| E03 | Transcription buffer and history | `stt/buffer.ts`, `stt/index.ts` | same | 038 | Tests | todo |
| E04 | Live transcript in the Operator (`Transcription.tsx` appears unused) | `Operator.tsx` | `ui-operator` | 077, 091 | 2-hour load | todo |
| E05 | Session recovery | `resilience.ts` | core | 025, 039 | Kill and restore | todo |
| E06 | Health and fallback state | `resilience.ts` | core | 039 | Tests | todo |

| E07 | Service records: start, end, discard, nuggets, analysis | `service-archive.ts`, `ServicePanel.tsx` | `kairo-cloud` | 032, 077 | Tests | todo |

## F. Lyrics

| ID | Feature | Existing source | Native home | Prompts | Verification | Status |
|---|---|---|---|---|---|---|
| F01 | Library, FTS, favourites | `lyrics/index.ts` | `kairo-lyrics` | 040, 080 | Differential search | todo |
| F02 | Songs folder as source of truth | `song-files.ts`, `workspace/` | `kairo-store` | 024, 040 | Round trip | todo |
| F03 | Online search and preview, all sources | `lyrics/online*`, etc. | `kairo-lyrics` | 041, 082 | HTML fixtures | todo |
| F04 | Translation with gloss | `translate.ts` | same | 041, 082 | Mock | todo |
| F05 | Import from file, clipboard, OpenLyrics | `lyrics/index.ts` | same | 040, 082 | Fixtures | todo |
| F06 | Slide grid, hotkeys, paint, zoom | `Lyrics.tsx` | `ui-lyrics` | 081 | UI tests | todo |
| F07 | Editing: sections, reorder, reflow, quick edit | `SongReflowEditor.tsx`, etc. | `ui-lyrics` | 082 | UI tests, IME | todo |
| F08 | Quick open | `SongQuickOpen.tsx` | `ui-lyrics` | 080 | UI tests | todo |
| F09 | Setlists and libraries | `setlist/`, `libraries/` | `kairo-transfer` | 044, 080 | Tests | todo |
| F10 | Song usage, CSV | `song-usage.ts` | same | 044, 080 | Tests | todo |

## G. Documents

| ID | Feature | Existing source | Native home | Prompts | Verification | Status |
|---|---|---|---|---|---|---|
| G01 | PDF import to 4K pages, cap, cancel | `Documents.tsx` | `kairo-documents` | 045, 085 | SSIM against pdf.js | todo |
| G02 | PPTX built-in conversion | `pptx-*` | same, web host | 006, 045 | HTML equality | todo |
| G03 | PowerPoint, WPS, Keynote, LibreOffice | `converters.ts` | same | 045 | Per platform | todo |
| G04 | LibreOffice on-demand install | `slide-converter.ts` | same | 045, 085 | Test | todo |
| G05 | Page turns with preload and wire ack | `program-surface.ts` | engine | 058 | Latency | todo |
| G06 | Embedded slide videos | `pptx-videos.ts`, `overlay.html` | engine | 045, 058 | Integration | todo |
| G07 | Unattended slideshow | `Documents.tsx` | `ui-documents` | 085 | Tests | todo |

## H. Theme and presentation design

| ID | Feature | Existing source | Native home | Prompts | Verification | Status |
|---|---|---|---|---|---|---|
| H01 | Theme editor canvas with undo | `OverlayCanvas.tsx`, `ThemeEditor.tsx` | `ui-theme` | 086 | UI tests | todo |
| H02 | All type, layout, element panels | `Theme*Panel.tsx` | `ui-theme` | 087 | UI tests | todo |
| H03 | Theme library, built-ins, per-kind defaults | `theme-library.ts` | domain | 013, 087 | Differential | todo |
| H04 | Screen Themes picker | `ThemePicker.tsx` | `ui-theme` | 087 | Tests | todo |
| H05 | Props designer | `PropsEditor.tsx` | `ui-theme` | 088 | Tests | todo |
| H06 | Screens configuration, looks | `ScreenConfiguration.tsx` | `ui-screens` | 075 | Integration | todo |

## I. Operator

| ID | Feature | Existing source | Native home | Prompts | Verification | Status |
|---|---|---|---|---|---|---|
| I01 | Live monitor and layer strip | `LiveOutputPreview.tsx`, etc. | `ui-operator` | 066, 076 | Manual | todo |
| I02 | Suggestions, queue, search | `Operator.tsx` | same | 077 | Scripted core | todo |
| I03 | Messages, timers, props, logo controls | `ProgramPanels.tsx` | same | 077 | UI tests | todo |
| I04 | Service panel and records | `ServicePanel.tsx`, `service-records/` | same | 032, 077 | Tests | todo |
| I05 | Customisable shortcuts, clicker keys | `keyboard-shortcuts.ts` | shell | 016, 071, 074 | Tests | todo |
| I06 | Media dock toggle, booth toolbox | `MediaDock.tsx`, `BoothToolbox.tsx` | shell, operator | 077, 083 | Tests | todo |

## J. Audience

| ID | Feature | Existing source | Native home | Prompts | Verification | Status |
|---|---|---|---|---|---|---|
| J01 | Kairo rooms: Q&A, moderation, voting | `sources/kairo.ts` | `kairo-audience` | 046, 090 | Local Worker | todo |
| J02 | Engagements, polls, moderators | `engage.ts` | same | 046, 090 | Local Worker | todo |
| J03 | Slido source, moderator sign-in | `sources/slido*` | web host | 006, 047 | Harness, event | todo |
| J04 | Sample source | `sources/sample.ts` | same | 046 | Test | todo |
| J05 | Show question or poll on screens | `program.ts` | scene | 054, 090 | Goldens | todo |

## K. Account and cloud

| ID | Feature | Existing source | Native home | Prompts | Verification | Status |
|---|---|---|---|---|---|---|
| K01 | Sign up, sign in, verify, reset, pairing | `cloud/session.ts` | `kairo-cloud` | 030, 073 | Local server | todo |
| K02 | API failover | `api-client.ts` | `kairo-net` | 030 | Failure injection | todo |
| K03 | Org secrets, managed providers | `session.ts`, `managed.ts` | `kairo-cloud` | 030 | Tests | todo |
| K04 | Licence | `license-*` | same | 031, 073 | Cross-verify tokens | todo |
| K05 | Onboarding and tour | `cloud/onboarding.ts`, UI | same | 031, 073 | Flow | todo |
| K06 | Usage stats, crash reports, opt-outs | `usage/`, `crash-reports/` | same | 032 | Mock | todo |
| K07 | Feedback | `session.ts`, `FeedbackRailButton.tsx` | same | 032, 071 | Mock | todo |
| K08 | Sermon upload with retry | `sermon-upload.ts` | same | 032 | Mock | todo |
| K09 | Managed provider gateway, feature gates | `managed.ts`, `license-service.ts` | same | 030, 031, 036 | Gateway mock | todo |
| K10 | Transcription grants, one-computer rule | `deepgram.ts`, `license-service.ts` | `kairo-stt` | 038 | Scripted server | todo |

## L. Application and data

| ID | Feature | Existing source | Native home | Prompts | Verification | Status |
|---|---|---|---|---|---|---|
| L01 | Settings and all migrations | `db/index.ts` | `kairo-store` | 021 | Fixture matrix | todo |
| L02 | Secure token and cache encryption | `secure-store.ts`, `api-cache-crypto.ts` | same | 022 | Cross-language | todo |
| L03 | Workspace location, move, adopt | `workspace/` | same | 024, 074 | Tests | todo |
| L04 | `.kairo` import and export, double-click | `transfer/`, `kairo-bundle.ts` | `kairo-transfer` | 044, 071, 091 | Round trip | todo |
| L05 | Native menus and import routes | `main/index.ts` | shell | 071 | Manual | todo |
| L06 | Quit confirmation on all paths | `quit-confirmation.ts` | shell | 025, 071 | Tests | todo |
| L07 | Single instance | `main/index.ts` | `kairo-store` | 020 | Test | todo |
| L08 | Launch sequence, 8 s ceiling | `bootstrap.ts`, `bootstrap/` | core, shell | 025, 072 | Tests | todo |
| L09 | Dark and light, independent of OS | `index.css`, `main/index.ts` | `kairo-ui-kit` | 007, 070 | Screens | todo |
| L10 | Vibrancy chrome | `main/index.ts`, `DESIGN.md` | shell | 007, 071 | Screens | todo |
| L11 | Auto-update, operator-initiated | `updater/` | `kairo-updater` | 048, 105 | Simulated feeds | todo |
| L12 | What's New | `whats-new.ts`, `changelog.ts` | shell | 071 | Test | todo |
| L13 | Logging and local crash record | `main/index.ts` | `kairo-store` | 020 | Test | todo |
| L14 | Installers, signing, associations | `package.json` | packaging | 101–104 | Clean machines | todo |
| L15 | Permission prompts and purpose strings | `package.json`, `capture/index.ts` | packaging | 038, 101 | Clean machine | todo |
| L16 | Existing users upgrade and can roll back | n/a | migration | 100, 105 | Profiles | todo |
| L17 | Remembered panel sizes, zoom, recent lookups | `localStorage` keys | `ui-state` | 021, 071–091, 100 | Import test | todo |
| L18 | Settings side effects, startup order, native dialogs | `ipc/index.ts`, `main/index.ts` | `kairo-core` | 067 | Tests | todo |
| L19 | Window chrome per OS, menus, UI zoom | `main/index.ts`, `AppShell.tsx` | shell | 071 | Manual | todo |
| L20 | Confirm and alert wording, field context menus, spell check | Renderer | `kairo-ui-kit` | 070, 082 | Manual | todo |

## Performance-sensitive rows

A04, A07, A16, B01, B02, B08, C01, C04, D04, E01, E02, G05, I01 and F06 each have a budget
in `06-testing-performance-strategy.md`. These rows may not be waived without the owner.
