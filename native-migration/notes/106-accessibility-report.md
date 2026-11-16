# 106 — Accessibility and keyboard pass: report

Date: 2026-11-15. Branch: `main`. Pinned GPUI Kit 0.9.1 (`gpui-fast` 0.1.14, AccessKit).
Decisions: DECISIONS.md, "2026-11-15 — 106".

## How the audit works

GPUI builds an AccessKit tree, the tree VoiceOver (macOS) and Narrator (Windows) read,
only while assistive technology is attached. `.cargo/config.toml` sets
`GPUI_A11Y_ACTIVE=1`, so every test window builds it. GPUI honours that variable only in
`test-support` builds, so shipped builds are not affected. `kairo_ui_kit::a11y_audit`
reads the tree with `Window::debug_a11y_tree_json`. Kit's test observation supplies the
disabled flag, which the debug dump leaves out.

- `a11y_audit::check(window)` runs in the draw helper of every UI test harness (18 files
  in `ui-shell`, `ui-operator`, `ui-lyrics`, `ui-scripture`, `ui-documents`, `ui-media`,
  `ui-audience`, `ui-web`, `ui-theme`, `ui-screens`, `ui-settings`, `ui-account`). Every
  frame any UI test draws is audited.
- Rules: every control has a name; every pressable role offers `Click` unless disabled.
- `A11yTree` also gives tests `named`, `with_role`, `focused`, `focus_within` and
  `twin_names`.
- `KAIRO_A11Y_AUDIT_LOG=<file> cargo test …` records violations instead of failing. This
  is how the survey below was made.

## Findings and fixes

The first survey of all UI suites found 284 distinct (control, rule) violations. All are
fixed except one Kit limitation.

| Finding | Where | Fix |
|---|---|---|
| Popover and menu triggers could not be pressed from VoiceOver or Narrator (Kit opens them on mouse press; the trigger offered no `Click`) | 79 Kit buttons across lyrics, scripture, media, operator, screens, settings, theme; rail Feedback, Audio and Account | `popover_trigger()` / `button_trigger()`: `Click` offered, and GPUI's fallback presses the trigger. The mouse path still works (test). |
| Live and Screen Themes header popovers: same | `ui-shell` `ToolbarButton` | Accessibility `Click` toggles the shell's open state; `expanded` reported |
| Custom buttons read as plain "button" (GPUI does not derive a name from child text) | Documents list and Import, lyrics drop zone, Search online, quick-open web search, media Add tile, scripture plans, items, suggestions and Search results, sermon references, toolbox tabs, video layouts, Lyrics import tabs, media inspector panels, detected verse text, queue present target, Restore defaults, props list rows | `aria_label` with the visible text; `aria_selected`/`aria_toggled` for state |
| Unnamed sliders | media playhead, speed and grade; documents slide size and seek; tracks seek; Engage style | Electron names where known ("Preview playhead", "Slide size") |
| Unnamed colour pickers | Engage text and card colour | Named by row |
| Unnamed text fields | media start/end, sermon review playlist name, service name and speaker, seconds per page, timer name, duration, hour, minute, stage message, new message, rename fields, usage from/to | `aria_label` |
| Canvas objects selectable only by pointer press | props stage objects, theme canvas delete handle | Accessibility `Click` selects or deletes; `selected` reported |
| Busy or unavailable custom buttons offered no press | documents Import, media Add, scripture Search results, sermon references, web Bookmark, theme screens (locked), theme picker apply, settings Back/Forward, the open song band | Empty click handler (nothing happens, as with the pointer); not exposed as disabled (no public `aria_disabled`) |
| Modal dialogs did not hold focus: Tab reached the app behind and focus was not returned on close; the Engage dialog's Escape only worked with focus inside | What's New, quit question, Import Song, Song usage, Media Inspector, documents import/converter, Engage dialogs, account gate | `ModalFocus` + `focus_trap`; the quit question focuses Keep Kairo open (DESIGN.md) |
| Engage dialog had no accessible name | `engage-dialog-panel` | Named by its title |
| Reduced transparency and increased contrast not read | app | AppKit read at launch and on focus; glass tones opaque (`solid_chrome`) |

