# 067 — Application wiring evidence

`real-engine-output.png`: full-screen capture on 2026-11-14 while
`KAIRO_HOLD_OUTPUT=8 cargo test -p kairo real_engine_startup -- --ignored` ran.
The test starts every composed service against a temporary profile and the
real `target/debug/kairo-engine`, enables a screen output on the only display
through `settings.set("overlay")`, adds a song and pushes its first slide with
`Actions::push_lyrics`. The engine drew "Application wiring verified" with its
reference line. Local startup, measured by the test: 1,685 ms (debug, no
network awaited).
