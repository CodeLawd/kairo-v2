# Prompt 102 — Windows packaging

**Phase:** 6. Hardening and cutover  
**Tracker ID:** `102-package-windows`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Ship the Windows installer.

## 2. Prerequisites

- Prompt [100](./100-data-migration.md) is marked `done`.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/package.json` — `build.win`, `build.nsis`
- `../kairo-presenter/.github/workflows/release.yml` — Windows steps

## 4. Implementation requirements

1. Decide between NSIS and the Kit-documented Inno Setup. Compatibility with electron-updater's installer handling and an in-place upgrade favours NSIS; verify and record.
2. Keep app identity, per-user install, choosable directory, shortcut, `.kairo` association and `AppUserModelID`.
3. Bundle the VC runtime if needed, FFmpeg, NDI and PDFium DLLs next to the binaries.

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

- Clean Windows 10 and 11 VMs. Upgrade over Electron. Uninstall removes the app and leaves user data.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Installer size against baseline.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Clean install and upgrade pass.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 104.

## Amendment: `gpui-webview` replaces CEF (2026-11-09)

Web content uses WebView2. Ship or bootstrap the Evergreen WebView2 runtime (it comes with Windows 11 and is missing on some Windows 10 installs), and test with it absent. See DECISIONS.md, "Replace CEF with `gpui-webview`".

## Amendment from prompt 043: bundle FFmpeg

Since 043, `kairo-media` links FFmpeg 9 dynamically through `ffmpeg-next =9.0.0` (DECISIONS.md, 043). Development builds load whatever FFmpeg the build machine has. On the development Mac that is Homebrew's copy under `/opt/homebrew/opt/ffmpeg/lib`. A package that does not bundle FFmpeg fails to start on any machine without that install. Users must not have to install FFmpeg.

1. Build FFmpeg 9 from source as an LGPL shared build: `--enable-shared --disable-static --disable-gpl --disable-nonfree --disable-version3`, with decoders limited to the containers and codecs the media dock accepts (mp4, webm, m4v, mov, ogv). Spike 004's configure line (`spikes/004-video/README.md`) is the starting point. Pin the source version and the configure flags in the release job. The major versions of `libavcodec`, `libavformat`, `libavutil` and `libswscale` must match the ones `ffmpeg-next` was built against.
2. Never ship a package-manager FFmpeg (Homebrew, vcpkg defaults, distro packages). Homebrew's is configured with `--enable-gpl --enable-libx264`, which would put the app under GPL terms.
3. Ship the FFmpeg LGPL licence text and a written offer or link for the exact FFmpeg source and configure flags used.
4. H.264 and HEVC decoding still need the legal review that DECISIONS.md requires before release. Record the outcome before the first public build.
5. Build FFmpeg with MSVC (or an MSVC-compatible MinGW build) and put `avcodec-*.dll`, `avformat-*.dll`, `avutil-*.dll` and `swscale-*.dll` next to `kairo.exe` and `kairo-engine.exe`, as requirement 3 says. Sign each DLL.
6. In the verification step, list every executable's imported DLLs with `dumpbin /dependents` and check that each FFmpeg DLL resolves from the install folder. Launch the installed app on a clean Windows VM with no FFmpeg on PATH, and check that a video thumbnail renders.
