# 06 — Testing and performance validation strategy

## 1. Test layers

| Layer | Tool | Applies to | CI |
|---|---|---|---|
| Unit | `#[test]` | Pure logic | Every push |
| Differential | `ts-oracle` plus `proptest` | Everything ported from `src/lib`, detector, normalizers | Every push, using the cache |
| Replay | Virtual-clock transcript corpus | Detector, orchestrator | Every push |
| Service | Mock HTTP and WebSocket servers | Cloud, STT, audience, lyrics | Every push |
| Compatibility | Real-shaped fixture files in both directions | Settings, songs, bundles, SQLite | Every push |
| UI integration | `#[gpui_kit::test]`, `TestWindowExt` | Interaction, focus, layout, accessibility | Every push |
| Golden image | `golden-capture`, software GPU adapter | Slide rendering | Every push |
| Engine integration | Real `kairo-engine` headless | Protocol, supervision, sinks with fakes | Every push |
| Hardware | Checklists run by a person | NDI, DeckLink, displays | Before release |
| Soak and chaos | `tools/soak` | Whole system | Before release, nightly when possible |

**Porting existing tests.** The roughly 200 test files are a specification. Each is carried over to
Rust, replaced by a differential check, or listed with a reason in the prompt that handled
it. Prompt 109 reconciles them.

**Oracle freshness.** Cached results are stamped with their Electron commit. CI runs from the
cache with no Electron checkout. The sync tool regenerates it when `src/lib` changes.

## 2. Golden images

- Reference frames come from the current Electron overlay through `golden-capture`.
- Line breaks and fitted font sizes are compared **exactly** where the spike says it is
  achievable. Pixels are compared by SSIM, since Chromium and the Rust stack anti-alias
  differently.
- Thresholds are set from spike 003 and recorded in DECISIONS.md. They must not be lowered
  to make a test pass without owner approval.
- Goldens of native output run on a software adapter so CI needs no GPU.

## 3. Performance budgets

Absolute figures below are **targets**. Comparison figures come from `baseline.md`, which
prompt 001 measures. Nothing here has been measured yet.

| Metric | Budget | Row |
|---|---|---|
| Cold start to interactive | Better than Electron, target under 1 s on Apple silicon | L08 |
| Idle memory, all processes, 1 screen output | At most 40% of Electron | — |
| Idle CPU, static slide on 4 outputs | Under 2% of one core, excluding NDI repeat sends | A16 |
| Added memory per extra static output | Under 30 MB | A04 |
| Slide text raster | p95 under 16 ms | B01 |
| Push command to frame committed | p95 under 2 frames at 60 Hz | B08 |
| Clicker to committed page, preloaded | Under 1 frame at 60 Hz; never worse than baseline | G05 |
| NDI conversion | p50 no worse than 0.6 ms (decision 008) | A04 |
| DeckLink 10-minute run | Zero late or dropped | A07 |
| 1080p background video | CPU below Electron | C01 |
| Audio glitches over 60 minutes | Zero | C04 |
| Mic to socket | Under 30 ms added | E01 |
| Detector per event | p99 under 2 ms, fast path | D04 |
| UI frame time while scrolling grids | No frame over 16 ms on baseline machine | F06 |
| Live preview | Under 5% of one core at 30 fps | I01 |
| Installed size, without web component | Below Electron | L14 |
| Memory growth, 4-hour soak | Under 5% after the first 30 minutes | A16 |

## 4. Isolation guarantees to prove

| Claim | Test |
|---|---|
| A UI stall cannot stop output | Block the GPUI thread for 30 s during video and page turns |
| A UI stall cannot stop transcription | Same test; confirm audio keeps reaching Deepgram |
| An engine crash recovers the picture | SIGKILL during a live slide, verify state replay and time to picture |
| A helper crash is contained | SIGKILL `decklink-helper`, `kairo-web-host` |
| No sink slows another | Stall one NDI receiver; check displays and other sinks |
| Frames never queue without bound | Stall every sink, watch memory |

## 5. Soak scenario

A scripted 4-hour service: looping 1080p video background, lyrics at realistic intervals,
verses from a transcript replay, running countdown, ticker, document turns, audience
questions, all available outputs. Recorded: RSS per process, GPU memory, file handles,
dropped and late frames per sink, UI frame times.

Chaos events during the second run: every row in section 4, network loss, display replug,
sleep and wake, Deepgram disconnect, disk nearly full during recording.

## 6. Checkpoints that stop the agent

- A failing gate. No prompt may be marked done with one.
- Human checkpoints: 008, 105, 110, 111, 112.
- Phase exit criteria in `05-roadmap.md`, checked by the first prompt of the next phase.
- Differential differences: treated as bugs in the port unless approved in DECISIONS.md.

## 7. Evidence

Results belong in the repository, under `native-migration/evidence/<prompt>/` for
screenshots and logs (kept small) or in PROGRESS.md notes. A claim with no evidence counts
as not verified.
