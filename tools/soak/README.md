# soak

Scripted service with chaos against the real `kairo-engine` (prompt 108).

```sh
KAIRO_API_URL=https://api.kairo.invalid KAIRO_WEB_URL=https://web.kairo.invalid \
  cargo build --release -p kairo-engine -p soak
cargo build --release -p kairo-sinks --example decklink_sim   # simulated card + helper
target/release/soak --label clean            # 10 min, leak check
target/release/soak --label chaos --chaos    # 10 min with chaos events
```

The placeholder `.invalid` endpoints only satisfy the release-build check. The soak never contacts them.

Runs are capped at 10 minutes (owner limit). For longer runs, loop the command. Reports go to
`native-migration/evidence/108/<label>/report.{md,json}`. Leak growth compares the first minute of the second cycle with the last minute (DECISIONS 108). The exit code is 0 only with zero
unrecovered failures and no leak finding.

## What runs

- **Engine:** the release `kairo-engine` under `kairo_core::engine::EngineHandle` (the real
  supervisor).
- **Outputs:** every output that reports ready: a rehearsal window on display 0, two NDI feeds,
  Syphon, and a DeckLink port through `decklink_sim` (the engine's real card path, simulated
  hardware). NDI and Syphon names carry the label, so two runs can share a machine.
- **Script:** runs on its own thread, which stands in for the UI thread. One 5-minute cycle,
  played twice in a 10-minute run:
  - lyrics every 8 s and verses every 6 s, over a looping 1080p30 H.264 background;
  - a 5-minute countdown prop, the info clock and a ticker message;
  - document page turns every 4 s with preload;
  - a shared live web page served on the loopback.
- **Transcription:** `DeepgramStt` with the default (Electron) timings. It sends 16 kHz PCM16 to
  a local stand-in server through a TCP proxy, and the proxy can be cut. Speech events feed the
  real `ResilienceActor`.
- **Samples, every 5 s:** for the soak process (supervisor, transcription, resilience),
  the engine, the helper and the web content processes:
  - RSS and open descriptors;
  - system-wide GPU memory in use (`ioreg`);
  - produced, dropped and late frames per sink.

## Chaos schedule (`--chaos`)

| s | Event | Recovery checked |
|---|---|---|
| 60 | Deepgram closes the socket | Reconnect, speech health back to ok, audio flowing |
| 95 | Block the script (UI) thread 30 s, over the playing video | Every output keeps drawing (300 or more frames), audio keeps reaching Deepgram |
| 140 | SIGKILL engine | Relaunch, engine state equals the client's, frame on screen |
| 175 | SIGKILL web view content processes | New content process, engine untouched, frames |
| 205 | SIGKILL card helper | Card faults with the helper's reason, operator refresh brings frames back |
| 240 | Network lost 20 s (proxy cut, licence server unreachable) | Offline fallback while down, audio buffered, reconnect and replay, health all good |
| 305 | SIGSTOP engine (hang) | Heartbeat timeout, relaunch, state replayed, frame |
| 340 | Display surface parked and restored | Frame and state intact |
| 375 | Recording on a 2.6 GB RAM disk filled to ~300 MB | Recording stops with "disk is nearly full", outputs unaffected |
| 425 | Recovery file: corrupt, read-only folder, restore without a service, double discard | Each handled as `resilience.ts` does |
| 455 | Two AI detector failures | Provider fallback, retry after 30 s, success restores all good |
| 505 | SIGKILL engine | As at 140 s |
| 545 | `--os-sleep` only: `pmset sleepnow` | Someone must wake the Mac within 40 s. Then engine, frames and transcription are checked |

The engine is killed at most twice in any 60 s, under the supervisor's budget of three relaunches.

## Limits (reported as not verified)

- **Display replug:** the OS display-change event needs a second physical display. The soak
  only exercises the park and restore path.
- **Sleep and wake:** opt-in, because it sleeps the machine and needs someone to wake it.
- **Connectivity check:** its DNS probe is not cut. Its result is fed to `ResilienceActor`
  while the proxy is cut.
- **UI process:** the `kairo` UI process (GPUI) is not running. The soak process holds the
  same supervisor, transcription and resilience services, but no GPUI frame times are recorded.
- **Web content attribution:** on macOS the soak relaunches itself with responsibility
  disclaimed. The OS then attributes the engine's web content processes to it, and no other
  app's processes are ever signalled.
