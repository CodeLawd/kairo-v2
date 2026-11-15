# GPUI Fast backend evidence

Date: 2026-11-15. Machine: Apple M2 Pro, 16 GiB. Window: 1280×800.

Aggregate JSON files contain no transcript content. CPU is the Kairo UI process only, 100% = one core. Draw times are GPUI Window::draw on the main thread; present intervals measure newly submitted frames, not display scanout. No redraws at idle means there is no per-frame timing sample, not that all main-thread work is free.

The release development hook loads the real application and services into an explicitly marked isolated profile under target/gpui-fast-benchmark/profile. It creates 5,000 song files and 2,000 tiny PNGs. The media measurement concerns library UI work, not video/image decoder throughput. A saved local transcript is replayed over the ordinary typed service topic with derived interim updates at 100 ms/event. Private replay data and profile files remain ignored under target; they are not evidence artifacts committed here.

Build with the same configuration for both backends:

```sh
KAIRO_API_URL=https://api.example.com KAIRO_WEB_URL=https://example.com cargo build -p kairo --release --locked --features dev-hooks
python3 tools/gpui-fast-benchmark.py fast operator
python3 tools/gpui-fast-benchmark.py fast stream
python3 tools/gpui-fast-benchmark.py fast lyrics
python3 tools/gpui-fast-benchmark.py fast media
```

Run sequentially on macOS, leaving the benchmark window visible and the machine otherwise idle. The runner currently expects the already prepared Bible and engine under target/package-macos, and a local replay.jsonl under target/gpui-fast-benchmark for the stream case. The app starts measurement only after startup, five seconds to select the route and five more to warm it. Native scroll events request 32 pixels at 60 Hz and reverse every 300 requests; actual submission pacing is in the JSON. The CPU sampler waits for that start marker. The runner closes only its own child process when finished.

The pre files were captured before enabling either gpui-fast feature, with the same clock fixes and development instrumentation. The pre binary is preserved locally as target/gpui-fast-benchmark/kairo-pre. Earlier attempts that timed out on Keychain, sampled startup or produced no scrolling draws are excluded. An unavailable-keychain implementation is used only for the explicitly isolated development profile; shipping builds and normal development launches keep the real KeyringProtector.

These are single 30-second runs, not a controlled multi-run statistical study. Other desktop apps and concurrent development sessions existed. The tracker records missing verification, including total main-thread CPU time, Windows/Linux composition and 120 Hz. Neither private transcripts nor process-sample dumps are published.
