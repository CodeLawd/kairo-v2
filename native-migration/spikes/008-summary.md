# Phase 0 summary for the go/no-go

Owner decision: **GO** (2026-11-09). All results are macOS arm64 unless stated. Full numbers are in each report.

| Spike | Result | Unknowns | Cost |
|---|---|---|---|
| 001 Baseline | Recorded: startup median 3,661 ms, idle RSS 401.6 MiB, CPU 11.3%, no outputs | Packaged operator and hardware metrics | None |
| 002 Engine process | PASS on macOS with a second display | Windows, Linux | Engine footprint 43.6 to 67.2 MB in soak |
| 003 Text | PASS on breaks 99.75%; FAIL on raster p95 25.28 ms against 16 ms | Windows, Linux, idle timing, custom fonts | Slide caching needed; decoration work in 052 |
| 004 Video | PASS: 54,000-frame soak, 43.6 ms p99, 10.66% CPU | Windows, Linux, zero-copy imports, codec licensing | 86.0 ms at a 2× trimmed wrap |
| 005 Sinks | PASS: NDI by loopback, ring by stub, colour goldens | Real NDI receiver, DeckLink hardware, Intel, Windows, Linux | Native code retained behind a C ABI |
| 006 Web host | PARTIAL, waived: four uses work locally | Real Slido sign-in, subframe header removal, H.264, Windows, Linux | 329 MB installed, 450 MB or more RSS in use |
| 007 UI | PARTIAL, waived: shell and 5,000-card grid work | 120 Hz, real IME, macOS 11, Windows, Linux | Custom rich text editor |

Against the baseline: the only like-for-like number is idle memory, and no whole-app native figure exists yet. Expected gain is therefore unproven until prompt 107.
