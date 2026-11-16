# Prompt 107 — Performance comparison with baseline.md

Measured 2026-11-16 on the baseline machine class (Apple M2 Pro, macOS 27.2), release builds, other work running (including a VM at ~50% CPU). Single-machine samples, not a controlled study.

| Baseline row | Electron (baseline.md) | Native (107) | Budget (06) | Result |
|---|---|---|---|---|
| Start to first interactive screen, fresh profile | median 3,661 ms (3,609–7,338) | spawn to first frame median **736 ms** (695–1,609), 5 trials; `main` to first frame 679 ms. Was 1,371 ms before the fixes below. Existing profile: 473 ms (1 run) | Better than Electron, target under 1 s | Met |
| Idle RSS, 0 outputs (main + descendants) | 411,264 KiB | median **224,560 KiB** (181,168–270,976), 55% of Electron | At most 40% (budget is for 1 output, Electron 1 output not measured) | **Not met; waived by owner 2026-11-16 (DECISIONS 107)** |
| Idle CPU, 0 outputs | 11.3% | baseline `ps` method **2.3%** (2.0–2.7); CPU-time delta over 10 s ≈ 1.5% UI + 0.1% engine | Under 2% on 4 outputs | Below Electron; 4-output row not measured |
| Idle 1, 2, 4 outputs | not measured | not measured (no extra displays) | | Not verified |
| Document page turn | not measured | not measured | | Not verified |
| Slide push to NDI | not measured | not measured (no NDI receiver); push to committed frame p95 7.33 ms carried from 052, not re-run | p95 under 2 frames | Not re-verified |
| Lyrics library, 5,000 songs | not measured | index open 323 ms, first library read 28 ms; search p50 3.4 ms, p95 5.5 ms (1,000 queries, matches Electron) | | Measured |
| Scripture search | warm FTS 0.08–0.83 ms (SQL only) | service search on the bundled Bible p50 0.93 ms, p95 6.2 ms (938 queries); quote search Rust p95 19.7 ms vs Electron 22.0 ms same run | p95 under 16 ms (DECISIONS 107) | Met |
| Verse grid scrolling | synthetic 8.3 ms median interval | not measured on screen | No frame over 16 ms | Not verified |
| Lyrics / media scrolling (native harness) | media not measured | lyrics draw p50 2.85 ms, p95 5.14 ms, present p95 25.4 ms, 27% CPU; media draw p95 3.98 ms, present p95 25.2 ms, 28% CPU | No frame over 16 ms | Draw within budget; present p95 25 ms means some missed vsyncs (as on 2026-11-15). Draw p95 and CPU are higher than 2026-11-15 (2.9 ms, 16%): regression vs 106 **not verified** (A/B build lost on reboot; machine loaded) |
| 1080p video CPU | not measured | not measured (no output) | Below Electron | Not verified |
| Installer and bundle | DMG 116,643,433 B; app 301,828 KiB | not re-measured here; 101/104 recorded DMG 68,445,794 B, app 187,736 KiB | Below Electron | Met per earlier measurement |
| Speech to screen | not measured | in-process speech final to ShowSlide p50 1.23 ms, p95 1.37 ms | | Partial (no audio-to-photon) |
| Slide raster (B01) | — | p50 2.73 ms, p95 11.52 ms, max 16.95 ms (400 cases warm) | p95 under 16 ms | Met |
| Detector per event (D04) | — | p50 47 µs, p99 247 µs (1,620 events) | p99 under 2 ms | Met |

## Fixes

Profiling (macOS `sample`, `xctrace` Time Profiler, per-step startup log) showed startup waiting on background services, not the UI thread:

- `kairo_net::transport::client_builder` shares one rustls configuration. reqwest reloaded the macOS root store for every client (~115 ms each, five at launch). `Reporting` step 505 ms → 13 ms.
- `warm_tls` loads that store on a thread at launch, parallel with UI init (`HttpPackFetcher::new` no longer pays it on the critical path).
- `ReqwestHttp` (lyrics) builds its client on first use.
- Booth audio device listing (CoreAudio start, ~80 ms on the UI thread) moved to a blocking task; labels arrive by `BoothFeed::Devices`.
- `ApplicationLifecycle` logs each startup step's duration (`[Startup] step finished`).

## Remaining large costs (not changed)

Fresh-profile `OpenScripture` copies the 45 MB bundled Bible (~145 ms, first launch only; 37 ms on an existing profile). Idle memory: window IOSurfaces 79 MB, malloc 61 MB, one 32 MB CG image.

## Automated tests

`crates/kairo-scene/tests/perf_budgets.rs` (raster) and `crates/kairo-scripture/tests/perf_budgets.rs` (search on a generated full-shape Bible, detector fast path; empty, whitespace, punctuation, unknown-word and out-of-range queries return nothing). Strict budgets in optimised builds or with `KAIRO_PERF_STRICT=1`; 4× in unoptimised workspace runs. CI job `performance` (macos-15, release, strict). Not yet run on GitHub.

## Method

`tools/baseline/native-startup.py` mirrors `startup.mjs`: new HOME per trial, spawn to `kairo-first-paint-ms`, 15 s wait, five `ps` samples over the process tree. Scroll: `tools/gpui-fast-benchmark.py` (now accepts `KAIRO_BENCH_BINARY`, `KAIRO_BIBLE_DB`, `KAIRO_ENGINE_BINARY`).
