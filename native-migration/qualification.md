# Qualification record

Manual checks on real hardware that headless tests cannot cover. One section per
prompt. "Not run" means the check still has to be done before release.

## 060 — Display outputs, stage windows and test patterns

Machine: MacBook Pro (M2 Pro), macOS, one built-in display (Built-in Retina
Display, 3600 × 2338 physical, scale 2). No external display was attached, so
every multi-display row is not run. Engine: `target/debug/kairo-engine` through
`cargo run -p kairo-engine --example drive`, 2026-11-13.

| Check | Result | Evidence |
|---|---|---|
| Rehearsal window on the control display: half width, centred, borderless content, letterboxed on black | Pass | `drive --text "Display check"`: 16:9 slide letterboxed inside the rehearsal window (screenshot read) |
| Identify: one numbered card per display for 3 s, description `name · W×H` in physical pixels as `describeDisplay`, transparent window | Pass after fix | First run showed a white box around the card: wry's `transparent` feature was off. Enabled it; card then floats on the content with its border, `1`, `Built-in Retina Display · 3600×2338`; gone after 3 s (screenshots read) |
| "Kairo's controls are here" on the control display only | Unit test only | `drive` does not report a control display; covered by `identify_content_is_numbered_escaped_and_marks_only_the_control_display` |
| Shape test card: yellow grid, name, shape, `1920 × 1080 frame · black bars are outside the frame`, 5 s | Pass | `drive --shape "Main screen"`: full-screen card (screenshot read) |
| Display sleep blocked while an output window is open, released after | Pass | `pmset -g assertions` during the run: `PreventUserIdleDisplaySleep named: "Kairo active display outputs"` (kairo-engine); 0 matching assertions after exit |
| Full-screen output on a non-control display | Not run | One display |
| Plug a display in | Not run | One display |
| Unplug a display (parks, never moves to another screen) | Not run | One display; `resolve_never_guesses` unit test |
| Sleep and wake | Not run | |
| Resolution change | Not run | One display; 250 ms debounce unit-tested |
| Moving the control window between displays (rehearsal follows) | Not run | One display; the GPUI reporter (`apps/kairo/src/display_reporting.rs`) is not exercised here |
| Re-matching a stored Electron display id after replug | Not run | `rematch_uses_electron_logical_size_on_scaled_displays`, store `display_rebinding_is_persisted…` tests |
| Vsync pacing, frame interval jitter p99 within one interval | Not measured | |
| Windows (Chromium display ids) | Not run | `windows_display_identity_matches_chromium_c_vectors` unit test only |

## 100 — First-run migration and rollback

Code: `kairo_store::migration` (`run`, `back_up`, `local_storage`, `devices`), `kairo_store::secrets::migrate_electron_keys`, called in `apps/kairo/src/main.rs` before `SettingsStore::open`; the shell's "Kairo was upgraded" card shows only lines that need the operator. Tests: `crates/kairo-store/tests/migration.rs` (profiles, generated in the test, no committed data), `crates/kairo-store/tests/migration_real.rs` (opt-in, real profile copy), `ui-shell` `upgrade_notes_show_only_when_given_and_dismiss`.

Forward is the migration plus the native settings launch. Back is Electron's real `src/main/db/index.ts` launch run on the migrated files through the oracle (`oracle:settings-launch.launch`, cached in `tests/oracle-cache`), checking that it starts and keeps the named values. In every profile no file present before the migration is gone afterwards.

| Profile | Forward | Back (Electron launch) | Shown to the operator |
|---|---|---|---|
| Fresh install | pass; marker stops a second run | pass | nothing |
| 1.0.28 (`proautomate-*` stores, no `whatsNew`, Chromium mic id, localStorage) | pass; backup byte-identical; 3 keys into `ui-state`; mic hash → `Scarlett 2i4 USB`; `whatsNew` 1.0.28 | pass; church, mic label, `whatsNew` kept | nothing |
| Recent (1.0.40, native `ui-state` already present, mic not connected) | pass; native value wins; Chromium id left in place | pass | the microphone line |
| 5,000 songs (20,000 sections, real `lyrics.sql`) | pass; 0.8 ms; 5,000 rows read after | pass | nothing |
| Moved workspace (exists / on an unplugged volume) | pass / pass | pass (path kept) | nothing / the folder line |
| External-drive media and tracks missing | pass | pass (paths kept) | two folder lines |
| Damaged `bible.db` | pass; file kept | — | the database line |
| Backup impossible | nothing changed, no marker (retries) | — | the backup line |
| Unreadable localStorage | layout defaults only | — | nothing (logged) |

