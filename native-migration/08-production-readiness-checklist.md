# 08 — Final production-readiness checklist

Complete every item before prompt 112. Unverified means unchecked.

## Functionality

- [ ] Every row in `04-feature-parity-matrix.md` is PASS or has a written owner waiver.
- [ ] `check-api-coverage` and `check-lib-coverage` pass.
- [ ] Each existing test file is ported, replaced or justified.
- [ ] Detector replay shows no unapproved differences.
- [ ] Sync queue is empty at the released Electron commit, after an agreed freeze.

## Data

- [ ] All migration profiles pass forward and back to the latest Electron release.
- [ ] Settings backup occurs before the first native write.
- [ ] Sign-in and encrypted cache migration results are documented per OS.
- [ ] Songs, media, bundles and databases remain interchangeable.

## Output and hardware

- [ ] Golden suites green, with thresholds unchanged or owner-approved.
- [ ] `qualification.md` complete: displays, NDI receivers, DeckLink, Syphon, capture.
- [ ] Crash and replug recovery proven on real outputs.
- [ ] Inherited unqualified items are listed, not implied to be fixed.

## Performance and reliability

- [ ] Every budget met or waived in writing.
- [ ] Two clean 4-hour soaks, including chaos.
- [ ] UI stall does not affect output or transcription.
- [ ] Pilot: at least two live Sundays on each platform in use.

## Security and privacy

- [ ] Secrets never logged. Fail-open and fail-closed behaviour verified.
- [ ] Web content and Wry IPC messages treated as untrusted; message bounds tested.
- [ ] Local socket authenticated; engine protocol fuzzed.
- [ ] Usage and crash opt-outs verified to send nothing.
- [ ] `cargo audit` and `cargo deny` clean, or exceptions recorded.

## Packaging and distribution

- [ ] Signed and notarised macOS builds; signed Windows build if certificates exist.
- [ ] Clean-machine install on each supported OS.
- [ ] `.kairo` association, icons, permission strings correct.
- [ ] Third-party notices shipped, covering FFmpeg, PDFium, Wry and NDI attribution.
- [ ] NDI redistribution checklist from decision 001 complete.
- [ ] FFmpeg and codec licensing reviewed by the owner.
- [ ] Release job refuses localhost URLs and publishes only after all platforms pass.

## Upgrade

- [ ] Real upgrade from the latest Electron release proven on each OS.
- [ ] Native to next-native update proven, including tampered-file rejection.
- [ ] Rollback steps written and rehearsed.
- [ ] Changelog entry written for church teams, per `CLAUDE.md`.

## Operations

- [ ] Log locations and diagnostics documented for support.
- [ ] Crash categories visible in the admin console.
- [ ] `CLAUDE.md`, `AGENTS.md`, `README.md` updated.
- [ ] Website system requirements and download page updated.

## Sign-off

- [ ] Owner approves general release. Date: ________
