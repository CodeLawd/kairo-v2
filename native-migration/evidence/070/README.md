# 070 — Design system evidence

Captured 2026-11-13 on macOS (Darwin 27.2), Retina, from
`cargo run -p kairo-ui-kit --bin kairo-gallery`. Each screenshot is the gallery
window only (`screencapture -l`).

| File | What |
|---|---|
| `gallery-dark.png`, `gallery-light.png` | Buttons, fields, number field, select, slider, switch, zoom, segmented control and tabs, code field, settings groups, marquee grid |
| `gallery-*-2.png` (`--scroll 1050`) | Rail items, library section, traffic lights, row context menu target, drag pill, brand tile and lockup, dialog triggers, frosted panels, colour tokens |
| `gallery-*-3.png` (`--scroll 2100`) | Type scale (with Yoruba ẹ ọ ṣ), all 108 icons in both weights, every keyframe rule |
| `dialog-confirm-*.png` (`--open confirm`) | `confirm` with the `TracksPanel` message |
| `dialog-alert-*.png` (`--open alert`) | `alert` with the `ScreenConfiguration` message |
| `dialog-settings-*.png` (`--open settings`) | Movable dialog with frosted title bar and traffic lights |

## Not captured

- **Electron counterparts.** As in prompt 007, an isolated Electron profile stops
  at onboarding, and a signed-in profile would put personal data in the evidence.
  The comparison below is against `DESIGN.md`, `index.css` and the component
  source, not a side-by-side picture. Not verified visually against Electron.
- Hover, pressed and focus-ring states: focus and selection are exercised by the
  UI tests, but none of these states is captured as an image.
- Windows and Linux: not run.

## Review against DESIGN.md

Matches: Source Sans 3 for controls, body and headings; zinc surface steps
(workspace, panels, controls, elevated, borders) from `index.css`; flat, with
no shadows (`theme.shadow = false`); blue action accent kept separate from live
red and the tool hues; restrained radius (6, 8 and 10px); selection as a quiet
accent wash; the light theme keeps its own token scale.

Differences, each logged in DECISIONS.md (2026-11-13, 070):

1. **Dark palette numbers.** DESIGN.md names #181818 / #202020 / #2C2C2C /
   #353535 / #404040. `index.css` (the runtime source, which the prompt
   protocol makes the specification) uses zinc 24 24 27 / 30 30 33 / 39 39 42 /
   51 51 56 / 63 63 70. The kit follows `index.css`.
2. **Frost.** `.glass` and `.modal-glass` use `backdrop-filter: blur(40px)`.
   GPUI cannot blur what is behind a surface inside one window, so the kit
   draws the same tint at the same alpha without blur. Window-level vibrancy
   for the top bar and rail belongs to the shell (071).
3. **Confirm buttons.** `window.confirm` showed OK and Cancel. The styled
   dialog names the action (Delete, Remove, Move to Trash, Download, Discard),
   as the Design Guides require. Messages are word for word.
4. **Motion.** GPUI elements have no general `transform` or `filter`. Opacity
   and translation play; scale, rotate, blur, brightness and background sweeps
   are kept as data in `motion::Frame` for elements that can use them.
5. **Buttons and inputs** use Kit's geometry (Kit `Button`, `Input`, `Select`,
   `Slider`, `Switch`, `OtpInput`) with Kairo colours. Heights match the
   shadcn defaults (h-8), not the older `.btn-primary` padding.
6. **The macOS window title bar** in the gallery is the system one; the app's
   own frosted header is 071's.

The gallery also showed one real defect, now fixed: the variable Source Sans 3
files registered as "Source Sans 3 ExtraLight", so every label fell back to the
system font. `tools/instance-fonts.py` now builds static 400 / 500 / 600 / 700
faces named "Source Sans 3"; `kairo-gallery --check-fonts` prints
`source-sans: ["Source Sans 3"]`.
