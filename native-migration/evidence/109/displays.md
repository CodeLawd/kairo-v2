# 109 — Display checks (two displays)

Machine: M2 Pro, built-in Retina (3600 × 2338 points at 2×, 120 Hz) plus a Sceptre Z27 (6016 × 3384 at 2×, 60 Hz), 2026-11-16.

## A01 Screen output, full screen on the chosen display — PASS

`cargo run -p kairo-engine --example drive -- --display 1 --full --identify --secs 15`.
The engine listed both displays. The surface was ready on display id 2 (Sceptre Z27, bounds x 3600, 6016 × 3384), and the first frame was committed within 2 s.
Owner confirmed: the slide filled the Sceptre edge to edge with no window frame, and the built-in screen was untouched.

## A14 Identify displays — PASS (test pattern checked separately)

Same run with `--identify`. Owner confirmed: a label on each display for about 3 s.

## A14 Test pattern — PASS

`drive --display 0 --shape "Main screen" --secs 45`. Owner saw the rehearsal window and the 5 s shape card.

## A15 Prevent display sleep while live — PASS

During the same run `pmset -g assertions` showed `kairo-engine` holding `PreventUserIdleDisplaySleep` named "Kairo active display outputs". After shutdown no `kairo-engine` assertion remained.

## A02 Rehearsal window when output shares the control display — PASS

`drive --display 0 --secs 90`: a framed rehearsal window opened on the built-in display. The owner dragged it onto the Sceptre and resized it: it stayed there as a window and kept showing the slide.
No `DisplayRebound` event fired. That matches Electron: `display-window.ts` does not force a rehearsal window's bounds (line 80) and does not rebind the output when the window is moved. The native `DisplayRebound` comes only from a display rescan (`app.rs`).

## A16 Recovery: physical display replug — replug part PASS

`drive --display 1 --full --secs 90`. Owner unplugged and replugged the Sceptre; engine events:

| t (s) | Event |
|---|---|
| 16.698 | Displays: built-in only; `main` Parked "Display not connected" |
| 39.439 | Displays: both; `main` Ready at 39.456; FrameCommitted seq 3 at 39.526 (87 ms after the display returned) |
| 46.054 | Displays: built-in only (second drop, cable reseating); Parked |
| 47.198 | Displays: both; Ready at 47.206; FrameCommitted at 47.229 (31 ms) |

Owner confirmed the slide came back on the Sceptre. The last slide was kept and repainted from cache (`cached=true`).
Crash recovery: prompt 108 chaos (`evidence/108/chaos`). Sleep and wake: not verified (108's `--os-sleep` not run).
This closes 108's "physical display replug not verified" item.
