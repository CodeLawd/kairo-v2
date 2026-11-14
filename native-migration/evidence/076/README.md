# 076 evidence

- `operator-empty.png`: `cargo run -p kairo` (debug) in a fresh profile, Operator page. The transcript panel and page content are empty slots until 077 and 083. The rail shows the empty monitor ("Nothing is live") with the Main Screen enabled, the clear-layer strip with every layer dark, and the output picker.

Not captured: the monitor showing live content and the strip clearing it in the running app. Nothing in the app can send content yet (077 and later), so live clearing is covered by the UI integration tests (`crates/ui-operator/tests/ui.rs`) and the engine clock by `apps/kairo-engine/tests/video.rs`. No Electron side-by-side: an isolated Electron profile stops at onboarding, as for 070 and 071.
