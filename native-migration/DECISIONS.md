# Decisions log

Append-only and newest last. This file **overrides** any prompt or planning document.
When a prompt must deviate from its text, record why here first.

Format:

```
## YYYY-MM-DD — <prompt id> — <short title>
Decision:
Why:
Alternatives rejected:
Affects prompts:
```

## 2026-11-09 — plan — Planning baseline

Decision: This plan was written against commit `de3bb54` on `ci/release-packaging-retries`.
The working tree also held uncommitted owner edits to `overlay.html`, `App.tsx`,
`AppShell.tsx`, `MediaDock.tsx`, `ScreenConfiguration.tsx`, and a new
`tests/document-page-swap.test.ts`. The audit read those files as they stood.
Why: Prompts compare against this commit when checking for drift.

## 2026-11-09 — plan — Directory location

Decision: Planning files live in `native-migration/`, and the new code in `kairo-native/`.
Why: `.gitignore` ignores `docs/*` except named files, and `native/` already holds C++ and
Objective-C++ helpers that the new app reuses.

## 2026-11-09 — plan — Proposed decisions awaiting prompt 008

These are recommendations, not settled facts, until the owner confirms at prompt 008:
two-process design, `wgpu` compositor, CPU text raster, FFmpeg, CEF web host as an
on-demand component, no `gpui-webview`.

## 2026-11-09 — plan — Final review corrections

Decision: A review against the code corrected these planning errors: provider keys are not stored in settings (they go through the managed gateway); `bible.db` is copied to writable userData; the theme font list is fixed; `Transcription.tsx` appears to be dead code. Handler-level logic gained an owner in prompt 067, and Electron `localStorage` gained a migration.
Why: Each was checked against source at the planning baseline.

## 2026-11-09 — plan — Separate repository for the native app

Decision: The new app lives in `kairo-gpui-kit`, at the workspace root, and this playbook moved there. `../kairo-presenter` stays the reference and keeps shipping. This supersedes the earlier `kairo-native/` location.
Why: The owner wants to keep updating the Electron app during the migration.
Consequences: prompt 009 adds a sync process; native helpers are copied with recorded source commits; CI works without the Electron checkout; releases still publish to `CodeLawd/kairo-presenter`, which the updater and website read.

## 2026-11-09 — 000 — Bootstrap scope and toolchain

Decision: Use stable Rust with rustfmt and Clippy, following the installation
guide's Rust 1.92-or-newer baseline. The published GPUI Kit 0.9.0 manifest
declares edition 2024 and does not declare a separate minimum Rust version.
Why: The installation guide ties the minimum to the Linux dependency graph.

Decision: The UI binary is the hello-world exception to empty workspace
members. Other application and tool binaries have a doc comment and an empty
entry point so Cargo can build them. Preserve the starter's GPUI initialization
and replace its sample counter with the requested hello-world view.
Why: The prompt requires a window smoke test and buildable binary members.

Decision: Install project-scoped GPUI Kit skills for Codex using
`npx skills add longbridge/gpui-kit --skill gpui-kit gpui-kit-design-guides --agent codex --yes`.
The installer placed them at `.agents/skills/gpui-kit/` and
`.agents/skills/gpui-kit-design-guides/` and wrote `skills-lock.json`.
Why: Both requested skills are available in the native checkout.

Decision: Keep the website separate from the desktop workspace. The operator can
continue using Next.js in a separate project.
Why: The operator confirmed this during bootstrap.

GPUI Kit 0.9.0 enables the platform's `runtime_shaders` feature. The bootstrap
CI follows the documented platform setup and adds no standalone Metal compiler step.

## 2026-11-09 — 000 — Local build and window evidence

Decision: Record the clean build of the workspace, including `apps/kairo`, at
153.02 seconds wall time, and the immediate unchanged `cargo build --locked -p kairo`
at 1.02 seconds. These were measured on macOS with Rust 1.99.1. The clean
measurement includes the first dependency downloads and compilation.

Decision: Capture the hello-world window by placing the same built executable
in a temporary macOS app bundle. `cargo run --locked -p kairo` launched the
executable, but the UI tool could not target the bare executable by path.
The app bundle was only for the screenshot and lives outside the workspace.

Decision: Keep the cross-platform CI gate open. The new native repository
has no Git remote, so there is no GitHub Actions run to verify.

## 2026-11-09 — 000 — Remove the dedicated web host

Decision: Remove the `kairo-web-host` binary from the native workspace. The
website remains a separate Next.js project. This supersedes the proposed CEF
host and any instruction to add that binary. Prompts that depend on the host
need a revised approach before implementation.

