# 07 — Technical risks and mitigations

Likelihood and impact are judgments, made before any spike has run. Prompt 008 updates
this file with evidence.

| ID | Risk | L | I | Mitigation | Owner prompt |
|---|---|---|---|---|---|
| R01 | Slide text wraps differently from today | M | High | 300-case corpus, exact line-break comparison, choice of stack by data | 003, 052 |
| R02 | Two processes cause focus, Dock, or permission problems on macOS | M | High | Spike before anything else; fallback below | 002 |
| R03 | `gpui-pre` API changes break the build | H | Med | Exact pin; upgrades as separate tasks only | 000 |
| R04 | GPUI raises the minimum macOS above 11, dropping older church Macs | M | High | Measure the true runtime minimum; owner decides | 007, 008 |
| R05 | GPUI requires Vulkan on Linux, where Electron fell back to software | H | Med | Clear message; owner sets Linux scope | 007, 008, 103 |
| R06 | FFmpeg loses formats, or codecs need licensing attention | M | High | Format matrix. Licensing note to the owner, with legal review as needed | 004, 008 |
| R07 | Detector behaves differently after the port | M | High | Differential replay, no tuning during the port | 035–037 |
| R08 | Regex engine differences go unnoticed | H | Med | Pattern-by-pattern review, `fancy-regex` where needed, Unicode fixtures | 014, 035 |
| R09 | JavaScript string and number semantics differ | H | Med | `kairo-domain::js` helpers, oracle fuzzing | 013 |
| R10 | Electron secrets cannot be migrated | M | Med | Same fallback as today: sign in again, cache rebuilds | 022 |
| R11 | Deployed updater will not accept the native package | M | High | Early proof, one-time Electron release as fallback | 048, 105 |
| R12 | CEF adds size, signing complexity and security patching | H | Med | Isolated, on-demand component; regular version updates | 006, 059 |
| R13 | `cef` crate lacks a needed API | M | Med | Spike; CPU paint path; C API directly if necessary | 006 |
| R14 | Rich text editing is weaker than Tiptap | M | Low | Only one screen uses it; scoped decision | 007, 079 |
| R15 | IME and combining marks fail in GPUI input (Yoruba matters) | M | High | Tested early with real text | 007, 014, 082 |
| R16 | Replacing a preview image each frame grows GPUI's atlas | M | Med | `drop_image`, 30-minute spike; NV12 surface on macOS | 002, 066 |
| R17 | Camera labels differ from those Chromium saved | M | Med | Label mapping; choose capture stack on this requirement | 057 |
| R18 | Audio device matching differs between cpal and Chromium | M | Med | Label-first with migration of old IDs | 038, 056 |
| R19 | PDFium renders differently from pdf.js | L | Med | SSIM corpus | 045 |
| R20 | NDI receivers reject UYVA | L | High | Same open question as Electron; BGRA fallback retained | 061, 110 |
| R21 | The scale of the UI rewrite (47,000 lines) causes silent feature loss | H | High | API inventory, parity matrix, source-file coverage check, state maps | 010, 091, 109 |
| R22 | Agents mark work done prematurely | M | High | Gates, evidence rule, human checkpoints | All |
| R23 | Electron changes while the port runs, including code already ported | H | High | Sync tool and queue, run before each phase; short freeze before cutover | 009, 109, 112 |
| R31 | Copied native helpers diverge between repos | M | Med | Source commit recorded; sync reports drift | 009, 062–064 |
| R32 | Native builds reach users early through the shared release feed | L | High | Private or draft releases only until approval | 104, 105 |
| R24 | Accessibility regressions in custom canvases | M | Med | Dedicated pass | 106 |
| R25 | Native dependencies add licence obligations | M | Med | `cargo deny`, third-party notices, NDI checklist | 101–104 |
| R26 | Hardware behaviour differs from the Electron path | M | High | Qualification by a person with real equipment | 110 |
| R27 | wgpu driver bugs on old GPUs | M | Med | Software fallback adapter; diagnostics; pilot on real church hardware | 051, 111 |
| R29 | System spell check lost in the song editor | M | Low | Evaluate; owner approves any regression | 082 |
| R30 | Chromium `localStorage` cannot be read during migration | L | Low | Only view preferences reset, except legacy nuggets, which are attempted | 100 |
| R28 | Crash in the engine during a service | M | High | Supervision and state replay; chaos tests | 050, 108 |

## Fallback if the two-process design fails (R02)

Keep `kairo-engine` for offscreen sinks, and have output windows created in the GPUI process
through `HasWindowHandle`-based native windows on the platform thread. Crash isolation for
display output would be lost, so the owner must accept that trade-off at prompt 008.

## Fallback if text parity is unreachable (R01)

Compare the cost of the remaining differences against using CEF for text-heavy layers. An
accepted visual change must be shown to the owner with examples.

## Stop conditions for the whole migration

Stop and return to the owner if any of the following occur:

- Spike 002 fails with no acceptable fallback.
- Spike 003 stays well below the line-break target.
- A format churches depend on cannot be played.
- Existing users' data cannot be preserved.
