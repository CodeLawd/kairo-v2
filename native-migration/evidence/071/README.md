# 071 — Application shell evidence

Captured 2026-11-14 on macOS from `cargo run -p kairo` (debug), window only.

| File | What |
|---|---|
| `shell-dark.png` | Rail, header and the Operator placeholder (076) |
| `whats-new-dark.png` | Kairo → What's New in Kairo (a dev build previews the top entry) |
| `quit-dark.png` | Kairo → Quit Kairo: the quit question |
| `after-escape.png` | Escape on the quit question: Kairo stays open |
| `zoom-in.png` | View → Zoom In (scale 1.1) |

Exercised by hand: the native menus (titles and items read back through System Events), What's New, Quit with Escape, Zoom In and Actual Size.

Not captured or not verified: Electron side by side (an isolated profile stops at onboarding, as in 007 and 070); light theme (the app follows `display.theme`, dark here); the Live, Media and Themes popovers (075, 083, 087, 091); Windows and Linux.
