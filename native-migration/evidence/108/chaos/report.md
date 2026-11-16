# Soak run `chaos`

Started 2026-11-16T09:06:05Z · 10.0 min · chaos on · engine `/Users/codelawd/Documents/development/kairo-gpui-kit/target/release/kairo-engine`

**Result: PASS** — 0 unrecovered failure(s), 0 leak finding(s).

## Outputs

Ready: screen, ndi-a, ndi-b, syphon, card

Shared web page on: ndi-b

## Memory and handles (from the second service cycle)

| Series | Baseline (first minute of cycle 2) | Last minute | Growth | Slope | Samples |
|---|---|---|---|---|---|
| Client RSS | 12.5 MiB | 11.0 MiB | -12.02% | -0.61 MiB/min | 61 |
| Engine RSS | — | — | not assessed | engine relaunched inside the window | — |
| Client handles | 18 | 18 | +0.00% | -0.02/min | 61 |
| Engine handles | — | — | not assessed | engine relaunched inside the window | — |
| GPU memory in use (system-wide) | 956.5 MiB | 957.4 MiB | +0.10% | -9.95 MiB/min | 61 |

Helper RSS 2.0 MiB → 14.0 MiB; web content processes 0 → 1 (0.0 MiB → 30.7 MiB).

## Frames per sink

| Sink | Produced | Dropped | Late |
|---|---|---|---|
| card | 19432 | 0 | 0 |
| ndi-a | 19461 | 0 | 0 |
| ndi-b | 32267 | 0 | 0 |
| screen | 27962 | 1318 | 0 |
| syphon | 19438 | 0 | 0 |

Script: commands 707, layer updates 10, page turns 30, refused 1, slides 70. Refused page swaps (fell back to a full document): 0.

## Chaos events

| At | Event | Outcome | Recovery | Detail |
|---|---|---|---|---|
| 60 s | DeepgramDisconnect | Recovered | 1021 ms | server closed the socket; reconnected in 1021 ms, speech health ok, audio flowing, replay buffer 0 B |
| 95 s | UiBlock | Recovered | 17 ms | script (UI) thread blocked 30 s: frames kept flowing (screen +930, ndi-a +930, syphon +929, card +929), 963200 B of audio reached Deepgram; commands resumed afterwards |
| 140 s | EngineKill | Recovered | 302 ms | SIGKILL pid 54933; relaunched as 55782 after 106 ms, state replayed, frame on screen at 302 ms |
| 175 s | WebContentKill | Recovered | 23 ms | SIGKILL web content [55800]; page reloaded in a new content process after 23 ms; engine 55782 kept running, other outputs drew |
| 205 s | HelperKill | Recovered | 75 ms | SIGKILL helper 55803; card faulted with the helper's reason in 44 ms; operator refresh restored frames at 75 ms; engine unaffected |
| 240 s | NetworkLoss | Recovered | 10995 ms | 20 s offline (proxy cut, licence server unreachable): health degraded with offline detector fallback, 640000 B of audio held; reconnected 10995 ms after the network returned, held audio replayed, health all good; outputs unaffected |
| 305 s | EngineHang | Recovered | 2353 ms | SIGSTOP (hang) pid 55782; relaunched as 56793 after 2163 ms, state replayed, frame on screen at 2353 ms |
| 340 s | DisplayReplug | Recovered | 50 ms | surface parked 3 s and restored; frame in 50 ms with state intact. The OS display-change event itself was not exercised (no second display to unplug) |
| 375 s | DiskNearlyFull | Recovered | 9106 ms | recording on a 2.6 GB RAM disk filled to ~300 MB free: stopped 9106 ms after filling with "Recording stopped: the disk is nearly full (0.3 GB left)"; file kept: true; outputs unaffected |
| 425 s | RecoveryFile | Recovered | 7 ms | restore without a service refused; corrupt file loaded as nothing; save into a read-only folder failed and was survived; save, load and double discard behaved |
| 455 s | DetectorFailure | Recovered | 30002 ms | two failures: provider fallback; retry after 30 s; success restored all good |
| 505 s | EngineKill | Recovered | 2750 ms | SIGKILL pid 56793; relaunched as 58034 after 130 ms, state replayed, frame on screen at 2750 ms |

## Engine log

**Launches** (4)

- 0.1 s: pid 54933 relaunch=false
- 140.1 s: pid 55782 relaunch=true
- 307.2 s: pid 56793 relaunch=true
- 505.1 s: pid 58034 relaunch=true

**Losses** (3)

- 140.0 s: engine exited (signal: 9 (SIGKILL)) (planned=false)
- 307.1 s: engine unresponsive (planned=false)
- 505.0 s: engine exited (signal: 9 (SIGKILL)) (planned=false)

**Errors** (4)

- 175.0 s: engine: web http://localhost:51399/soak: renderer terminated; reloading
- 340.2 s: screen: layer change for an output the compositor does not have
- 341.2 s: screen: layer change for an output the compositor does not have
- 342.2 s: screen: layer change for an output the compositor does not have

**Transcription** (11)

- 0.0 s: Connected
- 60.0 s: Reconnecting { attempt: 1, delay_ms: 1000 }
- 61.0 s: Connected
- 240.0 s: Error { message: "IO error: Connection reset by peer (os error 54)", fatal: false }
- 240.0 s: Reconnecting { attempt: 1, delay_ms: 1000 }
- 241.0 s: Reconnecting { attempt: 2, delay_ms: 2000 }
- 243.0 s: Reconnecting { attempt: 3, delay_ms: 4000 }
- 247.0 s: Reconnecting { attempt: 4, delay_ms: 8000 }
- 255.0 s: Reconnecting { attempt: 5, delay_ms: 16000 }
- 271.0 s: Connected
- 600.0 s: Disconnected

