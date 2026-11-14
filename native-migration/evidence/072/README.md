# 072 — Launch screen evidence

Captured 2026-11-14 on macOS (M2 Pro), window only, through the cmux-cua window capture. Native: `target/debug/kairo` (debug) with a fresh `HOME` in a scratch folder. Electron: the installed Kairo 1.0.33 (the same version as `../kairo-presenter` at `af978c5`) with `--user-data-dir` in a scratch folder. Frames were taken about every 200–400 ms from when each window appeared, so matching beats are a few hundred ms apart.

| File | What |
|---|---|
| `native-1-assemble.png`, `electron-1-assemble.png` | Tiles flying in from their corners |
| `native-2-wordmark.png`, `electron-2-wordmark.png` | Mark in place, "Kairo" rising letter by letter |
| `native-3-dot.png`, `electron-3-dot.png` | The i's dot dropping |
| `native-4-alive.png`, `electron-4-alive.png` | Loop: ripple lighting the tiles, "Powering up" band |
| `native-5-open.png` | The app after the splash (Operator placeholder, 076) |
| `electron-5-open-onboarding.png` | Electron opens onboarding on a fresh profile (073) |
| `native-reduced-splash.png`, `native-reduced-open.png` | `KAIRO_REDUCED_MOTION=1`: finished lockup, plain text, opens at about 2.5 s |
| `sheet-native.png`, `sheet-electron.png`, `sheet-reduced.png` | Nine (six) frames in order, cropped to the centre |

Compared by eye: same layout (300 px lockup, tagline, 30 px row), same beats in the same order, same colours. Visible differences, as DECISIONS.md (072) lists: no blur on the rising letters; the glow and sheen are approximations; tracking is laid out per character.

Measured (debug, fresh `HOME`, three launches): bootstrap 5.3, 3.2 and 3.1 ms (`[Bootstrap] Completed elapsed_ms`); first paint 1837, 1203 and 1688 ms. Bootstrap ends before the first frame, so the splash runs its full brand beat and closes at about 3.0 s (2.5 s under reduced motion), as in Electron.

Not captured: the progress row and the warning bar on screen. Native startup never outlasts the splash and no real resource failed, so both are covered by the UI tests in `crates/ui-shell/tests/launch.rs` (fake core with loaders that time out, fail and never answer), not by screenshots. Light theme not captured. Windows and Linux not run.
