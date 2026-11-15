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
