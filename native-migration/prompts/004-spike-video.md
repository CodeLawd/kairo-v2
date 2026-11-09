# Prompt 004 — Spike: video decode into wgpu

**Phase:** 0. Foundations and gating spikes  
**Tracker ID:** `004-spike-video`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Replace Chromium's `<video>`. Prove smooth decoding, looping and seeking into `wgpu` for the formats Kairo accepts.

## 2. Prerequisites

- Prompt [000](./000-workspace-bootstrap.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/lib/pa-media-range.ts` — accepted video and image extensions
- `../kairo-presenter/src/lib/media-playback.ts` — loop, trim, rate and filter semantics
- `../kairo-presenter/src/lib/overlay-fit.ts` — timed-range loop behaviour in prepareOverlayMedia and playOverlayVideos
- `../kairo-presenter/src/main/services/output/overlay.html` — pause, seek and document video behaviour

## 4. Implementation requirements

1. Build a spike with `ffmpeg-next` and a build of FFmpeg licensed LGPL, dynamically linked. Decode with VideoToolbox, D3D11VA and VAAPI where available; fall back to software.
2. Upload NV12 and P010 planes each frame and convert to RGBA in a WGSL shader using the stream's colour matrix and range. Measure at 1080p30, 1080p60, 4K30 and with three simultaneous 1080p streams.
3. Separately assess zero-copy import of IOSurface, D3D11 shared textures and DMA-BUF through wgpu-hal. Recommend whether it is worth doing, with numbers.
4. Prove gapless-looking loop, trimmed loop, 0.25× to 2× rate, frame-accurate-enough seek, pause, end detection, and H.264, HEVC, VP9, ProRes, and `.webm`/`.mov` with alpha, using files of each extension in `pa-media-range.ts`.
5. Produce a codec licensing note. Electron currently supplies H.264 and AAC through its bundled FFmpeg; state what Kairo must do for an LGPL FFmpeg distribution, and flag anything needing owner or legal review. Do not claim legal conclusions.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- Throwaway code under `spikes/`.
- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.
- Spike code never merges into app crates. Its value is the written report.

## 6. Expected deliverables

- Spike and `native-migration/spikes/004-video.md`.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- 30-minute loop on a 1080p file with RSS and frame-time logging.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Targets: 1080p30 under Electron's measured CPU in baseline.md; no RSS growth; p99 frame interval under 1.5 times nominal.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Formats table complete with PASS or FAIL per format and platform tested.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 008, 055.
