# Prompt 103 — Linux packaging

**Phase:** 6. Hardening and cutover  
**Tracker ID:** `103-package-linux`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Ship AppImage and deb.

## 2. Prerequisites

- Prompt [100](./100-data-migration.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/package.json` — `build.linux`, `build.deb`
- `native-migration/spikes/007-ui-platform.md` — Vulkan and windowing findings

## 4. Implementation requirements

1. AppImage and deb with desktop entry, icons, `.kairo` MIME, runtime dependencies. Vulkan-missing case gives a clear message instead of a crash.
2. Keep Linux category `AudioVideo`, desktop name `Kairo.desktop`, and review deb depends (`libasound2`, `libnss3`) for the new binary.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- Packaging scripts.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Ubuntu 22.04 and 24.04, X11 and Wayland. NDI load.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Size.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Clean install passes on both.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 104.

## Amendment: `gpui-webview` replaces CEF (2026-11-09)

Web content uses WebKitGTK 4.1 and needs X11 or XWayland. Declare the WebKitGTK and GTK 3 package dependencies, and start the app on X11 (`gpui_kit::platform::linux(WindowingModes::X11)`). See DECISIONS.md, "Replace CEF with `gpui-webview`".

## Amendment from prompt 043: bundle FFmpeg

Since 043, `kairo-media` links FFmpeg 9 dynamically through `ffmpeg-next =9.0.0` (DECISIONS.md, 043). Development builds load whatever FFmpeg the build machine has. On the development Mac that is Homebrew's copy under `/opt/homebrew/opt/ffmpeg/lib`. A package that does not bundle FFmpeg fails to start on any machine without that install. Users must not have to install FFmpeg.

1. Build FFmpeg 9 from source as an LGPL shared build: `--enable-shared --disable-static --disable-gpl --disable-nonfree --disable-version3`, with decoders limited to the containers and codecs the media dock accepts (mp4, webm, m4v, mov, ogv). Spike 004's configure line (`spikes/004-video/README.md`) is the starting point. Pin the source version and the configure flags in the release job. The major versions of `libavcodec`, `libavformat`, `libavutil` and `libswscale` must match the ones `ffmpeg-next` was built against.
2. Never ship a package-manager FFmpeg (Homebrew, vcpkg defaults, distro packages). Homebrew's is configured with `--enable-gpl --enable-libx264`, which would put the app under GPL terms.
3. Ship the FFmpeg LGPL licence text and a written offer or link for the exact FFmpeg source and configure flags used.
4. H.264 and HEVC decoding still need the legal review that DECISIONS.md requires before release. Record the outcome before the first public build.
5. AppImage: put the `.so` files in `usr/lib` inside the image, with an `$ORIGIN/../lib` rpath on the app binary and on `kairo-engine`. deb: either ship the bundled libraries in `/usr/lib/kairo` with that rpath, or depend on distro FFmpeg packages only if the target distros ship FFmpeg 9 LGPL builds with matching major versions. Bundling is the default. Record the choice.
6. In the verification step, run `ldd` on every binary and check that each FFmpeg library resolves inside the package. Launch on a clean Ubuntu LTS VM with no `libavcodec` installed, and check that a video thumbnail renders.