## 2026-11-09 — 001 — Begin baseline during hosted CI

Decision: The operator authorized starting the Electron baseline while the
remaining bootstrap CI jobs finish.

## 2026-11-09 — 002 — Engine spike structure and API differences

Decision: `spikes/engine-window` declares its own `[workspace]`, so its throwaway `winit` and `interprocess` dependencies stay out of the app lockfile. The root workspace gates do not cover it, so its gates are run separately.
Decision: Shared memory is a memory-mapped temporary file with three seqlocked frames. A SIGKILLed writer leaves no reader state to repair.
Decision: The 15 and 30 fps soaks ran concurrently to fit one 30-minute window. CPU was measured in separate 60-second runs.
API differences: `wgpu` 30 presents with `queue.present(frame)` and returns a `Result` from `get_mapped_range`. `interprocess` 2.4 deprecates `split`, so the stream is shared through `Arc` and `&Stream`. winit 0.30 has no non-focusable window, so the spike replaces `canBecomeKeyWindow` and `canBecomeMainWindow` for borderless windows through the Objective-C runtime.
Why: Each was read from the pinned crate source. Prompts 008 and 050 must carry the focus override forward, or find a supported alternative.

## 2026-11-09 — 005 — NDI binding and spike scope

Decision: Recommend a thin `libloading` binding for NDI, loading the runtime from the app folder. This is a recommendation for prompt 061 and is subject to prompt 008.
Why: `grafton-ndi` 2.0.0 links `libndi` at build time, so a missing library stops the process at launch. Decision 001 requires a broken NDI to disable only NDI.
Alternatives rejected: `grandi`, which requires a Node runtime.
Decision: The spike builds its C ABI in `spikes/sinks` and not in `kairo-native-sys`, because spike code stays out of app crates. The binding resolves exported symbols rather than the `NDIlib_v6` table.
API difference: wgpu 30 exposes the import route as `Device::create_texture_from_hal` with `hal::metal::Device::texture_from_raw`.
Affects prompts: 008, 061, 064.

## 2026-11-09 — 003 — Text spike structure and results

Decision: `spikes/003-text` declares its own `[workspace]`, so its comparison dependencies stay out of the app lockfile. Its gates are run separately.
Decision: Recommend `parley` + `swash` for slide text. `cosmic-text` tied on line breaks but needed a spike-side family and nearest-weight lookup through `fontdb`.
Decision: The corpus adds empty and blank-verse cases beyond the prompt's list, and capture runs with software compositing so output is repeatable.
Open for 008: full-slide raster p95 is 25.28 ms against the 16 ms target.
Affects prompts: 008, 052.

## 2026-11-09 — 006 — Restore the browser component for spike 006

Decision: The operator confirmed the native app needs an embedded browser for the live web layer, web preview, Slido and PPTX to PDF, and that CEF is acceptable. This corrects the 2026-11-09 prompt 000 decision "Remove the dedicated web host", which is superseded for the CEF host. That decision's statement that the website remains a separate Next.js project still stands.
Decision: Spike 006 lives under `spikes/` with its own `[workspace]`. Adding `kairo-web-host` to the app workspace is left to prompts after the go/no-go at 008.
Affects prompts: 006, 008, 045, 047, 059, 073, 089, 101.

## 2026-11-09 — 007 — UI spike findings

Decision: Measurements used GPUI's `frame_duration_snapshot`, the source `gpui-fps` reads, rather than its on-screen HUD.
API difference: `KeyBinding::new` rejects `secondary-`, which only OS-aware parsing accepts.
Decision: The side-by-side uses an isolated Electron profile, to keep personal data out of committed evidence. It only reaches onboarding.
Recommendation for prompt 070: a focused rich text editor on `EntityInputHandler`, not Kit's `Editor` or `TextView`.
Open question for the owner: whether Low Power Mode should keep GPUI's 30 fps default.
Affects prompts: 008, 070.

API differences (006): `cef` 156.4.0 names getters without `get_` (`view_rect`, `audio_parameters`). `PdfPrintSettings` takes inches where Electron takes microns. `execute_java_script` has no isolated world.

## 2026-11-09 — 008 — Waive open spike items for the go/no-go

Decision: The owner chose to start prompt 008 with prompts 006 and 007 still `blocked`, accepting their open items as known risks. Their tracker status is unchanged, because the work is not complete.
Accepted for 006: no real Slido sign-in yet; subframe header removal not implemented; Windows and Linux not run.
Accepted for 007: 120 Hz not proven; the Low Power Mode frame-rate question remains open.
Carry forward: prompt 047 proves real sign-in; prompts 070 and 107 revisit UI frame pacing.

