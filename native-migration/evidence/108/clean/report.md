# Soak run `clean`

Started 2026-11-16T08:56:02Z · 10.0 min · chaos off · engine `/Users/codelawd/Documents/development/kairo-gpui-kit/target/release/kairo-engine`

**Result: FAIL** — 0 unrecovered failure(s), 1 leak finding(s).

## Outputs

Ready: screen, ndi-a, ndi-b, syphon, card

Shared web page on: ndi-b

## Memory and handles (from the second service cycle)

| Series | Baseline (first minute of cycle 2) | Last minute | Growth | Slope | Samples |
|---|---|---|---|---|---|
| Client RSS | 9.7 MiB | 9.8 MiB | +0.48% | -0.01 MiB/min | 61 |
| Engine RSS | 627.1 MiB | 658.8 MiB | +5.06% | +0.57 MiB/min | 61 |
| Client handles | 18 | 18 | +0.00% | +0.00/min | 61 |
| Engine handles | 29 | 29 | +0.00% | -0.09/min | 61 |
| GPU memory in use (system-wide) | 981.0 MiB | 964.0 MiB | -1.74% | -15.65 MiB/min | 61 |

Helper RSS 2.0 MiB → 13.1 MiB; web content processes 0 → 1 (0.0 MiB → 33.4 MiB).

- Leak finding: engine RSS grew 5.1% after the warm-up

## Frames per sink

| Sink | Produced | Dropped | Late |
|---|---|---|---|
| card | 19353 | 0 | 0 |
| ndi-a | 19404 | 0 | 0 |
| ndi-b | 32668 | 0 | 0 |
| screen | 28876 | 192 | 0 |
| syphon | 19358 | 0 | 0 |

Script: commands 727, layer updates 10, page turns 30, refused 0, slides 74. Refused page swaps (fell back to a full document): 0.

## Engine log

**Launches** (1)

- 0.1 s: pid 51293 relaunch=false

**Losses** (0)


**Errors** (0)


**Transcription** (2)

- 0.0 s: Connected
- 600.0 s: Disconnected

