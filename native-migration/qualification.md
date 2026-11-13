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