## 2026-11-09 — 008 — Go decision

Decision: **GO**, given by the owner, who accepted every recommendation below as presented. Prompts 006 and 007 remain `blocked` under the waiver above.

1. Minimum macOS: target 11, and test on real macOS 11 before release. Why: the binary declares 11.0, but no run on it exists.
2. Text stack: `parley` + `swash`. Why: 399/400 exact breaks and no font-matching workaround. Raster p95 of 25.28 ms misses 16 ms, so slide text is cached and rasterised on change.
3. Video: FFmpeg, dynamically linked LGPL build. H.264 and HEVC need legal review before release. Why: the soak passed; licensing is unsettled.
4. NDI: `libloading` over the official NDI 6 SDK. Why: 10 cycles passed. A real receiver is still unverified.
5. Web host: CEF as an on-demand signed component, offered during onboarding. Why: 148 MB zipped on macOS arm64, with its own security updates.
6. Zero-copy video: later, only if profiling shows a need.
7. Rich text: a focused editor on GPUI `EntityInputHandler` with marks as domain ranges. Not prototyped.
8. Linux: best-effort, Vulkan required, after macOS and Windows. Why: not run in any spike.
9. Intel Macs: remain supported until tested. Why: dropping them could strand existing churches.

Sync: run `tools/electron-sync` before each phase and before prompts 100, 109 and 112, as `SYNC.md` states.
Low Power Mode keeps GPUI's 30 fps default.
Conditions: confirm whether H.264 in the web layer matters to churches, since standard CEF cannot play it and an unplayable required format is a stop condition. Windows is untested across the spikes and is the largest remaining risk.

## 2026-11-09 — 011 — Oracle comparison rules

Decision: Rust ports are compared with the TypeScript originals through `kairo-oracle`, by these rules.

1. Object key order is ignored.
2. Numbers compare exactly as `f64`. A tolerance applies only where a test calls `compare_with_tolerance` and says why. `-0` equals `0`, because `JSON.stringify` cannot tell them apart.
3. `NaN`, `Infinity` and `-Infinity` travel as `{"$oracle":"number","value":"NaN"}`; Rust produces them with `kairo_oracle::number`.
4. An `undefined` object property equals an absent key, matching `#[serde(skip_serializing_if = "Option::is_none")]`. It never equals `null`, and `null` never equals an absent key. An `undefined` return value or array element must be written as `kairo_oracle::undefined()`.
5. A TypeScript throw or rejected promise must be matched by `kairo_oracle::thrown()`. The error name and message are cached but not compared, so ports may use their own error types.
6. `Date`, `Map`, `Set` and typed arrays are tagged in both directions. `bigint`, functions, symbols and circular values are harness errors, not results.
7. Results are cached per export in `tests/oracle-cache/<module>.<export>.jsonl`, keyed by a hash of the argument JSON, with the Electron commit and whether `src/lib` was dirty in the first line. Tests read only the cache unless `KAIRO_ORACLE_MODE` is `live` or `refresh`.

Why: these match how serde serialises the planned domain types, keep CI free of Node, and make a cache miss a loud failure that names `cargo run -p ts-oracle -- --regenerate`.
Affects prompts: 012 to 016, 035, 036.

## 2026-11-09 — 012 — How the domain types map TypeScript

Decision: `kairo-domain` ports every persisted and exchanged data type in the prompt's source list, plus the types they import (`displays`, `keyboard-shortcuts`, `scripture-trace`, `lyrics-duplicate`). API interfaces (`*API`), storage interfaces with methods, and functions are not types and are left to their owning prompts.

