# Prompt 113 — UI parity cleanup (sign-in and Screens)

**Phase:** 6. Hardening and cutover
**Tracker ID:** `113-ui-parity-cleanup`
**Raised by:** prompt 109 (FAIL rows from the owner's side-by-side check, 2026-11-16)

> **Agent start protocol.** Same as every prompt: read `START_HERE.md`, `PROGRESS.md` and `DECISIONS.md`, check drift since the last-synced commit in `SYNC.md`, and set this prompt to `in-progress` before changing anything.

## 1. Objective

Make the native sign-in screen and Screens window match Electron. The owner noted that Electron's design also needs cleaning up. Settle the target design with the owner first: Electron as it is, or a cleaned-up design applied to both.

## 2. Findings (native vs Electron)

Compared against Electron 1.0.33 as installed. That build is 7 commits older than the spec commit `af978c5`, so check each item against the source at the spec commit, not the screenshot.

Sign-in (`src/renderer/src/components/account/AuthPanel.tsx`):

1. Native has no "Continue with Google" button and no "or" divider.
2. Native shows no loading spinner while signing in.

Screens (`src/renderer/src/components/screens/ScreenConfiguration.tsx`; native `crates/ui-screens`):

3. List rows: native centres the name, with the status on a separate line. Electron left-aligns the name, with the device subtitle inside the row.
4. Stage group: native shows a group switch and "No stage displays yet" when empty. Check Electron's rule for an empty group.
5. Detail subtitle: native "Video card · Device disconnected"; Electron "DeckLink Duo 2 · DeckLink Duo (2) · Device disconnected".
6. Tabs: native full-width segmented control; Electron a compact centred pill.
7. Hardware tab: native has an "Enabled" row and an extra "Desktop Video 16.4" line. The source at `af978c5` has no per-screen enabled control in the detail.
8. Output: native shows a plain box with "— disconnected"; Electron a select with a chevron and a styled refresh button.
9. Content: native shows a full-width box with no chevron; Electron a select, with Test pattern segmented beside it.
10. Check that native has the "Device disconnected" link and the "Signal: Waiting for the card" disclosure.
11. Footer: Electron left-aligns "Looks & transition" and "Identify screens" and fills the Test verse and Clear buttons. Native centres the labels and outlines the buttons.

Raised later by the owner (2026-11-16):

12. Sign-in: native's "Continue with Google" button has no Google "G" logo (`crates/ui-account/src/auth.rs:396`, label only). Electron shows the logo before the label.
13. Motion: Electron animates entrances and small interactions across the app, such as modals and popovers scaling or fading in, and menus and dropdowns opening. Native only animates the opening fade and the setup wizard (`kairo_ui_kit::motion`). Every other overlay appears instantly. List each animated surface in the source at `af978c5` (Tailwind/CSS transitions, `animate-*` classes, framer-motion, if used) with its duration and easing. Add the matching motion to native, through shared helpers in `kairo_ui_kit::motion` rather than case by case. Respect the system's reduce-motion setting.
14. Live rail: the output picker under the Live preview ("Main Screen" with a chevron, `crates/ui-operator/src/rail.rs:679`, `picker`) is sized to its label. Electron makes it full width, matching the preview above it.
15. Buttons, app-wide. Example: transcript "Start listening" (`crates/ui-operator/src/booth/transcript.rs:406`, via `widgets::primary_button`). Electron draws a solid bright-blue fill, white label, a filled play icon, about 34 px tall with roomier padding, and roughly 8 px radius. Native draws a dark navy fill that reads as dimmed or translucent, an outlined play icon, and a smaller button (`.small()`). Find out why `k.accent(Step::S500)` renders navy: the disabled or busy opacity, a wrong step mapping, or the gpui-kit custom variant blending. Then match Electron's primary, secondary, ghost and icon buttons (fill, size, padding, radius, icon weight) through the shared helpers, not per screen. Compare against the Tailwind classes at `af978c5`.
16. Documents (owner decision, 2026-11-16; a deliberate change, not Electron parity): remove the large live-slide preview above the slide grid (`crates/ui-documents/src/view.rs`, near the "Preview only · No output connected" header at :655 and :2443). Keep the grid alone. The live slide is already marked by its red border and red number. Keep the "Preview only · No output connected" status line if it has nowhere else to go. Record the change in DECISIONS.md, and add it as a row in SYNC.md so later Electron syncs don't bring the preview back.
17. Lyrics slide grid (owner, 2026-11-16; `crates/ui-lyrics/src/grid.rs`, `view/song_view.rs`):
    - At the default zoom the grid only uses about two-thirds of the panel, with two fixed-width columns and empty space on the right. It must fill the panel's full width, with columns sized from the available width and the zoom level.
    - Slides must not start a new row for each section. Every slide flows in one continuous grid, filling each row before wrapping. The section label stays on the section's first slide. Verify against Electron's song grid at `af978c5`. If Electron still breaks rows by section, treat this as an owner decision and record it in DECISIONS.md and SYNC.md.
18. **Blocker: the Live preview shows nothing when no output is reachable.** Owner's profile: three outputs (Main Screen, Lower Third, Pastors Screen), all `kind=device`, `enabled=true`, with no DeckLink card connected. Sending scripture, lyrics or media logs `scripture is live but no screen took it` (`kairo-orchestrator/src/orchestrator.rs:1195`). `EngineHandle::unavailable_reason` returns "Display not connected" (`kairo-core/src/engine.rs:328`) because the mirror has no surface for the output. The rail's monitor (`ui-operator/src/rail.rs:247` `preview_surface`, `monitor`) subscribes to that output's engine frames, so it shows only the transparent checkerboard. Electron draws its monitor in the renderer, so the operator always sees what is live, even in preview-only mode.
    - Fix: whenever an output is enabled but its sink is missing (card or display absent, NDI unavailable), the engine still composes that output's program into a preview-only surface (`SinkSpec::Preview`), and pushes succeed against it. Only the physical sink is reported missing, through the existing health line and card status.
    - With no outputs at all, compose a default program surface, so the monitor and "Preview only" mode still work.
    - Keep the dispatch-health warning, but don't count a preview-only push as "no screen took it" for the operator's live state.
    - Test: device output with no card, push scripture, and assert the monitor gets a non-empty frame.
19. Padding, app-wide: native buttons, selects, search and text inputs are tighter than Electron. Example: the scripture toolbar (translation select, "Scripture Lookup" search, the chevron and Options buttons). Take Electron's sizes (height, horizontal padding, gap, font size) from the Tailwind classes at `af978c5`. Apply them once, through the theme or shared component sizes, not per screen. Do this together with 15.
20. Theme editor's floating toolbar (text, superscript, image, layout icons): the icon buttons are cramped, and the separators touch the icons. Add Electron's spacing between items and around the dividers.
21. Theme backgrounds are missing in previews:
    - The theme editor's theme list (left column thumbnails) draws each theme without its background image. "Main Scripture Theme" shows flat dark, but its canvas shows the open-Bible photo.
    - The scripture verse grid draws cards on the transparency checkerboard, not the active theme's background.
    - Both must render the theme's background (image, video poster frame or colour) under the text automatically, as the canvas does.
22. Scripture verse cards (owner design call, 2026-11-16): cards show the theme background (see 21). Make the reference strip under each card ("Luke 7:40 … 1") a little taller, on solid black. Remove the card's rounded corners, and drop or soften the outer border. The live card keeps a clear red marker. Record the change in DECISIONS.md and SYNC.md.
23. Output kind naming (owner, 2026-11-16): the "+" menu and the detail header call a display output "Kairo screen", which reads as confusing. Electron says "Screen". Pick a clearer name for a physical display output, such as "External display", and use it in every place: the add menu, the detail header, the display map label and the list rows. Keep the menu order consistent with Electron (Screen, NDI feed, Syphon, Video card). Record the name in DECISIONS.md.
24. Screens → Display tab layout: native stacks Output, Fit and Content full width, leaves Fit as a small select with a stray icon button below it, and has no field backgrounds. Electron uses a two-column grid (Output | Fit with the identify button inline; Content below in the left column), filled field backgrounds, and roomier padding. Match Electron's layout.
25. Slide import dialog: background, layout and styling differ from Electron (scrim, card, icon tile, buttons). Match Electron's import progress and error dialog. Show error text in plain language; never print a raw `LoadLibraryError(DlOpen{..})` to the user, and log the detail instead.

Not a finding: the header on/off switch and delete button in the 1.0.33 screenshot were removed in `af978c5`. Native matches the source (SYNC row, 109).

## 3. Also outstanding from 109

The owner waived the rest of the 109 matrix without walking it (DECISIONS 109). Before cutover (112), walk the unaudited rows of `04-feature-parity-matrix.md`, together with the A10 stage-display side-by-side check that was not finished.

## 4. Gates

`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and no edits in `../kairo-presenter`.
