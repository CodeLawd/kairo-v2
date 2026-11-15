# Screens configuration implementation plan

Spec: `prompts/075-screens-ui.md`; current Electron screens components and integration flow.

Architecture: `ui-screens` owns the retained Screens window and pure editing rules. The app supplies a backend that applies edits against current shared settings under the existing settings lock, preserving concurrent theme changes. Core topics feed settings, displays, cards, stages and media; engine events trigger status refreshes without UI polling. Keep GPUI Kit 0.9.1 and Kairo tokens.

1. Add model and regression tests for card conversion, latest-state edits, deletion target/last audience, output limits, initial selection/show keys, binding conflicts and playlist restrictions.
2. Add the backend seam and Screens window: sidebar, canvas, detail tabs, forms, looks grid, transition and setup check. Surface loading, errors, unavailable hardware and disconnected saved choices.
3. Wire backend to settings, surface actor, engine, media and ui-state; install shell Screens overlay and Live/Syphon controls where supported.
4. Derive UI integration tests from Electron's card/add/bind/restart/delete flow and failure paths. Run fmt, clippy and workspace tests; launch and compare the windows, capture evidence. Record measurements and unverified conditions honestly.

Completion: all configuration UI and persistence tasks are implemented, including the final full-renderer private Theme preview. The owner explicitly skipped further builds, tests and app launches; this overrides the remaining qualification gates. Prior verification results and failures are retained in evidence/075/README.md. Final checks were source review, rustfmt and git diff whitespace checking only.