1. Every TypeScript `number` is `f64`, including counts and timestamps, so any number a file holds reads back unchanged. `kairo_domain::json::to_js_value` writes whole numbers as integers, as `JSON.stringify` does.
2. `x?: T` is `Option<T>`, absent when `None`. `x: T | null` is `Option<T>` that must be present. `x?: T | null` is `Option<Option<T>>`. In persisted types (those with an `extra` map), `null` in an `x?: T` field is a parse error rather than `None`, because `None` would drop the key on save; exchanged types accept it, since servers and IPC may send `null`.
3. String-literal unions are enums with exact renames and an `ALL` list; number-literal unions (`0 | 1 | 2 | 3`, frame rates) are enums written as integers; `true` literals are `serde_util::True`. Unknown values fail the parse; they are never mapped to a default.
4. Discriminated unions use serde's internal tagging on the TypeScript tag field. `ProgramQuestion`, which has no tag, is `untagged` with the join card first.
5. Persisted objects keep unknown keys in `#[serde(flatten)] extra`. Maps keyed by open-ended names (usage counts, shortcut commands) keep string keys or an `extra`, so newer names survive.
6. Settings have two layers. `AppSettings` is the canonical shape. `StoredSettings` is the file as found: each of the 18 sections is `Stored::Typed` when canonical and `Stored::Raw(Value)` otherwise, absent sections stay absent and `null` sections are kept. Other store files load as `Stored<T>` as a whole. Nothing is repaired on read; prompts 013 and 021 own that. Why: files written before a section existed (the owner's `proautomate-settings.json` lacks five sections and carries ProPresenter keys) must load without loss, and the TypeScript types do not describe them either.
7. `serde_json::Value` appears only where the shape is unknown by definition: `extra`, `Stored::Raw`, `EngageCommand` `poll-save` (`unknown`, checked by `normalizePoll`) and `ManagedRequest.body` (`unknown`, a provider's own body).
8. `DEFAULT_OVERLAY_SETTINGS` and the theme defaults are built by `overlay-defaults.ts`, which prompt 013 ports, so `AppSettings::defaults` takes the overlay section as an argument. Its TypeScript value is compared by round trip only.
9. Oracle helpers: the runner now reads exported constants (no arguments) and loads `oracle:<name>` modules from `tools/ts-oracle/modules`. `settings-defaults.ts` evaluates the `defaults` literal from `src/main/db/index.ts` with the TypeScript compiler, and builds a fresh-install file with theme ids and time pinned. Cache file names replace `:` and `/` for Windows.

Affects prompts: 013 to 016, 021, and every prompt that reads or writes these types.

## 2026-11-09 — 013 — Preserve first-pass overlay migration behavior

Electron creates default outputs with a sparse background theme. Normalizing
those outputs again fills `hue`, `saturation`, `brightness`, `contrast`,
`blurPx` and `mediaLoop`. Keep the first-pass result identical to Electron's
persisted shape. For output and settings normalization, assert that these
six additions are the only second-pass changes and that the third pass
matches the second. Other normalizers retain the first-pass idempotence check
where applicable.

## 2026-11-09 — 014 — Match JavaScript lyric text operations in Rust

Lyrics use UTF-16 code units where Electron exposes offsets or measures line
length. Rust strings preserve paired surrogates as Unicode scalars. RTF and
cursor-splitting operations work in UTF-16 first so they can handle offsets
inside a surrogate pair.

The translation helpers implement the repeated-character backreference with
an ASCII scan after key normalization. The glued section label lookahead
checks the following character explicitly. Neither pattern needs
`fancy-regex`.

Keep the source test cases that exercise service behavior with the code they
test. Prompt 024 owns `workspace-song-file.test.ts`, prompt 040 owns
`lyrics-usr-title.test.ts` and its file-import path, and prompts 040/041
own the canonicalization and online-ranking assertions in
`lyrics-multilingual-search.test.ts`.

## 2026-11-09 — 015 — Keep scripture renderer initialization with its view

The React static-render assertion in `scripture-initial-state.test.ts` checks
that a missing-API-key warning stays hidden while translation options are
loading. Prompt 078 owns the corresponding native scripture view test.
The other scripture offline-state tests exercise domain behavior and are
covered in prompt 015.

## 2026-11-09 — 016 — Domain boundaries for the remaining shared exports

- The coverage checker inventories declarations and re-exports using the
  reference checkout's TypeScript compiler. Native counterparts are indexed
  by source module. Explicit mappings cover names that differ, including
  constants represented by `Default` implementations. An unmapped export
  or a stale exception fails the check. The generated table records the
  owner and reason for each excluded Electron channel, DOM or rendering
  operation, async UI coordinator, and service archive runtime operation.
- Time and timezone come from the caller. Timers use epoch milliseconds and
  `chrono::TimeZone` for wall-clock deadlines, with chrono's OS clock feature
  disabled. The deadline picks the earlier instant in a DST overlap and
  advances through a gap. Capture naming uses local calendar components.
  Service text exports and locale-aware title ordering receive formatters
  or a comparator from their caller.
- Validation retains canonical Rust field types. Invalid non-boolean
  document slideshow flags fall back to defaults, and library or named
  setlist records without numeric timestamps are dropped as malformed.
  UTF-16 text limits stop at a scalar boundary if the JavaScript slice
  would split a surrogate pair. The wall-clock implementation accepts
  the date range representable by chrono.
- The transfer parser distinguishes unreadable JSON, foreign files, future
  versions, unsupported kinds, empty song lists and missing playlist bodies.
  It reuses song-file coercion, keeps the first duplicate song ID, drops
  unknown setlist references, and preserves unavailable scripture rows.
  The transfer service owns file I/O, the byte limit, picker cancellation,
  and committing or discarding previews.
- The library and usage ports preserve stale assignment handling, report
  totals rather than deltas, and keep today's usage after acknowledgment.
  The audience parser rejects invalid command shapes, limits request IDs
  and message size, and keeps the vote command's optional request ID.
  The web and account helpers validate URLs before returning them.

## 2026-11-09 — 020 — Paths, logging, crash handling and single instance

Decision: `kairo-store` holds `paths`, `logging`, `crash`, `instance`, `launch`, `build_config` and `os_locale`, and `apps/kairo` uses them at startup.

1. Packaged means a release build (`debug_assertions` off); a debug build is a dev build. Electron's `app.isPackaged` has no direct counterpart. The dev-profile rule is unchanged: a dev build uses `appData/Kairo Dev` with logs in its `logs` folder, unless `KAIRO_SHARED_PROFILE=1`.
2. Paths are computed, not asked of a framework, to match Electron byte for byte: macOS `~/Library/Application Support/Kairo` and `~/Library/Logs/Kairo`; Windows the Roaming AppData known folder plus `Kairo` and `Kairo\logs`; Linux `$XDG_CONFIG_HOME` (when set and non-empty, as Chromium does) or `~/.config`, plus `Kairo` and `Kairo/logs`. `crashDumps` is `userData/Crashpad` everywhere. Linux Documents and Videos fall back to `~/Documents` and `~/Videos` (Chromium's xdg-user-dirs fallback); on macOS and Windows a missing known folder is `None`, where Electron throws, and the workspace root falls back to home as `workspace/index.ts` does.
3. The native log is `kairo.log` in electron-log's folder, not `main.log`, so the two apps never interleave lines in one file. Format and rotation copy electron-log: `[yyyy-mm-dd hh:mm:ss.mmm] [level] text`, 1 MiB, one `.old.log`. Writes go through `tracing-appender`'s non-blocking (lossy) writer, so a full queue drops lines rather than block the caller.
4. Crashes: the panic hook logs, writes `session-recovery.json` from a registered snapshot provider (two-space JSON, as `JSON.stringify(state, null, 2)`), and adds an entry to a local ledger, `kairo-native-crashes.json` (count plus the newest 10, home folder scrubbed with `scrub_home`, messages cut to 1,000 characters). It does not add to `kairo-crash-reports.json`: that queue's `CrashKind` allows only `renderer` and `child_process`, which the server validates, and Electron does not queue uncaught exceptions either. Prompt 032 decides whether ledger entries are uploaded. The snapshot provider runs inside the hook, so it must not panic (a second panic aborts) and is reached with `try_lock`.
5. Minidumps: not implemented, by evaluation. Panics are caught by the hook. Native faults (FFmpeg, NDI, GPU drivers) need an out-of-process writer; the candidates are Embark's `crash-handler` with `minidumper`, which write Breakpad-compatible dumps through a monitor process. That belongs with the engine process (prompts 050 onward) or crash reports (032), where the second process already exists. Electron's own behaviour is local minidumps without upload.
6. Single instance: an exclusive `File::try_lock` on `userData/kairo-native.lock` plus an `interprocess` local socket named from a hash of userData (a file in the per-user temporary folder on macOS, the abstract namespace on Linux, a named pipe on Windows). A second launch sends `{argv, cwd}` as one JSON line and waits for `ok`, retrying the connection for 5 s. Relative `.kairo` arguments resolve against the sender's folder; Electron used the receiving process's folder, which is wrong for a second launch from another folder. The native lock does not exclude the Electron app, which has its own `SingletonLock`; running both at once is the owner's choice during migration.
7. Build-time URLs: `KAIRO_API_URL`, `KAIRO_WEB_URL`, `KAIRO_AUDIENCE_API_URL` and `KAIRO_LICENSE_PUBLIC_KEY` replace the `MAIN_VITE_*` names, read with `option_env!`; empty counts as unset. In a release build a constant assertion fails compilation unless the API, web and any audience URL are `https` and not local (`localhost`, `127.0.0.1`, `[::1]`, `0.0.0.0`). Anyone profiling a release build locally sets non-local URLs; there is no escape hatch.
8. `main-window.ts` (dialog parent) is GPUI window management, so it goes to the shell prompt (070), not here. Usage counting of uncaught errors (`usageService.trackError`) goes to the usage prompt.

Affects prompts: 021 to 025, 032, 050, 070.