Real profile (a copy of this Mac's Electron userData, 88 MB, 2026-11-15): 226 ms; 10 localStorage keys imported; the saved microphone matched by its Chromium hash. The two key rewraps failed in that harness because the unsigned test binary cannot read the Electron Safe Storage keychain item; the signed app reads it (with macOS's prompt, 022).

Not verified: reinstalling the latest Electron release on a migrated profile and opening it (the rollback above runs Electron's own settings launch, not the installed app); key rewrap end to end in the signed app; Windows and Linux (microphone matching assumes the raw id is the part of the cpal id after `host:`, checked only for CoreAudio).

## 110 — Hardware qualification

Covers the outstanding physical items from Electron decisions 001, 007 and 008
and `native/decklink/README.md` (no drift since `af978c5`). A person must watch
each output and report. Record only what they report. Each 10-minute run must
show zero late or dropped frames.

Machine: MacBook Pro (M2 Pro), built-in Liquid Retina XDR only (3024 × 1964,
scale 2), 2026-11-20.

### Agent probes (no person watching)

| Check | Result | Evidence |
|---|---|---|
| DeckLink driver | Pass | `native/decklink/prebuilt/decklink-helper --version` → `16.4` |
| DeckLink ports | **None** | `--list` → `"ports":[]`. Echo Express SE-III TB3 shows in Thunderbolt, but no Blackmagic device on PCI: check chassis power, card seating and Desktop Video Setup |
| Helper signature | Ad-hoc only | `codesign -dv`: `Signature=adhoc`; signed-package check still open |
| NDI runtime loads | Pass | SDK 6.3.2.0 from `/Library/NDI SDK for Apple/lib/macOS/libndi.dylib` via `KAIRO_NDI_RUNTIME`; without it the probe fails: `no libndi.dylib in the app folder` (expected outside the bundle) |
| NDI discovery | Pass | `receive` found `JOSHUAS-MACBOOK-PRO.LOCAL (Kairo Scripture)` |
| NDI frame format | Pass | One frame 1920 × 1080, FourCC `0x41565955` (UYVA), stride 3840; fully transparent (nothing on air). Kit receiver only, not a third-party receiver |

### Checklist for the person watching

Use **Pass**, **Fail: what was seen**, or **Not run: why**.

**NDI receivers (UYVA keying).** Put a reference slide with text over a
transparent background on air. On each receiver, check the source shows, text
is crisp, transparent areas key over the receiver's other video, and colour
matches the operator preview.

| Receiver | Version | Shows | Keys | Colour | Notes |
|---|---|---|---|---|---|
| NDI Studio Monitor | | | | | |
| OBS (NDI plugin) | | | | | |
| vMix | | | | | |
| ProPresenter | | | | | |

**DeckLink Duo 2.** Record driver, firmware, profile, connector mapping and mode.

| Check | Result | Notes |
|---|---|---|
| `--list` shows every expected port with the right labels | | |
| Each port plays 1080p29.97 picture on a monitor | | |
| Two outputs on the same port: second is refused with a clear error | | |
| Port busy in ProPresenter: Kairo reports busy, does not take it | | |
| Mode change while playing (1080p → 1080i): picture recovers | | |
| Interlaced motion (1080i59.94, moving lyrics/scroll): no combing or field-order judder | | |
| SD mode: not offered (HD only until Rec.601 lands) | | |
| Profile change in Desktop Video Setup while open: outputs reconcile, no crash | | |
| Key/fill: stays disabled | | |
| Helper crash (kill `decklink-helper`): engine keeps running, output shows error, reopen works | | |

**Load and pacing.** GPU path unless stated. Note operator CPU at each step.

| Setup | 10-min run | Late/dropped | CPU | Notes |
|---|---|---|---|---|
| 1 card | | | | |
| 1 card + 1 NDI | | | | |
| 2 cards + 1 NDI | | | | |
| 4 cards + 1 NDI | | | | |
| Colour on card and NDI equals CPU path on reference slide | | | | |
| Forced capture error mid-run: picture holds, fallback logged once | | | | |

**Displays and power.**

| Check | Result | Notes |
|---|---|---|
| Retina primary | | |
| Non-Retina primary | | |
| Unplug output display while live | | |
| Replug: output returns to the right display | | |
| Sleep/wake while outputs live | | |
| Unplug and replug Echo Express while live: error shown, ports return, output resumes | | |

**Signed package.**

| Check | Result | Notes |
|---|---|---|
| `decklink-helper` and `libndi.dylib` in the signed, notarized app, signed with the app's Developer ID | | |
| Card and NDI outputs work from the installed DMG on a clean machine | | |
| Build without the capture path renders through the CPU path | | |

Not verified: everything in the checklist above. The Intel, Windows and Linux
NDI targets from decision 001 are not run.