Remaining limitation (not a blocker for macOS keyboard use; recorded): Kit's
`ColorPicker` swatch button offers no `Click`, so VoiceOver must use VO-Shift-Space (a
mouse click) on it. It is named, and it opens from the keyboard (Enter or Space) as before.
A fix needs a GPUI Kit change.

## Reduced motion, transparency and contrast

- Reduced motion: already honoured since 072 (lockup, splash, detected-card entrance,
  transcript, refresh spinners). No new motion was added in this prompt.
- Reduced transparency or increased contrast: `KairoColors::solid_chrome` makes `.glass`,
  `.modal-glass` and the material tint opaque. Header, rail and sidebars were already
  solid (no vibrancy in native). Read from AppKit at launch and when the window regains
  focus; `KAIRO_SOLID_CHROME=1|0` overrides.

## Tests added

- `kairo-ui-kit`: audit parser and rules; `solid_chrome` tokens and appearance change;
  `ModalFocus` focus moves in and comes back.
- `ui-shell/tests/a11y.rs`: top bar and rail named, landmarks (`Navigation`, `Toolbar`,
  `Main`), rail order, the current workspace selected across routes; rail popovers still
  open with the mouse; the quit question holds focus, cycles Tab inside, focuses Keep
  Kairo open, closes on Escape and gives focus back.
- Every existing UI test now also asserts the audit rules on every frame it draws.

## Manual screen reader script

Run on a signed build (or `cargo run -p kairo`) with a test profile. Record pass or fail per step.

VoiceOver (macOS: Cmd-F5 to start; VO = Control-Option):

1. Launch. VO-U, Landmarks: "Presentation controls" toolbar, "Workspaces" navigation,
   main region named after the workspace.
2. VO-Right through the rail: Operator, Bible, Lyrics, Slides, Engage, Web, Theme, then
   Feedback, Audio, Account, Settings. The current workspace reads "selected".
3. VO-Space on Bible: the Bible workspace opens. VO-Space on Feedback, Audio and Account:
   each popover opens; Escape closes it.
4. Header: Screens, Live and Screen Themes read name and state; VO-Space on Live opens the
   Live menu ("expanded").
5. Lyrics: open a song. Each slide card reads "Show slide N"; VO-Space shows it. Song list
   options and Song actions open their menus with VO-Space.
6. Bible: search a verse. Suggestions read reference, translation and text; plan items
   read reference and translation; "Search results, N".
7. Operator, Show tab, Props: each stage object reads "Text object" (and so on) with its
   text as description. VO-Space selects it.
8. Theme: select a layer on the canvas; the delete handle reads "Delete Line" (or the
   layer's name); VO-Space deletes it.
9. Open Media, then a media item's inspector: focus moves into "Media Inspector". Tab never
   leaves it; Escape closes it and focus returns to the tile.
10. Cmd-Q: "Quit Kairo?" alert; focus is on Keep Kairo open; Tab cycles inside; Escape
    keeps Kairo open and focus returns.
11. System Settings → Accessibility → Display: turn on Reduce transparency, switch back to
    Kairo. Settings sidebar and dialogs are opaque. Same with Increase contrast. Turn on
    Reduce motion: the launch lockup and card entrances are still.

Narrator (Windows: Win-Ctrl-Enter): steps 1–10 with Caps Lock+Arrow to move and Caps
Lock+Enter to invoke. Step 11 is not implemented on Windows (settings not read).

## Verified and not verified

Verified (automated): see PROGRESS.md 106 notes for the gate results.

Not verified:
- A live VoiceOver session was not run by a person. The script above was not executed.
- Narrator: no Windows machine; never run. Windows and Linux do not read reduced motion,
  transparency or contrast.
- Kit `ColorPicker` swatch press from assistive technology (Kit limitation above).
- Busy or unavailable custom buttons are not announced as dimmed.
