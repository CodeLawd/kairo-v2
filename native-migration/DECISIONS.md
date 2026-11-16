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

## 2026-11-09 — 022 — Electron OS-crypt and native data keys

The Electron dependency is `^33.2.0`. The installed verification runtime reports
Electron 33.4.11 and Chromium 130.0.6723.191. Electron wraps a base64-encoded
32-byte data key in `safeStorage`. Both Kairo stores then use AES-256-GCM with
a 12-byte nonce, a 16-byte tag and no associated data. JSON and SQLite carry the
ciphertext, IV and tag in separate base64 fields.

- macOS `v10` uses the application's Safe Storage generic-password item.
  Chromium derives a 128-bit AES key with PBKDF2-HMAC-SHA1, salt `saltysalt`,
  and 1,003 iterations. The payload is CBC with PKCS#7 padding and a 16-space
  IV. The signed application's keychain access control can prompt the operator
  when the native executable first reads the item. [Chromium macOS OS-crypt](https://raw.githubusercontent.com/chromium/chromium/130.0.6723.191/components/os_crypt/sync/os_crypt_mac.mm)
- Windows `v10` uses a 12-byte nonce followed by AES-256-GCM ciphertext and
  tag. Its Chromium key is in `Local State` under `os_crypt.encrypted_key` as
  base64 of a `DPAPI` marker and the protected key. Legacy unversioned blobs
  go directly to DPAPI. Electron reads Local State from session data, which
  defaults to userData. [Chromium Windows OS-crypt](https://raw.githubusercontent.com/chromium/chromium/130.0.6723.191/components/os_crypt/sync/os_crypt_win.cc), [Electron browser process](https://raw.githubusercontent.com/electron/electron/v33.4.11/shell/browser/browser_process_impl.cc)
- Linux `v10` uses the basic-text password. `v11` uses the selected desktop
  backend's password. Chromium derives the CBC key using the same salt and
  IV as macOS with one PBKDF2 iteration. Electron reports the selected backend,
  which can include Secret Service or a KWallet variant. [Chromium Linux OS-crypt](https://raw.githubusercontent.com/chromium/chromium/130.0.6723.191/components/os_crypt/sync/os_crypt_linux.cc), [Electron safeStorage](https://github.com/electron/electron/blob/v33.4.11/shell/browser/api/electron_api_safe_storage.cc)

Decision: Keep Electron's wrapped key files for rollback. Rewrap recovered data
keys in profile-specific native keyring entries. Keep cloud credentials and
licensed-text cache keys separate. Account, settings and scripture service
callers consume these persistence APIs in prompts 021, 030 and 034.

## 2026-11-09 — 023 — SQLite ownership

Decision: Use bundled SQLite with FTS5 and keep Electron's schema and tokenizer
declarations. Own connections on dedicated threads with bounded async queues.
Install the seed with an atomic no-clobber publish, and open standalone pack
files read-only and immutable for validation. Domain query services and pack
installation are owned by the later scripture and lyrics prompts.

### 022 verification and recovery

On macOS the fixture used a unique ad-hoc signed Electron app and a disposable
userData directory. A separately signed Rust test executable opened both
legacy wraps, read the encrypted token and verse, then reopened the credential store
using its native keyring entry. The legacy wraps remained in place and
the test cleaned up its fixture keychain entries. macOS may show an Allow
prompt when the executable's signature changes. The dialog itself was not
captured. Run the opt-in `real_macos_electron_profile_survives_migration` test
with the profile produced by `crates/kairo-store/tests/fixtures/electron-secure-profile.cjs`
inside a uniquely named `KairoMigrationFixture-*` test bundle.

The cloud store returns no remembered sign-in on a keychain or read failure.
Corrupt JSON, malformed payload fields, invalid authentication, and unusable
keys discard the affected credential. Writes use private atomic replacement.
The notice is the existing credentials notice. The API.Bible cache stays
disabled when protected storage is unavailable. A missing or unusable data
key triggers removal of the old cache and side files before installing a
replacement key. This runs once under exclusive profile ownership before
opening SQLite connections. A cleanup failure leaves the new key unset for
retry. Callers can surface the same notice when cache data was discarded.

The Windows implementation tries DPAPI with Local State or the legacy
direct envelope. The Linux implementation reads Secret Service or an
already-running KWallet daemon for `v11`, and accepts Chromium's `v10`
basic-text wrapper for import into a protected native entry. Windows and Linux
runtime migration and permission behavior still need platform runs. The native
settings setter blanks incoming provider keys. The cloud session startup in
030 can reuse `retire_provider_keys` to blank old values from both the
`stt` and `lyrics` sections without disturbing unrelated settings.

### 023 validation boundaries

An absent bundled seed starts an empty Bible database with canonical books.
An existing destination takes precedence, including across upgrades. Copy,
open, schema, and worker failures are returned to callers. SQLite replays
committed WAL frames on reopen and drops an interrupted transaction. A
read-only pack is validated against fixed SQL, SQLite integrity checking and
canonical verse numbering, including translation overrides. A nonempty WAL
is rejected because an immutable connection would ignore those frames. The
quote index uses an in-memory database and in-memory temporary storage.

The layer owns schema and connection errors, seed installation, pack file and
row validation, and cache table cleanup. Search fallback, API cache coverage
and freshness, pack download and decompression, and lyrics folder and import
operations remain with the consuming services. The TypeScript development
seed, pack build and import tools stay in the Electron checkout.

## 2026-11-09 — 021 — Settings store and migrations

Decision: `kairo-store::json_store` is one electron-store-compatible store for every file; `kairo-store::settings` runs the `db/index.ts` launch repairs in order and gives `settings:get`, `settings:set` and `settings:getAll`.

1. Files are compared with Electron as JSON values, not byte for byte. Values, number text and layout (`JSON.stringify(value, undefined, '\t')`) match; object key order may not. Electron keeps whatever key order its input had (`normalizePresentationSettings` spreads stored objects, built-in themes keep their literal's order), and typed Rust values cannot. Key order has no meaning to either app's reader, and the oracle already ignores it (011).
2. Values stay in memory after opening; electron-store re-reads the file on every `get`. Only Kairo writes the file while it runs (single instance, 020).
3. Writes run on a per-store background thread: a set updates memory and queues the newest snapshot; a burst becomes one write; `flush` waits. Each write is a temporary file in the same folder, `fsync`, rename and a folder `fsync` on Unix. Temporary files left by a killed write are removed when the store next opens.
4. A file that is not valid JSON is kept as `<name>.corrupt-<time>.json` and the store starts from its defaults. Electron throws and does not start; refusing to start over a bad settings file is worse during a service.
5. A failed write (full disk, read-only folder) is logged and kept in `last_write_error`; values stay in memory and the app runs. Electron throws. A read-only *file* in a writable folder saves in both apps, because a rename replaces it.
6. Hostile shapes Electron crashes on (`stt: null` reads `null.llmProvider`) are read as empty objects (`{...value}` semantics), so the repair completes.
7. `settings:set` returns `SettingsEffect`s (reconcile outputs, broadcast settings, sync the church profile, apply scripture, prune props) instead of calling services, which belong to later prompts. Secrets for `stt` and `lyrics` are merged with `clearKeys` and then stored blank, as Electron does; `secretsConfigured` is passed in from the licence (032).
8. `kjvDefaultV1` is `SettingsStore::migrate_unreadable_nkjv_default`, taking a `PostOpenServices` that answers whether a Bible pack is installed. Prompt 067 calls it at app ready, before the window loads, where `src/main/index.ts` runs it. Not wired yet: no scripture service exists.
9. `ui-state.json` is a plain store (no defaults) for view preferences Electron kept in `localStorage`; prompt 100 fills it.
10. The oracle can now run the Electron app's real startup: `oracle:settings-launch.launch` runs `src/main/db/index.ts` in a child Node process with `electron` and `electron-log` stubbed (`tools/ts-oracle/modules/electron-launch/`) and `Date.now` and `crypto.randomUUID` pinned. The pinned ids match, which also proves the repairs call for ids in Electron's order (`normalizeThemeLibrary` makes a default id on every call).

Affects prompts: 022 to 025, 032, 067, 100.

## 2026-11-09 — 025 — Core transport and GPUI verification

Decision: Register one actor per service type, with a typed message and reply.
A full mailbox rejects fire-and-forget sends. Calls wait asynchronously for
capacity and skip cancelled queued work. Latest-value topics replace snapshots.
Ordered topics retain 64 events per topic and report a gap to lagging subscribers.
The GPUI bridge requests a display frame only when high-rate state arrives and
samples the freshest state at that frame. The view owns the cancellable task.

The pinned GPUI Kit 0.9.0 `test-support` feature caused Cargo to resolve GPUI
Fast's exact Taffy 0.15.0 requirement against the workspace's locked 0.15.1.
The verification view runs against the existing desktop backend as an example
without that feature. The GPUI Kit pin and Taffy lock remain as they were.

Bootstrap preserves completed resources when another loader errors, panics or
runs past eight seconds. Its request-scoped progress reports the first pending
label and ends with Ready. Timed-out loaders are dropped, so late results cannot
change the returned snapshot. The app shell still supplies concrete loaders,
applies the startup snapshot and handles window-scoped quit confirmation in 067
and 071. Output dispatch and service restoration belong to 039.

Quit finishes capture before closing audience and quote work, starts the NDI
stop, and allows two seconds for output release. Actor mailboxes stop after
that sequence. Queued and running async work is cancelled when actors stop.
Filesystem work that already started may finish on a blocking worker.

The resilience actor publishes health and recovers the existing
`session-recovery.json` via the crash-safe store writer. Inactive services
cannot restore a snapshot. Missing and corrupt snapshots are treated as absent
at the actor boundary. Recovery monitoring runs only while requested by the
session owner. Provider retries carry tickets so an old timer cannot end a new
fallback, or override an offline transition or successful request. Probe
scheduling and applying a validated recovery snapshot to the orchestrator
remain with its service owner.

## 2026-11-09 — 024 — Workspace folders and song files

Decision: `kairo-store::workspace` holds the workspace service, song-file helpers and the path checks.

1. Cross-volume moves copy each entry to a hidden `.<name>.kairo-moving` beside its destination, rename it into place, then delete the source. Electron copies straight to the final name, so an interrupted copy left a partial file that the next move then kept (the destination "wins"). Here an interruption leaves the source whole and the partial copy hidden; the next move clears it and retries. After the rename and before the delete both copies exist, which loses nothing.
2. Song files are written atomically (temporary file, `fsync`, rename); Electron writes them in place. The name and bytes are identical to `serializeSongFile`.
3. `is_inside_root` is lexical, as `isInsideRoot` is; `is_under_allowed_folder` resolves both sides through the filesystem first, as `isUnderAllowedFolder` in `src/main/index.ts` does, so a symlink pointing out of an allowed folder and a broken symlink are refused. Paths are compared by component, so a sibling sharing the prefix is outside.
4. Services this module needs but does not own are left to their prompts: the folder picker for `workspace:chooseFolder` (settings UI), rebuilding SQLite from `Songs/` for `workspace:resyncSongs` and after a move (lyrics, 040), `mediaService.setFolder` after `workspace:adoptMedia` and the media rescan after a move (043), and opening a folder in Finder or Explorer for `workspace:reveal` and `workspace:revealSongs` (the UI; this module returns the folder to open). `listMediaFolders` and the rest of `tests/media-folders.test.ts` are the media service's (043).
5. The workspace root is computed from settings each time (`workspace.folder`, trimmed and resolved), with `ensure` cached by root, as in Electron.

Affects prompts: 040, 043, 070.

## 2026-11-09 — web — Replace CEF with `gpui-webview` and platform web views

Decision: The owner chose `gpui-webview` (GPUI Kit's Wry-based crate) over CEF. This supersedes item 5 of the 008 go decision and the 006 entry "Restore the browser component". There is no on-demand browser download: each OS supplies the engine. That is WKWebView on macOS, WebView2 on Windows, and WebKitGTK on Linux.

What a source review of `gpui-webview` 0.9.1 (with `lb-wry` 0.53.3) found. Nothing was built or measured, so every point here is unverified on hardware:

1. The crate wraps a native child view placed over the GPUI window. GPUI's renderer does not draw the page. Wry offers navigation, init scripts, script evaluation, an IPC handler, navigation and new-window handlers, transparency, zoom and a persistent data directory. It has no offscreen rendering and no frame callback.
2. The crate needs `gpui-kit` 0.9.1. The workspace pins `=0.9.0`, so the first prompt that adds it bumps the pin.
3. GPUI menus, popovers and dialogs draw above the page, and a partly scrolled page clips correctly, only with the `gpui-fast` feature on both `gpui-kit` and `gpui-webview`, and only on macOS and Linux. On Windows, and on gpui-pre, the page covers any GPUI content in its bounds.
4. Linux needs X11 or XWayland (`gpui_kit::platform::linux(WindowingModes::X11)`). Wayland-only sessions are not supported.
5. Init scripts run in the page's main world, as CEF's did. Slido reads are untrusted (no change from 006).
6. Gain over CEF: H.264 and HEVC play wherever the OS supports them, which removes the 008 condition about H.264. It also drops about 148 MB zipped, the five CEF helper apps, and Kairo's own CEF security patching.

How each use maps:

| Use | Plan | Risk |
|---|---|---|
| Web tab preview (089) | `gpui-webview` embedded in the operator window | Needs `gpui-fast` for overlays and clipping. On Windows, keep GPUI overlays outside the page bounds |
| Slido (047) | A separate GPUI window with a `gpui-webview`. Init script, navigation and new-window rules, and a persistent data directory per profile | Permission denial may need platform calls Wry does not expose |
| PPTX to PDF (045) | Hidden web view. Print through the platform: `WKWebView.createPDF` or `printOperation` on macOS, `ICoreWebView2_7::PrintToPdf` on Windows, `WebKitPrintOperation` on Linux, through Wry's raw handles | Wry has no PDF call; this is about 3 small per-OS functions. Paged output must match 1440×810 pt slides |
| Live web layer on outputs (059) | The program page runs in a Wry web view owned by `kairo-engine` (winit, not GPUI). Display outputs can show it as a native child view above the compositor surface. NDI, DeckLink, Syphon and the recorder need frames, which need platform capture. On macOS that is ScreenCaptureKit on the web view's window, or `takeSnapshot` as a fallback. On Windows it is Windows.Graphics.Capture on the WebView2 visual. On Linux it is WebKitGTK snapshot | **Unproven and the main risk.** Fades, transitions and layers above the page cannot be composited into a native view the way they could with CEF frames. Page audio cannot be routed to the audio engine through public APIs |

Condition: prompt 059 starts with a measured capture check: 1920×1080, 30 fps, p99 latency under 50 ms, on macOS and Windows. If capture fails, stop and ask the owner. The options are dropping web from capture-only outputs, or bringing back CEF for the program layer only.

Affects prompts: 045, 047, 059, 070, 071, 073, 089, 101, 102, 103.

## 2026-11-09 — 030 — Desktop cloud transport

Decision: Use reqwest 0.12.28 with rustls and the native trust store. Desktop
requests retain `X-PA-Client: desktop`. Health checks choose a host before
sending writes. Only reads can automatically move to the alternate host after
a transport failure. HTTP failures retain the server's message, including the
first Nest validation message. Redirects are disabled to keep credentials and
writes on configured origins. A token retry cannot cross an account change.

Axios 1.16.1's installed Node adapter uses `proxy-from-env` and Node's TLS
configuration. Reqwest honors the proxy environment, including `NO_PROXY`,
and can read static system proxy configuration on supported platforms. Native
roots replace Node's root selection. Node-specific options such as
`NODE_EXTRA_CA_CERTS` do not configure this Rust client. Electron `net` also
supports Chromium PAC/WPAD and integrated proxy authentication. Those behaviors
and Electron session certificate hooks are outside this transport. This
comparison was made from the installed sources and upstream docs, with
enterprise proxy and certificate fixtures still unverified.
[Reqwest 0.12.28](https://docs.rs/reqwest/0.12.28/reqwest/)
[Electron 33.4.11 net](https://github.com/electron/electron/blob/v33.4.11/docs/api/net.md)
[Node TLS configuration](https://nodejs.org/docs/latest-v20.x/api/cli.html#node_extra_ca_certsfile)

Decision: Keep rotating refresh tokens in memory as well as secure storage so
keychain failure only affects persistence across launches. Finish an in-flight
rotation even when the requesting view drops it. Stale sessions retain their
account snapshot and back off. A resume or focus signal bypasses that backoff.
While stale, a five-second wall-clock check detects long sleeps even if the
runtime monotonic clock paused. The shell can also forward its native wake
signal directly to `reconnect`. The compiled configuration launches the
system browser for pairing and account links. Explicit test configurations
emit a browser event for the embedding shell instead.

Provider keys were retired in the Electron source. The legacy org-secret pull
accepts the throttle flag and performs zero requests. The four account licence
inventory items belong to prompt 031, which owns licence activation, status,
refresh and notifications. The account focus command reconnects the session.
The licence refresh on focus is wired with the licence service in 031.

Local verification uses a scratch copy of the unchanged Nest app and an empty
loopback MongoDB. The scratch Audience config uses a different development
port when 8787 is occupied. The server's reuse grace deliberately permits a
short race and an unused direct successor. The opt-in native test consumes
successors and waits beyond the race window before asserting family revocation.

## 2026-11-09 — 031 — Licence and onboarding

Decision: Verify the server's Ed25519 signature over its UTF-8
`JSON.stringify(claims)` envelope with ring. A compiled public key takes
precedence over a protected pin established by a successful authenticated
online lease. Cached snapshots never establish their own key. Keep the
Electron grace ceiling of 14 days, the subscription expiration bound, and
the 60-second rollback tolerance. Invalid, expired or differently bound
leases cannot enable a feature. A transport failure leaves the valid
offline lease in place. Licence activation and refresh preserve server form
messages, and provider readiness comes from the licence snapshot.

Startup wiring must migrate onboarding before the cloud session clears
legacy provider fields. The migration uses their presence without copying
the values. Persist the wizard snapshot before committing its migration flag.
The native boot shell and real transcription provider grants are later
integration checks.

## 2026-11-09 — 032 — Reporting privacy and archive wiring

Decision: Keep the current server crash and usage enums, which the server
validates against allowlists. The native source of each crash is carried
in `process`. Admin consumers can label `engine` as Engine, `web-host` as Web
host, `ui-panic` as UI panic, and `window` as Window. The wire kinds are
`child_process` for engine and web host and `renderer` for UI panic and window.
Usage retains `child_process_gone` for helpers, `renderer_gone` for windows
and `uncaught` for UI panic. No new wire enum values are required.

Decision: `note_error` accepts bounded diagnostic categories. Native
uploads omit raw console text, panic payloads and log tails. Legacy crash
queues are sanitized before any upload. The local panic ledger stamps the
privacy flags for each new entry once reporting is initialized, so re-enabling
a setting does not retroactively authorize an opted-out panic. Settings
changes discard unsent reports and cancel the corresponding HTTP wait.
Cancellation cannot retract bytes already received by the server. Usage
uploads only allowlisted event counters and setup metadata. Feedback and
explicit sermon recap uploads use their own content-bearing contracts and
are separate from diagnostic reporting.

The service records worker subscribes to the shared AppCore publisher for
transcripts, scripture suggestions, live plan and orchestrator state.
The caller supplies stop and clear hooks. Untitled services use the local
calendar and an English medium date format. Locale-sensitive title formatting
and live orchestration with the production shell remain later integration
checks.

## 2026-11-09 — 043 — Media and tracks libraries

Decision: FFmpeg is linked into the workspace through `ffmpeg-next =9.0.0`, against FFmpeg 9 shared libraries (Homebrew 9.0.2 on the development Mac). This replaces spike 004's `=8.1.0` pin, which does not compile against FFmpeg 9. Why: the owner chose to always link FFmpeg so every build has posters. Effect: every development and CI machine needs FFmpeg 9 development libraries and `pkg-config`, and the `kairo-media` poster tests need the `ffmpeg` command on PATH to make fixtures. Packaging (101–103) must bundle the matching FFmpeg 9 LGPL shared libraries, so users install nothing. The engine (050 onward) uses the same version.

Decision: posters are the decoded frame at or after 0.1 s, or the last frame of a shorter clip. Frames wider than 640 px are scaled down to 640 px wide. Each poster is written as a PNG with a JSON sidecar that holds its size and the container duration. The cache key is the first 16 bytes of SHA-256 over the resolved path, the mtime in nanoseconds and the size. `prune_cache` removes keys that are no longer in the library.

Decision: media delete stays an unlink, as in Electron (`fs.unlink`). Tracks remove moves the file to the OS Trash with the `trash` crate. A file already missing is treated as removed.

Decision: file clipboard. On macOS it reads and writes `NSFilenamesPboardType` through `objc2-app-kit`, then falls back to reading `public.file-url` and plain text. On Windows (`CF_HDROP`) and Linux (`text/uri-list` on X11 and Wayland) it uses `arboard` 3.7 file lists, then falls back to plain text. Copy and Cut fill only the in-app clipboard, as in Electron.

Decision: reveal (`media:revealItem`, `tracks:reveal`) returns the path. The UI opens the file manager, as decided for workspace in 024. Folder pickers (`chooseFolder`, `importFiles`, `addMediaToPlaylist`) are UI work (083, 084). `push`, `clear`, `seek`, and the surface half of `setPlayback` and `setPaused` belong to output and engine work. Their inventory rows stay `todo`.

Decision: three differences from Electron.
- Settings-era playlists migrate against the files of the scan in progress. Electron used the previous scan's items, which are empty at startup, so it emptied every migrated playlist.
- The repair of moved entries also runs against the scan in progress.
- Playlists are read leniently. Non-string item ids are dropped, and a missing `createdAt` reads as 0. Electron failed the whole manifest when an id was not a string.

Decision: `localeCompare` uses `feruca`, the CLDR root collation with non-ignorable variable weighting. A test checks its order against Node 22. Tracks sorting (`sensitivity: 'base'`) folds case and Latin accents before collating.

Decision: scans can run off the service. `scan_media_folder` is a pure function. `apply_scan` drops a result whose folder no longer matches the settings. Snapshot pushes are coalesced: each subscriber keeps only the newest snapshot. Error text for I/O failures is Rust's (`No such file or directory (os error 2)`), not Node's (`ENOENT: …`). The operator-facing prefixes match Electron.

Affects prompts: 050, 083, 084, 101, 102, 103.

## 2026-11-09 — 033 — Scripture lookup service boundaries

Decision: `kairo-scripture` holds `ScriptureService` (search, lookups, translation options, local packs, suggestions and row-translation requests), `bible_db` (the `bible-db.ts` queries on `kairo-store`'s SQLite workers), `reference`, `verse_bounds`, `versification`, `pack` and `installer`.

1. API.Bible is reached through the `api::ApiBible` trait, which prompt 034 implements over its client and encrypted cache. With no source attached the service behaves as Electron does with an empty API key and never touches the network; this is the startup path (`getTranslationOptions('')` is `get_translation_options(false, _)`). `init_offline_bibles` takes the cache opener and fails soft, as `offline-bibles.ts` does. The search routing, phrase-hit mapping and options logic stay in the service, so 034 supplies only id resolution, search and passage or chapter lookups.
2. Pack downloads go through the `pack::PackFetcher` trait. The production `HttpPackFetcher` uses `reqwest` 0.12 with rustls and native roots, the same choice as prompt 030, and a 120 s limit on the whole request. It lives here until `kairo-net` exposes a streaming download; it must be polled inside a tokio runtime. Transport failures are classified by kind (timeout, network) rather than by message, because `fetch` only says "fetch failed"; every other failure keeps Electron's message-based recovery text.
3. Pack file validation is prompt 023's `validate_bible_pack`. Its messages differ from `validatePackFile`'s (it walks the canonical numbering instead of counting first), and it adds an integrity check and a WAL refusal. Operators never see these: downloads show the installer's recovery text, and file-picker installation is not exposed. `validatePackRowsFor`, the converter (`scripts/build-bible-pack.ts`) and their CLI tests stay in the Electron checkout.
4. Duplicate install requests share one `futures_util::future::Shared` future instead of a promise. It runs while anyone awaits it; if every caller is dropped the download pauses and the next request resumes it. `setImmediate` before install is a single executor yield.
5. Not ported here, with owners: the quote worker and corpus versions (`searchGeneralQuote`, cache index rebuilds) go to 037, which subscribes to `on_corpus_change`; `retranslate` calls the orchestrator (039); `analyzeTranscript` has no caller in Electron and is not ported. The IPC, preload and packaging contract assertions in `local-nkjv-routing.test.ts` belong to 067 and 101.
6. `BIBLE_VERSE_COUNTS` is now public in `kairo-store`, and `PackVerse` derives `Clone`. `verse-bounds.ts` keeps its own table: it differs from the pack counts in eight chapters, as in Electron.
7. The oracle drives Electron's real `ScriptureService` in a child started as Electron's own main-process tests are (Electron as Node, `scripts/test-setup.cjs`), because better-sqlite3 is built for Electron's ABI. The cached comparison builds a generated Bible from the test's arguments; the bundled-Bible comparison is an ignored live test.
8. The phrase search's two `LIKE` passes are answered from memory (`like_index`), not by SQLite. Electron's query can only scan every verse in rowid order, which measured 16–18 ms per pass on the bundled Bible and put a phrase search at 14–19 ms median in Electron itself, over the 10 ms budget. The index keeps each verse's ASCII-lower-cased text in rowid order (12.4 MB of text for the bundled 93,306 verses), applies SQLite's `LIKE` rules (`%`, `_` as one character, ASCII-only case folding, the 50,000-byte pattern limit) and then reads only the matching rows. It is built when the database opens (30.6 ms measured) and rebuilt when `BibleDb` writes or when a reader's `PRAGMA data_version` shows another connection committed and the table's largest rowid or row count changed. A generated-text test checks it against SQLite's own `LIKE`, and both oracle comparisons check the results end to end. FTS, ranking and every other query still run in SQLite with Electron's SQL.

Affects prompts: 034, 035, 037, 039, 067, 078.

## 035: Deterministic scripture detector source map

- State (409–517, 800–856, 884–1174): pending input and origin, resolution generations, remembered book and chapter, pending chapter fallback, reading range, quote windows and provider revisions, last published translation context, deduplication and lifecycle.
- Regex tables and normalization (1620–2188): canonical aliases, STT repairs, spoken numbers, citation boundaries and verse lists, fused-number disambiguation, teen correction and verse bounds.
- Fast path (550–727, 765–799): resolve interim or final explicit citations, attach passage context, publish valid references and supersede stale model work. Window matching supplies another chance for split finals.
- Debounce (518–548, 728–764, 1176–1215 and live wiring): a chapter-only final arms a 5 s fallback. Wiring coalesces interim quote requests over 250 ms, clears evidence on audio epoch changes and cancels interim work on a final. The model queue retains the latest pending text.
- Dedupe (1049–1055, 1420–1536): expire old entries, suppress contained ranges, let broader ranges replace narrower ones, allow new translations, and let released entries be detected again locally.
- Model boundary (1216–1420, 1537–1617 and live wiring): run local rules first, then pass unresolved scripture-like speech or a damaged citation to the model. Prompt 036 owns provider calls, parsing, retries and cancellation.

The signal port combines the five boolean regex checks into one alternation. Its
Rust regex uses ASCII word boundaries, digits and case folding, and explicitly
expands JavaScript whitespace (including U+FEFF). The distinct-book branch
reuses the canonical book names from the domain port.

Chapter quote scoring reuses domain tokenization and spoken verse text. It
computes exact and one-word-gap runs in one traversal and groups scores by verse
before checking for ties. Empty candidates, short readings, insufficient runs
and tied scores produce no match.

## 2026-11-10 — 034 — API.Bible through the managed gateway

Decision: `kairo-scripture::api_bible` owns the API.Bible client, encrypted
cache, lookup and downloads behind a `BibleTransport` trait. Production uses
`kairo-cloud::bible_gateway::BibleGateway`, which sends
`{ provider: "bible", path: "/v1/...", method: "GET", query }` through
`LicensingService::request`, as Electron's `managedJson('bible', ...)` does.
The desktop never holds an API.Bible key.

1. The account key (`LicensingService::account_key`, session epoch plus
   church id) replaces Electron's API key for authorization. When it changes
   or the user signs out, `ApiBibleSource::set_gateway` drops every
   authorized id and the lookup, so a stale id cannot reach the server, come
   back 403 and purge valid text. Call `BibleGateway::sync` at startup and on
   each session change.
2. Electron attaches API lookups even when the cache cannot open
   (`NO_CACHE`). Native matches this with `ApiBibleSource::new(None, ..)`.
   Startup must attach the source with `cache: None` when `ApiCacheCrypto`
   fails, rather than calling 033's `init_offline_bibles` with an error,
   which would detach API.Bible completely.
3. When one download worker fails, the other worker is cancelled at its
   next await. Electron's `Promise.all` rejects but leaves the other worker
   running until its queue ends. Cancelling is safer for the rate limit and
   does not change the stored result: the run is `failed` either way.
4. A cache row that fails AES-GCM authentication is an error, never shown.
   A lost key discards the database, as decided for 022.
5. Electron does not throttle downloads while a service is live, so native
   does not either. The 2-chapter concurrency and 429 back-off are the only
   limits.

Affects prompts: 036, 037 (quote corpus reads `read_quote_batch`), 070.

## 035: Citation pattern review and failure paths

The citation parser uses `fancy-regex` to preserve lookaheads and the repeated
chapter backreference. Its string matcher does not support scoped ASCII
mode, so the port spells out JavaScript ASCII digits and word boundaries.
It expands JavaScript whitespace explicitly and rewrites the compound-number
hyphen/space character class as an equivalent alternation. Case-insensitive
citation rules scan ASCII-lowercased text while preserving the source text
from the original spans. Regex runtime errors skip a failed substitution
or stop a scan rather than panicking on transcript input.

The replay checks references, order, origin, virtual emission time,
translations, resolver, confidence and trigger text. General-corpus quote
index search and provisional results belong to 037. The rules replay
supplies deterministic chapter and damaged-citation lookup candidates and
uses no model or general-corpus quote provider.

Handled local rejection paths include empty and repeated interim speech,
ordinary numbered prose, invalid or ambiguous fused numbers, unknown books
and invalid verse bounds, unmatched remembered fragments, chapter timer
cancellation, missing lookup candidates, ambiguous chapter quotes, invalid
or uncorroborated damaged citations, reading-passage suppression, expired
context and dedup entries, and duplicates without new translations. Local
citation events advance synchronously on the caller's clock. The model
boundary exposes unresolved final text and a resolution generation.

## 036: Managed scripture model

- `kairo-scripture` owns the latest-pending queue, the provider-neutral runner,
  and model result validation. The `kairo-cloud` adapter uses
  `LicensingService::request` and its existing reqwest transport to
  `/v1/managed/request`. The gateway owns usage accounting. Model calls
  always carry the scripture purpose and no credentials or streaming flag.
- The Electron system and direct-only prompts are stored together as their
  exact UTF-8 request value. The Electron SDK capture compares full gateway
  envelopes for both providers, the default model, a named override and an
  explicit empty override. The latter is different from an omitted override.
- Retry handling keeps the installed SDKs' two retries for 408, 409, 429 and
  server failures before the detector sees the error. The detector retries
  malformed text immediately and rate limits with 1/2/4 s backoff. A missing
  provider switches once per detector and resets its model to the other
  provider's default. The gateway's user-visible reason is preserved.
- A dropped or signalled cancellation owner cancels pending network/backoff
  futures. The 15 s default deadline covers provider attempts and backoff.
  Empty DeepSeek choices yield an empty result. Invalid provider envelopes
  fail with a native error. An empty or malformed Anthropic text response
  enters the parse-retry path. Malformed entries and invalid verse bounds
  are skipped. Valid empty arrays count as success.
- Model work takes place outside the synchronous local detector. On return,
  model suggestions share the local reading suppression and dedup cache.
  Non-citation claims require local quote evidence and paraphrases are
  dropped. A later local resolution makes an older model result review-only
  and drops its partial results. Model translations come from their own
  source text. Prompt 039 owns application wiring and 037 owns general quote
  evidence lookup. The model and citation replay commands are
  `cargo test -p kairo-scripture --test model_detector --test detector_replay -- --include-ignored --nocapture`.

## 2026-11-10 — 037 — Quotation worker and licensed index

Decision: `kairo-scripture::quote` runs FTS retrieval and scoring on a
dedicated `scripture-quotes` OS thread, which owns a read-only `bible.db`
connection and an in-memory SQLite index of licensed text. Commands match
Electron's worker: search, and versioned `cache-begin`, `cache-append` (with
an acknowledgement) and `cache-commit`.

1. A failed thread cannot be terminated mid-query as a Node worker can.
   The client drops its command channel, so the thread exits after its
   current command, and a new thread starts on the next request. Replies
   from the old thread are ignored by generation.
2. Failure listeners run before the in-flight searches are rejected, the
   order Electron's synchronous `emit` gives.
3. The worker connection sets `PRAGMA cache_size = -16000`, and the
   workspace builds the bundled SQLite with `SQLITE_DEFAULT_MEMSTATUS=0`
   (`.cargo/config.toml`). Both are better-sqlite3 build defaults. Measured,
   they bring quote search p50 below Electron's.
5. rusqlite is bumped from 0.37 to 0.41 in `kairo-store` and
   `kairo-scripture`, moving the bundled SQLite from 3.50.2 to 3.54.1
   (Electron's better-sqlite3 has 3.53.1). With it, quote search p95 is about
   9% below Electron's. The only code change: `usize` no longer implements
   `ToSql`, so one test fixture casts to `i64`. All workspace tests and the
   1,000-query bundled FTS oracle still match.
6. Startup wiring: create `QuoteCorpus` on AppCore's runtime, `open` it
   with the profile `bible.db`, `attach_api_cache` with the 034 cache (or
   `None`), and call `invalidate_quote_corpus` when local packs change
   (`ScriptureService::on_corpus_change`). The detector (039) uses
   `search_general_quote`, `is_general_quote_eligible` at publication and
   `on_general_quote_corpus_change` to retract cards.

Affects prompts: 039, 070.

## 040: Local lyrics library

- `LyricsService` uses the existing lyrics WAL schema and database workers.
  Reads use a single SQLite snapshot for ranked ids and their sections.
  A write replaces song metadata, section JSON and FTS rows in one transaction.
  Song files are mirrored after commit on the same writer. Workspace paths
  and the one-time export flag come from the shared settings store.
- Search retains Electron's FTS query sanitization, rank and 50-row limit,
  followed by the 50-row LIKE fallback over title, artist and serialized
  sections. The two lists are deduplicated before stable Unicode title
  sorting with ICU4X. Blank queries use the library's SQLite NOCASE ordering.
- A differential run exposed a rank tie at the FTS limit. Electron's libuv
  directory enumeration sorts filenames. The native sync now sorts the same
  way before inserting. For bulk import, one FTS deletion scan removes the
  replaced ids before inserts; repeated ids are still processed in order.
- File and text preview has no persistence. It uses the shared duplicate
  matcher. SongSelect imports strip BOM/CRLF, accept recognized colon
  metadata and recover titles from filenames. Text imports preserve orphan
  lines, markers and repeated choruses, then apply the shared slide breaks.
  Local OpenLyrics XML parses titles, authors, copyright, CCLI and verse order.
  Electron has no XML parser in this path, so local XML has independent tests.
- Empty libraries, missing ids and empty search return normal empty results.
  FTS syntax errors fall through to LIKE. Invalid section JSON uses the
  existing section codec. Missing or unreadable song files follow the
  workspace reader's skip behavior. External deletion removes the indexed
  song after the one-time export. Mirroring is best effort as in Electron.
  Malformed XML is rejected; empty text remains importable; duplicate CCLI
  returns the existing song. Licensed CCLI and URL requests retain Electron's
  explicit errors. Online provider preparation and review belong to 041.
- The clipboard and slide-push IPC actions depend on the native window and
  presentation runtime, so their inventory ownership moves to 080. The
  local service closes the library, preview and local import operations and
  the workspace song resync. No Electron service callback has a caller.

## 2026-11-10 — 038 — Audio capture and Deepgram transcription

Decision: capture moves into the core (`kairo-stt::capture`). The cpal callback only downmixes to mono and pushes into an `rtrb` SPSC ring: no allocation, locks or logging. A `kairo-audio-capture` thread drains the ring every 10 ms, resamples to 16 kHz with `rubato` 5 (`Fft`, 10 ms input chunks, FixedSync::Input), and passes the result through the worklet's chunker. The chunker emits 1600-sample PCM16 chunks and a level meter checked once per 128-sample render quantum at a 0.1 s audio clock, so levels keep Electron's ~10 Hz rate. `audio:pcmChunk` becomes an in-process `AudioRelay::feed`. The relay keeps the 5 s cap (`MAX_QUEUED_BYTES = 160000`) and drops the newest chunk when full.

Decision: device ids are cpal `DeviceId` strings (`coreaudio:<uid>` and so on), with a synthetic `default` entry labelled `System Default`. Electron stored only the opaque Chromium `deviceId`, a per-origin hash that cannot be mapped to a device. A saved value that equals the label of exactly one present device (the pre-renderer CoreAudio-label era) is migrated to that device's id with `migrate_saved_device_id`. Any other stale value falls back to the default without changing settings, as `resolveCaptureDeviceId` does. When the saved device is unavailable at start, capture falls back to the default and reports `FellBackToDefault` (Electron's `OverconstrainedError`/`NotFoundError` retry). Unplugging a device reports `Audio input disconnected`. A stream invalidation rebuilds the stream on the same device. On macOS cpal follows default-device changes itself (`DeviceChanged`); other hosts poll the default every 2 s and rebuild when it changes.

Decision: microphone permission uses `AVCaptureDevice` authorization. Not determined prompts once; denied and restricted return `false` without a prompt, as `getMediaAccessStatus`/`askForMediaAccess` did. The Info.plist purpose string stays "Kairo listens to the service audio so it can transcribe the sermon in real time." (`permission::MICROPHONE_PURPOSE`). Packaging (101) must set it. Other platforms report granted.

Decision: Deepgram runs as one tokio actor over `tokio-tungstenite` 0.30 with rustls and native roots. The query string is byte-identical to what `@deepgram/sdk` 5.3.0 sends, captured from the SDK against a local socket server: `model=nova-3&language=…&punctuate=true&smart_format=true&interim_results=true&utterance_end_ms=1500&vad_events=true&encoding=linear16&sample_rate=16000&channels=1&no_delay=true` plus one `keyterm=` per term, encoded as `encodeURIComponent` encodes it. Authentication is `Authorization: Bearer <grant token>`. The SDK's `x-fern-*`, user-agent and `x-deepgram-session-id` headers are not sent. Keepalive (`{"type":"KeepAlive"}` every 10 s), `CloseStream` on close, the 30 s replay buffer that drops the oldest audio, connection generation as audio epoch, grant renewal (retry 5, 15, 30 s; one fresh session after a refusal; stop on a second refusal), release before each new grant, the session-limit wait (30 s, 8 tries) and reconnect backoff (1 to 30 s, 20 tries) follow `deepgram.ts`.

Decision: one addition, a half-open check. Each keepalive also sends a WebSocket ping. If no frame (pong, result or other) arrives for 30 s, the socket is treated as closed and reconnects. Electron had no detection beyond a failed send. `SttEvent::Error` carries `fatal`: true when transcription has stopped (refusal, reconnects exhausted), false for transient socket errors. Electron used one `error` event for both.

Decision: `KAIRO_STT_REPLAY_PATH` recording lives in `kairo-stt::recording` and is opened by `SttService`. The file is exclusive, mode 0600, written on its own thread, and marks dropped runs with a `gap` line that replay refuses. `replay::replay_pcm` is the core of `replay-audio.ts`: real-time pacing in 20 ms frames, then 150 silence frames. Running it as a command line needs the app's signed-in session, which bootstrap wiring (067) provides.

Affects prompts: 039, 067, 077, 101.

Two reviewed filesystem cases intentionally differ from Electron. If legacy
export fails, the flag remains unset and the next sync retries the remaining
songs. File replacement and deletion follow the same embedded-id rule used
for folder import, including alternate filenames, so the next sync cannot
restore a song solely because its file was renamed.

## 2026-11-10 — 039 — Orchestrator, voice navigation and recovery

Decision: the orchestrator lives in a new crate, `kairo-orchestrator`, not in `kairo-core` as the blueprint drew it. `kairo-core` cannot depend on `kairo-stt` or `kairo-scripture`, because both reach `kairo-core` through `kairo-cloud`. The crate sits above the service crates, and the UI composition root (067) wires it.

Decision: the orchestrator is one actor. Every public call holds the state lock for its whole run, so calls are handled one at a time in arrival order. Auto-present countdowns are timer tasks that re-enter through the same lock. A countdown that was cancelled, dismissed or superseded before it fires finds its entry gone and does nothing. Voice navigation runs its own serialized worker (as `ScriptureVoiceNavigation`'s promise queue did) and presents through the actor.

Decision: everything the orchestrator drives sits behind traits in `clients.rs`. `EngineClient` (surfaces: `show_slide`, `set_background`, `swap_document_page`, `show_document`, `clear_text`, `clear`, `live_targets`, `all_surfaces`), `ProgramClient`, `MediaClient`, `ScriptureClient`, `SttClient`, `SessionGate`, `SettingsClient` and `UsageClient`. Test fakes live only under `tests/`. Surface ids are output ids. Phase 4 implements `EngineClient` over the engine protocol, and 065 and 067 implement the others.

Decision: 039 composes the detector, as 036 left it. `detector::LiveDetector` runs the deterministic `LiveCitationDetector` on each transcript in arrival order, with local chapter and quote evidence and the live plan index. Unresolved transcripts go to the `ModelQueue` and `ManagedModelDetector` (production transport: `kairo-cloud::scripture_gateway`). In fallback mode they go to the regex instead. A 50 ms ticker fires due chapter fallbacks and dispatches model work. Events mirror the `ScriptureDetector` facade: detection, quoteRetracted, planProgress, stats, error, requestSuccess. General-corpus quotes (`suggestionOnly`) are not produced by this composition yet, so `is_general_quote_current` treats every emitted reference as current. Wiring 037's global quote search into the facade is left for the integration prompt (067). `kairo_scripture::citation::canonical_book` became `pub` so references can be released and set as the reading passage. It expects lowercase input.

Decision: scripture traces (`scriptureTrace` marks) are not recorded. They are diagnostics with no effect on what reaches a screen, and the trace sink belongs to the integration. Latency was measured with fake-engine timestamps instead.

Decision: `start_from_ui` is `orchestrator:start`. It asserts the transcription licence, refuses with "Create a service in Operator before starting transcription." without an active service, and replaces the caller's credentials with `kairo-managed`, with provider Deepgram. `restore_session` needs an active service too, validates each part of the snapshot on its own, and wires the detector the same way as a cold start. The snapshot keeps the Electron keys and order (`session`, `pendingSuggestions`, `transcriptHistory`, `config`), and `spawn_monitoring` saves it every 30 s through `kairo-core::resilience::RecoveryFile`. The same task polls connectivity every 10 s, and going offline switches the detector to the local fallback. Two detector failures in a row start the fallback through `Resilience::detector_error`, and a probe 30 s later lets the model try again.

Decision: the end-to-end oracle (`tools/ts-oracle/modules/orchestrator`) runs Electron's real `orchestrator.ts`, detector, live wiring, voice navigation and dispatch under Electron-as-Node. A `Module._resolveFilename` preload swaps every service singleton for a recording stub. Mocked timers give it a virtual clock.

Decision: the Bible database handle the Electron orchestrator opened and never read is dropped; the scripture service owns the database. Sermon-plan reference extraction and document reading stay with 042. 039 ports the plan store and `LivePlanService`.

Affects prompts: 042, 050, 065, 067, 077.

## 2026-11-10 — 041 — Online lyric search and translation

Decision: `kairo-lyrics::online` ports the online tier. HTML is parsed with `scraper` 0.24 in place of `cheerio`. Cheerio's `remove()` is reproduced by skipping the removed elements and their subtrees when selecting and reading text. `<br>` reads as a newline and `data-exclude-from-selection` nodes are skipped. HTTP goes through an `Http` trait (production: `reqwest` 0.12 with rustls and native roots). Managed calls go through a `ManagedGateway` trait, which `kairo-cloud` implements for `LicensingService` (`lyrics_gateway.rs`). Web search (DeepSeek, then Brave), page review (DeepSeek) and translation (Anthropic or DeepSeek, otherwise Google Translate) all use that gateway, and caller keys are ignored as in Electron.

Decision: user agents, timeouts, caches and sources are unchanged:
- LRCLIB identifies as Kairo. Genius, the lyric sites and the search engines get the stock Chrome 122 agent and browser headers.
- The allowlist and the Blogger hymn hosts are the same; no source was added.
- Genius requests stay 350 ms apart, the keyless engines 1.1 s, and managed search reserves slots (DeepSeek 250 ms, Brave 1.1 s).
- One difference: Genius and the snippet resolver now reserve slots too. Electron's last-request check let concurrent callers wake together, so concurrent calls are now spaced, never faster than Electron.
- The fan-out matches `Promise.allSettled`: every query variant is sent to Genius and LRCLIB, plus structured LRCLIB calls, at once. Candidate pages are capped at 2 Genius and 3 scraped pages per batch.

Decision: fixtures under `crates/kairo-lyrics/tests/fixtures/online` hold only public-domain lyrics. DuckDuckGo, Mojeek, Brave and the Yoruba Hymns page were captured live with the code's own headers. The LRCLIB search response keeps its real structure and metadata, but its lyric bodies are replaced with public-domain "Amazing Grace" text so no copyrighted lyrics are committed. Genius returned 403 to both curl and Electron's own axios code on 2026-11-10 (the "rate limiting" message an operator would see), so `genius-medley.html` is a Genius-shaped page written for the test. The oracle (`tools/ts-oracle/modules/lyrics-online.ts`) runs Electron's code over the same bytes through a stubbed axios.

Decision: three parts of `index.ts` land here, assigned by 040: `searchOnline` (marks songs the library already holds and warms the first two previews), `previewOnline`, and the online branch of `importSong`. They are `LyricsService::search_online`, `preview_online` and `import_online`, taking the `OnlineLyrics` service. `import_song` still refuses an online source, so callers use `import_online`. Online preview shows the import slide breaks, because the native text parser applies them while parsing (040). Electron previewed without them and applied them only on import.

Decision: `reviewOnlineLyrics` and `OnlineLyricsPreparation` are ported as written. The model only selects source line ids and section starts. Any malformed reply leaves the text unreviewed. Snapshots are kept ten minutes, at most 50, keyed with a SHA-256 of the credential. A failed preparation is forgotten.

Affects prompts: 067, 080, 084.
## 2026-11-10 — 042 — Sermon notes import

Decision: `kairo-scripture::sermon_notes` ports `analyzeScriptureReferences`, `readSermonDocument`, the `sermon-notes-review.ts` text helpers (`plainTextToEditorHtml`, `escapeHtml`, `normalizedReferenceLabel`; `mapTextRangeToPos` works on the TipTap document and stays with 079) and the `importSermonNotes`/`scanSermonNotes` handlers. The reference pattern is built from the same tables: `BOOKS` from `bible-db.ts` (now exported as `kairo_store::db::BOOKS_JSON`) and `build_translation_alias_map`. JavaScript semantics are kept: ASCII `\b` and `\d`, JavaScript's `\s` set, and the `i` flag as `[xX]` classes so only ASCII letters fold. The continuation pattern's sticky match and lookahead use `fancy-regex` anchored at the cursor. Match offsets are UTF-16 code units.

Decision: `.docx` is read with `zip` 2 and `roxmltree` 0.21 in place of `mammoth`. Text follows `extractRawText` (each paragraph followed by a blank line, tabs and line breaks kept, deletions dropped). HTML follows `convertToHtml` with the default style map: `h1`–`h6` for Heading 1–6, nested `ul`/`ol` from `numbering.xml`, `strong`, `em`, `s`, `sup`, `sub`, links, tables and `<br />`, empty paragraphs omitted, adjacent runs merged. Parts over 256 MB are refused. Other mammoth style mappings (underline, small caps, footnotes, comments, images) are not reproduced; the corpus HTML matches mammoth byte for byte.

Decision: PDF text uses `pdfium-render` 0.9.5 (`pdfium_latest`, `thread_safe`, dynamic binding), loaded once per process from `set_pdfium_library_dir`, then `PDFIUM_DYNAMIC_LIB_PATH`, then the executable's directory and `../Frameworks`, then the system path. A missing library is a `PdfiumUnavailable` error, not a panic. Pages are joined as `pdf-parse` joins them (`"\n\n"` before each page), and PDFium's CRLF line ends become LF. Reading reports per-page progress. Shipping the library is packaging's job (101–103). For development the prebuilt macOS arm64 build (bblanchon/pdfium-binaries latest release, VERSION 159.0.8138.0) sits in the git-ignored `.cache/pdfium`.

Decision: error messages. The unsupported-extension message is Electron's exactly. Read failures name the file and the cause in the app's own words ("Could not read the Word document: ...", "Could not read the PDF: the document is password protected"), where Electron surfaced mammoth's or pdf.js's internal text. Text files decode as Node's `utf8` does: invalid bytes become U+FFFD and a BOM is kept.

Decision: importing runs on `spawn_blocking` with `ImportProgress` (reading pages, then scanning), so a large document never holds an actor. The open dialog belongs to the shell (067): it filters on `ACCEPTED_EXTENSIONS` and passes `None` on cancel. The default translation is passed in from `settings.scripture.defaultTranslation`.

Decision: list, save and delete go through `LivePlanService` (`list_plans`, `save_plan` then `refresh`, `delete_plan` then `handle_deleted`), so the single store instance the live index reads is the one written, as the IPC handlers did. `PlanSource` gained `list`, `save` and `delete`. The JSON store edits the raw `plans` array, so entries this build cannot parse are kept.

Decision: the oracle (`tools/ts-oracle/modules/sermon-notes.ts`) runs Electron's real `sermon-plans.ts` in Electron as Node through tsx with `electron` and `electron-log` stubbed. `Date.now` is pinned only around the scan. Under plain Node 24 `pdf-parse` (pdf.js 1.10) rejected every PDF with "bad XRef entry". Under Electron's Node it also rejects PDFs of about 1 KB, so fixture PDFs are padded past 4 KB with a comment.

Affects prompts: 067, 079, 101, 102, 103.


## 2026-11-10 — 044 — Setlist, libraries, passages, usage and transfer

Decision: the services live in `kairo-transfer` and write the same electron-store files as Electron (`kairo-setlist.json`, `kairo-libraries.json`, `kairo-passages.json`, `song-usage.json`) through `kairo_store::settings::open_store`. Every read normalises, as the TypeScript does, so a `select` of an unknown list returns `activeId: null` and the next read falls back to the first list.

Decision: the `.kairo` round trip is checked against Electron's real `src/lib/kairo-bundle.ts` (`tools/ts-oracle/modules/kairo-bundle-roundtrip.ts`). Songs and setlist files re-export byte for byte. A scripture playlist does not, in Electron either: export writes the plan as stored, but reading drops fields the reader does not know (`sourceText`, a verse's `translation`) and replaces an empty item's `error` with "No verse text in the imported file". The tests therefore compare Rust's re-export with Electron's re-export, not with the file.

Decision: the review keeps the parsed bundle in the service under a token; `commit` takes decisions only and reports `CommitProgress` per song. The dialogs are a `FileDialogs` trait owned by the shell. Files opened before the review screen is ready queue in `KairoOpenQueue` until `ready`; a file that fails to read is delivered as `{ error, fileName }`.

Decision: release-mode timing needs non-local build URLs (prompt 024 decision 7). The 500-song measurement ran with `KAIRO_API_URL=https://api.example.com KAIRO_WEB_URL=https://example.com`.

Affects prompts: 045, 067, 091.


## 2026-11-10 — 045 — Document import pipeline

Decision: `kairo-documents` renders pages itself. `DocumentsService::render` runs PDFium (`pdfium-render` 0.9.5, dynamic binding found as in 042: an explicit directory, `PDFIUM_DYNAMIC_LIB_PATH`, the executable's `../Frameworks` and folder, then the system) at Electron's rule: scale `min(3840 / w, 2160 / h)`, the canvas rounded up, a white background, PNG. It replaces the window's pdf.js loop of `documents:savePage` calls. `save_page` stays with Electron's checks (next page only, at most 40 MB, at most 3840 px a side) for a caller that rasterises elsewhere. PDFium calls are serialised by the binding, so pages render one at a time and PNG encoding runs on a pool of up to 4 threads. A cancel stops before the next page.

Decision: the PPTX parser is a regex-for-regex port of `pptx-slides.ts`. `roxmltree` replaces cheerio where the TypeScript parses XML (relationships, slide order, videos), `zip` replaces jszip, and `fancy-regex` covers the one back-reference. A lookahead `<a:t(?=\s|>)` becomes `<a:t(?:\s[^>]*)?>`. The generated `slides.html` matches Electron byte for byte over a 9-deck corpus. The corpus covers text runs, a table, groups, a cropped picture, a line, a picture background, an inherited master and layout with a second theme and a `clrMap`, 4:3 with `showMasterSp="0"`, and three broken packages. The comparison runs through the oracle, which runs Electron's real `convertPptxBuiltin` up to its BrowserWindow (`tools/ts-oracle/modules/pptx-html.ts`).

Decision: on macOS the HTML is printed by an offscreen `WKWebView` called through `objc2-web-kit` (`webkit::print_slides`), not through Wry. Wry 0.53 has no PDF call and needs a parent window, which a service does not have. Both drive the same WKWebView. The page loads with read access to the work folder, then the code waits for `readyState`, `document.fonts` and every image (prompt 008 amendment). At `pageZoom` 0.75 the 1920 px slides become 1440×810 pt. `createPDF` captures each slide's rect, and PDFium appends the captures into one PDF. PDFium's `FPDF_ImportNPagesToOne` with a 1×1 grid copies pages unscaled, so zoom does the scaling. WebKit needs the main thread: `WebKitPrinter` takes a dispatcher that runs the job there, and the job runs the run loop until it finishes or 120 s pass (`CONVERT_MS`).

Decision (amended 2026-11-13): Windows and Linux printers are written. Windows: `webview2::WebView2Printer` (`webview2-com` 0.38, `windows` 0.61, the versions Wry pins). Each print runs on its own STA thread with a hidden `STATIC` popup window, an environment whose profile lives in the work folder, and a controller sized to the slide. It waits for `READY_SCRIPT` plus `location.protocol === 'file:'`, so the initial blank page never counts as ready. Then `ICoreWebView2_7::PrintToPdf` runs with the page in inches (`page_width_pt / 72`, 20 in across), portrait (width is already the long side, as Electron's explicit `pageSize` does), scale 1, zero margins, backgrounds on, no header or footer. CSS px are 1/96 in, so 1920 px fills 20 in and no zoom is needed. `webview2::runtime_version()` uses `GetAvailableCoreWebView2BrowserVersionString`. Without a runtime, the print fails with "Microsoft Edge WebView2 Runtime is not installed." Linux: `webkitgtk::WebKitGtkPrinter` (`webkit2gtk` 2.0.1 with `v2_38`, as Wry) takes a dispatcher to the GTK thread, as macOS does. It loads into a `gtk::OffscreenWindow` with print backgrounds on, waits with `run_javascript` (the 2.38 call; `evaluate_javascript` needs 2.40), then runs `WebKitPrintOperation::print` to `Print to File` PDF at a custom paper size in points with zero margins. Each printer writes `slides.pdf` in the work folder, reads it and removes it. Checked: Linux and Windows-GNU Clippy `-D warnings` in Docker (Debian bookworm, `libwebkit2gtk-4.1-dev`, `gcc-mingw-w64`); the Linux `webkit_print` test ran under Xvfb on aarch64 with PDFium chromium/8138 (3 of 3 runs: 3 pages at 1440×810 pt, picture and fill colours right, 185 to 233 ms). Not run: Windows (no Windows machine; the MSVC target was not checked either, because `ring` needs the MSVC headers).

Decision: converter detection and invocation follow `converters.ts` and `office-apps.ts` through an `OfficeProbes` trait (directory listing, files, `mdfind`, `which`/`where`, environment). Tests therefore never depend on installed apps. The AppleScript and PowerShell text is Electron's. Paths reach PowerShell only as `KAIRO_PPT_SOURCE_B64` and `KAIRO_PPT_DEST_B64`. Child processes have Electron's time limits, and output past 1 MB is dropped where `maxBuffer` would fail.

Decision: Electron behaviour kept as-is, though it may look wrong: resuming a `.ppt` with `simplified` fails with the message `simplified`, because the built-in renderer reads `.pptx` only. `slideSize` is in EMU, from `readPptxVideos`. A document's pages are listed as `<dir>/<i>.png` paths. The `pa-media` allowlist (`allowPickedOverlayMedia`) has no native counterpart and is dropped. Removing a document from libraries (`librariesService.forgetItem`) stays in the IPC handler, so the shell (067) calls `kairo_transfer::remove_document_from_libraries`.

Decision: the slide converter keeps the pinned LibreOffice 26.2.7 URLs, checksums and sizes. It downloads with reqwest, checking SHA-256 while streaming, and unpacks with `hdiutil` and `ditto` or `msiexec /a`. Concurrent installs share one run through an async mutex, and status changes go out on a broadcast channel (`documents:converter:changed`).

Decision: the page comparison renders the corpus with the app's pdfjs-dist 4.10.38 under Node on `@napi-rs/canvas` (`tools/ts-oracle/modules/pdf-pages.ts`), not in Chromium's canvas as the app does. Fonts are embedded (Liberation Sans, OFL, `tests/fixtures/fonts` with its licence), so neither renderer substitutes a font. The test fails below a mean 8×8-block SSIM of 0.95.

Affects prompts: 058, 067, 085.

## 2026-11-10 — 046 — Audience rooms and Engage

Decision: `kairo-audience` holds `AudienceService` (queue and on-screen state), `KairoAudienceSource` (operator WebSocket on `tokio-tungstenite` 0.30), `SampleAudienceSource` and `EngageFlows` (list, create, resume with optional reopen, reopen, delete, invite, revoke, `audience:start`). Slido is 047; `audience:start` with `slido` reports "That audience source is not available." until then. Opening the join page in the browser (`shell.openExternal`) is the shell's; the service gives only the running room's URL.

Decision: the source keeps Electron's rules: reconnect after `min(30 s, 1 s × 2^min(attempts, 5))` plus up to 500 ms, odd attempts on the fallback base; a 15 s ping, terminate when the previous ping got no pong; 10 s handshake timeout; message and frame cap `AUDIENCE_MAX_SNAPSHOT_BYTES`; 404/410 ends the session; 401/403 renews once, then asks to reopen; 8 s ack timeouts for commands, settings and end; moderation queued (latest per question) and resent after each snapshot. Incoming messages are parsed loosely from JSON on the socket task, field by field, as the TypeScript reads them: the strict `AudienceServerEvent` would reject the snapshots the recovery test and older rooms send. A snapshot without valid `settings` leaves the session's settings alone; Electron would have set them to `undefined`.

Decision: output calls (show or clear the question and poll layers) run after the service lock is released, because the program service reports a clear back through `sync_cleared`. One change notification goes out per operation; Electron sometimes sent two or three in a row for one update (clear, clear poll, then the update), and `detachEngagement` sent its last one before the final changes. Listeners now see the final state once.

Decision: QR codes reproduce `qrcode` 1.5.4 exactly, not only in what a phone scans. The `qrcode` crate 0.14 picks different segments and masks, so the encoder ports node-qrcode's segment graph (`buildGraph` with its `lastCount` quirk), `dijkstrajs` (a stably sorted queue, no visited set), version choice and mask penalties (N1 to N4), and uses the crate only for bit packing, error correction and drawing. Pixels follow `qrToImageData` (384 px, margin 2, level M). Checked against Electron's module grid and decoded PNG pixels for 50 URLs (`tools/ts-oracle/modules/audience-qr.ts`). PNG bytes differ (different compressor); pixels are identical.

Decision: HTTP failures read as Node's `fetch` did: "The operation was aborted due to timeout" for the 90 s timeout, "fetch failed" otherwise. A failed `open` during resume reports the room's text, even when empty, as Electron did.

Decision: the live test runs the Electron checkout's real `audience-routes.ts` and `AudienceRoom` Durable Object in Miniflare on a local port (`tools/audience-worker/serve.mjs`), with account checks and the engagement index answered locally as `scripts/test-engage-room.mjs` does. It is `#[ignore]` because it needs Node and the checkout's `server/node_modules`.

Affects prompts: 047, 054, 067, 090.

## 2026-11-10 — 044 — Setlist, libraries, passages, usage and transfer

Decision: `kairo-transfer` holds `SetlistService`, `LibrariesService`, `PassagesService`, `SongUsageService`, `TransferService` and `KairoOpenQueue`. Each writes the same electron-store file as Electron (`kairo-setlist`, `kairo-libraries`, `kairo-passages`, `song-usage`) through `kairo_store::settings::open_store`, normalises on every read with the 015 domain functions, and replaces the whole store on write as `db.store = state` did. `on_changed` listeners stand in for the `*:changed` broadcasts. Time comes from an injected clock; ids are UUID v4.

Decision: dialogs belong to the shell. `TransferService::export` and `pick_and_preview` take a `FileDialogs` implementation (save `.kairo`; open several `.kairo`/`.json`). `SongUsageService::export_csv` takes the chosen path, or `None` when cancelled, and a `chrono::TimeZone` from the caller. `song_usage_file_name` gives Electron's UTC-dated default name. Export builds the bundle, and raises its errors, before the dialog opens, as Electron does.

Decision: the scripture playlist side of transfer goes through `LivePlanService` (new `plan(id)`; `save_plan` already refreshes the live index), so import writes the one store the live index reads.

Decision: the delete clean-up is `delete_song` (lyrics delete, then setlist and song-library clean-up, skipped only if the delete itself errors) and `remove_document_from_libraries`, which the documents service (045) calls on remove. The `.kairo` open queue holds files until `ready` and delivers each as a preview or `{ error, fileName }`; `kairo_paths_in` filters argv as Electron does. Wiring `open-file`, second-instance argv and the window to the queue is 067's.

Decision: a missing picked file reports Node's `ENOENT: no such file or directory, stat '<path>'`, since Electron stats before reading. Bundle text decodes as Node's `utf8` (invalid bytes become U+FFFD).

Decision: gates ran excluding `kairo-stt`, `kairo-audience`, `ui-web` and `kairo-updater`, which other sessions (047, 048 and an in-progress `kairo-stt` edit) were changing and which failed fmt, Clippy or compilation at the time.

Affects prompts: 045, 067, 091.

## 2026-11-10 — 048 — Auto-update

Decision: `kairo-updater` replaces electron-updater with one path on every platform: read the electron-builder feed, pick this machine's file, download through `reqwest` with `system-proxy` (the OS proxy, as `electron.net`), hash SHA-512 while streaming, compare with the feed's base64 value, and only then stage. Nothing downloads until the operator asks; the first check runs at start.

Decision: the feed is `https://github.com/CodeLawd/kairo-presenter/releases/latest/download/<file>`, where `<file>` is `latest-mac.yml`, `latest.yml`, `latest-linux.yml` or, on ARM Linux, `latest-linux-arm64.yml`. GitHub's `latest` skips drafts and pre-releases, which is what `releaseType: release` asked of electron-updater. An update is offered only when its semver is greater than the running version. Release notes come from the release's Atom entry (`releases.atom`, entry linking to `tag/v<version>`), as electron-updater read them; a missing feed of notes is not an error. There is no YAML crate in the workspace, so a strict reader handles the subset electron-builder writes (top-level scalars, `files` list, quoted scalars, `releaseNotes` block); anything else is refused. `stagingPercentage` and `minimumSystemVersion` are not acted on; Kairo's releases do not set them.

Decision: macOS always uses the self-update path from `mac-self-update.ts` (zip, `ditto`, bundle id match, `codesign --verify --deep --strict`, quarantine removed, detached swap script byte for byte). For a Developer ID signed build, the native equivalent of Squirrel.Mac's check is added: the new bundle must also pass `codesign --verify -R=<designated requirement of the running app>`, read with `codesign -d -r-`. An ad-hoc or unsigned build checks bundle id and seal only, as today.

Decision: Windows runs the NSIS installer with electron-updater's arguments: `--updated --force-run` for an explicit install, `--updated /S` when installing on an ordinary quit. Linux AppImage stages the file and, after quit, a detached script copies it beside `$APPIMAGE`, marks it executable and renames it over. A Linux run without `$APPIMAGE` is a package install: it still hears about updates, and Download tells the operator to get the new version from the website.

Decision: install is safer than Electron's in two places. With live output running it returns `NeedsConfirmation` until the operator confirms (Electron's before-install hook allowed the quit without asking). Replaceability is checked before the before-install hook runs, so hardware is not released for an install that cannot happen. A check while downloading or downloaded leaves the status alone. Stale `kairo-update-*` folders from an interrupted run are removed at start.

Decision: progress is reported once per whole percent, capped at 99 until the checksum passes, as the self-update path did. Network failures are worded so `updateErrorMessage` recognises them ("network error", "timed out"); a transfer that ends short of the feed's `size` counts as a network failure.

Affects prompts: 067, 101, 102, 103, 105.

## 2026-11-10 — 047 — Slido source

Decision: `gpui-kit` moves from `=0.9.0` to `=0.9.1`, the version `gpui-webview` 0.9.1 needs (owner's choice; see the web entry of 2026-11-09). `ui-web` depends on `gpui-webview =0.9.1` and `lb-wry =0.53.3`.

Decision: the source logic (`kairo-audience`, `SlidoAudienceSource`) is free of any window toolkit and talks to a `SlidoWindowFactory`; `ui-web`'s `SlidoWebHost` implements it with a separate, initially hidden GPUI window holding one `gpui-webview`. Wry views are not `Send`, so the host owns them on the GPUI thread behind a command channel. `audience:start` with `slido` works once the shell gives `EngageFlows::with_slido` a host; without one it reports "That audience source is not available."

Decision: `slido-selectors.ts` is compiled unchanged with esbuild into `crates/kairo-audience/src/slido/selectors.js` (`tools/slido-page/build.sh`; a rebuild is byte-identical). The preload becomes `bridge.js`, with `ipcRenderer.send` replaced by Wry's `window.ipc.postMessage`; both run as one init script. Wry's init script runs in the page's world, not an isolated one, so the page can forge messages: the source accepts a message only from a frame with the window's current origin, parses it as untrusted JSON, and applies Electron's checks (host, path, types, 5,000 questions, 200-character ids, safe-integer votes, normalised text, 80-character authors).

Decision: what Wry lacks and how each gap is covered.
- No in-page navigation event: the bridge reports a changed `location.href`, and the host copies the live URL every 250 ms.
- No failed-load event: a page that never finishes loading within 30 s shows "Couldn’t reach Slido. Check your internet connection." No render-process-gone event: the watchdog's "Questions stopped coming in from Slido." covers a dead page, and "The Slido page stopped. Disconnect and connect again." is not shown.
- The navigation handler also sees frames, so `about:blank` and `about:srcdoc` frames are allowed besides the Slido allowlist. New windows: moderator only, allowlisted addresses only; participants get none. Downloads are refused.
- Persisted profile: a `WebContext` data directory per profile, plus on macOS 14 and later a `WKWebsiteDataStore` identifier derived from the profile path. A cookie survived a second process in the harness.
- Hide on close: `on_window_should_close` returns false and hides. macOS hides with `orderOut:`; Windows and Linux minimise, because GPUI has no hide.

Decision: permissions are denied in two layers. A main-frame init script makes `getUserMedia`, `getDisplayMedia`, geolocation and notification permission fail in the page. On macOS, the web view's UI delegate (Wry's, which grants camera and microphone) is moved to a subclass that answers deny, leaving other web views alone. Geolocation on macOS also needs app location access, which Kairo never requests. Windows (WebView2 `PermissionRequested`) and Linux (WebKitGTK `permission-request`) native denial are not written; only the script layer applies there.

Affects prompts: 067, 089, 090.

## 2026-11-10 — 050 — Engine process, protocol and supervision

Decision: the wire format is postcard 1.1 behind a little-endian `u32` length prefix, at most 16 MiB a frame. `PROTOCOL_VERSION` is 1. The first message is `Hello { protocol, token }`; the engine answers `Welcome` or `Rejected { reason }` and closes. A version mismatch is logged on both sides as "protocol version mismatch: client N, engine M". The token travels in the `KAIRO_ENGINE_TOKEN` environment variable, which the engine removes before starting any thread, because a command-line argument is visible to other users' `ps`. The socket name is `kairo-engine-<uuid v4>.sock` per launch. A client must say hello within 2 s; each connection has its own thread, and a newer authenticated connection replaces the older one, whose later messages are dropped.

Decision: domain values whose serde shape uses `flatten` or `untagged` (`OverlayTheme`, `OverlayBackground`, `DocumentSlide`, `MediaPlayback`, `VideoSettings`, `ConfidenceInfo`, `DocumentVideoCommand`) cannot be encoded by postcard, so they cross as JSON text (`kairo_engine_proto::Json`) inside the binary frame. The engine does not depend on `kairo-domain`; the scene builder (052 onward) parses them. Everything else is typed.

Decision: the protocol crate holds `SurfaceState`, a port of `ProgramSurface`'s bookkeeping (`lastRender`, `lastOutputId`, document mode and slide, layers, config, test pattern, parking) with one `apply` function. The client applies each command to its copy first and returns `ProgramSurface`'s booleans from it at once; a refused command is not sent, as Electron returned early. The engine applies the same function to its copy. `replay()` produces the commands that rebuild a state on a fresh engine, in `recover`/`ready` order (config, layers, test pattern, slide instant, document, then park). A relaunch sends `RetainSurfaces` and every surface's replay under the same lock that orders live commands. Theme JSON is edited as `serde_json::Value`, so background comparisons in `clearText` ignore key order where `JSON.stringify` did not; the only effect is skipping a repaint whose background is equal.

Decision: `EngineClient` is implemented as `kairo_orchestrator::engine::EngineSurfaces` over `kairo_core::engine::EngineHandle`. The prompt asked for the client in `kairo-core`; the trait lives in `kairo-orchestrator`, which depends on `kairo-core`, so `kairo-core` holds the client and supervisor and the trait adapter sits next to the trait. Outputs come in through `set_outputs`, for prompt 065 to call.

Decision: supervision. Launch, connect within 5 s, handshake, replay. A heartbeat goes every 1 s and is answered from the engine's winit loop, not the socket thread, so a stuck loop counts as a hang. No answer for 3 s, an exit, or a lost connection is a failure: the engine is killed and relaunched. Failures use `allowFailure`'s budget (3 per rolling 60 s), counted only for unplanned exits; spawn failures count. When the budget is spent the status is `Failed`, every surface reports "Output engine stopped" and pushes return false. `restart()` is planned and free. `shutdown()` sends `Shutdown`, waits 2 s, then kills. The engine's stderr is forwarded into Kairo's log at the same level under target `kairo_engine`.

Decision: UI-death grace. The engine counts from start-up and from each disconnect; with no client for `--grace-ms` (default 10 s, configurable through `EngineOptions::grace`) it exits. A client that reconnects with the token inside the grace cancels it. A new `kairo` launches its own engine, so an orphaned engine from a crashed UI can show its last picture alongside the new engine's for up to the grace period.

Decision: frames are drawn from `about_to_wait` when a surface's content changes, not from `RedrawRequested`, because macOS withholds redraws from occluded windows. A frame that cannot be presented (occluded, timeout) is retried every 100 ms and counted as dropped. `FrameCommitted { output, seq }` reports the newest command sequence on that surface once its frame is presented. Offscreen sinks (NDI, Syphon, card) report `Unavailable` ("… is not available in this engine build") until 061 to 063; pushes to them return false, as Electron's did while a sender was unavailable.

Decision: until 051 every frame is cleared to opaque black. `VideoEnded` answers true (no video plays yet, 055) and `DocumentPlayback` answers none (058). `SwapPage` is decided by the shared reducer; when 058 adds page decoding that can fail, it becomes a request with a reply.

Decision: display handling carried from spike 002: `resolveDisplay` (id, then a unique label and size, never another screen, never a claimed one), the rehearsal half-width centred window, accessory activation policy, no default menu, the Objective-C replacement of `canBecomeKeyWindow` and `canBecomeMainWindow` (still needed in winit 0.30.13), screen-saver level, all Spaces, hidden cursor. Displays are polled every 250 ms, and an output whose display goes is closed in the same pass. The spike's unplug finding (macOS moves the window before Kairo reacts) is not fixed here: reacting to `NSApplicationDidChangeScreenParametersNotification` and the 250 ms debounce belong with display sinks, prompt 060.

Decision: the frame ring (`kairo_engine_proto::ring`) is POSIX shared memory with `ring.h`'s exact layout, checked by compile-time offset asserts and by a test that compiles a reader against the Electron checkout's `ring.h` and reads frames this writer published. The writer never picks the newest complete slot once anything is published, tracks in-flight slots locally, drops a frame when no slot is free, and returns an abandoned write's sequence to even without publishing. Windows has no ring yet; 062 decides it with the helper.

Affects prompts: 051, 052, 055, 058, 060, 061, 062, 065, 066, 067.

## 2026-11-10 — 051 — Compositor core and layer stack

Decision: colour. Frames are `Rgba8Unorm`, premultiplied, and blended as stored with `One, OneMinusSrcAlpha`: sRGB-encoded values are not linearised, because Chromium's compositor blends them that way. Display swapchains use a non-sRGB format (`Bgra8Unorm` on macOS) so encoded values are not encoded twice. On screen, `#C81E1E` and `#1E3CC8` backgrounds read back from a screenshot as (200,30,30) and (30,60,200) exactly. There is one shader and one pipeline per target format: textured or solid quads with rounded-corner coverage, plus an analytic Gaussian box shadow (sigma = blur / 2) for the pip camera's `0 16px 48px rgba(0,0,0,0.5)`.

Decision: the layer stack (`kairo_compositor::layers`) holds the 14 slots of `#pa-program` bottom to top: background (in `#pa-content-bg`), camera, then `#pa-content-fore`'s web, web pointer, elements below, text, elements above, props, timer, message, poll, question, info, logo. `swap` is `swapLayer`; `set_keyed` is `setBackground`/`setElements` (same key keeps the node and takes the new content without a fade). A retired node fades linearly from its current opacity over `ms`, is removed 50 ms after (`ms + 50`), or at once when `ms` is 0, and is never current again. New nodes are drawn above old ones. Each node is one quad, so CSS group opacity equals per-quad opacity; a live source's black backdrop is drawn only until its first frame, so the two never stack under one fade.

Decision: `videoLayout`, `applyCameraLayout` and `applyCameraNodeStyle` are ported with JavaScript `Number(x) || d` coercion, including the second normalisation `applyCameraLayout` applies to the already normalised `config.video` (a stored box at `xPct: 0` moves to the legacy corner). A truthy `corner` without `indexOf` makes `videoLayout` throw, and the whole `__configure` is then ignored, as in Electron. A differential test runs the three functions extracted from `overlay.html` in Node over 608 generated layouts at frame heights 1080 and 1440; Rust matches every case. The answers are cached in `crates/kairo-compositor/tests/oracle/camera-layout.json` for CI and compared live when the Electron checkout and `node` are present.

Decision: threading. `Compositor::start` opens the adapter and device, then a `compositor` thread owns them and every output's scene. The winit thread creates window surfaces (AppKit needs that) and hands them over; the compositor thread configures and presents them. The thread blocks on its mailbox when nothing is dirty or fading, so a still program draws nothing. Fades redraw each frame, paced by vsync on windows and every 16.7 ms offscreen. `Committed { output, seq }` is sent after submit, present and a blocking device poll, so the GPU work is done. A window frame that cannot be presented is retried after 100 ms. A lost device makes the engine exit with status 75, so the supervisor relaunches it and replays every surface. The compositor starts on the first display window, so an engine without windows needs no GPU.

Decision: the engine's `program` module is the `overlay.html` shell API at layer level: `__setContent` (background keyed by `TAG:file:cue` for media, else the background JSON, standing in for `outerHTML`; elements; words), `__clearText`, `__markDocument` (opaque black program background), `__setWeb` (same url and reload keep the page; the frame is white), `__setWebPointer` (tip offset −10, −5, 56 px box, clamped to the frame), the six markup layers, `__setInfo` (shown and hidden, never faded), `__setCamera(null)`, and `__configure` (transition clamped to 0–5000 ms; `video_layout: None` is `DEFAULT_VIDEO_SETTINGS` from `src/lib/program.ts`). Every applied command is sent with its sequence, so `FrameCommitted` follows even when no pixels change. A newly opened window is rebuilt from the surface state, as `ready` and `recover` do.

Decision: layer pixels arrive with the scene prompts. Until then nodes exist without pixels (`Fill::Empty`), so every swap and fade already behaves as it will. Colour backgrounds draw now with a small parser (`#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb()`/`rgba()` with numbers or percentages, `transparent`, `black`, `white`); other CSS colours, gradients, images and video are logged once per surface and left for 053 and 055. A camera with a label is not captured (057) and keeps the old node, as `__setCamera` does when no device matches. Test patterns are 060.

Decision: goldens run on the software adapter when one exists (`force_fallback_adapter`), else any adapter. CI installs `mesa-vulkan-drivers` (lavapipe) on Linux and sets `KAIRO_REQUIRE_GPU` on Linux and Windows, so those suites cannot skip there; macOS runners may lack a GPU and skip. Stored PNGs are compared at ±3 per channel, with up to 0.5% of pixels allowed beyond that for anti-aliased edges and the shadow. They were generated on Metal; lavapipe and WARP have not been run.

Decision: fixed a defect in 050. When the UI dies, so does the engine's stderr pipe. The engine's next log line failed to write, `tracing-subscriber` reported the failure with `eprintln!`, and `eprintln!` panics on a broken stderr, so the client thread died before reporting the disconnect. The grace period never started, and the engine kept its windows up indefinitely: three such engines from earlier test runs were found still running. The engine's log writer now drops what it cannot write, `log_internal_errors` is off, and a drop guard reports the disconnect however the client thread ends. A regression test closes stderr before disconnecting.

API differences (wgpu 30.0.1): `VertexState::buffers` takes `&[Option<VertexBufferLayout>]`; `RequestAdapterOptions` has `apply_limit_buckets`; `SurfaceConfiguration` has `color_space`, so it is built from `get_default_config`.

Affects prompts: 052, 053, 054, 055, 057, 058, 059, 060, 061, 066.

## 2026-11-10 — 052 — Scene model and slide text

Decision: `SlideScene` is data, not markup. `SlideScene::build(theme, reference, text, frame, coloured_lines)` gives up to two `TextBox`es (verse, then reference) with a pixel rectangle, padding, vertical alignment, optional backdrop, size (`Fixed` or `Auto { cap }`), a `TextStyle` (family, weight, line height, letter spacing, alignment, decoration, colour with opacity, optional shadow, optional stroke) and paragraphs that already have whitespace collapsed and `text-transform` applied. The verse box exists only when `text` is non-blank (JavaScript `trim`, so U+00A0 and U+3000 count as blank), the reference box only when it is shown and non-blank. Coloured lines replace the body only when the list is non-empty; a line's colour passes `sanitizeOverlayColor` (`#rgb` or `#rrggbb`, nothing else). Because text is a field and never concatenated into a string, nothing in it can reach a style: `<span style=...>`, `&amp;` and quotes stay characters, and a test checks that hostile text and a clean one give identical styles. `escapeHtml` and `escapeAndBreak` have no counterpart.

Decision: whitespace and lines follow the browser. Plain text splits at `\n` (the template's `<br>`); runs of space, tab, CR and form feed collapse and the ends trim; a no-break space stays. A newline inside a coloured line is only whitespace, because only the plain path converted newlines to `<br>`. A trailing newline adds no line (`a<br>` is one line), a blank paragraph between others is one line high, and a lone `<br>` (such as `" \n "` in coloured mode) is one blank line. Letter spacing is off for cursive scripts (Arabic, Syriac, N'Ko, Mongolian), as Chromium does; the corpus showed a 22.8 px width difference on the Arabic line without it. Parley takes the base direction from the first strong character, Chromium from the (unset) page, so a left-to-right mark is put in front of any paragraph holding right-to-left characters. This answers the open spike question on RTL base direction: it can be forced, with an invisible character that shapes to nothing and takes no spacing.

Decision: one auto-fit. `kairo_scene::fit` replaces `estimateAutoFitVerseFontPx`, `largestFontThatFits`, `overlayTextOverflows` and the copy in `overlay.html`. The size is the largest whole pixel from 12 up to a cap at which real shaping fits the text in the box (height and width, each rounded, with one pixel of slack, as `overlayTextOverflows`), searched with `overlay.html`'s loop (`lo <= hi`, whole-number midpoints) because that is what produced the output; the editor preview's variant gives the same size when the fit is monotonic. Clarification of the prompt's "retire the estimate": the cap is the smart-fill rule, `min(estimate(text), estimate(SMART_FILL_REFERENCE))`, and that rule is arithmetic on character counts, not shaping. Chromium's final size is the largest size at or below that cap, so a text whose estimate is lower than its real fit ends at the estimate. Dropping it would give larger text than Electron for long passages, so the cap arithmetic stays, once, inside `fit::smart_fill_cap`, with JavaScript's UTF-16 lengths and whitespace set. The fit itself never estimates.

Decision: fonts. Themes choose from `FONT_STACKS` (listed in `typeset.rs`); older CSS stacks go through the same parser (`FontFamilyName::parse_css_list`). The chain tried for each glyph is the theme's list in order, then the bundled Source Sans 3, then the system sans-serif, then parley's per-script system fallback. A list CSS cannot parse is a dropped declaration, so the browser default applies, a serif. Source Sans 3 is bundled from `@fontsource-variable/source-sans-3` 5.3.0 (the package the renderer imports), five subsets (latin, latin-ext, vietnamese, greek, cyrillic; 310 KB), converted from woff2 to TTF with fontTools (`flavor = None`, variations kept) because parley and swash do not read woff2. Each subset is registered under its own family name (`Source Sans 3`, `Source Sans 3 Latin Extended`, …) because a family is matched by weight, not coverage, so the next family is what supplies a missing glyph. Yoruba needs latin-ext (ṣ, U+1E63) and vietnamese (ẹ, ọ, U+1EB9, U+1ECD, and the combining tone marks); a test checks every Yoruba letter and mark is covered by the bundle alone. The SIL OFL 1.1 text is next to the fonts (`crates/kairo-scene/fonts/OFL.txt`). Custom user-uploaded fonts are not covered.

Decision: painting, from measurements against Chromium 130 on the corpus. Text is drawn from cached glyph coverage masks (per font, glyph, size, variation and quarter-pixel x), blitted with the baseline snapped to a whole pixel, as Skia does, instead of filling outline paths. That cut the corpus raster median from 5.2 to 2.3 ms and the 400-case p95 from 33 ms to 10.8 ms. Stroke is `-webkit-text-stroke`: centred on the outline, painted under the fill, miter joins; inside uses 0.55 of the width with a half-pixel minimum, dashed and dotted are solid. The shadow is the fill blurred at sigma = blur / 2; it does not include the stroke (answering the open spike question: leaving the stroke out raised the outline group's mean text-block SSIM from 0.9375 to 0.9408). Decorations: thickness is a tenth of the size (at least 1 px); the bar's centre snaps to a whole pixel; an underline's top hangs `ceil(thickness / 2)` below the baseline; a line-through sits a third of the font's ascent above it; both are drawn in the stroke colour when there is a stroke, and cast the shadow. These were fitted to Chromium's pixels on Georgia and Helvetica Neue runs (Georgia 54 px line-through: 5.0 px thick at 16.5 px above the baseline, matching ascent/3 = 16.5); the spike's constants (0.3 and 0.055 of the size) were the main loss. Underlines skip ink: the bar is cut at each glyph's ink inside the bar's rows, widened by one thickness; Chromium's exact dilation was not isolated.

Decision: colour. `colorWithOpacity` is ported with its quirks: it rewrites only `#rgb`, `#rrggbb` and `rgb()`/`rgba()` whose first three arguments are plain numbers (their own alpha is replaced by the opacity); anything else, including `#rrggbbaa` and percentage `rgb()`, passes through and keeps its own alpha. A colour CSS cannot parse drops its declaration: text falls back to the document's black, and a stroke, shadow or backdrop is simply absent. Colours in a normalised theme are `#` hex or `rgb()`/`rgba()`, which is all `parse_css` reads; named colours and `hsl()` are not.

Decision: raster and threads. `kairo_scene::rasterize` returns premultiplied RGBA for the union of what the boxes draw (not the whole frame). `RasterPool<T>` runs it on worker threads with an LRU keyed by the scene's content hash and bounded in bytes, builds the scene on the worker (theme parsing too), survives a panicking or failing job, and also offers `render` on the calling thread for thumbnails. Each thread has its own font collection, shaping context and glyph cache (24 MiB); nothing is shared. In the engine, `ShowSlide`, `SetBackground`, a repainting `ClearText`, `ShowDocument` and a rebuilt window carry a `TextRequest` in the plan the shell returns. The text node is swapped in at once as a placeholder in the operation list and its pixels replace the placeholder when the worker finishes. Per surface, commands apply in the order sent: while a slide's pixels are pending, later commands wait behind it, so a clear cannot overtake a slide; commits stay in order (tested against a real GPU). A slide always swaps a text node, empty when it has no words, as `renderOverlayHTML` always yields a `.pa-layer`; a blank slide (a document page, an empty verse) asks nothing of a worker. A theme that does not deserialise strictly is normalised with the domain normaliser; one that is not JSON, or coloured lines that are not `{text, color?}` entries (the template would have thrown), fail the slide: the error is logged and no text is shown. The engine reads no settings, network or database; it depends on `kairo-scene` and `kairo-domain`.

Decision: tests and thresholds. The corpus test rebuilds all 480 prompt 003 cases in Rust (`tests/support.rs`, from `corpus.cjs` and the domain's built-in themes), builds each scene through the real path, and compares line breaks and fitted size with Chromium's own layout (`tests/golden/003-text/layout.json`, committed) and pixels with Chromium's flat-background renders, which are 46 MB and so are not committed: they are cached in `.cache/golden-capture/003/text` (ignored) or `KAIRO_TEXT_REFERENCE`, produced by `tools/golden-capture` (Chromium 130.0.6723.191 from Electron at `af978c5`; the regenerated `layout.json` is byte-identical to the committed one). Without them the SSIM test says so and returns, unless `KAIRO_REQUIRE_ORACLE` is set. Gates: at least 99% of text layers break exactly; every fitted size within 1 px; SSIM (8×8 luminance blocks, spike 003's definition) whole-frame mean at least 0.99 and worst at least 0.90, text-block mean at least 0.92 and worst at least 0.55. Measured: 639 of 640 layers exact (399 of 400 text cases; the miss is spike 003's `midnight-long-right-fit`, 0.3 px of accumulated advance at 13 px), 200 of 200 fitted sizes equal; SSIM 0.99399 and 0.92361 whole-frame, 0.93473 and 0.61209 text blocks (spike 003: 0.98627, 0.81611, 0.87788, 0.47964).

Decision: `[profile.dev.package.*] opt-level = 3` for `kairo-scene` and the crates it shapes and rasterises with (parley, harfrust, fontique, swash, zeno, tiny-skia, skrifa, read-fonts, icu_*). Unoptimised, the corpus test took 190 s on a loaded machine against about 6 s optimised. Only these packages change.

Decision: `tools/check-lib-coverage` only recognises counterparts in `kairo-domain`, so the exports prompt 052 implemented stay listed as exclusions, with these native counterparts: `estimateAutoFitVerseFontPx` is `kairo_scene::fit::smart_fill_cap`; `MIN_AUTO_FIT_PX`, `MAX_AUTO_FIT_PX` and `SMART_FILL_REFERENCE` are the constants of `kairo_scene::fit`; `largestFontThatFits` and `applyOverlayAutoFit` are `fit::largest_that_fits`; `overlayTextOverflows` is the predicate in `raster::layout_box`. Owned by 053 in that file but done here because text needs them: `colorWithOpacity` is `color::css_with_opacity`, `sanitizeOverlayColor` is `color::sanitize_overlay_color`, `OverlayColoredLine` is `ColoredLine`, `formatOverlayColoredText` is `paragraphs_for`. Prompt 053 should reuse them.

Edge cases handled, from `overlay-template.ts`, `overlay-fit.ts`, `overlay.html` and `program-surface.ts`, each with a test unless marked: blank verse leaves no verse box and blank or hidden reference no reference box (a scene with neither draws nothing, and the slide still swaps an empty text node); empty `coloredLines` falls back to the text; an unsafe line colour is dropped; a coloured entry without a string `text` fails the slide as the template's `escapeHtml(undefined)` threw; `<`, `&`, quotes and `style=` in text or reference are literal; fit never goes below 12 and tolerates one pixel of overflow on each axis; a cap below 12, fractional or not a positive number (`|| 240`) is handled; a fixed verse that overflows is top-anchored while a reference and a fitted verse keep their alignment; an outline that is disabled or 0 wide draws no stroke, and so does an unparsable colour; the same for the shadow; whitespace-only paragraphs, a trailing newline and an unbreakable word (`overflow-wrap`); backdrop on or off; padding larger than the box (the content is at least 1 px); NaN, infinite, negative and huge theme numbers and zero-sized or off-frame boxes (no panic, tested); a slide with no words (document pages) asks nothing of a worker; `clearText` only swaps the text node out unless the background changed, then it repaints (shell tests); `clear` removes the node; a worker that panics or a scene that cannot be built answers and the pool keeps working; a result for a surface that closed or was rebuilt meanwhile is ignored; the text slot is only filled when the placeholder swap exists. Not covered: `text-transform` locale rules (Turkish `i`), `hanging-punctuation`, hyphenation, fonts a user uploads, and colours other than the hex and `rgb()` forms.

Known differences from Chromium, not fixed: Skia's glyph anti-aliasing against tiny-skia's coverage masks (text-block SSIM tops out near 0.93 to 0.95 on Latin, lower on small text where few blocks hold ink); Arabic glyph shapes in the fallback font differ slightly (the `rtl` group's text-block mean is 0.83); `justify` spacing is parley's. A fitted verse clips to its content box rounded to pixels and a fixed one to its border box, which the pixmap's edge provides.

Affects prompts: 053, 054, 076, 086.

## 2026-11-11 — 053 — Backgrounds, elements, shapes and filters

Decision: where each thing is drawn. Colour and gradient backgrounds, the four CSS colour filters, `object-fit`, flips, turns, blur and the background's blur cover are drawn by the compositor's one shader (`kairo-compositor`); the pictures are decoded and the vector elements (rectangle, ellipse, catalogue shapes) are drawn on the CPU by `kairo-scene` (`media`, `vector`), on the engine's two `visuals` workers, never on the winit or compositor threads. A gradient is `Fill::Gradient` (CSS angle: the gradient line is `|w sin a| + |h cos a|` long through the centre, colours interpolated premultiplied as CSS does). A layer of elements is one keyed node holding `Fill::Group`; when it fades (opacity below 1) with two or more children it is drawn into an offscreen frame and faded as one, so overlapping children do not show through each other (CSS group opacity), and otherwise drawn straight.

Decision: filters, from `mediaFilterCss`'s order, on straight colour with each stage clamped, as Chromium's colour matrices run: `hue-rotate`, `saturate`, `brightness`, `contrast`. A background's wrapper carries the colour filter over a blurred picture, so it is blur then colour; an element's `filter: hue-rotate() … blur()` is colour then blur (`Look::blur_first`). Blur is a separable Gaussian whose sigma is the CSS radius, run on a copy padded by 3 sigma so edges fade to transparent as CSS `blur()` does, folded to linear taps. A still picture's blurred copy is cached by raster, size, sigma and filter; a video's is redone each frame. The background's `1 + blur*4/1080` scale is `Look::scale` about the box centre, cut to the box.

Decision: elements. A box is the percentages snapped to whole pixels (Chromium snaps a box). A rectangle or ellipse's stroke is a ring inside the box (`box-shadow: inset`), drawn over the fill, whose inner corners are `radius − width` (an ellipse's inner radii are each `−width`). A shape is its path stretched to the box, even-odd, with a round-joined stroke of fixed pixel width (`non-scaling-stroke`) that may spill past the box. Rotation and flips (flips first) are applied to the geometry for vector elements, so edges stay sharp; for pictures the GPU turns the quad. An unknown shape, a media element with no file, or an empty or off-frame box draws nothing; at most 40 elements (`normalize_overlay_elements` already caps and cleans). An unreadable fill colour on a shape is an invalid attribute, so black; on a rectangle it is no background.

Decision: identity. The background key is `TAG:file:cue` (`HTML:` plus the markup for colour and gradient); the cue is part of a video's key only (the template wrote `data-media-cue` on `<video>` alone, and the earlier shell code added it to images too). A kept node (same key) takes the new opacity, filter, fit and loop and keeps its pixels or its playing video: the shell sends `Fill::Pending` and `Stack::set_keyed` merges it. Identical element sets are the same node: the shell remembers the key each elements slot holds and sends nothing, so element videos and animated pictures continue across verses (`overlay.html` returned when `innerHTML` matched). Element videos carry `MediaParams { element: true }` (muted, no transport); `VideoTransport` and `PatchBackground` only ever address the background node. Video frames are prompt 055's: until then a video node holds `Fill::Source` with a stable id from its key and draws nothing, as the earlier build did.

Decision: pictures are late, words are not. A picture is an `<img>`: the node exists at once and its pixels arrive when decoded, as `Op::Update { slot, key, content }` (ignored unless the key is still the slot's current one, and dropped when a newer request for the slot superseded it, by a generation counter in the shell). Vector element layers are quick and have no file, so the slide waits for their first stage only; a layer with pictures sends a second result with them. Measured: a 6000 × 4000 png background: slide committed at 264 ms, picture at 440 ms (debug). Updates for a surface go through the same in-order queue as commands, so one can never overtake the keyed op that makes its node.

Decision: formats. By content, not name. gif (composited frames, Chromium's rule that a delay of 10 ms or less plays as 100 ms, loop count from the Netscape extension: none plays once, 0 forever, n plays n + 1 times), animated webp and apng (loop count as the file says, same delay rule), still webp, avif (`avif-decode`, pure Rust `rav1d`, primary picture only), bmp, png, jpeg (EXIF orientation applied), svg (usvg and resvg, drawn at the size it will have, nothing outside the file is read, system fonts for text). Limits: 32,768 px a side and 150 MP a frame, 4,000 frames and 384 MiB of frames, files over 256 MiB refused. Pictures larger than their box are reduced once with Lanczos on premultiplied data; never enlarged. Animated avif (a sequence) shows its primary picture; colour profiles in files are not applied (not verified against a wide-gamut file).

Decision: a failed picture draws nothing. Chromium draws a thin frame and a broken-image icon for a missing or damaged file even with `alt=""` (measured: the corpus cases `media-missing-file` and `media-broken-file`); a congregation screen should not show that, so the engine logs a warning and shows no picture, and the rest of the slide is unaffected. Those two cases have a looser limit for it.

Decision: thresholds. The corpus is 111 themes (`tools/golden-capture/visuals.cjs`): backgrounds (colour, opacity, rgba, six gradient angles, png/webp/bmp/avif/svg/gif in cover, contain and fill), filters (each of the four, up and down, together, over alpha), blur (4, 12, 30, cover, with filters), elements (rectangle, radius, stroke, thick stroke, rotation, ellipse, order), 33 catalogue shapes plus stroke, opacity, stretch, rotation, flips and spill, and images as elements (every fit, flips, turn, filter, blur, svg, webp, alpha, missing and broken files). Chromium 130.0.6723.191 at Electron `af978c5`, off-screen (an on-screen capture converted colours to the display's profile, shifting `#1b4f72` to (42,78,111); off-screen it is exact). Each case: SSIM (8 × 8 luminance blocks, spike 003's definition) at least 0.99 and at most 0.5% of pixels more than 24 levels off; the two failed-file cases at least 0.98 and 0.3%; mean at least 0.999. Measured: mean 0.99953, worst 0.98923 (the failed files), next worst 0.9972 (a shape stretched 90 × 12). The references are 111 PNGs (about 5 MB), cached under `.cache/golden-capture/053/visuals` or `KAIRO_VISUAL_REFERENCE`, not committed; `tests/golden/053-visuals/cases.json` (the themes) and `tests/fixtures/visuals` (one 4:3 test card per format, regenerated by `cargo run -p kairo-scene --example visual_fixtures`) are. Without them the test skips unless `KAIRO_REQUIRE_ORACLE` is set. Compositor-level tests need no references: fit, flips and turns, every filter against the CSS matrices, blur sigma (84% one sigma in), gradient angles, group fade isolation, animated frame timing, `Pending` and late-pixel rules.

Decision: `[profile.dev.package.*] opt-level = 3` added for `rav1d`, `avif-decode`, `image`, `image-webp`, `png`, `gif`, `resvg`, `usvg`, `kurbo`, `zune-jpeg`, `fdeflate`, `miniz_oxide`, `weezl` and `kairo-compositor`; an unoptimised AV1 decode made the avif case take seconds.

API differences: `tiny-skia` 0.11 has no SVG path parser, so catalogue paths are parsed with `kurbo::BezPath::from_svg` (arcs become cubics) and drawn through `PathBuilder`. `image` 0.25 does not expose the gif or webp loop count, so both are read from the file (`NETSCAPE2.0`, `ANIM`, and `acTL` for apng). `wgpu::RenderPass::forget_lifetime` is used to end a pass before offscreen work in the middle of a frame.

Affects prompts: 054, 055, 058, 086, 088.

## 2026-11-11 — 054 — Messages, props, timers, polls, questions, info, logo

Decision: program layers cross the protocol as data. `SetLayer { kind, layer: Option<Json> }` carries a `ProgramLayer` (`kairo_domain::program`: message, props, timer design, poll, question, logo), as architecture 5.3 names it (`SetLayer { kind, layer }`); `None` is the empty markup that cleared the layer. `program_layers_for` ports `programLayersFor` with the same emptiness rules (blank message, inactive or empty props, timer only with the Look's countdown and the operator's switch, logo only while on). `SetInfo` keeps `ConfidenceInfo`. `PROTOCOL_VERSION` is 2. No markup is made or parsed anywhere, so operator text can never become a style. A payload that cannot be read clears its layer and logs an error.

Decision: fonts. `program.ts`, `overlay.html` and `stage.html` name `'Source Sans 3','Helvetica Neue',Arial,sans-serif`, but nothing in the output or stage windows loads Source Sans 3, and the renderer registers its bundled copy as `Source Sans 3 Variable`. So the screens draw Helvetica Neue on macOS and Arial on Windows, unless the machine has Source Sans 3 installed. The engine draws what the screens show: the stack without `Source Sans 3` (`program::SANS`). The bundled face stays a glyph fallback behind it. Not reproduced: a machine with Source Sans 3 installed system-wide, where Electron would use it.

Decision: where things are drawn. `kairo-scene::program` plans each layer as pieces: pixels drawn now on the CPU (text through the 052 typesetter and glyph raster, boxes through tiny-skia), pictures the engine decodes on its `visuals` workers (props images, the logo; late, as in 053), a QR code scaled by the GPU, a moving piece, and live readouts. The engine turns a plan into one group node per layer; each set is a new node and the old one crossfades out with `transitionMs` (`swapLayer` never keeps a node), the info layer and test pattern cut. Layout follows the CSS rule by rule (the source comments name each rule): message bar padding `0.45 / 0.8 em`, props boxes `pct()` of the frame, poll card in an 8 % inset with `max-width = round(1760 × widthPct)`, question card in 72 px padding with `max-height: 100%`, join row with the QR's 16 px border-box padding, info bar `padding: 20px 48px` with the message at `barPx × 1.25 + 40`, stage boxes at their fixed pixels.

Decision: layout details measured against Chromium 130. `line-height: normal` is the font's ascent, descent and gap with the total rounded to a whole pixel (parley's `MetricsRelative` laid out once, then again at that height); used only by program layers (`LINE_HEIGHT_NORMAL`; themes never ask for it). Square boxes are snapped to pixels; rounded ones (cards, pills, the stage message) are drawn anti-aliased where they fall, as Chromium paints a rounded background. The poll row is a two-item flex row: both spans start at their one-line width and shrink in proportion when they do not fit, never below their widest word, so the count wraps too (Flexbox 9.7). `colorWithOpacity` is ported as written: only `#rgb` and `#rrggbb` take the card opacity. Poll numbers print as JavaScript prints them.

Decision: countdowns and clocks tick in the engine. A timer object or clock is a live piece: its own child of the layer's group. Every 250 ms (`setInterval(…, 250)` in `propTimerSchedule` and `stage.html`) the engine reads each live piece at `Date.now()`; only a readout whose text, colour or dimming changed is redrawn, alone, on a worker, and swapped into the group (`Op::Update`). The rest of the layer is reused as it is, so a running timer redraws a readout-sized tile once a second and nothing else. The rules are `timerReadout`'s: seconds from `endsAt` with `Math.ceil`, roll over or hold at 0, the overrun colour when `remaining < 0` or exactly 0 while running (never for a zero-length timer), 0.55 opacity when paused part-way, `infoFormat` text (`5:00`, `1:02:03`, `-0:12`). The clock is `formatClock` on the local time zone (`chrono::Local`), 12-hour with AM/PM. A replaced or cleared layer stops ticking, a late redraw for it is dropped (generation check), and a leaving node is never updated (`propTimerUpdate` skips `[data-leaving]`).

Decision: the ticker and the motion pattern move on the GPU. `Content::motion` is a linear, infinite translate along x timed by the node's age (`alternate` for the diagnostic sweep), and `Content::clip` is the bar's `overflow: hidden`. The ticker's words start `padding-left: 100%` (one content width) in and move by that plus their own width over `max(8, round(length × 0.22))` seconds, `length` in UTF-16 units of the trimmed text; newlines become ` · ` (`'   ·   '` collapsed by `nowrap`). A moving node keeps its output drawing at the display rate while shown.

Decision: test patterns and the stage. `SetTestPattern` draws `outputTestPatternHTML` (bars, fine text, motion sweep, and the `name · label test` card, `name` being the surface's label) in a new compositor slot, `Slot::Pattern`, above every other slot and outside the camera transforms (`position: fixed` on the body). The stage display is `program::plan_stage(StageView)`, `__setStage`'s payload, laid out at 1920 × 1080 with its clock and countdown live; `stage.html`'s rules are kept: hidden clock, timer or next, a lone countdown sitting at the left (`space-between` with one item), `full` when next is hidden, `next.text || next.reference` and `—` when there is no next, the message box only for non-empty text, the empty reference keeping its 12 px margin. Stage windows are prompt 060's; the engine has the renderer (`LayerSource::Stage`) and no stage command yet.

Decision: edge cases, from the source. Message: blank after trim clears. Props: inactive, or no background and no objects, skipped; none left clears; an image with no file, or an unreadable colour, draws nothing; text is `pre-wrap` cut to its box; a missing or broken picture draws nothing (053). Timer: as above; the design always has a readout (the normaliser adds one). Poll: no voters is 0 %; counts below 0 are 0; at most 100 %; missing totals are 0. Question: author `null` or empty is `Anonymous`; `showAuthor` off hides it; the QR is drawn only from a `data:image/png;base64,` URL made of base64 characters (anything else, including a remote URL or SVG, draws no QR and keeps the rest); a blank headline is hidden; `showJoinUrl` off hides the address; text beyond the card's `max-height` is cut. Logo: off clears; no picture is the colour alone. Info: `None` hides; an empty stage message is hidden (`:empty`) but whitespace shows the box; without the clock the message moves to 40 px from the edge; the backdrop only behind a shown clock.

Decision: thresholds. The corpus is 58 cases (`tools/golden-capture/programs.cjs`): messages (bottom, top, breaks, wrapping, large and coloured, small with rgba, ticker), props (text alignments, weights, lines and spaces, clipping, opacity, shapes, background, images, stacking order, timer idle, paused, overrun, held at zero, hours), timer designs, polls (fonts, widths, opacity, no votes, wrapping rows), questions (author rules, positions, fonts, overflow, join with and without QR, headline and address), logo, info (positions, sizes, message only, backdrop), stage (every switch, message, empty, fallbacks, overflow, held at zero) and the three patterns. Chromium 130.0.6723.191 at Electron `af978c5`, off-screen, `Date` pinned so the clock reads the same; timers are paused so a capture does not depend on when it ran; animations held at their first frame (the engine's corpus rig holds moving pieces at their start, a test-only switch). Each case: SSIM at least 0.975 and at most 1 % of pixels more than 24 levels off; `stage-overflow` 0.96 and 2.5 %, `question-overflow` 0.975 and 2 %, because Chromium stacks lines in 1/64 px units and snaps each baseline, so a long run of large lines drifts by a pixel at different lines; mean at least 0.995. References are cached under `.cache/golden-capture/054/programs` (or `KAIRO_PROGRAM_REFERENCE`); `tests/golden/054-programs/cases.json` is committed. Without references the comparison skips unless `KAIRO_REQUIRE_ORACLE` is set.

Affects prompts: 060, 065, 077, 088.

## 2026-11-11 — 055 — Video playback layer

Decision: one player thread per source id, and the source id is the one the scene builder already gives a video node: a hash of `VIDEO:file:cue` for a background, `element:id:file` for an element. Outputs that show the same file at the same cue therefore draw the same texture from one decoder and cannot drift apart; a new cue is a new id and starts from the beginning. Why: requirement 2 and the `mediaCue` rule fall out of the identity the compositor already uses for keyed nodes. Effect: a shared player is created with the first output's start, end and rate; a later output that joins with different trim or rate values plays the shared stream (the app sends the same background to every program output, so they agree in practice). Loop is the one setting that changes in place, and the last report wins.

Decision: the compositor tells the engine what to play. After every batch of operations, and when a node is removed at the end of its fade, it compares each output's video nodes (node id, source id, `MediaParams`, and whether it is the current background) with what it last reported and sends `CompositorEvent::Media` when they differ; a closed output reports none. `MediaParams` now carries the file path. `kairo_video::Videos::sync` turns those reports into players: a node it has not seen is a new `<video>` element and joins the running player for its id, unless the same output already showed that id through another node (cleared and shown again before the old one was gone), which restarts it, as a new element did in Electron. A kept node (same key: text changed, a slider moved) keeps playing and only takes the loop. A source no output uses any more stops its player, which removes its texture as it exits, unless a newer player owns the id (a generation check; a replaced player also stops drawing into it).

Decision: `VideoTransport` becomes `Op::Media(MediaControl)` in the surface's ordered operation stream, so pause and seek reach the background that is current when they are applied, after the slide before them. The compositor answers with `CompositorEvent::Transport { id }` for the current background's source, after the `Media` report that made it; with no background video nothing happens (`live(layers.bg, 'video')` was null). Element videos never take the transport (`OVERLAY_TRANSPORT_VIDEOS`).

Decision: the playback rules live in `kairo_video::transport::Transport`, apart from threads and FFmpeg, and run on a fake clock in unit tests. They are `overlay.html`'s and `overlay-fit.ts`'s: `videoBounds` (end is `min(data-media-end, duration)`, a start at or past the end plays from 0); the first frame shows even when paused (`preload`); a pause set before the video is ready holds; resuming at or past the end rewinds to the range start; seek ignores a time that is not finite and clamps others to the range; at the end of a non-looping range the video stops there, paused, and `__videoEnded` is true; `__videoEnded` is also true with no video, after an error, and when paused at or past the end; turning loop on for a finished video restarts it unless the operator paused it; a loop change otherwise never restarts. A stream that stops before its stated duration ends (or wraps) at its last frame.

Decision: the clock. Frames are timed on a line of media seconds that only moves forward while playing; each lap of a looping range starts where the last one ended, so the decoder runs through the wrap ahead of the clock (queue of 4 frames) and the first frames after it are ready before they are due. This fixes spike 004's 86 ms wrap gap: on the generated 30 fps clip the wrap interval is one frame period, like any other. When the decoder falls behind, the clock holds one frame after the frame on screen instead of skipping (Chromium waits the same way); frames the clock has already passed are counted as dropped. Rate scales the clock. Sync between outputs is exact (one texture). Audio is prompt 056's: this crate decodes no audio, so A/V sync is not verified here; 056 should slave audio to this clock or this clock to the audio device.

Decision: decoding follows spike 004: hardware first (VideoToolbox, D3D11VA, VAAPI) with a probe of the first frame, software when the hardware cannot open the stream, and a reopen in software at the failed position if the hardware fails mid-stream, each logged. ProRes and VP9 with alpha decode in software (libvpx for VP9 alpha; without libvpx the transparency is lost and a warning is logged). Frames are NV12 or P010 (or NV12 plus an 8-bit alpha plane) copied into `kairo_compositor::YuvFrame`; the compositor uploads the planes and converts them to premultiplied RGBA in a compute shader (`yuv.wgsl`, BT.601, BT.709, SMPTE 240M and BT.2020 matrices, limited and full range), into the source texture video nodes already draw. A damaged packet is skipped; a file that cannot be opened, has no video stream or no frames, or fails to decode in software shows nothing (or keeps its last frame), counts as ended, and is reported once as `Event::Error` ("video PATH: reason"). A frame the GPU cannot take (planes too short, larger than the device limit) is refused, reported once per source, and the last frame stays. `KAIRO_VIDEO_SOFTWARE` forces software decode.

Decision: zero-copy import (IOSurface and the others) stays deferred, as spike 004 recommended. Measured on the M2 Pro with a 1080p30 H.264 file decoded by VideoToolbox: one stream through the compositor costs about 18% of one core, of which decoding alone (frames discarded) is about 6.7%; the compositor thread itself is about 2% busy, most of it `write_texture` copies, and the rest is driver and kernel time.

Affects prompts: 056, 058, 083.

## 2026-11-11 — 056 — Program audio, routing and tracks

Decision: one mixer per process, clocked by the device. `kairo-audio` replaces the per-window Web Audio graphs with one mixer that holds every source (video, document video, camera, web, tracks; up to 32) with its own speaker gain, and up to 8 NDI taps, each fed at full level by the sources its output shows (`source.connect(bus)` before the gain). An audio thread renders the mixer into a ring the device callback drains, keeping about 85 ms queued, so the device's clock paces everything; with no device wanted or none present it runs on the wall clock, so taps keep their pace. `Mixer::render` and the device callback never lock or allocate (a counting-allocator test proves both): sources arrive and leave through rings, settings are atomics, a removed source goes back through a ring to be freed elsewhere. The audio thread asks for QoS user-interactive on macOS; a 43 ms cushion underran while the machine was compiling, 85 ms did not (soak below).

Decision: who is audible. `routing::audio_surface` is `audioSurface`: the first enabled screen that shows backgrounds, follows the service and has a ready surface, else the primary NDI output; lobby screens never. `surface_audio` is `applyConfig`'s per-surface `audio.enabled` (only there), `cameraAudible` (there, with a camera and an audio input) and `ndiAudio` (NDI audio on and a sender that carries it). These are pure functions for the surface manager (prompt 065), which sends them in `Configure`. The engine trusts the config; if two surfaces ever arrive enabled, the first by name plays, so the room never hears two copies.

Decision: device by label. `outputLabel` is matched exactly against device names, as `sinkIdFor` matched Chromium labels; empty or unmatched is the system default. The choice is made again on every label change, every second (hot-plug: a device plugged in takes over, one pulled out falls back to the default), and when the stream fails. Chromium labels and cpal names can differ for the same device (Chromium adds suffixes on some systems); a saved label that no longer matches falls back to the default, as Electron did when a label went missing. Not verified on Windows or Linux.

Decision: video sound follows its picture. Each sounding video node (element videos are decoration, never sound) gets a clip player keyed by the picture's source id, so outputs showing the same file and cue share one, as they share the decoder. The 055 player keeps the clock: every 50 ms the engine reads its position, pause and end; the sound starts when the picture is seen playing, pauses with it, and is moved when it is more than 80 ms off. The device queue is allowed for: the sound is aimed that far ahead so it is heard with its picture. This reverses Chromium, where audio is the master clock; DECISIONS 055 made the picture's clock, and sound follows it. Measured on a 30 fps test clip with a 440 Hz track (engine, rehearsal window, default speakers): mean offset 14.8 ms, worst 43.2 ms, one resync at start.

Decision: files. Sound is decoded with FFmpeg and converted here (libswresample's channel-layout API differs across the FFmpeg versions `ffmpeg-next` builds against): every sample format, mono on both sides, 5.1 folded with the Web Audio speaker coefficients (`L + 0.7071 (C + SL)`), other layouts by their first two channels, any rate to 48 kHz with rubato. Rate is a resample to `48000 / rate`, so the pitch moves with the speed; Chromium keeps the pitch (`preservesPitch`). Not reproduced; recorded here. Range, loop (seamless, decoded across the wrap), pause, seek clamped to the range, stop at the end, and play-after-end from the range start are the media element's rules. The position is what the mixer has taken.

Decision: gains change over one render (at most 21 ms) instead of at once as `gain.value =` did, to avoid a click. A source counts as playing (running dry is then a glitch) once 100 ms is queued after a start or seek.

Decision: tracks and the fallback run in the operator process, each on its own mixer and device, never on NDI. `TracksPlayer` follows the tracks library (`follow(&TracksLibrary)`): the live item plays, `livePaused` pauses, a new id starts from 0, play after the end starts from 0, seek is never before 0, volume is always full (the element's default), and a live id with no item stops. `FallbackPlayer` plays the live background video's sound at the presentation volume while `routing::fallback_active` holds (sound on, a video live, no ready screen or NDI output that follows the service and shows backgrounds; video cards, Syphon and lobbies do not count, so a rig of only hardware outputs hears it), with a fresh player per cue, range or rate, and loop, pause and volume applied in place. Both re-match the device on hot-plug (Electron's fallback did not).

Decision: the NDI tap. Blocks are 1024-frame planar stereo float at 48 kHz, as `KairoTap` posted them; each passes `handleNdiAudio`'s checks (`AudioBlock::validate`: 1 to 8 channels, 1 to 16384 samples, 8 to 192 kHz, data length) before it leaves the audio thread's side. A tap that is off does no work: the mixer skips it and its sender thread does not read it. A tap with no sources still posts silent blocks, as the always-connected bus did. One sender thread serves every tap. The engine has no NDI sender yet (prompt 061): blocks are made, checked, counted and dropped, with one log line.

Decision: edge cases. A file with no audio stream plays silent; a missing, damaged or unreadable file logs a warning and plays nothing; damaged packets are skipped. No device: no speaker, taps on the wall clock. A full mixer or more NDI feeds than taps logs a warning and that source is silent. Camera and web sources have their kinds and slots; their capture is prompts 057 and 059.

Decision (2026-11-13): a started or seeked clip is held until 250 ms is queued. The mixer neither reads a held source nor counts it as an underrun. Before this, a clip was heard as soon as it had data and counted as playing at 100 ms queued, barely above the output's 85 ms cushion, and the soak found start-up underruns (cold decoders). A media element also waits for enough data before it plays. A file shorter than 250 ms is released at its end.

Affects prompts: 057, 058, 059, 061, 065, 084.

## 2026-11-13 — 059 — Capture gate and Windows qualification

The owner explicitly approved proceeding with implementation and deferring Windows qualification in this session. macOS `lb-wry` 0.53.3 `WKWebView.takeSnapshot` captured 300 measured 1920×1080 frames at 30 Hz after 30 warm-up frames: debug build p50 5.852 ms, p99 9.014 ms, max 14.164 ms, 0 missed ticks and 0 errors (M2 Pro, macOS 27.2). The copy into CPU-owned pixels is included. The probe uses an explicit 1920×1080 logical child-view rectangle; Wry's default child bounds are square, so the first attempt was rejected for incorrect dimensions. This qualifies the snapshot fallback only; ScreenCaptureKit, Windows.Graphics.Capture and Windows performance remain unverified. The capture probe is in `apps/kairo-engine/examples/web_capture.rs`. No GPUI dependency or version change is needed.

### 059 amendment to the 2026-11-09 web entry — captured display outputs

The owner selected captured frames through the compositor for **display outputs too**, replacing the native-child-view display plan in the 2026-11-09 entry. This preserves one shared page across displays and the compositor's fades, slide layers and pointer. The owner explicitly conditioned this amendment on the section 4.1 capture measurement passing at 1920×1080 / 30 fps, p99 < 50 ms **on macOS and Windows**. If a measured capture fails, set 059 blocked and ask the owner. Measure and record the added display latency in PROGRESS.md; capture API request duration alone is not that measurement.

This later condition supersedes the earlier permission in this session to defer Windows qualification for this captured-display path. macOS snapshot capture passed; Windows has no measurement in this environment, so the amendment's gate is incomplete and production web-layer implementation has not proceeded. Added display latency is not measured. The probe is a development example, not an enabled output feature.

## 2026-11-13 — 057 — Live camera layer

Decision: capture through AVFoundation directly (`objc2-av-foundation` 0.3.2, already in the lock), not `nokhwa`. Chromium on macOS captures with AVFoundation, so names, formats, muxed capture-card devices and the camera permission prompt are the ones Electron had. `nokhwa` wraps the same framework on macOS but builds its own device names and pulls older `objc` crates; its Media Foundation and V4L2 backends would not change labels that must match Chromium's either. Windows (Media Foundation) and Linux (V4L2) are not built in this prompt: there `kairo_camera::system()` reports `CameraError::Unsupported` on every request, which reaches the operator as an error while the screens keep what they showed. Not verified on either platform.

Decision: labels. Chromium's camera label is `localizedName` plus ` (vvvv:pppp)` when the device's unique id ends in eight hex digits (`GetNameAndModel`). A saved label is matched exactly first; if nothing matches, it is compared with each bare device name after removing a `Default - ` (or `Communications - `) prefix and then each trailing ` (…)` group in turn. That way saved labels still resolve when Chromium added a suffix this port derives differently (`(Built-in)`, a model id from another source). Audio input labels are Core Audio names through cpal, matched the same way; `Default - …` is the system default input.

Decision: format and frames. The format nearest the ideal 1920 × 1080 at 30 fps wins by the spec's fitness distance (ties: the larger frame); the frame rate is 30 when the format allows it, else the nearest end of its range. Frames are requested as NV12 studio range, late frames are discarded by AVFoundation, and each frame is copied once into a `YuvFrame` for the GPU conversion from prompt 055 (BT.709 at 720 lines and above, BT.601 below; colour tags on the buffer are not read). The node is `Fill::Source` cover-fitted on opaque black in `Slot::Camera`, laid out by the 051 camera layout. Camera source ids start at 2^48, clear of the video players' ids.

Decision: life cycle. Outputs that want the same label share one capture and one source id. Opening runs on its own thread. A camera change during the open discards the stale stream when it arrives, with no event (`cameraLabel !== wanted`). Until the new stream is live, the old node stays, as `__setCamera` kept it. Not found, denied and busy all fail the same way for the operator (Electron resolved `false`); the engine sends `Event::Error` with `Camera not found: LABEL`, `Camera access is not allowed`, or `The camera is in use by another program`. Asking again for a label that failed tries again. Mid-show, an unplugged device keeps its last frame on screen, as a `<video>` with an ended track does, and is opened again on the same source id once it is listed again (checked every second; Electron left it frozen until the operator chose again). A device taken by another program (no frame for 2 s while `isInUseByAnotherApplication` or the session stopped) is reported and resumes in place.

Decision: camera sound. `audioLabel` opens that input through cpal. Core Audio input has no echo cancellation, noise suppression or gain control, so the `false` constraints hold without settings. Mono goes to both sides, otherwise the first two channels are used. It is resampled to 48 kHz in the callback without allocating. It is a `SourceKind::Camera` mixer source per input label, heard at the volume only on the audio surface with `cameraAudible`, and tapped by every output that shows it. A live input runs on its own clock, so its queue is bounded at 60 ms (input is dropped until half drains). An input that stops is reopened every second.

Decision: Objective-C exceptions. A device that refuses a call (a frame duration a hair outside its range: 1/30 s rounded to 33 ms is 30.3 fps, which aborted the first real run) raises an exception, which Rust cannot unwind through. Every AVFoundation entry point and the frame callback run inside `objc2::exception::catch` (feature `exception`). A caught exception fails the request with its reason. Frame durations are taken from the device's own rate range at its ends.

Not done: glass-to-output latency against Electron (needs a second camera filming a timecode display; not measured), capture cards (none attached).

Affects prompts: 065, 077.

### 059 owner amendment — defer Windows qualification (2026-11-13)

The owner explicitly chose to skip the Windows capture measurement for now and proceed with building the web layer, testing Windows when the app can run there and reverting if necessary. This supersedes the both-platform pre-implementation condition above. Proceed with captured frames on display outputs as already selected, using the passing macOS snapshot measurement. Windows runtime capture/performance is unverified until tested on Windows; never report it as measured. Measure added display latency on macOS during implementation.

### 059 implementation details

- Shared pages have an identity independent of their original URL. A redirect or link adopts that running page when intended state changes to its new address; returning to the original address can create a separate page while another consumer stays on the navigated address. Unused owners are released before applying the eight-page limit; capacity failures retry when a slot becomes free.
- The canonical web viewport is 1920×1080 on every output. Other compositor frame heights contain that viewport on white, preserving page proportions; pointer coordinates and its 56 px arrow are transformed through the same fit. This follows from using one page across outputs with different frame heights rather than relaying separate browser layouts.
- WKWebView must have a mapped source window for animation and media to run; hiding it stalled requestAnimationFrame. The source stays non-interactive and behind other windows, outside normal window cycling. Timer throttling is disabled through Wry; outputs draw only captures. No source window or page remains without consumers.
- Windows uses WGC on a borderless 1920×1080 physical source window, WebView2 rasterization scale 1, a two-frame pool and D3D11 staging readback. Empty polls after a static frame are normal. Input is delivered to the owned WebView2 render HWND; a runtime whose HWND hierarchy differs reports an error rather than sending input elsewhere. Runtime and performance qualification remain deferred.
- A URL-only WebView2 navigation callback cannot enforce the iframe top-navigation restriction. Before loading a Windows page, enable response-stage CDP Fetch interception for HTTP(S) documents and add an intersecting CSP `sandbox allow-scripts allow-same-origin allow-forms allow-presentation` plus strict referrer policy, retaining all original headers. The stable `Fetch.fulfillRequest` preserves the original response body when body is omitted at response stage ([official protocol schema](https://raw.githubusercontent.com/ChromeDevTools/devtools-protocol/master/pdl/domains/Fetch.pdl)). Failure to install/apply the sandbox stops loading, never falls back to an unrestricted page. Camera/mic/device permissions are separately denied by native delegates. macOS uses WKNavigationDelegate frame provenance and a denying WKUIDelegate, replacing Wry's permission-granting default.
- Capture callbacks feed the GPU immediately; waiting for the next 30 Hz engine tick added an avoidable frame of delay. Timings distinguish snapshot request→CPU pixels from pixels→GPU-complete display submission; they exclude sampling age and physical scanout. Diagnostic PNG readback runs off the window thread.
- Shutdown is asynchronous while the GPU releases surfaces. A sample of the stalled engine showed main joining the compositor while the compositor dropped a wgpu surface whose Window destruction needed main. The event loop now services native destruction until the GPU thread is finished, then joins and exits; no timeout increase or test weakening.
- Web page audio cannot be intercepted into the mixer via public WKWebView, WebView2 or WebKitGTK APIs. A single page can play to the system's normal device (HTML audio playback verified on macOS), but program speaker gain/device selection and isolated NDI audio taps do not control it. Windows/Linux audio is not verified. DRM capture, PiP/fullscreen behaviour on capture outputs and physical scanout latency are not qualified here.

## 2026-11-13 — 058 — Document pages

Decision: `SwapPage` is answered by a new event, `PageSwapped { output, seq, swapped }`, not by a query. It stays a command, so it keeps its place in the surface's order and still gets `FrameCommitted`. The event is sent once per turn, before the frame that carries a swapped page. The shared reducer still applies a turn optimistically (`mediaPath` changes at once). When the engine answers false, the caller sends the full `ShowDocument` with that page, as Electron's caller did, and that push puts the state and the screen back in step.

Decision: a turn keeps the page node and its key and fills it with an `Op::Update` once the page is decoded, so a turn is one texture change and not a rebuild. Until then the old page stays. A turn whose result arrives after a newer turn, or after any repaint of the background (`is_current` on the background slot), is cancelled: its op becomes an update that matches no node, and the turn is answered false. Turns are applied in the order they were pressed, so an older page can never follow a newer one.

Decision: decoded pages stay on the GPU in a per-surface LRU of 4 (`PAGE_CACHE`: the page shown, the pages on either side and one spare). The pages are rasters that the compositor uploads at once (`Compositor::keep_resident`) and excludes from its 240-frame expiry until the surface keeps others or closes. The CPU decode is shared with backgrounds (`visuals::Shared::picture`, keyed by file, mtime, size and fit).

Decision: `showDocument`'s theme rule is applied in the engine: the background's `blurPx` becomes 0, and in document mode the page picture blocks the command's frame (`__markDocument` awaited `decode`). An unreadable page leaves an empty node on the black frame and logs a warning; Electron's `markDocument` rejected in that case.

Affects prompts: 065, 085.

Decision: embedded document videos live in a new compositor slot, `Slot::DocumentVideos`, drawn right after the background in the same box (Electron put them in `#pa-bg-layer`). One keyed node per slide holds a group of `Fill::Source` children. Each child sits at its `box` inside the slide's contain-fit box and is clipped to that box (`overflow: hidden`). Each video has its own source id (`DOCV:id:page:generation:index:file`), so two slides or two boxes showing one file never share a player. Videos start hidden (opacity 0). The engine pauses each new player and seeks it to the clip start when the compositor first reports it (`documents::prime`), so a video waits where `loadedmetadata` put it. Showing or hiding a video is an `Op::Update` of the same node, so it keeps playing. A clip ends at its `end` through the player's range (`timeupdate` paused it there). Videos are audible, like the page's `applyAudio`.

Decision: `__controlDocumentVideo` and `__documentPlayback` are pure rules in `apps/kairo-engine/src/documents.rs`, over the player's status (position, range end, paused, ended, error), with Electron's clamping and restart conditions. When the duration is unknown, the clip's end is used, or else 0 for status and unbounded for commands. A seek to NaN moves nothing (the page threw). Play or restart with no player, or with one that failed, hides and pauses the video and sets "Video playback could not start. Check the video format and try again." A decode error reports "This embedded video cannot be played. Convert it to MP4 (H.264/AAC) or WebM and import again.", and takes precedence. A slide whose `width` or `height` is unusable gets no video node, but still reports status. A page turn's new videos replace the old ones only when the page is shown. A refused turn drops all its operations, so the old page and its videos stay.

### 059 fullscreen capture blocker (2026-11-13)

The owner's Windows deferral remains in effect. It does not waive macOS live
animation correctness. Fullscreen output fully occludes the mapped source and
WKWebView stops requestAnimationFrame; captures keep delivering frame 0. A
ScreenCaptureKit own-window experiment delivered fast pixels but did not make
animation advance. Wry's disabled throttling and setting public
WKInactiveSchedulingPolicy::None directly also failed to resolve it. The
unsuccessful experiment was removed; no new capture dependency remains.
The earlier mapped-window implementation note only holds for partial occlusion.
059 is blocked pending a viable native host/capture solution or an explicit
architecture amendment; it is not blocked on having Windows hardware.

### 059 WebKit capture visibility (2026-11-13)

Resolves the fullscreen blocker above without changing the selected shared-page
and captured-display architecture. WebKit independently treats an occluded window
as invisible; disabling inactive scheduling does not keep requestAnimationFrame
running. The backend calls the private per-view
`_setWindowOcclusionDetectionEnabled:NO` before loading untrusted content.
[WebKit's declaration](https://github.com/WebKit/WebKit/blob/main/Source/WebKit/UIProcess/API/Cocoa/WKWebViewPrivate.h)
confirms the selector. Both setter and getter are checked with respondsToSelector,
and the disabled policy is read back. Missing/ignored SPI fails page creation
with an explicit error; it never silently falls back to known frozen output.

This is a deliberate compatibility tradeoff: it uses private WebKit SPI, isolated
to our owned capture view. It does not globally replace AppKit methods, inject
synthetic animation scheduling, or expose the source above the output. Future
macOS qualification must run the native fullscreen animation regression; a
selector check alone cannot guarantee unchanged scheduling semantics. A backend
with supported offscreen rendering remains an alternative architecture if this
SPI stops working. No CEF/GPUI dependency change is introduced.

The native regression failed before this change and passed with it. A real
fullscreen engine output continued animating after navigation (saved frame 381,
previously stuck at frame 0). Windows runtime qualification stays owner-deferred.

## 2026-11-13 — 060 — Display sink boundaries and identity

Decision: keep display placement/diagnostics/power in the winit engine; the GPUI
bootstrap reports its native display id to the supervised engine on open/move.
Control-display state is replayed before opening outputs. Settings/output routing
remain 065; expose guarded display-rebinding events and a persistence helper so
replug cannot revert a newer operator assignment. The engine does not open stores.

DisplayInfo keeps physical bounds for winit, and exposes logical sizes for stored
Electron bindings. Native names replace winit's synthetic macOS "Monitor #…".
Mac identity is CGDirectDisplayID, matching Electron. Windows uses Chromium 130's
adapter LUID/target id formatted as LowPart/HighPart/target and PersistentHash,
with its GDI device-name fallback; GPUI's HMONITOR is a separate native routing id.
References: Chromium 130 ui/display/win/display_info.cc and
ui/display/win/display_config_helper.cc. Runtime qualification is separate from
source/cross-build verification. No random monitor hash is an Electron identity.
Protocol encoding changes bump its version from 3 to 4.

Stage view is retained surface state carried as JSON (the existing StageView
serde model) and rendered by the 054 stage planner. Physical shape diagnostics
are separate from the existing persistent program bars/text/motion pattern slot.
Transient diagnostic windows expire independently (3 s identify, 5 s shape card),
and replacement cannot let an old expiry close a newer request.

060 diagnostics implementation: the transient identify/shape windows load the
reference HTML/CSS directly into a local Wry child (JavaScript disabled, default-src
none, escaped text, no navigation/download/popups). Wry already ships with the
engine; this preserves the exact translucent identify card and yellow grid rather
than adding transparent-swapchain behavior to the compositor. They are independent
of shared remote program pages and are destroyed with their timeout/window. Program
and stage display sinks continue through the vsync compositor. This replaces the
plan's proposed diagnostic raster comparison with direct HTML/content and native
window verification; no new dependency or remote load is introduced.

## 2026-11-13 — 061 — NDI output

Decision: the binding is spike 005's `libloading` binding, moved into `kairo-sinks::ndi` with the symbols resolved once at load and the runtime held by an `Arc` that every sender keeps. Struct layouts follow the NDI 6.3.2 headers and are checked by tests. The runtime is looked for in `KAIRO_NDI_RUNTIME` (a file), then beside the executable, then the bundle's `Frameworks` and `lib` folders, never a system path. No SDK file is in the repository: tests and runs use the runtime in the git-ignored `spikes/sinks/app-folder`. Packaging (prompt 110) must place the runtime in the app folder, ship `libndi_licenses.txt`, and carry decision 001's attribution (`NDI® is a registered trademark of Vizrt NDI AB`, the ndi.video link near the NDI settings). A missing or broken runtime disables NDI only, with the reason logged.

Decision: conversion. `kairo-compositor::uyva` converts the output's premultiplied RGBA frame target with `uyva.wgsl`, the integer maths of `uyvy.h` (Rec.709 limited, chroma from the pair average, nearest-neighbour scaling, alpha plane after the UYVY plane). The result is byte-identical to the CPU reference on Electron's self-test vectors, a 1080p gradient, 1080p to 720p, 4K to 1080p, and the UYVA alpha plane. One invocation writes four pixels, because storage buffers are written a word at a time, so the output width must be a multiple of 4 (1920 and 1280 are). Read-back uses 3 staging slots and at most 5 frame buffers: a burst beyond the slots, or a sender holding every buffer, drops the frame (counted as skipped) instead of queueing it. `map_async` completes in the compositor's existing wait after each frame, so no extra wait is added. Bind groups are made once per source texture.

Decision: the feed (`kairo-sinks::output::NdiOutput`) is `NdiService`. The sender is created on the feed's own thread with the video clock on. A failure keeps its reason (`senderError`) and is retried after 5 s, so a name clash with a second copy is cured without a restart. New pixels are sent at once; the current frame repeats every 100 ms; a send starts only after the last returned, so there is no backlog. Until the first frame, and after a clear, it sends 1920 × 1080 transparent black BGRA. Frames are declared 30000/1001 progressive. A UYVA frame of the wrong size is ignored with a warning. Audio blocks that fail `handleNdiAudio`'s checks are refused, and send failures are logged at most once every 10 s.

Decision: names and lifetime. The engine loads the runtime and starts the primary sender (`NDI_SENDER_NAME`, "Kairo Scripture") at launch. A surface whose `SinkSpec::Ndi` name is that name uses it; when the surface closes, the primary is cleared to transparent and stays on the network. Any other name gets its own sender, which is removed when the surface closes, and recreated when the name changes (the client sends `Kairo <name>`, as `reconcileNdi` does). An NDI surface is a 1920 × 1080 offscreen compositor output with capture on. Without a runtime, the surface reports `Unavailable { reason: "NDI sender unavailable" }`, Electron's text. Program sound from each NDI tap goes to the feed of the output that owns the tap.

Not met: p99 of the conversion and read-back against Electron's 2.2 ms. Measured with other sessions' builds running on the machine; see PROGRESS 061.

Affects prompts: 062, 065, 110.

## 2026-11-13 — 063 — Syphon output

Decision: the N-API shim `native/syphon/syphon.mm` is ported as a C ABI in `crates/kairo-native-sys/native/syphon/syphon_c.mm`, from Electron commit `5679dc5` (the last change to `native/syphon`; tree checked at `af978c5`, no uncommitted changes there). Re-check for drift at cutover. Syphon's vendored base server, client and messaging sources (`vendor/Syphon`, BSD, Syphon-Framework `f476167`) are copied unchanged and compiled in by `build.rs` with `cc`, as `build.mjs` did. Kept: one Metal device and queue made once; open falls back to the name "Kairo"; `publish` (a GPU blit from a BGRA IOSurface, dropped while a copy runs, a closed server refuses frames, close waits for the last copy); `publishPixels`; `stats`; `available`; the in-process test client. N-API pieces became plain C: errors are messages in a caller buffer, the done callback is a C function with a context. Added: `surface`/`publish_surface` (below), `metal_texture` (an IOSurface wrapped for wgpu), a cross-process client found through `SyphonServerDirectory` (tests and the `syphon_client` example), a test-surface helper standing in for gpu-frames' `createTestSurface`, and a guard that refuses a misaligned IOSurface instead of letting Metal abort (spike 005 finding).

Decision: zero read-back by drawing into the server's own surface. `SyphonServerBase newSurfaceForWidth:height:` returns the server's IOSurface, kept until the size changes. The sink wraps it as an `MTLTexture`, imports it into wgpu with `wgpu::hal::metal::Device::texture_from_raw` and `Device::create_texture_from_hal` (wgpu 30, as spike 005 described), and the compositor copies each finished frame into it with a 1:1 textured quad cleared to transparent (`Renderer::copy`), so alpha is kept and RGBA becomes BGRA on the GPU. After the frame's GPU wait, which the compositor already does before `Committed`, the sink calls `[server publish]`. This replaces Electron's second blit and its busy-drop: the compositor draws synchronously, so there is never a copy in flight. If wgpu is not on Metal or refuses the surface, the sink falls back for good to reading the frame back, swapping to BGRA and `publish_pixels`. Syphon clients may read the surface while the next frame is drawn into it, as with Syphon's own Metal server.

Decision: `kairo-compositor` gains a generic `FrameSink` (encode while the frame is recorded, finished after the GPU wait) attached with `Compositor::attach(output, Option<sink>)`. It does not know about Syphon. Prompt 061 adds a separate capture hook for NDI in the same file; the two are independent and must be merged by hand.

Decision: naming. `SinkSpec::syphon(output_name, index)` (in `kairo-engine-proto`) is `reconcileSyphon`'s rule: "Kairo <output name>", or "Kairo Syphon n" for an unnamed output. The engine uses `SinkSpec::Syphon { name }` as the server name verbatim; an `OpenSurface` with a different name stops the old server and starts a new one (clients see one leave and the other appear), and the surface's state is replayed into it.

Decision: the server is created and stopped on the engine's main thread, which runs the event loop: Syphon answers discovery requests through `NSDistributedNotificationCenter` observers on that thread's run loop. Frames are published from the compositor thread (Syphon's connection manager uses its own dispatch queue), as Electron published from a Metal completion thread.

Decision: Electron's `clearFrame` (a transparent frame when no layer is up) is kept as `SyphonChannel::clear`, and never starts a server. In the engine an empty program already draws a transparent frame into the server's surface, so no separate clear is sent.

Affects prompts: 061 (merge of the compositor hook and `kairo-sinks`), 065 (the surface manager builds `SinkSpec::syphon` and reports `status()` reasons), 091.

## 2026-11-13 — 062 — DeckLink output

Decision: Electron's `native/decklink` is copied unchanged into `native/decklink` here (commit `1067495`, tree checked at `af978c5`), with `prebuilt/decklink-helper` (universal) and `SOURCE_SHA256`, and `native/gpu-frames/ring.h`, which the helper includes. The helper's source and protocol are unchanged. The engine ships the prebuilt helper; a test re-computes `prepare-mac-natives.mjs`'s source hash over the copied files and checks it against `SOURCE_SHA256`, so a source edit without a rebuild fails. The SDK stays a build-time input only and is not committed.

Decision: the device service lives in the engine, not the operator, because the engine spawns the helper and writes the frame ring. `kairo-sinks::video_devices` ports `VideoDeviceService` (serialized reconcile, release-before-acquire, port ownership with "a live output keeps its port", faults retried only on refresh, cached ports in steady state, every reason string) and `provider-loader`; it runs on its own thread so a slow card driver (helper startup up to 10 s) never delays display windows. `kairo-sinks::decklink` ports the provider (`--list` with the `--version` fallback, the 15.3.1 floor, compatibility mode, driver notices, missing runtime or helper) and `createPlaybackSink`/the worker: one thread pipe per helper with at most one unsent frame (the newest), stderr kept for the reason, startup timeout with SIGTERM then SIGKILL after 4 s, and close by stdin EOF with SIGKILL after 4 s. The Node worker thread is not needed: frames never touch the engine's main thread.

Decision: protocol. `SinkSpec::Device { port_id, ring }` becomes `SinkSpec::Device { port_id, mode_id, key_fill }`: the engine makes the ring now, so the client no longer names it. Added `Command::RefreshVideoDevices`, `Query::VideoDevices` (the `VideoDeviceSnapshot` as JSON; empty until a card surface or a refresh starts the service) and `Event::VideoDevices` (sent whenever ports, providers or output states change). A card surface's binding is enabled unless the surface is parked; bindings are only sent when they change, so a reconcile with no change does no card work. Port re-matching by label: Electron has none for cards (ports are stable `decklink:<id>:<n>` ids), so none is added.

Decision: frames. A card surface draws offscreen at 1920 wide with the mode's shape (a 1280 × 720 mode draws 1920 × 1080), and the compositor's UYVA converter (061) scales it to the mode raster without the alpha plane; the UYVY plane is written into the frame ring the helper reads on the hardware clock. A ring slot the helper is still reading is skipped, not waited for. Without a ring (the helper refused it, or `shm_open` failed), the finished frame is read back as BGRA and sent down stdin, as Electron's CPU path. Key/fill stays refused ("DeckLink key/fill is not qualified in this build").

Decision: quit releases the cards first, bounded at 2 s (`HARDWARE_RELEASE_TIMEOUT_MS`); after that the engine exits anyway and the helpers, which unlink the ring themselves, are killed by their own 4 s escalation or by the engine's exit.

Decision: `KAIRO_ENABLE_DECKLINK=0` still turns cards off (kept until hardware qualification, prompt 110). `KAIRO_DECKLINK_HELPER` points the engine at another helper build; it is for development and qualification only. `kairo-sinks`' `decklink_sim` example uses it to stand in for a card on machines without one: it speaks the helper's command line, reads the engine's real frame ring and saves the newest frame as a PNG. It is not shipped. The driver-downloads link is Electron's `https://www.blackmagicdesign.com/support` (`decklink::DRIVER_DOWNLOADS`), for the operator's `openDriverDownloads`.

Not done here: the operator's `videoDevices` calls (`listPorts`, `status`, `onChanged`, `setTestPattern`, `openDriverDownloads`) are wired by 065/067 onto the engine's query, event and `SetTestPattern`; their inventory rows stay `todo` until then.

Affects prompts: 065 (the surface manager sends `SinkSpec::Device` and reads `Event::VideoDevices`), 067, 075, 110.


## 2026-11-13 — 064 — Capture to file

Decision: the recorder is `crates/kairo-native-sys/native/recorder.mm`, a C ABI port of Electron's `native/recorder/recorder.mm` at kairo-presenter `af978c5` (re-check for drift at cutover; Electron's copy is untouched). The recording logic is unchanged: the "current" frame, the fixed frame-grid clock on its own queue, transparent black before the first frame, the sound desk through `AVCaptureSession` on the host clock, AAC beside H.264/HEVC and 24-bit PCM beside ProRes, 5 s movie fragments, the first frame on an instant stop, and the end on the last frame slot. N-API is replaced by C functions with a completion callback for `stop`. `pushTexture` (Chromium's IOSurface) and its Metal copy are not carried: the engine's frames reach the recorder as BGRA read back from the compositor (`pushPixels`), and are scaled on the CPU when the recording is 720p. `kr_microphone_access` asks for the microphone as `askForMediaAccess` did. Built by `build.rs` with `cc` on macOS only.

Decision: the session (`CaptureService`) runs in the engine (`apps/kairo-engine/src/capture.rs`), which owns the frames: `Command::StartCapture { output, source_name, settings }` and `Command::StopCapture`, with `Event::Capture(CaptureStatus JSON)` on every change (protocol version 5). The UI process keeps choosing the source output (`captureSourceFor`) and the folder, and sends them. Every check and message is Electron's: unavailable off macOS ("Recording is only available on a Mac for now"), source not showing, folder not writable, under 2 GB free to start, microphone refused, audio input missing, the encoder's open errors without the `open: ` prefix. Once a second: writer errors stop the recording; every tenth check, under 512 MB free stops it and finishes the file; warnings for a source that is not showing ("recording black until it's back") and a struggling encoder (more than fps/2 late frames in a second). The default folder is ~/Movies/Kairo. Captures are not replayed after an engine relaunch (that file is lost), so `StartCapture` is not mirrored.

Decision: the tap. The compositor's single NDI capture became named taps (`Tap::Ndi` UYVA, `Tap::Record` BGRA), each with its own read-back ring, so a slow recorder never holds up NDI or the window. The recorder's frames go through a one-slot hand-off to a feeder thread (newest wins), so the encoder copy never runs on the compositor thread. The tap is re-attached whenever its output is rebuilt (window reopened, NDI reopened), so it follows its output; while the output is closed the recorder writes its last frame and warns. `Shutdown` (the client's quit and the orphan grace exit) finishes the file before the engine exits (`before-quit`), waiting at most 20 s.

Found and fixed: the engine's event loop did not wake for the once-a-second checks, so the disk check ran far less often than every 10 s; `CaptureFrameRate as u32` is the variant's position, not its number (use `.value()`).

Affects prompts: 065, 091.

## 2026-11-13 — 065 — Surface manager, lobbies and program service

Decision: new crate `kairo-output` holds `SurfaceManager` (a core actor, `SurfaceActor`; the mailbox serializes reconciles), `LobbyPlayer`, `ProgramService` and a test-only `FakeEngine` (feature `fake`, enabled only by its own dev-dependency). `kairo-core::engine` gains the object-safe `OutputEngine` trait, implemented by `EngineHandle` (and `Arc<T>`), so the manager and `kairo-orchestrator::engine::EngineSurfaces` (now generic, default `EngineHandle`) run unchanged on the fake and the real engine. Settings and the media library are reached through `OutputSettings` (implemented for `Mutex<SettingsStore>`) and `LobbyMedia` (implemented for `Mutex<MediaService<S>>`); prompt 067 wires the real instances.

Decision: the engine already places windows, claims displays (program before stage), rebinds replugged displays, parks lost cards and blocks display sleep (060, 062). The manager does not repeat that work: it resolves displays client-side only to keep Electron's status strings ("No display chosen", "Display in use by another screen", "Display not connected", "Display in use by a program screen", "Live", the rehearsal text, the two playlist texts, "Screen window failed to open" and "Stage window failed to open" when the engine reports the sink `Unavailable`), and sends the resolved id in the binding. A re-matched id is persisted with `persist_display_rebinding` (only while the stored id is still the old one) both at reconcile and on `Event::DisplayRebound`, and `SettingsRewritten` is emitted so the operator re-reads settings. The power blocker stays in the engine (060).

Decision: "no engine work on a no-change reconcile" is met by remembering, per surface, the config, layers and stage view last sent, and opening a surface only when its spec changed. A program change sends only changed layers (Electron's `applyLayers`). A timer or a prop carrying a timer view includes `now`, so while one is on screens each program change resends that layer, as Electron did.

Decision: `nextPaint` waits are `EngineHandle::wait_frame` on `Event::FrameCommitted` (already used by `EngineSurfaces` for page turns and documents); the manager itself never waited for a paint.

Decision: NDI and Syphon renames destroy the surface and open a new one (Electron's `dropNdi`/`dropSyphon`: the new sender starts blank), although the engine could keep the content.

Decision: card lobbies start when the engine's card snapshot says the output is ready (`Event::VideoDevices`), instead of Electron's re-run of the whole reconcile on every card change; card acquisition and release already happen in the engine on their own thread.

Decision: `program.ndiAudioSupported` is `Query::NdiAudioSupported` → `Reply::NdiAudioSupported(bool)` (protocol 6), answered true when the engine loaded the NDI runtime: every native sender carries audio, so Electron's adapter check reduces to the runtime being present. `kairo_output::manager::ndi_audio_supported` returns false when the engine does not answer. `program.pickImage` (a file dialog) and `program.onWebNavigated` forwarding are UI/wiring (067/089); `videoDevices.openDriverDownloads` opens `kairo_sinks::decklink::DRIVER_DOWNLOADS` from the shell (067).

Affects prompts: 066, 067, 075, 076.

## 2026-11-13 — 070 — Extend the 007 waiver to the design system

Decision: The owner chose to start prompt 070 with prompt 007 still `blocked`, under the same waiver given for prompt 008. 007's tracker status is unchanged, because its work is not complete.
Accepted for 070: 120 Hz pacing not proven; grid open under load not proven; real Yoruba IME, macOS 11, Windows and Linux not run; Electron shell side-by-side not captured.
Carry forward: 070 re-measures UI frame pacing, as the 008 amendment requires. Closing 007 itself still needs the idle-machine rerun, a real IME, macOS 11, Windows and Linux runs, and the Electron shell capture.
Affects prompts: 007, 070.

## 2026-11-13 — 070 — Design system (`kairo-ui-kit`)

Decision: tokens follow `index.css` where it disagrees with DESIGN.md's hex list (dark zinc 24 24 27 … 63 63 70, not #181818 … #404040), because the prompt protocol makes the current source the specification. Electron's `app-theme` and `operator-visual-system` tests are stale (they expect amber and `--surface: 12 12 12`) and were not used.
Decision: Kit components (`Button`, `Input`, `Select`, `Slider`, `Switch`, `OtpInput`, `AlertDialog`, `PopupMenu`, tooltips) are used directly, with Kairo tokens mapped onto Kit's theme (`theme::apply_tokens`). Kairo adds only what Kit lacks: `SegmentedControl` (also the tablist used where Electron hand-rolls `role="tab"`), `NumberField` (Shift ×10 on keys, buttons and label scrub), `ZoomControl`, `PrefGroup`, `SelectionBar`, `TrafficLights`, `MovableDialog`, `FrostedPanel`, `LibrarySection`, `Marquee`, `CodeField`, `RailItem`, `DragPill`, `KairoMark`.
Decision: not ported, no importers (confirmed by reading): `badge`, `field`, `input-group`, `resizable`, `scroll-area`, `secret-key-field`, `skeleton`, `tabs`, `toggle-group`; also `label`, `separator`, `textarea` and `toggle`, whose only importers are those unused files. `formatTimestamp`, `truncate` and `debounce` in `lib/utils.ts` have no importers. `downloadFile` is a save dialog, left to the screens that save. `paneSizing.ts` has no importers.
Decision: fonts. The variable Source Sans 3 subsets register on macOS as "Source Sans 3 ExtraLight", so a request for "Source Sans 3" fell back to the system font. `crates/kairo-ui-kit/tools/instance-fonts.py` (fontTools 4.55.3) instances 400, 500, 600 and 700 from each subset, merges the five subsets per weight and names them family "Source Sans 3" (OFL kept beside them). Prompt 052's engine fonts are unchanged.
Decision: icons. `tools/generate-icons.cjs` renders only the 117 names the renderer imports from `@/icons` (108 distinct glyphs; the rest are aliases) from Hugeicons free 4.4.0 (MIT) and Phosphor 2.1.10 (MIT), plus the hand-drawn Bible, in regular and fill weights. Licences are in `assets/licenses`.
Decision: `window.confirm` and `window.alert` become `components::dialogs::{confirm, alert}` on Kit's alert dialog. Every message is kept word for word (`dialogs::messages`, tested against the browser text). The confirm button names the action instead of OK. The text-field right-click menu is Kit `Input`'s built-in native menu (Cut, Copy, Paste, Select All); headless tests cannot open a native menu, so its commands are tested through their key bindings.
Decision: `MovableDialog` is its own overlay (scrim, focus trap, Escape, drag handle, clamp of 48px edge and 8px top), because Kit's `Dialog` is always centred and has no offset.
Decision: frost has no backdrop blur. GPUI cannot blur behind a surface inside a window; `.glass` and `.modal-glass` keep their tint and alpha.
Decision: motion keyframes are data (`motion::ALL`, 28 rules from `index.css`, `tailwind.config.js` and `onboarding.css`). `animate` plays opacity and translation and shows the end frame under reduced motion. Scale, rotate, blur, brightness and background sweeps are exposed but not applied, as GPUI has no general transform or filter.
Decision: `gpui-fast` stays off for the app. Only prompt 089 embeds a web page in the operator window, and the kit works on either backend; 089 decides.
API difference: the pinned Kit has no public `aria_disabled`; disabled custom actions use Kit `Button` so assistive technology sees the state. Custom elements must call `.test_support()` after `.id()` to be visible to UI-test queries. Kit's alert dialog exposes no accessible label for its title.
Lock: `test-support` pulls `gpui-fast`, whose exact `taffy =0.15.0` and `unicode-properties =0.1.3` pins conflicted with the locked 0.15.1 and 0.1.5. Both were moved down to those versions (within `gpui-pre`'s `^` ranges), which unblocks UI tests workspace-wide. This supersedes the 025 note that left the Taffy lock alone.
Affects prompts: 052, 071 and every UI prompt, 089.

## 2026-11-14 — 066 — Operator preview transport and thumbnails

Decision: the live preview is what the engine drew, not a second render. `Command::SetPreview { output, preview: Option<PreviewTarget { path, width, height, fps }> }` (protocol 7) is kept in `SurfaceState`, so the client's replay restores it in a relaunched engine. The UI process (`kairo_core::preview::PreviewHub`) makes one owner-only file per output in an owner-only temp folder (`kairo-preview-<pid>-<uuid>`, removed with the hub) and the engine maps it (`kairo_engine_proto::preview::PreviewMap`): three seqlocked slots of up to 1280 × 720 straight-alpha BGRA, newest-slot index and a frame counter. Readers take only the newest complete frame; a torn or in-progress slot is skipped; a writer killed mid-frame is tolerated, as in spike 002's map. Several views of one output share one map; the engine is asked for the largest size and fastest rate held (capped at the map and 60 fps). The last view letting go sends `preview: None` and removes the file. `PreviewSubscription::ensure` re-sends the request once a second if the surface's state lacks it (a destroyed and reopened surface).

Decision: in the engine the preview is a compositor tap (`Tap::Preview`). The converter's new mode averages up to 8 × 8 evenly spaced source pixels per output pixel and un-premultiplies, because GPUI draws images with straight alpha (`swap_rgba_pa_to_bgra` and the polychrome sprite shader in gpui-pre 0.3.12). It is byte-identical to `uyva::preview_reference` on Metal. The size keeps the output's shape inside the requested box (`preview::fit`, width a multiple of 4). The tap converts at most once per `1/fps`; a frame held back by the rate asks for a draw when the interval ends, so the picture at rest is always sent. A still program sends nothing.

Decision: taps now also run when a window frame cannot be presented (occluded). Before, a rehearsal window behind the operator window fed no taps, so its preview, NDI feed and recording froze while covered. This changes 061/064 behaviour for occluded windows (they now keep sending, at the 100 ms retry rate).

Decision: the GPUI side is `kairo_core_gpui::preview::LivePreview`. Each new frame becomes a `RenderImage`; the image it replaced is evicted with `window.drop_image` one frame later (`FrameSwap`), and everything held is evicted on release. "No preview work when the page is hidden" is decided by painting: a view not painted for 1 s (another page, a minimised or covered window) drops its subscription; it is taken up again on the next render. While shown but still it repaints every 250 ms to keep that signal. The requested size follows the painted device-pixel size in 32-pixel steps. A checkerboard sits behind see-through parts, as `ScaledOverlayPreview`'s `backdrop`, switchable with `set_backdrop`. Choosing the output, the "Nothing is live" / "No screens on" / "Preview only" frames, the web layer, document previews and video transport stay with the operator panel (076, 078, 081): the view shows black until its first frame.

Decision: thumbnails (`kairo_core_gpui::thumbnails::Thumbnails`) hold the slide's text layer only, built with `SlideScene::build` at 1920 × 1080 (Electron's fixed thumbnail frame, so auto-fit does not treat the card as the frame) on `RasterPool` workers, reduced to the card size there and un-premultiplied. The card draws the theme background beneath. In Electron a video background was already a still. Requests carry a key computed once (`ThumbnailRequest::new`). Images are an LRU keyed by the scene's content hash; two requests with the same scene share an image; an evicted image is removed from every window's atlas (`App::drop_image`) and redrawn when asked for again. `get` answers `Pending` (placeholder) until the worker finishes; workers put results in a queue that a GPUI timer takes every 16 ms only while jobs are outstanding, because GPUI's scheduler must not be woken from foreign threads (its test scheduler rejects it). A scene that cannot be built stays `Failed` (placeholder).

Affects prompts: 067, 076, 078, 081, 086.

## 2026-11-16 — Owner — 109 — Parity audit closed with a waiver

Decision: the owner closed prompt 109 after a partial audit, to move on. Done:
- Sync queue emptied at Electron `af978c5` (SYNC.md).
- A01, A02, A14 and A15 PASS on two displays, and A16's replug part PASS (`evidence/109/displays.md`).
- The owner's side-by-side check of sign-in and Screens raised FAILs, written up as prompt 113.

Waived: every other row of `04-feature-parity-matrix.md`, the API items in `api-inventory.md`, and the A10 stage-display side-by-side check, none of them walked. Prompt 113 must walk them before cutover (112).
Why: the owner judged the Electron UI itself due for cleanup, and chose to fix the UI in 113 rather than hold 109.
Affects prompts: 109, 113, 112.

## 2026-11-16 — Owner — 108 — Engine RSS 5.06% waived

Decision: the owner accepted prompt 108's clean-run engine RSS growth of 5.06% (627.1 to 658.8 MiB, slope +0.57 MiB/min, cycle-2 baseline) against the under-5% budget, and marked 108 done. Why: 0.06 points is inside the sample-to-sample swing seen in earlier runs (about 170 to 640 MiB). Handles and client RSS were flat, and all 12 chaos events recovered. Follow-up: before release, loop the soak for longer, and investigate engine memory if the growth persists. A slow leak is not ruled out.
Affects prompts: 108, 109.

## 2026-11-16 — Owner — 108 — Leak baseline starts at the second cycle

Decision: in prompt 108, RSS and handle growth are measured from the first minute of the second 5-minute service cycle (300–360 s) to the last minute. This replaces "after the first 2 minutes". Why: the 2-minute baseline came before the first document segment (180 s), so first-time page, decoder and cache loads counted as growth. The first clean run measured +14.2% engine RSS that way. The series peaked at about 6 minutes, then stayed flat or fell. Limit: a 10-minute run gives only about 4 minutes of comparison window; a slower leak is not ruled out.
Affects prompts: 108.

## 2026-11-14 — Owner — Long runs are capped at 10 minutes

Decision: the owner limited every soak, spike and endurance run in the remaining prompts to 10 minutes. Prompt 006's web-host run is 10 minutes instead of 20; prompt 108's scripted service is 10 minutes instead of 4 hours, still run twice, with RSS growth measured after the first 2 minutes instead of 30. Prompt 110 already used 10-minute runs. Prompt 091's "2-hour service" is the size of transcript the view must handle, not a run length, and is unchanged. Results from these shorter runs are reported as 10-minute results; a leak slower than they can show is not ruled out.

Affects prompts: 006, 108.

## 2026-11-13 — 071 — Start the shell before 067

Decision: The owner chose to start prompt 071 with prompt 067 still `todo`. The shell does not fake 067's work. Where 071 meets 067 (startup order, settings side effects, native dialogs, and the focus effects: pull org secrets throttled, wake uploaders and usage), the shell raises typed events through a boundary that 067 implements. Until 067 lands, the app handles those events with the services that already exist and leaves the rest unhandled, recorded as not verified in PROGRESS.md.
Affects prompts: 067, 071.

## 2026-11-14 — 071 — Application shell

Decision: `ui-shell` owns the operator window: rail, header, routed workspaces (placeholders name their prompts: 076, 078, 080, 085, 086, 089, 090), overlay windows (Settings 074, Screens 075, Props 088, Capture 091, song quick open 080), quit question, What's New, update pill and toast, activation chip, keychain notice, feedback popover and the panic fallback. Services reach it through `ShellState` (fed by the app) and `ShellRequest` (raised by the shell). Until prompt 067 composes the services, an item whose data has not arrived renders nothing, as its Electron component does before its first status, and requests other than What's New, external links and contained panics are logged as waiting.
Decision: menus follow `setupApplicationMenu`. Dropped as browser-only: View → Reload, Force Reload, Toggle Developer Tools. Zoom In, Zoom Out and Actual Size drive a persisted interface scale (Chromium's zoom steps) stored in `ui-scale.json` in the user-data folder, not `settings.json`, so Electron never reads an unknown key.
Decision: customisable workspace commands are matched in the shell's key handler against `display.shortcuts` and dispatched as `WorkspaceCommand`; they are skipped while the focused element's key context includes `Input`. They are not GPUI key bindings, because rebinding the keymap would clear Kit's text-field bindings. ⌘F song search fires in text fields, as the original's capture listener does.
Decision: closing the window asks the quit question and confirming quits, as `respondToQuit` → `app.quit()` does in the source. Requirement 9's "closing leaves the app running" describes the `closed` handler, which the source only reaches when a close is allowed without quitting; the Dock reopen (`on_reopen`) is implemented for that case.
Decision: dialogs use the opaque `--glass` tone, not `.modal-glass`'s 78% tint: without backdrop blur (GPUI cannot blur inside a window) the workspace showed through the text.
Decision: the panic boundary contains panics while the shell builds a workspace's element and allows three rebuilds within 60 s. A panic inside an entity view's render is outside the shell's call stack, and unwinding through the macOS run loop is undefined, so those go to the existing crash hook (025).
Decision: the changelog is embedded from `crates/ui-shell/data/changelog.json`, written by `tools/embed-changelog --regenerate` (Node imports the TypeScript directly). Builds never read the sibling repository.
Decision: 070's icon inventory missed Phosphor glyphs the renderer imports directly from `@phosphor-icons/react` (the rail, What's New, feedback). The generator now includes them as `Ph<Name>`: 125 distinct glyphs. The rail item always uses the filled weight, as `AppShell` does.
Not ported: `packaged-locales.test.ts` (a Chromium Blink locale in the Windows package; no equivalent).
API difference: GPUI has no Dock-icon call; `apps/kairo/src/dock.rs` uses `objc2-app-kit` `setApplicationIconImage`. `cx.set_app_identity("com.kairo.app", "Kairo")` sets the Windows AppUserModelID. `on_reopen` is on the application, not the app context.
Affects prompts: 067, 070, 072–091.

## 2026-11-14 — 067 — Application wiring

Context: Codex built most of this prompt in the 071 checkout and stopped at a usage limit with `rustfmt` failing and `kairo-core/tests/scripture_traces.rs` referring to a module that did not exist. Claude reviewed it against the prompt and finished it.
Decision: `kairo-core` holds the ordered rules without depending on service crates: `application::ApplicationLifecycle` (startup steps, background tasks, resume and focus), `settings::SettingsWiring` (dispatches the effects `SettingsStore::apply_set` returns, in source order, outside the store lock), `dialogs::dialog_request` (every picker's title, button, filters and default path), `media_access` (picked-media allowlist and folder realpath rule) and `scripture_traces::ScriptureTraces` (the trace service: ids, clock, structured events). `apps/kairo` composes the services (`services::ApplicationServices`, `Startup`), product effects (`actions::Actions`) and dialog or OS handlers (`handlers::Handlers`). Native panels are AppKit `NSOpenPanel`/`NSSavePanel` on macOS (`apps/kairo/src/dialogs.rs`).
Decision: to keep source order, services that used to start on construction gained a local open and a separate start: `LicensingService::open_local`/`start`, `CloudSession::open_local`/`start`, `EngineHandle::new`/`start` (supervision starts at the NDI step), and `init_surface_manager` apart from reconcile. Licence and session start without awaiting the network.
Decision: the detector gives the general-corpus quote search first refusal before the managed model fallback (`final_before_model`, `interim_before_quote`, `submit_model_work`), and stale model work never re-enters the queue.
Inventory: rows replaced here, by function. settings: `SettingsWiring::{get, get_all, set, broadcast_changed, test_api_key}`. scripture: `Orchestrator::{approve_suggestion, dismiss_suggestion, present_scripture_directly, retranslate_suggestion}`, `Actions::{set_auto_mode, set_confidence}`, `LivePlanService::{state, set_plan, on_change}`, `ScriptureService::on_suggestion`, `ScriptureTraces::{recent_with_metrics, mark}`. media: `Actions::{push_media, clear_media, seek_media, set_media_paused, set_media_playback, choose_media_folder, import_media, add_backgrounds}`. documents: `Actions::{push_document, control_document, document_playback}`. lyrics: `Actions::push_lyrics`. workspace: `Handlers::{choose_workspace, adopt_media, reveal_workspace}`. capture: `Handlers::{choose_capture_folder, reveal_capture, start_capture, stop_capture, capture_status, capture_audio_inputs}`. tracks: `Actions::{choose_tracks_folder, import_tracks}`. `ndi.pickOverlayMedia`: `Actions::pick_overlay_media`. `program.pickImage`: `Actions::pick_program_image`. `videoDevices.openDriverDownloads`: `cx.open_url(kairo_sinks::decklink::DRIVER_DOWNLOADS)`. app (071 shell): quit, What's New, import and full-screen rows. Left `todo` with their owners: `lyrics.readClipboard` (080), `ndi.getStatus` full per-output shape (061; readiness is composed for the header), `program.onWebNavigated` (089), `app.bootstrap` and `app.onBootstrapProgress` (072).
Not wired: trace recording by the orchestrator (039 left it open), so `recentTraces` returns what callers mark.
Affects prompts: 039, 061, 071, 072, 080, 089.

## 2026-11-14 — 071 — Finish the shell

Decision: a contained workspace panic is recorded as Electron's `render-process-gone` was: `Telemetry::record_crash(NativeCrash::Window, CrashReason::Panic, 0)`, a renderer crash of the main window that also counts `renderer_gone`. The panic text stays in the local log; crash reports carry only fixed labels. (`main.rs` had logged the request without forwarding it, so the telemetry branch never ran.)
Decision: launch to first paint is logged by the app (`[Startup] first frame presented`, `first_paint_ms`): from the start of `main` to the callback after the window's first frame, which runs once that frame is presented. `KAIRO_REPORT_STARTUP=1` also prints it to stderr for measurement.
Measured (release, M2 Pro, placeholder build URLs as prompt 054 used, a fresh `HOME` per launch, `KAIRO_BIBLE_DB` pointing at a copy of the bundled Bible because an unbundled release build has no Resources folder): 1724.0, 1051.6, 1041.7, 1132.1, 1198.3 ms; median 1132 ms. Electron's baseline (`baseline.md`) is a median of 3661 ms to the first interactive onboarding or sign-in screen; the milestones differ, as the native shell paints before bootstrap finishes and 072 owns the launch screen.
Accepted by the owner as owned elsewhere or as known limits: the Live, Media and Screen Themes popover contents (075, 083, 087, 091); the startup warning bar and splash (072); page sidebars (each page's prompt); no Electron side-by-side (isolated profile stops at onboarding, as for 070); Windows and Linux not run.
Affects prompts: 032, 071, 072.

## 2026-11-14 — 072 — Launch screen

Decision: the splash lives in `ui-shell`. `bootstrap::Launch` is the state from `useBootstrapStore` and `App.tsx` (phase, progress, errors, warning dismissed, splash held, intro done), with no clock; `launch::LaunchScreen` draws it; `Shell::begin_launch` starts it after the app subscribes, emitting `ShellRequest::Bootstrap { generation }`. The app answers with `ApplicationServices::bootstrap` (`run_bootstrap` over the same nine local reads as `app:bootstrap`, translations offline) through `UiEvent::Bootstrap`. Each load has a generation, so late progress or a stale result from before a Retry is ignored, as Electron unsubscribes after each load. `ShellOptions::splash` turns the splash off (tests, which then open at once).
Decision: timings are Electron's: `SPLASH_MIN_VISIBLE_MS` 2,500 under reduced motion; otherwise the splash closes 450 ms after the intro has played out (2,550 ms, the dot landing), with a fallback at max(2,500, 2,550 + 2,000) = 4,550 ms. Electron's `animationend` becomes "a frame was drawn past 2,550 ms", so a hidden window that draws no frames falls back to the timer as it does in Electron. Native startup finishes inside the splash (bootstrap measured at 3.1–5.3 ms, debug, three launches); no delay beyond Electron's was added.
Decision: failure paths, all from the source. A failed resource opens the app with the warning bar (`describeBootstrapWarning`'s text and labels, Retry, dismiss labelled "Dismiss startup warning"); a failed request opens it with a settings error carrying the message, as Electron's catch does. Native has no IPC to reject, so a load whose task ends without a snapshot (dropped or panicked) answers "The startup request did not finish." from a drop guard. Retry clears errors and the dismissal, resets progress to "Getting everything ready…" and shows the progress screen until the new snapshot; a failed retry keeps the last snapshot. Progress shows only when loading outlasts the hold; percent is rounded and clamped, 0 for a total that is not positive or not a number. Loader timeouts and panics were already handled by `run_bootstrap` (025).
Decision: the lockup (`kairo_ui_kit::launch_mark`) is drawn from `lockup-paths.ts`'s path data on a canvas, every frame a pure function of time since the splash appeared, following `AnimatedMark.tsx`'s timeline and per-keyframe easings. Differences, because GPUI has no filters or SVG masks: the letters rise without their 24 px blur; the tiles' 36 px glow is four widening copies of the tile at falling opacity; the sheen is the same skewed triangular band drawn in 4 px clipped cells over the lockup at rest. Text tracking (0.16 em, 0.3 em) is laid out a character at a time, and "Powering up"'s light band colours each character by its distance from the band, since GPUI cannot clip a gradient to text. The progress bar's 200 ms width transition is not animated.
Decision: Electron's fading copy of the splash is a fresh mount at `opacity-0`, so it never shows; what the operator sees is the app frame's `animate-fade-in` (400 ms). Native draws only that fade, each time the app opens from the splash (Retry included). The source scopes reduced motion to the `splash-*` and lockup classes, so the fade plays under reduced motion too.
Decision: reduced motion is read from AppKit (`accessibilityDisplayShouldReduceMotion`) at launch and whenever the window regains focus, and set on both GPUI (`App::set_reduce_motion`) and the Kairo theme. `KAIRO_REDUCED_MOTION=1` or `0` overrides it for testing. Windows and Linux are not read yet and report motion allowed. Under reduced motion the lockup is the finished lockup, the copy is at rest, "Powering up" is plain `--fg-500`, and no animation frames are requested.
Decision: two tokens added to `kairo-ui-kit`: `warning_text` (Tailwind yellow-300, which Electron uses in both themes) and `splash_boot` (`rgb(113 113 122)`, the band's muted ends). `kairo_core::bootstrap::neutral_snapshot` is public so tests and callers can build snapshots.
Not ported here: `hydrateIntegrations` (API.Bible authorization, audio device labels, NDI status after the splash) has no native consumer yet; its rules `shouldShowApiBibleWarning` and `isAuthorizationFailure` are in `ui_shell::bootstrap` for the scripture, audio and screens prompts (078, 074, 075). Applying the snapshot's theme and hydrating stores is unnecessary in native: the window opens with the theme from the same settings store, and services hold their own state; the snapshot is kept on `Launch` for 073 (onboarding and account gate).
Affects prompts: 073, 074, 075, 078.

## 2026-11-14 — 076 — Operator workspace, live rail and monitor

Decision: `ui-operator` holds the frame. `OperatorWorkspace` lays out the transcript panel, the page content with the media dock under it, and the live rail at full height (`BoothWorkspace`). The panels' contents arrive through `set_transcript`, `set_content`, `set_dock` and `LiveRail::set_toolbox` (077 transcript, queue and program panels; 083 dock; the booth toolbox). Until those land, the slots are empty. `Shell::set_workspace(route, view)` replaces a route's placeholder; `apps/kairo` hands the Operator its workspace. A panic inside an entity view's render is outside the shell's panic boundary, as 071 recorded.
Decision: the workspace reads its state through `OperatorBackend` (`snapshot`, `run`, `video_time`, `preview_hub`). `apps/kairo/src/operator.rs` implements it over the services: program state, media and tracks libraries, overlay outputs from settings, and a live document from the engine's own `SurfaceState::document_mode` (Electron kept `liveDocumentPreview` in renderer state). Requests run on the service runtime and report back; a failure is logged and shown under the monitor (Electron only logged it), and the strip is enabled again.
Decision: one 250 ms workspace clock (`workspace::TICK`) re-reads the live state and asks the engine for the video clock while the transport shows. Electron got the same data by subscription and from the preview `<video>`'s own events; here the snapshot is read on the tick and once more right after a request finishes. No control runs its own interval. The 066 `LivePreview` keeps its own frame poll; that is the preview transport, not a clock.
Decision: the monitor is the engine's frame (066), so `PreviewBody`, `ProgramLayersPreview`, `VerseThemePreview` and the web mirror have no counterpart: whatever the output draws, including program layers, the clock and a live web page, the monitor shows. Empty states keep Electron's copy: "No screens on" (no enabled output to read), "Nothing is live" (no text, background or program layer), and, new, "Preview unavailable" when the preview map could not be created. Difference: Electron drew a disabled output's preview from its theme; the engine has no surface for a disabled output, so the monitor shows "No screens on" with "Preview only — this screen is off." under it.
Decision: `Query::VideoTime { output }` → `Reply::VideoTime(Option<VideoTime>)` (`current_time`, `duration` 0 until known, `ended` as `__videoEnded`). `PROTOCOL_VERSION` is 8. The transport's readout and scrubber need the clock that `onTime` gave Electron. The scrubber is a Kit `Slider` over 0–1000 mapped onto the duration; seeks are clamped as `LiveOutputPreview.seek` clamps them.
Decision: widths are stored in `ui-state.json` under `operator-preview-width` and `operator-transcript-width`, as strings, as `localStorage` held them, so a profile moved back to Electron keeps them. Legacy defaults (320, 240) read as unset. The rail width is a GPUI global shared by every page that shows the rail. Difference: the transcript width is stored on every change, not only on pointer up; the store coalesces bursts into one write (022). The remembered frame sizes (`kairo.frame.screens.v4`, `kairo.frame.props`, `kairo.frame.settings.v2`) are read, clamped and written by `layout::{read,write,clamp}_frame_size`, in Electron's JSON text; the windows that use them are 075, 088 and 074's.
Decision: the resize edges are Kit `resize_handle`s with arrow-key resizing by 16 px on a focusable band; the drag listeners go on the handle, whose band blocks the pointer from its parent. The transcript edge sits inside the panel's right edge, because the page content drawn after it would otherwise cover its outer half.
Not ported: `OperatorToolbar`'s legend. Its hover group has no visible trigger, so it shows nothing; its live-plan subscription belongs to 077. `live-ui-rendering.integration.cjs` tests that the update pill does not repaint for transcript telemetry, which is the shell's (071).
Affects prompts: 066, 071, 074, 075, 077, 083, 088.

## 2026-11-14 — 073 — Account, onboarding, coach tour

Decision: `ui-account` holds the views (`AuthPanel`, `AccountGate`, `VerifyEmail`, `ActivationDialog`, `LicensePanel`, `OnboardingWizard` with `TourSlides` and the display picker, `CoachTour`) over one `Account` model (session, pairing, licence, onboarding, settings, displays, local zone). The app implements `AccountBackend` (`apps/kairo/src/account.rs`) over `CloudSession`, `LicensingService`, the onboarding actor and `SettingsWiring`. Every call runs on the service runtime and answers through a oneshot, so views await it on GPUI's executor. `account::feed` forwards the published session, pairing, licence, onboarding and settings snapshots as `UiEvent::Account`, current value first, plus a seed of the licence, displays and settings at start.
Decision: `AccountFlow` ports `App.tsx`'s ordering. Setup is offered first (`shouldOfferOnboarding`) and stays up until it dismisses itself; the sign-in wall (`shouldOfferAccountGate`) follows; a reset onboarding state clears this launch's dismissal; tips start after setup closes (`coachTourFor`). The wall drops running tips. The shell draws the flow through `Shell::set_front`, over the booth and under the quit question, only once the launch screen is gone. Difference: Electron did not mount the booth behind the wall; here it stays mounted, covered by the opaque wall, so workspaces keep their state across a sign-out.
Decision: coach targets are anchors, not selectors. `kairo_ui_kit::coach::coach_anchor(name)` is an absolutely positioned marker inside the control that records its bounds each paint and changes no layout. Placed: the workspace rail (`nav[aria-label="Workspaces"]`), the Live/Clear group (`.header-live`), Screens (`.header-statuses > :first-child`), the live rail's preview block (`[data-tour="live-preview"]`). `transcript-start`, `detected` and `automation` belong to 077's panels; `scripture-query`, `lyrics-new` and `documents-import` to 078, 080 and 082. Until those prompts add their anchors, a missing target is skipped after 1.5 s, as `CoachTour.tsx` does.
Decision: `StepChurchProfile`, `TimezoneSelect`, `StepDone` and `StepApiKeys` are not ported: the wizard's steps are `welcome`, `account` and `output` (`ONBOARDING_STEPS`), and nothing imports those files. `timezone-search.ts`, `device-code.ts`, `return-path.ts` and `auth-state.ts` were ported to `kairo-domain` by 030 and 031.
Decision: the wizard's focus trap takes its element's id, so the trap carries `onboarding-wizard`; the dialog was otherwise unfindable and unannounced (the UI test caught it). With `test-support`, `.test_support()` changes an element's type, so helpers return `impl Styled + IntoElement` rather than `Stateful<Div>`.
Affects prompts: 074 ("Show tips again", "Run setup again", licence panel), 077, 078, 080, 082.

## 2026-11-14 — 074 — Settings

Decision: `ui-settings` holds the window. `SettingsWindow` is a `MovableDialog` (760×640, minimum 640×480, the remembered `kairo.frame.settings.v2` size read and clamped by the app) with the System Settings layout: a glass sidebar (traffic lights, search, account card, panes) and title bar that both drag the window, the back/forward capsule, and `PrefGroup` boxes. Panes are `impl SettingsWindow` blocks in `panes/`; the rules (navigation, history, number fields, meter, labels, pack and trace text, shortcut conflicts) are in `model` and tested without a window. The app's side is `SettingsBackend`, implemented by `apps/kairo/src/settings.rs` over the services; one backend per app subscribes once to the updater, the slide converter and Bible install progress and fans pushes out to each open window. The shell builds the window through `Shell::set_overlay_builder`, so `openSettings(section)` opens at that pane.
Decision: `display.fontSize` and `display.transcriptionFontSize` take effect, which they did not in Electron (no renderer code read them). `kairo_ui_kit::theme::DisplaySizes` holds the interface size, the transcript size and the View menu's zoom; the interface rem is `fontSize × zoom`, so every rem-sized text follows a saved size, and an appearance change keeps it. The app applies the stored sizes at launch and whenever saved settings arrive. The live transcript reads `DisplaySizes::transcription_px`; the transcript panel is 077's, in progress in another session, so the transcript size is applied but not yet drawn by anything.
Decision: `settings:testApiKey` stays `SettingsWiring::test_api_key` (067): `{ ok: false, message: "Provider credentials are managed in your church account." }`. No pane calls it. `secret-key-field.tsx` is imported by nothing and is not ported.
Decision: saves follow the source. Dark mode, the privacy switches and a valid gloss colour are written at once; General writes `display` and `lyrics`; Scripture writes `scripture`; Verses on screen writes only `template`, `showTranslation`, `showVerseNumbers` and `maxVerses`; Audio writes `audio` and merges `outputLabel` into the stored `presentation`; Shortcuts writes `display` with the bindings. A failed save shows "Couldn’t save: …" instead of Electron's silent unhandled rejection, and "Saved" shows only on success. A failed first read shows the defaults with a notice (Electron showed the startup snapshot without saying so).
Decision: the input meter runs only while the Audio pane shows, through `kairo_stt::capture::start_capture` on the selected input (`resolveCaptureDeviceId`); dropping its handle stops capture. The device lists are re-read every 3 s while the pane shows, standing in for the browser's `devicechange` event.
Differences: the sidebar search glyph is `ScanSearch` (Kairo's icon set has no plain search glyph). The meter's green, amber and red are the `Green` and `Amber` hues and `destructive`. Switches use Kit's accent, not `teal-500`. The Saved button turns primary instead of `#2f6b3a`. Removing a Bible download asks with Kairo's `confirm`; a removal error shows on the row, not inside the dialog. Cancelling the folder picker shows no note (the source said "Workspace moved." even when nothing moved). The English flag beside the library heading is not drawn. The frame keeps its remembered size but has no resize edges. The "Integrations" heading never showed (`INTEGRATION_SECTIONS` is empty) and is not ported. Space or Enter on a focused shortcut button both records the key and, in GPUI, activates the button; the activation that follows a recorded key is ignored so it does not start recording again.
Affects prompts: 067, 071, 073, 075, 077.

## 2026-11-14 — 077 — Operator booth: transcript, detections, queue and program panels

Decision: `ui_operator::booth` holds one `Booth` model (every flow of `Operator.tsx`'s second half) and the views read it: `TranscriptPanel` (with `ServicePanel`), `DetectedPanel`, `Toolbox` (`QueuePanel`, `TimersPanel`, `MessagesPanel`, `PropsPanel`, `VideoPanel`, `SoundPanel`). The core is reached only through `BoothBackend`; `apps/kairo/src/booth.rs` implements it over the services and core topics. `BoothViews::install` fills the workspace slots from 076.

Decision: the operator shortcuts use `cx.observe_keystrokes`, filtered to the operator window while it is active. A key that an action already used is skipped (`defaultPrevented`). The key-context stack stands the shortcuts down for text fields (`Input`), dialogs (`MovableDialog`) and menus (`PopupMenu`, `Popover`). `window.on_key_event` was not usable because it binds to its own dispatch node, not to the window. Difference: GPUI's `KeystrokeEvent` carries no repeat flag, so a held key repeats (Electron ignored `e.repeat`). A clicker sends separate presses, so this does not affect it.

Decision: `markSuggestionRendered` becomes `Booth::flush_marks`, which runs from the detected view's render and stamps after two `window.on_next_frame` callbacks, as the source's two `requestAnimationFrame`s did. The GPUI test platform runs no platform frames, so tests check that a mark is scheduled after the paint and not stamped on arrival (`marks_in_flight`).

Decision: the activation prompt (`DeepgramKeyDialog`) is `ui_account::ActivationDialog`, opened by the app on `BoothEvent::OpenActivation` in a new window-wide `OperatorWorkspace::set_dialog` slot, so `ui-operator` does not depend on `ui-account`. Service dialogs and the activation dialog draw over the whole window through `deferred(anchored())` (Radix portal).

Decision: `report` (`window.alert`) becomes `BoothEvent::Alert`, which the toolbox shows with `kairo_ui_kit::alert`. Retranslation failures are `BoothEvent::Toast` (a Kit error notification).

Not ported: the "N new" held-detections button. The source only ever sets `heldSuggestions` to `[]`, so it never shows. `DefaultBoothSearch` (the toolbox search on screens other than the Operator) is not ported; it belongs to the screens that use it.

Differences: prop thumbnails show the prop's background and an icon until 088 provides the prop renderer (`PropPreview`). The Audio tab shows program sound, with a `Toolbox::set_tracks` slot for 084's tracks list. Colour inputs use the Kit `ColorPicker`, and selects use dropdown menus. `downloadFile` writes to `~/Downloads` (numbered on repeat) and reveals the file. Video-input labels come from the engine process, so "Allow video access" reports that rather than prompting. Listing cameras in the app is left to 075/091. The recovery banner sits at the top of the detected column rather than above the whole workspace.

Affects prompts: 073, 075, 078, 084, 088, 091.

Amendment (074, owner review): the content pane is solid `surface`, not `.modal-glass`. Electron's glass blurs the darker booth behind the window, so its pane reads darker than the `surface-secondary` group boxes; GPUI has no backdrop blur here, and the glass over the frame's own fill came out the same colour as the boxes, hiding them. The sidebar keeps its glass tone.

## 2026-11-14 — 075 — Screens configuration

Implementation design: `ui-screens` owns the Screens window and edit rules. App backend edits read the latest settings while holding `SettingsWiring`'s lock, then apply only the requested output/presentation fields; deferred theme writes therefore cannot replace the current output list. Status comes from topics and engine events, not UI polling. `show:setup`, `show:looks`, `show:transition` and `show:check` select the same shared page and are retained in ui-state. Appearance follows Electron's sidebar/canvas/tabbed detail layout and the solid content-pane amendment from 074.

## 2026-11-14 — 078 — Scripture workspace

Decision: `ui-scripture` holds the page. `model::Scripture` ports `Scripture.tsx`'s flows (lookup with translation fallback and reload, live suggestions after 250 ms, chapter/verse toggle, going live with Sending/Sent/Error and the 3 s settle, grid keys, Prev/Next, playlists, picks, library, options); `view::ScriptureWorkspace` draws the sidebar, Bible bar, notices, grid, selection bar and queue dock; `rows` holds the pure rules (`types.ts`, recent lookups, the search list, highlight, grid lines). The core is reached only through `ScriptureBackend`; `apps/kairo/src/scripture.rs` implements it over the services. The live rail is the operator's `LiveRail` entity, shared because only one route draws at a time.
Decision: the grid is virtualised by line (a passage header, or one row of cards) with Kit `v_virtual_list`, so only lines near the viewport are built. This replaces `VerseThemePreview`'s `lazy` IntersectionObserver; there is no non-lazy grid card, and the live monitor is the engine frame (076). Width is measured each frame; columns follow `repeat(auto-fill, minmax(w, 1fr))`. Cards are the dark checkerboard with white text and no theme artwork (DESIGN.md); the slide text is `format_overlay_verse_text` and the reference line `format_overlay_reference`. Tokens `checker_light` (`#303030`) and `checker_dark` (`#242424`) added to `kairo-ui-kit`, both themes.
Decision: `kairo-scripture-recent-lookups` is stored in `ui-state.json` as the JSON text `localStorage` held, and read in either form.
Decision: the search list's ↑/↓, Tab, Esc and Enter go through `cx.intercept_keystrokes` while the field has focus, because Kit `Input` binds those keys as actions, which run before key-down listeners. Grid keys answer only while the page itself has focus (a card click gives it); fields, menus and the sidebar keep theirs, as the source's target checks did.
Decision: `scriptureViewState` and `scriptureOutputClearToken` live in the model; header Clear reaches it through the app (`ShellRequest::ClearLive` → `Scripture::header_cleared`). The page stays mounted, so Electron's restore-after-remount has no counterpart; a translation picked under an older default is dropped on `page_shown`.
Decision: "Use for this service" sits in the Bible bar: the shell has no header toolbar slot.
Decision: until 079 adds the sermon notes review, an imported draft becomes its playlist directly through `Scripture::confirm_import` with the detected references; 079 replaces the app's handler with the review dialog.
Decision: the API.Bible warning uses `should_show_api_bible_warning` with the state `Unchecked` (Electron's authorization probe is not run natively, 072), so it shows only when no Bible key is configured.
Differences: the playlist sidebar is a fixed 240 px (no resize edge or stored width); the rubber-band marquee over cards is not ported (⌘/Shift-click picks are); a failed playlist open or delete shows its error (Electron left it unhandled); "Get more" opens Settings but leaves the Options panel open; the translation and default-Bible selects are Kit dropdown menus.
Affects prompts: 079, 080.

075 implementation details: Screens is a `MovableDialog` at Electron's 800×620 default and 760×480 minimum, with keyboard/pointer resizing persisted under `kairo.frame.screens.v4`. Source status intervals are replaced by core topics (settings, displays, stage, video devices, media) and engine surface events. A window's topic task owns an abort-on-drop guard. The app owns an ordered write mailbox; closing/dropping a window flushes queued edits to that mailbox, so pending writes survive closure and remain ordered before edits from a reopened window. Failed writes report errors and restore rejected numeric values; focused name drafts survive unrelated pushes. Numeric input subscriptions are released with their fields.

075 appearance differences: Kit switches and menu-backed selects replace HTML controls. The content pane uses 074's solid `surface` amendment. Hardware signal counters are visible under their summary instead of folded into HTML details. The Theme tab uses 066's native text thumbnail, with solid/gradient/image background; animated/video backgrounds, theme elements and advanced background effects are not reproduced in this sample preview (live output rendering remains the engine's). The source's `openDriverDownloads` rejection cannot be observed through GPUI's `App::open_url`, which returns no result. Only missing/unsupported drivers offer that action. Refresh uses the refresh glyph as its spinner and stays still under reduced motion.

## 2026-11-14 — 079 — Sermon notes review

Decision (overrides the 007 recommendation of a custom editor on `EntityInputHandler`): the notes are a Kit `EditorState` from the pinned 0.9.1, drawn by rendering the state itself (`gpui_kit::base` `Editor`), with line numbers, folding, search, auto-close and smart indent off and soft wrap on. Kit 0.9.1 has `TextDecoration` collections (UTF-8 ranges with a `HighlightStyle`) that follow edits, so highlights stay on their words between scans, and the editor already has IME composition, undo/redo, paste and selection. 007 predates those decorations. The component `Input` was not used because it projects the global (dark) theme onto the text on every render; the base view keeps the `InputEditorStyle` set once from `KairoColors::light()`, so the dialog stays light as DESIGN.md asks.
Decision: `ui_scripture::review::SermonReview` ports `SermonNotesReviewModal.tsx`; the Scripture page opens it on `ScriptureEvent::ReviewSermonNotes` and creates the playlist through `Scripture::confirm_import` (`ScriptureEvent::ImportFinished` closes it, or re-enables it on failure, as Electron kept the modal open). The 078 stop-gap (create without review) is gone.
Decision: scans go through `ScriptureCall::ScanSermonNotes`, which the app runs with `scan_sermon_notes` on `spawn_blocking` with the default translation; edits rescan 300 ms after the last one, and a scan answer older than the latest edit is ignored (`scanSequenceRef`). The scan's UTF-16 offsets become byte ranges in the editor text (`review::utf16_to_byte`).
Decision: tokens `paper` (`#F9FAFB`), `sermon_mark` (`#CAD5E2`) and `sermon_mark_active` (`#B9D7FF`, also the notes selection) added to `kairo-ui-kit`, both themes.
Differences: the notes are plain text, not the draft's HTML, so bold, italics and lists from Word files are not shown (the scanned text is the same plain text Electron's `getText` produced); the canonical label (`data-reference`) is not drawn on highlights; scrolling the notes does not move the active reference (Electron's reading-line sync), and a jump selects the reference and focuses the notes without a smooth scroll of the list; the dialog is centred and not movable.
Affects prompts: 080.


## 2026-11-14 — Owner — Close 075 without another build cycle

The owner asked to complete 075 while skipping the build process, after stopping all verification launches because of repeated Keychain prompts. No further compilation, tests or app launches are performed. Prompt 075 is closed as implementation-complete with verification explicitly deferred under this owner override; earlier failures and missing manual evidence remain recorded, not converted into passing results.

The remaining Theme sample limitation is replaced by a private `SinkSpec::Preview` (protocol **9**). It opens only an offscreen compositor surface: no display window, NDI/Syphon sender, card, or audio output. The app supplies the complete sample theme through the normal ShowSlide path, so backgrounds, animated media, elements, filters and effects use the existing engine renderer and the 066 LivePreview transport. The Theme tab owns the session and destroys the private surface when hidden/closed. If a session cannot be opened, the existing basic still sample remains available. This addition is statically reviewed and formatted only; its new lifecycle/wire/replay/media-isolation tests are written but deliberately not executed. Existing app and engine binaries must eventually be rebuilt together for protocol 9.

075 final static review found no blocker in the new sink/API/lifecycle path. Sample media gets private decoder identities (background cue and element ids) so the preview cannot change a live output's shared video loop/transport. A failed preview-session open may retry on a later topic update. Four final regression tests remain unexecuted under the no-build instruction.

## 2026-11-14 — 080 — Lyrics library and quick open

Decision: `ui-lyrics` holds the library portion of `Lyrics.tsx`. `model::Lyrics` (the state map is `notes/lyrics-state-map.md`) owns the songs, libraries, setlists, selection, filter and sort, the filter query with 180 ms full-text search, the opt-in web search and its right-pane preview (cached per hit, stale answers dropped by sequence), favourites, delete, setlist drops and picks, transfer exports and the song usage report. `view::LyricsWorkspace` draws the rail (Kit `LibrarySection` for songs, a Favorites row, the Playlists section), the list band and its menu, the virtualised song list, the filter, the right pane and the usage dialog. `quick_open::QuickOpen` is `SongQuickOpen.tsx`; the app registers it as the shell's `Overlay::SongSearch` (⌘F / Ctrl+F). `rows` holds the pure rules. The core is reached only through `LyricsBackend`; `apps/kairo/src/lyrics.rs` implements it (`LyricsService`, `OnlineLyrics` over `ReqwestHttp` and the licensing gateway, `SetlistService`, `LibrariesService`, `SongUsageService`, `TransferService`). The live rail is the operator's.
Decision: the song pane shows the selected song's sections as coloured labels and plain lines until 081 replaces it with the slide grid. ＋ New song (coach anchor `lyrics-new`) and the empty state's New song emit `LyricsEvent::NewSong`; the app only logs it until 082 builds the editor. "Edit Song" in the row menu and "Import songs…" in the list menu are left to 082 (the editor and the import modal). `lyrics.readClipboard` stays `todo` in the inventory for the same reason: only the import modal's Paste tab reads it.
Decision: tokens `favorite` (`#FACC15`, the star) and `section_color(SectionKind)`/`section_text()` (`section-colors.ts`, both themes) added to `kairo-ui-kit`.
Differences: the sidebar is a fixed 280 px (no resize handle). "Export songs…" exports the preselection (the setlist on screen, else the selected song) straight to a `.kairo` file instead of opening the song picker dialog, which no prompt has ported; it is disabled when nothing would be exported. Dragging a song out of quick open onto the sidebar is not ported (⌘↵ queues it instead). The usage report's dates are `yyyy-mm-dd` text fields rather than date pickers; an unreadable date or a `from` after `to` is ignored, as the inputs' `min`/`max` were. A failed favourite, delete or usage export shows the error on the page; Electron left the first two unhandled and used `window.alert` for the third. The library loads off the UI thread and arrives as a feed (Electron seeded it from the startup snapshot). The core has no "songs changed" event yet, so songs committed from a `.kairo` import elsewhere appear after the next library read, not live. Added: ↑/↓ in the focused song list move the selection; ↵ in the filter searches the web when nothing local matches.
Affects prompts: 081, 082.

## 2026-11-14 — 081 — Lyrics slide grid

Decision: the song view is `ui_lyrics::view::song_view` over the `Lyrics` model, with the pure rules in `ui_lyrics::grid` (cards across section boundaries, `auto-fill` layout, arrow stepping across playlist songs, hint copy, label choices). Pushes go through `LyricsCall::PushSlide` (app: `Actions::push_lyrics`, which also records song usage); edits through `LyricsCall::Update` (`LyricsService::update_song`; a missing song answers an error and the page shows the operation's failure text, as Electron's `if (!updated) throw`). The gloss colour comes from settings (`lyrics.glossColor`) and follows changes.
Decision: picked cards use the accent blue border and check (the prompt's "blue picked, red live"); Electron drew picked white. Live is the `live` red, and a live card never also shows the picked style.
Decision: Electron has paint mode in code but no control that turns it on (`setPaintColor` is only ever reset). The prompt asks for paint colours, so the song ⋯ menu gains "Paint line colour" (the gloss colour, or clear to automatic); a click on a line then saves it, Esc ends paint, and cards do not push while painting.
Differences: keys (arrows, section hotkeys, Esc) act while the song view has focus (a click in it gives focus), not window-wide; the list's own ↑/↓ (080) keeps working while the list has focus. The Hot Key submenu is a scrollable list of the 36 keys (current marked ✓, keys on other sections "(moves here)") instead of a 6×6 grid. Marquee (rubber-band) selection, auto-scroll while dragging a slide, the pinned (sticky) playlist song bands, follow-on-scroll (scrolling the playlist does not change the selected song) and the live-output preview payload (`setLiveOutputPreview`; the rail shows the engine's own frame, 066) are not ported. "Edit song…" (slide menu, Edit button), Translate to English, Undo translation and the .txt/.usr downloads belong to 082. The grid is not virtualised: songs are tens of cards; a playlist lays out every song.
Affects prompts: 082.

## 2026-11-14 — 082 — Lyrics editing and import

Decision: the song editor is `ui_lyrics::editor::SongEditor` over the pure document `ui_lyrics::reflow_edit::ReflowDoc` (sections of slides, typing, caret split ⌥↵/⌘↵, join on Backspace at a slide's start, structured paste of blank lines and `[Chorus]` headers, section type/name/order/delete, new section from a slide, add section, delete slide, and the editor's own undo history with 1 s typing bursts and 200 steps, all in UTF-8 byte offsets). Each slide is a Kit `EditorState` (IME, selection, paste hook); the editor intercepts ⌥↵, Backspace at offset 0, ⌘Z/⇧⌘Z/⌘Y and ⌘S before the field's own bindings. The model owns the edit state (`Editing`: fields, baseline for the dirty check, saving, error), Save (new song: `lyrics.import` of the `[Label]`-marked text, title "Untitled" when empty; existing: `lyrics.update`), Cancel, and the discard question (`LyricsEvent::ConfirmDiscard`, then `discard_and_continue` or `keep_editing`) for selecting another song, New song or editing another song while dirty.
Decision: Import Song is `ui_lyrics::import::ImportDialog` with Electron's three tabs. Files come from a picker (`LyricsCall::PickSongFiles`) or an OS drop (`ExternalPaths`, `ReadSongFiles`); `source_for_file` ports `sourceForFile` (RTF to text, SongSelect header, clipboard parser, heading lift, file-name title). `lyrics.readClipboard` is the UI's own clipboard read (`cx.read_from_clipboard`), so its inventory row is done here. File → Import → Song lyrics opens the dialog (`LyricsFeed::OpenImport` from `ImportRequested`).
Decision: spell check (requirement 2). GPUI's input has no system spell check, but a platform path is viable: `apps/kairo/src/spell.rs` asks AppKit's `NSSpellChecker` for misspelt ranges (UTF-16 ranges mapped to UTF-8 bytes) and the editor draws them as wavy underlines through Kit text decorations, re-checked 300 ms after typing stops. The check runs on the background executor because the first call waits for the system spell server (measured 1.97–2.57 s in the ignored test); `NSSpellChecker` answered correctly off the main thread. Not ported: the right-click suggestions and Learn Spelling that Chromium's checker offered, and spell check on Windows/Linux (none reported). Owner review of these remaining gaps is recorded in PROGRESS 082.
Decision: Translate to English is in the song ⋯ menu (Sung in: Auto-detect, Yoruba, Igbo, Hausa, French, Spanish, Portuguese) over `OnlineLyrics::translate_sections` with the stored `stt.llmProvider`; glosses are stripped before sending and line colours dropped after, as Electron did; Undo translation strips them. As in Electron the menu is hidden while editing, so translating inside the editor (Electron's unreachable branch) is not offered.
Differences: the section gutter is a menu button with the section name (Rename…, type list, Move up/down, Delete section) instead of a vertical-text gutter with a popover, and Rename edits inline below it. Slides have a fixed 84 px editor height instead of growing with content. The import dialog is centred and not movable; its drop zone takes OS file drops. The .txt and .usr downloads in the song menu are not ported (no prompt owns a save-as-text dialog yet). The "Edit Song" row menu, "Edit song…" slide menu, Edit button, list menu "Import songs…" and empty-state Import now exist (080/081 left them).
Affects prompts: none.

## 2026-11-15 — 083 — Media dock and inspector

Decision: `ui-media` holds the dock. `model::MediaDock` ports `MediaDock.tsx`'s flows and `useMediaDockStore` (open, live item for the header), `view::MediaDockView` draws it, `inspector::Inspector` is `MediaInspector.tsx` with `TrimTimeline.tsx`, `rows` holds the pure rules. The core is reached only through `MediaBackend`; `apps/kairo/src/media.rs` implements it over `MediaService` and the existing actions (`push_media` with `background_target_problem`, `delete_media` which clears a live background from the screens, `set_media_playback`, `import_media`, `add_backgrounds`, `choose_media_folder`, and `import_media_files`, now public, for OS drops). One dock entity is shared: the operator, Scripture and Lyrics pages each draw it under their column, beside the live rail, as Electron's `BoothWorkspace` did. The header's Media button (was disabled) toggles it through `ShellRequest::ToggleMedia`, shows pressed while open, and carries the live dot and "— name on screen" (`Shell::set_media`).
Decision (preview playback, chosen and logged as the prompt asks): an in-process decoder, not engine frames. Engine frames (066) exist only per output and show what is live; a dock preview must play a file that is not on screen. `kairo_media::preview::PreviewStream` decodes with FFmpeg scaled to a thumbnail width; `ui_media::player::PreviewPlayer` runs it on GPUI's background executor and hands frames to the UI as images (replaced frames are dropped from the atlas). Brightness, contrast and saturation are applied to the decoded pixels, so a slider re-grades the current frame and never reopens or seeks the file (tested). Electron previewed with a muted `<video>` that showed the first frame; here the selected video card plays its preview (the last card clicked, right-clicked or copied), and every video tile shows the cached FFmpeg poster (`poster_for`, off the UI thread) until then. A file FFmpeg cannot read is marked "Can’t play".
Decision: the four `localStorage` keys (`media-dock-height`, `media-dock-zoom`, `kairo.mediaDock.sidebarHidden`, `kairo.mediaDock.sidebarWidth`) are stored in `ui-state` under the same names and values (the sidebar flag as the string "true"/"false").
Differences: `ProgramAudioFallback.tsx` is not ported here: it is feature-matrix row C05, owned by 056 (`kairo-audio`). The inspector's speed shows the slider value but the private preview's pitch is not kept (no audio is played in the preview, as in Electron, which muted it). The dock's slide-in animation is not ported (it opens at its height). Scaling in the inspector is a menu button instead of a `<select>`. The rubber band's hit test uses the grid's computed layout, so rows scrolled out of view count, as the kit `Marquee` intends.
Affects prompts: 056 (audio fallback), 084.

## 2026-11-15 — 084 — Tracks

Decision: House audio is `ui_media::tracks::TracksPanel` (model and view in one entity, rules as free functions) in the booth toolbox's Audio tab (`Toolbox::set_tracks`, left by 077), over `TracksBackend`; `apps/kairo/src/tracks.rs` implements it with `TracksService` and the existing actions (`import_tracks`, `choose_tracks_folder`). `TracksPlayer.tsx`'s hidden `<audio>` element is 056's `kairo_audio::tracks::TracksPlayer`, which already followed the library inside a task; it now lives in `ApplicationServices::track_player` so the panel can read `time()` and `error()` and `seek()`. `booth::install` returns its `BoothViews` so the panel can be placed and File → Import → Audio files can show the Audio tab (`TracksEvent::ShowAudioTab`).
Decision: position reads follow the prompt's "display rates only while playing": a 16 ms timer runs while a live track is not paused and not ended, and stops otherwise (measured 60 reads in a second of play, none while paused). Electron's `timeupdate` fired about four times a second.
Decision: the tracks player now follows `presentation.audio.outputLabel` (initially and on every settings change), as `TracksPlayer.tsx`'s `setSinkId` did; 056 had left the label unset, so tracks always played on the default device.
Differences: a player failure (for example no audio output device, or an unreadable file) is shown in the panel's error line; Electron swallowed `audio.play()` rejections. The seek bar is the Kit slider over 1,000 steps of the track's length rather than a 0.05 s-step range input. Rename is an inline field limited to 120 characters, as before; Esc cancels.
Affects prompts: none.


## 2026-11-15 — 085 — Documents

Decision: `ui-documents` holds the page (`DocumentsPage`), its rules (`model`) and a `DocumentsBackend` trait; `apps/kairo/src/documents.rs` implements it over `DocumentsService`, the slide converter, actions, libraries and settings. The page no longer renders PDFs: one `DocumentsBackend::import` call picks, prepares, renders with PDFium in the service and finishes, sending `ImportStep` progress; the cancel flag is checked between pages, and a cancel or failure removes the half-written import (`cancel(id)`), as `importDocument`'s catch did.
Decision: the slide preview (`DocumentSlidePreview`) draws each embedded video with ui-media's in-process `PreviewPlayer` (083), opened from `<page dir>/media/<file>` with the clip's range. With an output, it is a monitor that follows the polled status (seek past 0.4 s drift while playing, 0.04 s while paused; play, pause and hide with the outputs). With no output (`applied: false`), the preview's players take the transport and publish the same `DocumentPlaybackStatus`, so the slideshow's video gate works the same way. Without a media backend (tests) the preview marks each video's box.
Decision: the live slide clears (`LiveCleared`) when the program state stops showing a document page (`reference` `Page N` with no text), standing in for `liveDocumentPreview` becoming null.
Differences: the local preview is silent (`PreviewPlayer` decodes video only; Electron played the sound there). The playback poll runs while the slide has videos, whether or not the Documents route is showing (Electron stopped it when hidden and not in a slideshow); the page has no visibility signal yet. The slide size is a Kit slider snapped to 10 px steps.
Affects prompts: 086.

## 2026-11-15 — 085 — Documents

Decision: the Documents page is `ui_documents::DocumentsPage` over `DocumentsBackend`; `apps/kairo/src/documents.rs` implements it with `DocumentsService`, `SlideConverter`, `LibrariesService`, settings and the existing actions (`push_document`, `remove_document`, `document_playback`, `control_document`). Importing is one `import` call: pick, `prepare`, the service's PDFium `render` (checking the cancel flag between pages) and `finish`, with progress steps; any failure or cancel calls `cancel` to remove the half-written import. `documents:savePage` has no UI caller any more.
Decision: the live mark is the page's own record of its last push (Electron's `liveDocumentPreview`), cleared when `ProgramState.current` stops being a document page (`Page N`, no text). The clicker uses the operator's `previous`/`next` bindings while the page has focus; rename and seconds fields, dialogs and the slideshow menu keep their keys.
Decision: `kairo.documents.slideWidth` is stored in ui-state as a number, 140–420 in steps of 10, default 240; a missing or out-of-range value reads as 240. The grid is a virtual list of rows, so a 500-page deck draws only visible rows.
Differences: with no output connected the preview band plays the slide's videos itself, silently (see the entry above). The header line sits at the top of the page, not in the app toolbar slot. Import notes are always expanded. The video transport is the page's own (play/pause, restart, ±5 s, seek bar), not the operator rail's component.
Affects prompts: 086.

## 2026-11-15 — 086 — Theme editor canvas

Decision: `ui-theme` holds `ThemeEditor` (state in `editor.rs`, drawing in `view.rs`) over `ThemeBackend`; `apps/kairo/src/theme.rs` implements it with the settings wiring, `pick_overlay_media` and a private engine preview surface. Pure rules live in `canvas` (layers, snap, turned resize, rotate, nudge, hit test, drag), `history` (600 ms coalescing, 100 steps, as `useThemeHistory`), `ops` (one `Op` applied by the same function to the editor's copy and the stored settings: save, add, remove, baseline, screen toggle, make default) and `model`.
Decision: the slide is the engine's raster, drawn by a private `SinkSpec::Preview` surface through `LivePreview` (the Screens Theme sample's mechanism), so preview equals output. Handles, outline, labels, delete button and centre guides are GPUI-drawn over it; hit testing is geometric (rotated boxes), not per-element, because GPUI cannot rotate an element. Draft changes reach the engine once per rendered frame (revision counter), not per mouse event.
Decision: writes are one mailbox of read-modify-write ops (`persist`) writing only `themeLibrary`, `themeDefaults` and `overlay` when they changed; the editor applies each op optimistically, ignores the settings feed while any write is in flight, then reads the stored state back. A failed write shows "Screen changes could not be saved. Please try again." (Electron's output-write message, which it only logged for the library).
Decision: the library tile pictures are small GPUI drawings (background colour or gradient, elements, sample words), not engine frames: one engine surface per tile is too heavy. The slide in the middle is exact; tiles are indicative. Shape tiles are `kairo_scene::vector::render_vector` pictures tinted to the text colour.
Decision: Electron has no marquee or multi-select on this canvas (one `selectedLayer`), so none is added. The remembered theme (`themeSelectedId`) stays in memory for the life of the editor, as the app store did; the editor is kept alive by the shell, so it survives tab switches.
Differences: body cursor is the open/closed hand (GPUI has no four-arrow cursor); on a turned element the name label and delete button sit at the turned corner but are not themselves turned, and handle squares turn with the box; no hover-fade transitions; the right column shows the canvas help text until 087 attaches the inspector with `ThemeEditor::set_inspector`; Reset and Save theme live under that column as in Electron. `window.confirm` for Delete is the Kit confirm dialog. Aspect for new media is read from the file header (image decode at ≤1024 px, FFmpeg for video) instead of a browser element.
Affects prompts: 087 (inspector slot, `edit_theme`, `selected_layer`, events `DraftChanged` and `SelectionChanged`).

## 2026-11-15 — 087 — Theme editor panels

Decision: `ui-theme::ThemeInspector` fills the editor's right column through `ThemeEditor::set_inspector` (attached in `apps/kairo/src/theme.rs::install`): Background, Text and Layout tabs for the theme, and the element panel for a selected shape, picture or video, as `ThemeEditor.tsx` composes `BackgroundFields`, `ThemeTypePanel`, `ThemeLayoutPanel` and `ThemeElementPanel`. Every change is one `edit_theme` call, so it is undoable, previewed and saved by the editor's 400 ms debounce. The rules are pure functions in `ui-theme::panels` (font, weight and size lists, hex text, `parseColorParts`, `colorWithOpacity`, size commit, reference position, stroke width, shape change, adjustments).
Decision: fields are entities kept between frames, keyed by field and dropped when the selection, tab or theme changes. The field whose edit produced the draft is not written back while that draft stands (so typing is not overwritten); any other change (undo, another control, the canvas) writes every field back.
Decision: the font list is `kairo_scene::FONT_STACKS`; a theme whose stack is not one of them gets an extra entry named after the stack's first family (`Default font` when empty), so an older theme shows its own font rather than a blank list.
Decision: the header's Screen Themes button opens `ui-theme::ThemePicker` in a shell popover (`Shell::set_screen_themes`). A pick is `Op::Assign { theme_id, output_id }`; `ops::apply` now returns `Result` and Assign fails with "Screen is no longer available" when the screen has gone or no longer draws a picture, which the picker shows as "Theme could not be applied. Try again." Picks never touch defaults or other screens.
Decision: `TransitionPanel` and `SetupChecklist` (listed in prompt 087) already ship in the Screens window from prompt 075 (`ui-screens/src/fields.rs`); not rebuilt here.
Differences: the size box's presets are a dropdown beside it (no HTML datalist). The hex box keeps what is typed until it loses focus (Electron's controlled input expanded `ff0` to `FFFF00` mid-typing). The swatch is the Kit colour picker popover, not the OS colour panel. Selecting any text layer while on Background switches to Text (Electron did so from the layers list and the label's edit action). Picker and element picture thumbnails are GPUI drawings or the image file; a video shows an icon, not its first frame. Number fields' drag-to-scrub, arrow keys and Shift×10 come from `kairo_ui_kit::NumberField`.
Affects prompts: 088.

## 2026-11-15 — 088 — Props editor

Decision: `ui-operator::PropsEditor` (`props.rs`) is the shell's Props overlay, registered in `apps/kairo/src/booth.rs::install_props`. It lives in `ui-operator` because the booth owns the presentation and program state it edits. `BoothEvent::OpenProps(select)` opens it at a prop or `timer:<id>`, or moves an open editor there. The window is a `MovableDialog` (Escape and the scrim close it); Electron's own traffic lights and close button are the dialog's.
Decision: edits are held as a draft and written through `Booth::set_presentation` 250 ms after they settle (`useDraftPresentation`); Save, Escape, closing and dropping the editor write what waits. A timer is designed as a prop-shaped picture and written to that timer's theme; its readout object cannot be deleted. Deleting a prop that is on screen takes it down first (`SetProp { on: false }`); deleting the timer on screens shows the next one (`TimerShow`); the last timer stays; new timers stop at `MAX_PROGRAM_TIMERS`, objects at `MAX_PROP_OBJECTS`.
Decision: pictures are added with a new `BoothCall::PickImage` (answers `BoothAnswer::Picked`), served by the existing `Actions::pick_program_image` (prompt 065). The answer is applied after the booth update finishes, so the editor can read the booth.
Decision: the stage and the thumbnails (editor lists and the Operator's Show tiles, which drew a placeholder until now) are GPUI drawings of the prop's objects at the 1920 × 1080 frame's scale (`props::prop_picture`), not engine frames. One 250 ms task drives every timer readout in the window and asks for a frame only while a timer object exists.
Decision: keys act only while the editor itself has focus (clicking the stage gives it focus), so typing in a field never deletes or nudges an object.
Differences: colour pickers are the Kit colour popover, not the OS colour panel. The prop list's delete button shows on hover as in Electron. Text in a picture wraps in the box with GPUI text, so line breaks can differ slightly from the screens' typesetter.
Affects prompts: 089.

## 2026-11-15 — 089 — Web workspace

Decision: `ui-web::WebPage` (`page.rs`) is the Web route, over `WebBackend` (`apps/kairo/src/web.rs`: `ProgramService::set_web`/`reload_web`, the output actor's `ScrollWeb`/`WebPointer`, `settings.set('web')`, and feeds of `ProgramState.web` and settings) and a `PreviewPage` seam. The real preview is `NativePreview`, one `gpui-webview` page with `MUTE_SCRIPT` and `SCROLL_SCRIPT` as initialisation scripts, reporting scroll, adaptive layout and pointer through Wry's IPC (Electron used `postMessage` from an iframe). Tests drive a scripted preview.
Decision: per the 2026-11-09 amendment the preview is its own page, laid out 1920 wide by `set_zoom(box width / 1920)`; "preview shows the same frames that go to air" becomes "preview follows the program page": while the live page is the preview's and Screens follow is on, links followed are sent with `setWeb` and scrolling with `ScrollWeb`. The pointer is reported in the page's own 1920-wide coordinates and sent only over the live page, inside 1920 × 1080, with Pointer on and Screens follow on. A page that lays out by the window's height gets the screens' exact 16:9 box; others fill the area with the cut line "Screens show down to here".
Decision: `gpui-fast` is not enabled in this workspace, so the native page draws over any GPUI content in its bounds. The preview is therefore hidden (and the pointer taken off screens) whenever the Web route is not showing or any shell overlay window is open (`apps/kairo/src/web.rs::install` observes the shell). Header popovers sit above the page bounds. This also covers "hidden tab stops preview work": hidden, the page is not drawn and the native view is set invisible.
Decision: the preview refuses new windows (`target=_blank`), downloads and any address that is not http(s) or `about:blank`, matching the iframe sandbox (no popups) and `setWeb`'s http(s) rule. With no system web view (`WebView::build` fails) the page says so and pages can still be shown on screens.
Differences: navigation is learned from Wry's page-load Finished event, so in-page (history API) navigations are not seen (as for Slido, 047). A navigation that arrives as the address we loaded clears the "next navigation is our load" flag (Electron left it set, so the next link replaced the entry instead of adding one). The local pointer mark on the operator's main screen preview (`useAppStore.webPointer`) is not drawn; screens get the pointer. Bookmarks sidebar is a fixed 240 px (Electron's is resizable).
Affects prompts: 090.

## 2026-11-15 — 090 — Engage workspace

Decision: started at the owner's direction while 047 (Slido) is still in-progress; its open item (a real moderator sign-in and reading a live event) stays open. The Slido card's connection states are built against `SlidoAudienceSource`'s statuses as they are.
Decision: `ui-audience::AudiencePage` is the Engage route (`apps/kairo/src/audience.rs`: `ServicesAudience` over `AudienceService`, `EngageFlows`, the cloud session topic and `presentation.question`), with the live rail. All of `components/audience/` is ported: engagements list (cache, loading, offline, error, first-engagement and signed-out cards, open, delete with confirm), the Kairo card (starting steps, status, error, QR download, copy join link, Show/Hide code, audience page, settings, End with confirm, Reopen), Slido (connect form with steps and error, card with status and detail, sign-in/window/copy link, skip approval and include pending restart the source, disconnect), sample questions, Q&A (tabs with counts, Popular/Recent, moderation per state, Show/Clear, replies to everyone or the asker, Fix typo while in review), polls (list, editor with 2–12 answers, open/close/reopen voting, show/clear on screen, results switch, per-card errors), new engagement (title default, optional dates, room settings, start now or save, 8 s slow note), and engagement settings (General, On Screens with presets and preview, Collaboration with invite and revoke).
Decision: `kairo:engagements-cache:v1` is kept in the per-viewer UI state (`UiState`, the `localStorage` stand-in) as the same JSON array; anything but an array is ignored; signing out removes it. Copy buttons write the GPUI clipboard and say "Link copied"/"Copied" for 1.5 s. Download QR writes `kairo-<code>-qr.png` to Downloads (a unique name if taken) and reveals it, as the browser download did. `audience:page` answers the address and the page opens it with `cx.open_url`.
Decision: the question list is a variable-height GPUI `list` (virtualised): with 500 questions in a 1400 × 1000 test window, 6 are drawn. One 5 s task keeps Slido "updated …" and question ages current.
Differences: dates are typed `YYYY-MM-DD` (no native date picker in the Kit); dialogs draw over the page inside the window and are not movable; colours use the Kit colour picker; the On Screens preview is a GPUI drawing of the card, not `renderQuestionHTML`; the sidebar is a fixed 280 px (Electron's is resizable). Answered and loading counts on the tabs show as "Open 3" text rather than a pill.
Affects prompts: 091.

## 2026-11-15 — 091 — Transfer, capture popover and leftovers

Decision: new crate `ui-transfer` (`TransferHost` over `TransferBackend`, `apps/kairo/src/transfer.rs`) is mounted once by the shell (`Shell::set_transfer`), drawn over the workspace after the notices. It holds the import review (`ImportReviewModal`), the song export picker (`ExportSongsModal`) and the 6-second result notice. Its rules are in `ui_transfer::model` (tested). The app publishes `KairoOpened` (Finder, second launch, a page's "Import .kairo file…", File → Import → Kairo file through `ImportRequested(Kairo)`), `TransferNotice` (every export and every open or pick failure, from `transfer::export` and `transfer::pick`, which the Lyrics and Scripture backends now call) and `PickSongs`. After a commit `TransferCommitted` makes the Lyrics page reload the library and the Scripture page reload sermon plans (`commitKairoImport`).
Decision: Lyrics' "Export songs…" now opens the picker (`KairoExport::Pick`) with the playlist on screen or the selected song ticked, and is no longer disabled with nothing selected, as in Electron. Single-song, setlist, library and playlist exports still save directly.
Decision: while a transfer window is open the shell counts as covered (`Shell::covered`), so the Web preview hides as it does for overlays (089).
Decision: new crate `ui-capture`: `Capture` (shared state, `useCaptureStore`), `LiveMenu` (the Live popover body: Start/Stop Capture with the elapsed clock, warning and error, Capture Settings…, one switch per Syphon output with its readiness as the tooltip, and Set up Syphon… / Syphon Settings…) and `CaptureSettingsWindow` (the Capture Settings overlay, `Overlay::Capture`). The backend (`apps/kairo/src/capture.rs`) uses the 064/065 handlers (`start_capture`, `stop_capture`, `choose_capture_folder`, `reveal_capture`, `capture_audio_inputs`), `settings.set('capture')`, `screens::persist(Edit::Output { enabled })` for the switches, and reads readiness from `EngineHandle::unavailable_reason` every 3 s. A start or stop that fails shows its message where a refusal shows. Off the Mac the popover shows only the Live detail line.
Decision: the Live button shows `label · m:ss` while recording with a red dot on its icon, ticking once a second (`ShellState::recording_started_at`). `Shell::open_screens(target)` opens Screens at `add:syphon`/`kind:syphon`.
Decision: `ndi:getStatus` and `program:webNavigated` are marked done: readiness is `unavailable_reason` (header Screens status 071, Screens 075, Live popover 091); the preview's navigation is `ui_web::PreviewEvent::Navigated` (089).
Decision: `Transcription.tsx` is not imported anywhere at Electron `af978c5` and is not ported. The only calls it alone makes are `orchestrator.dismissAuto`, `orchestrator.getStats`, `transcription.getHistory` and `transcription.clearHistory`; the shipping Electron Operator does not expose them either, and their services are ported (038, 039). Port them as UI only if the owner asks. `tools/ts-oracle/renderer-owners.cjs` writes `native-migration/renderer-owners.md`: all 199 renderer files have an owner; 24 are not reachable from `index.html` (listed there; unused shadcn parts, old onboarding steps, `VerseBlock.tsx`, `paneSizing.ts`, `offline-bible-view-model.ts`, `kairo-stacked.png`, `Transcription.tsx`).
Decision: `kairo.page-sidebar-width` is `kairo_ui_kit::components::page_sidebar` (already shared by the pages).
Differences: the picker sorts by lower-cased title (no ICU `localeCompare`). Scrim clicks do not close the transfer and capture windows (the `MovableDialog` contract); Escape, Close and Cancel do. Selects are dropdown buttons. The export count counts only ticked songs still in the library.
Affects prompts: 100, 109.

## 2026-11-15 — 100 — First-run migration

Decision: `kairo_store::migration::run` runs once per profile, in the boot task before any store opens, and records itself in `kairo-native-migration.json` (version 1, time, report). An upgrade (a `kairo-settings.json` or `proautomate-settings.json` exists) is backed up first to `<userData>/Native Migration Backups/<yyyy-mm-dd hh-mm-ss>/`: every top-level `*.json` (not `.tmp-` files), `Preferences`, `api-bible-cache.key` and `cloud/`. `cloud/` is included because `SecureStore::read` deletes a blob it cannot decrypt. If the backup fails nothing else runs and no marker is written, so the next launch retries. Databases are not copied: they are opened read-only and only checked.
Decision: secrets use a new `migrate_electron_keys`, which loads or unwraps and saves each data key (cloud, API.Bible cache) without touching the data it protects; Electron's wrapped files stay for rollback. A failed cloud key is shown ("you may need to sign in again"); a failed cache key is only logged, because the cache rebuilds.
Decision: microphone. Electron saved Chromium's `deviceId`, which is `hex(HMAC-SHA256(key = "file://", message = CoreAudio UID + salt))`, with the salt at `Preferences` → `electron.media.device_id_salt`. This was checked against this Mac's real profile and is exact, so the saved id is matched by hashing each present input (the cpal id after `coreaudio:`). A match rewrites `audio.deviceId` to the device's label, which the audio service turns into its id (074, `migrate_saved_device_id`). Electron reads a label as an unknown id and uses the default, so rollback keeps working. With no match the value is left alone and the operator is told the microphone is not connected. This supersedes 038's "cannot be mapped".
Decision: `localStorage` is read from a temporary copy of `Local Storage/leveldb` (`rusty-leveldb` 4.0.2), origin `file://`, keys `_file://\0` + encoded key, values `\x01` Latin-1 or `\x00` UTF-16LE. Every key is copied into `ui-state.json` as Electron's exact string; a key already there wins. This covers every key prompts 071 to 091 read without a list to keep in step. Two readers that only accepted numbers or JSON now also accept the string (the shared page sidebar width in `apps/kairo/src/main.rs`, the Engage cache in `apps/kairo/src/audience.rs`); the rest already did. `kairo-message-nuggets` goes through the same path, and the Operator's legacy-nugget import reads it. Any failure reads as an empty import: the layout uses its defaults and nothing is shown.
Decision: checks. Each of `lyrics.db`, `bible.db`, `api-bible-cache.db` that exists is opened read-only (`PRAGMA schema_version`). `workspace.folder`, `media.folder` and `tracks.folder` must exist when set. With no workspace set, the default's parent must exist. Only failures that need the operator are shown, in one dismissable card; everything else is logged.
Decision: the sync ran first (`electron-sync`). The only data change since `de3bb54` is the `media-dock-zoom` localStorage key, which native already reads and the importer copies.
Affects prompts: 101 to 105, 109.

## 2026-11-15 — 101 — macOS packaging

Decision: shell scripts in `packaging/macos` (pins in `config.sh`) and a CI job, `.github/workflows/package-macos.yml`, which builds, signs, notarizes and uploads to workflow storage. Publishing stays with 104. One package per architecture (`arm64` on `macos-15`, `x86_64` on `macos-15-intel`), as the Electron release does, with electron-builder's file names (`Kairo-<v>-arm64.dmg`, `Kairo-<v>-arm64-mac.zip`; Intel without `-arm64`), which `pick_mac_zip` and the website rely on. Why: a universal build would need a universal FFmpeg and change the updater feed. The bundle version is the `kairo` package version, so 104 must set the workspace version to the release number.
Decision: layout. `Contents/MacOS/Kairo` and `Contents/MacOS/kairo-engine` (the engine is a bare accessory executable beside the app, found by `main.rs` as today, signed as `com.kairo.app.engine`; it sets `ActivationPolicy::Accessory` itself). It is not a nested helper app, so TCC prompts and grants belong to Kairo. Libraries in `Contents/Frameworks` (FFmpeg, PDFium, NDI), each `@rpath/<name>`, with `@executable_path/../Frameworks` on both executables and `@loader_path` on each library. Build-machine rpaths are removed. The DeckLink helper stays at `Resources/decklink/decklink-helper`, where `kairo-sinks` looks for it. Universal inputs (NDI runtime, DeckLink helper) are thinned to the package's architecture.
Decision: entitlements, reassessed one by one against Electron's `build/entitlements.mac.plist`. Dropped: `allow-jit` and `allow-unsigned-executable-memory`, since there is no V8 and WKWebView runs out of process. Also dropped: `disable-library-validation` on the app and engine, because every bundled library is re-signed with the app's identity, as electron-builder already did for `libndi.dylib` in 1.0.33 (team `YK7WF42S33`). Also dropped: `network.client` and `network.server`, which apply only inside the App Sandbox, which Kairo does not use. Kept: `device.audio-input` and `device.camera` on the app and the engine. The DeckLink helper alone keeps `disable-library-validation`, because it loads Blackmagic's `DeckLinkAPI.framework`, which another team signs.
Decision: Info.plist follows the installed 1.0.33 bundle: `com.kairo.app`, the three usage strings word for word, `NSBonjourServices` `_ndi._tcp`, the `.kairo` document type (Editor, Default), category, dark mode, `LSMinimumSystemVersion` (13.0, see the owner decision below), `NSAppTransportSecurity` with `NSAllowsArbitraryLoads` and `NSAllowsLocalNetworking`. ATS matters more now: WKWebView content follows ATS, and the web layer loads any address the operator types, including local `http://`. Dropped: Electron-only keys (`ElectronAsarIntegrity`, `LSEnvironment` `MallocNanoZone`, `NSMainNibFile`, `AtomApplication`), and the Bluetooth usage strings, which are electron-builder boilerplate. Kairo uses no Bluetooth.
Decision: FFmpeg 9.0.2 from `ffmpeg.org`, checked by SHA-256 (the same hash as Homebrew's formula pin), configured LGPL-only with `--disable-everything` and the demuxers, decoders, parsers, VideoToolbox hwaccels and bitstream filters for the media dock (mp4, webm, m4v, mov, ogv) and tracks (mp3, wav, m4a, aac, ogg, flac). FFmpeg's own decoders, plus libvpx 1.17.0 (BSD-3-Clause, decoders only, static inside libavcodec, checksum as Homebrew pins it). Without libvpx, transparent VP9 WebM loses its alpha (decision 055), and the `ten_bit_and_alpha_streams_decode` test caught this against the first build. `libswresample` is built and shipped, though Kairo does not link it, because FFmpeg's Opus decoder needs it (configure otherwise drops Opus with a warning, which the script now treats as an error, as it does an unknown component name). `--install-name-dir=@rpath`. Licence text, source link, hash, configure line and a written offer are in `Resources/licenses/ffmpeg`, and libvpx's licence and patent grant are in `Resources/licenses/libvpx`. Checked: `kairo-media`, `kairo-video` and `kairo-audio` tests (110) pass when linked against this build instead of Homebrew's. Not settled: the H.264 and HEVC legal review that decision 008 requires before release.
Decision: inputs not built here are fetched and checksummed at package time: `bible.db`, `icon.png` and `icon.icns` from `CodeLawd/kairo-presenter` at `af978c5` (raw GitHub URL; the repository is public), PDFium `chromium/8138` per arch from bblanchon/pdfium-binaries, and the NDI runtime from `@grandi/darwin-*` 2.0.2 on npm. That is the same `libndi.dylib` the Electron release ships, checked by the file's hash. Nothing is committed.
Decision: signing. With `KAIRO_SIGN_IDENTITY`, the build uses the hardened runtime and secure timestamps, signs inside-out (libraries, DeckLink helper, engine, bundle; no `--deep`), then notarizes and staples the app, the zip made from it and the signed DMG. With `KAIRO_ADHOC_SIGN=1` (CI without `CSC_LINK`), it makes an ad-hoc signature without the hardened runtime, matching Electron's after-pack fallback. Secret names are unchanged from the Electron release.
Decision: the DMG layout (540×380, app at 140,200, Applications at 400,200, `<product> <version>` title, volume icon) is set through Finder AppleScript, because electron-builder's `dmgbuild` is not available. `KAIRO_DMG_LAYOUT=0` makes a plain DMG on headless machines; a release must not set it.
Decision: `verify-package.sh` ports the Electron checks that still mean something: the bundle layout replaces `app.asar`, `bible.db` is checked, the NDI runtime is checked, foreign architectures are refused, there is a size ceiling, and no repository junk is allowed. It adds: Info.plist keys; every Mach-O is one architecture and has a minimum macOS of 11 or lower; there are no absolute non-system library paths and no absolute rpaths; every `@rpath` library is in `Frameworks`; no string names a Homebrew FFmpeg; the FFmpeg configuration is LGPL with the expected majors; each Mach-O has a strict signature, and for Developer ID also the hardened runtime, a timestamp and one team; entitlements are as above; and Gatekeeper plus the stapled ticket when notarized. Electron's ASAR, better-sqlite3 and node_modules checks have no counterpart.
Affects prompts: 102, 103, 104, 105.

## 2026-11-15 — 101 — Owner — Minimum macOS 13

Decision: the native app requires macOS 13. This supersedes item 1 of the 008 go decision (macOS 11). `LSMinimumSystemVersion`, `MACOSX_DEPLOYMENT_TARGET`, the FFmpeg and libvpx builds (`darwin22`) all follow `KAIRO_MIN_MACOS` in `packaging/macos/config.sh`. `verify-package.sh` fails on any Mach-O that needs a newer macOS than the minimum, with no exceptions.
Why: the PDFium build (`chromium/8138`) and the NDI 6.3.2 runtime both declare macOS 13.0. On 11 and 12 they would not load, which would turn off PDF import (which pdf.js did there in Electron) and NDI output. The NDI runtime is the file Electron 1.0.33 already ships, so Electron's NDI never worked below 13 either.
Alternatives rejected: keeping 11 with those features off; looking for older builds of both libraries.
Effect on 104: Electron installs on macOS 11 and 12 must not be offered the native release, because they could not open it. electron-updater 6.8.9 (`AppUpdater.checkIfUpdateSupported`) skips a release whose feed `minimumSystemVersion` is above `os.release()`, which is the Darwin kernel version. The native release's `latest-mac.yml` must therefore carry `minimumSystemVersion: 22.0.0` (Darwin 22 is macOS 13), not `13.0`. `kairo-updater` has no such check. A later native release that raises the minimum again would need one. The website's download page must state macOS 13.
Electron files read: `node_modules/electron-updater/out/AppUpdater.js` and `src/main/services/updater/index.ts` (read only).
Affects prompts: 104, 105, 107.

## 2026-11-15 — 102 — Windows packaging

Decision: NSIS, not Inno Setup, made by electron-builder 26.15.3's NSIS target with `--prepackaged` on a folder `package.ps1` stages (`packaging/windows`, pinned in `package.json` and `package-lock.json`). Why: Electron installs came from the same NSIS target (electron-builder 26.15.3 in kairo-presenter). Using it keeps the installer GUID (`440e67a7-8e87-5084-89a4-da16aeff9350`, the UUID v5 of `com.kairo.app`, checked with electron-builder's own function) and the `Software\{GUID}` and Uninstall keys. It also keeps the per-user default folder `%LOCALAPPDATA%\Programs\Kairo`, the choosable folder (`oneClick: false`, `allowToChangeInstallationDirectory`) and the shortcuts with AppUserModelID `com.kairo.app`. The same goes for the `.kairo` association and the old-version uninstall (`/S /KEEP_APP_DATA --updated`), so installing over Electron upgrades in place and keeps `%APPDATA%\Kairo`. The in-app updater's `--updated`, `/S` and `--force-run` (decision 048) keep their meaning, and electron-builder also writes `latest.yml` and the blockmap. `name: kairo-presenter` and `author.name` match Electron's `package.json` (the uninstall entry's Publisher and the updater cache folder). `electronVersion: 33.2.0` is set only to pass electron-builder's framework check; the NSIS target does not read it. Inno Setup was rejected because it would need a migration of Electron's registry entries and a different updater install path.
Decision: no VC++ redistributable. The Rust binaries build with `+crt-static`. PDFium (`chromium/8138`) and the NDI runtime (`@grandi/win32-x64` 2.0.2, Vizrt-signed, kept unsigned by us) link their C runtime statically. FFmpeg is built with MinGW-w64 UCRT64, using `-static-libgcc` and Win32 threads, and needs only the UCRT that Windows 10 and 11 include. `package.ps1` copies a VC++ DLL app-locally only if a binary still imports one. `verify-package.ps1` fails if one would come from System32.
Decision: FFmpeg 9.0.2 and libvpx 1.17.0 use the shared pins and configure flags (new `packaging/common.sh`, which `packaging/macos/config.sh` now reads), with D3D11VA hwaccels instead of VideoToolbox. MSVC import libraries (`lib/avcodec.lib` …) come from FFmpeg's `.def` files through `dlltool`, in the layout `ffmpeg-sys-next` reads on MSVC (as the BtbN builds in CI). The DLLs sit next to `Kairo.exe` and `kairo-engine.exe`.
Decision: the icon and version information on `Kairo.exe` and `kairo-engine.exe` are set with `resedit` 1.7.2 (`edit-resources.mjs`), the library electron-builder uses. The `rcedit` npm package is marked unsupported by npm in every version. The WebView2 Evergreen bootstrapper is fetched at package time, accepted only with a valid Microsoft Authenticode signature, and run by `installer.nsh` (`/silent /install`, per-user when not elevated) when no runtime is registered. A failure tells the operator and the install goes on.
Decision: `verify-package.ps1` reads PE imports itself (`pe.ps1`), so it runs on a clean VM without Visual Studio. `install-test.ps1` covers a clean install, the upgrade over Electron 1.0.33, a launch with no FFmpeg on PATH, and an uninstall that keeps user data. CI (`.github/workflows/package-windows.yml`) builds on `windows-2022` and installs on `windows-2022` and `windows-2025`.
Marked done at the owner's direction with nothing run on Windows: see PROGRESS.md 102.
Affects prompts: 103, 104.

## 2026-11-15 — 103 — Linux packaging

Decision: AppImage and deb are made by electron-builder 26.15.3's Linux targets with `--prepackaged` on a folder `packaging/linux/package.sh` stages, as on Windows (102). The identity matches the Electron release: deb package `kairo-presenter` in `/opt/Kairo` with `/usr/bin/kairo-presenter`, `Kairo.desktop` (`syncDesktopName`), category `AudioVideo`, artifact names `Kairo-<v>.AppImage` and `kairo-presenter_<v>_amd64.deb`, and `latest-linux.yml` for the AppImage updater (048). So apt upgrades an Electron install in place and keeps `~/.config/Kairo`. The `.kairo` association now carries `mimeType: application/x-kairo`. Electron declared none, so its Linux packages registered no type; the deb now installs a shared-mime-info entry for `*.kairo`, and the desktop entry lists it.
Decision: FFmpeg ships in `<app>/lib`, the same for both formats, rather than the amendment's `usr/lib` (AppImage) and `/usr/lib/kairo` (deb). electron-builder puts the same folder in both, so one layout serves both. `kairo-presenter` and `kairo-engine` carry the runpath `$ORIGIN/lib`, and each FFmpeg library `$ORIGIN` (set with patchelf). Bundling, not distribution FFmpeg: Ubuntu's libavcodec is GPL and its major differs by release. PDFium (`libpdfium.so`) and the NDI runtime (`libndi.so.6`, from `@grandi/linux-x64` 2.0.2) sit beside the executables, where `kairo-documents`, `kairo-scripture` and `kairo-sinks` look. FFmpeg is built LGPL on Ubuntu 22.04 with the shared pins and VA-API hwaccels (`--enable-vaapi`; libva is MIT and declared).
Decision: deb `Depends` comes from `dpkg-shlibdeps` over every shipped binary, with the bundled libraries treated as private (`-l`). Added to it are `libvulkan1`, `libegl1` and `xdg-utils` (opened at run time). `Recommends` holds `mesa-vulkan-drivers` and `zenity`. Electron's `libnss3` was Chromium's and is dropped. `libasound2` comes from the binaries. The NDI runtime adds Avahi's client libraries. Packages built on 22.04 name the pre-`t64` libraries, which 24.04's `t64` packages provide.
Decision: Linux starts on X11 only: `gpui_kit::platform::linux(WindowingModes::X11)` (gpui-pre-platform 0.3.12) instead of `application()`, which allowed native Wayland, where the WebKitGTK views cannot live. `apps/kairo/src/startup_error.rs` covers two cases. With no `DISPLAY` (where GPUI would panic), it shows a message. When the operator window cannot open (no Vulkan or OpenGL driver, as wgpu's `VULKAN | GL` finds no adapter), it shows a message, where before Kairo exited silently. The message goes to zenity, then kdialog, then notify-send, always also to stderr, and Kairo exits with status 1. A missing Vulkan alone is not an error, because wgpu falls back to OpenGL. `gpui-fast` stays off (decisions 070 and 089). The owner asked about it during 103; it changes the whole UI backend and is left for its own task.
Decision: verification is by CI only, at the owner's direction (2026-11-15), with nothing run locally. `.github/workflows/package-linux.yml` builds on `ubuntu-22.04` and runs the `kairo-media`, `kairo-video` and `kairo-audio` tests linked against the bundled FFmpeg. In clean `ubuntu:22.04` and `ubuntu:24.04` containers, both clean and over the Electron 1.0.33 deb, it runs `install-test.sh`. That script installs from apt with only the deb's Depends, checks no system FFmpeg is pulled in, runs `verify-package.sh` (ldd: FFmpeg from `/opt/Kairo/lib`; glibc at most 2.35), checks the `*.kairo` MIME type, and launches under Xvfb with Mesa's software Vulkan and OpenGL (engine started, NDI runtime loaded). It also checks the no-display and no-driver messages, runs the unpacked AppImage, and purges, keeping user data. Until 104 sets the release version, the workspace's 0.1.0 is installed over 1.0.33 with `--allow-downgrades` and a warning.
Affects prompts: 104, 107.

## 2026-11-15 — Owner — gpui-fast-switch backend decision

The owner requests GPUI Fast for the whole application with gpui-kit and gpui-webview pinned at =0.9.1. This supersedes 070’s decision to leave gpui-fast off and the gpui-fast-off portions of 089 and 103. Preserve X11-only Linux startup and retain web overlay hiding on platforms until composition is demonstrated. Measure the existing gpui-pre release before changing features; do not claim unmeasured performance or unverified composition.

### gpui-fast-switch — retained-mode source audit (2026-11-15)

Graphify's promoted graph at `dc38bf2` matched the checkout before this work; current local changes were checked separately. No production view in `apps/kairo` implements `Render`, a custom `Element`, or `EntityInputHandler`. Rendering uses Kit entities, value elements and `canvas`; input handlers are Kit editor entities (the direct `EntityInputHandler` imports are in the unchanged lyrics-editing and sermon-review integration tests).

| Checked views / source | Invalidation and outcome |
|---|---|
| `kairo-core-gpui::preview::LivePreview` | Frame-map polling calls notify for new frames and the 250 ms still heartbeat; canvas paint records visibility and size; hidden after 1 s releases subscription. `FrameSwap`, per-window `drop_image` and release-time `App::drop_image` retained. No invalidation change needed. |
| `ui-operator::OperatorWorkspace`, `LiveRail` | Existing 250 ms backend snapshot loop updates tracked rail state and calls its setters; transport samples notify only when changed. Width/UI-state reads seed constructors and setters; no mutex read in render. Resize drag views are immutable payloads. No change needed. |
| `ui-operator::Booth`, `TranscriptPanel`, `DetectedPanel`, `QueueSearch`, `QueuePanel`, `Toolbox` | Service topic streams pass through channels into Booth.apply, which notifies. Panels observe/read Booth and editor entities. Pending-auto expiration explicitly notifies; text edits, results and async replies notify. Paint-latency channels update diagnostic counters only. No change needed. |
| `ui-operator::TimersPanel`, `ServicePanel`, `VideoPanel`, `SoundPanel`, `MessagesPanel` | Existing timer clocks (250 ms), running-service clock (1 s), video polling and state subscriptions notify their own views. Saved/paused readouts are static. Slider entities own input updates. LayoutDrag is immutable. No change needed. |
| `ui-operator::PropsPanel` | **Changed:** a timer inside a prop thumbnail reads wall time without a tracked mutation. Own 250 ms notification task starts only while props contain timer objects and is cancelled when they do not. A sibling TimersPanel's clock cannot invalidate this retained panel. Regression test reproduced the missing notification. |
| `ui-operator::PropsEditor` | Existing 250 ms clock notifies when a timer object is in the design. Draft, gesture and autosave updates notify; otherwise reads tracked Booth/local state. No change needed. |
| `ui-media::MediaDockView`, `Inspector`, `TracksPanel`, CardDrag/HeightDrag/WidthDrag/HandleDrag | Media model subscriptions notify; poster completions write the model and notify; PreviewPlayer frame/error/position messages notify, and owning views observe it. Frames replaced in the atlas are evicted. Tracks sample only while playing and notify; persisted UI state is read at construction, then entity-owned. Drag payloads are immutable. No change needed. |
| `ui-lyrics::LyricsWorkspace`, `QuickOpen`, `SongEditor`, `ImportDialog` | Read tracked Lyrics/editor entities and observe model changes. Async search/translation/import/spelling replies notify. Editor Instant is undo grouping, not a render clock. No change needed. |
| `ui-scripture::ScriptureWorkspace`, `SermonReview`, `VerseCard`, ItemDrag | Model/editor entities and subscriptions own updates; thumbnails notify through tracked cache state, virtual lists own scroll position. Card and drag elements are value inputs. No change needed. |
| `ui-documents::DocumentsPage` | Import progress and service feeds notify; slideshow timers and 350 ms playback polling notify; embedded PreviewPlayers are observed, replaced atlas images released. No change needed. |
| `ui-theme::ThemeEditor`, `ThemeInspector`, `ColorField`, `ThemePicker` | Entity-owned edits, colour/slider subscriptions, preview responses, saves and saved-flash expiry notify. Canvas reads editor state and engine LivePreview; history wall time is edit grouping, not animation. No change needed. |
| `ui-audience::AudiencePage` | Feed/reply updates notify, existing 5 s clock updates Slido freshness/question ages. Input and menu state are tracked. No change needed. |
| `ui-web::page::WebPage`, `ui-web::slido::WebPage`, `NativePreview` | Channel deliveries notify page state; Slido atomics/mutexes are host controls and IPC state, not render inputs. Native WebView geometry/visibility comes from the Kit adapter. No app-owned native paint implementation. No change needed for invalidation. |
| `ui-capture::LiveMenu` | **Changed:** own 1 s clock while recording on macOS; stops on status change. Shell's recording clock cannot invalidate a retained child popover. Regression test reproduced this and checks redraws stop after recording ends. |
| `ui-capture::CaptureSettingsWindow`, `ui-transfer::TransferHost` | Model observations, async progress/feed messages and input callbacks already notify. No change needed. |
| `ui-settings::SettingsWindow` and its account pane | **Changed:** minute clock notifies only on Account with a last-sync timestamp, so relative age updates without a service push or pointer activity. Other panes use tracked settings and subscribed backend/meter events; saved-flash expiry already notifies. Regression test reproduced missing aging notifications. |
| `ui-screens::ScreensWindow`, FrameResize | Backend replies, display/readiness feeds, gestures and editor callbacks notify. Open-time Instant is diagnostic, not rendered animation. No change needed. |
| `ui-account::AccountFlow`, `AccountGate`, `ActivationDialog`, `AuthPanel`, `VerifyEmail`, `LicensePanel`, `DisplayPicker` | Tracked Account plus subscriptions and local setters/async completions notify. No change needed. |
| `ui-account::OnboardingWizard`, `CoachTour` | Existing request_animation_frame drives Instant-based motion and target following. Fast 0.1.14's Window::request_animation_frame explicitly notifies the requesting view next frame, so no redundant notify-from-render is added. Coach anchor registry mutations are followed by this frame request. No change needed. |
| `ui-shell::Shell`, `LaunchScreen`, `ToolbarButton` | Existing launch/fade frame requests, bootstrap feeds, toast/feedback expiry and running recording clock invalidate Shell. Crash-guard Instants are evaluated after an action or existing notification, not an animation. Pure toolbar/launch elements get values from Shell. No change needed. |
| `kairo-ui-kit`: NumberField/Scrub, ZoomControl, CodeField, LibrarySection/LibraryDragItem, MovableDialog/DialogDrag, SidebarDrag | Stateful setters, input events, drag callbacks and landed-feedback expiry notify or refresh windows; PageSidebar is a tracked global. Drag payloads are immutable. No change needed. |
| `kairo-ui-kit`: PrefGroup, FrostedPanel, TrafficLights, SelectionBar/SelectionAction, RailItem, DragPill, MarqueeBand, SegmentedControl, KairoMark/KairoLockup, AnimatedLockup, StageGlow, Gallery | Value components receive all render inputs from owners; animation time is supplied by the owner's existing frame loop. Gallery changes go through callbacks/notifications. Tokens/themes are GPUI globals, not externally mutated locks. No change needed. |
| `apps/kairo` wiring | Settings and service locks are read in backend snapshots/calls, then delivered through typed topics/feeds into entities; renderers do not read these locks. Main-thread UI bridge, engine notifications, theme updates and incoming launches retain their existing explicit updates. Development benchmark uses that same transcript topic bridge. No production invalidation change needed. |

Benchmark setup is behind the existing `dev-hooks` feature, never enabled by packaging: an explicitly named, marked isolated profile with 5,000 generated song files and 2,000 tiny PNGs, real application services/views, native scroll events, and GPUI profiler frame events. No keys are read or saved for that empty profile: the normal unavailable-keychain path is used after a process sample proved Keychain decryption was blocking startup. The normal application still uses KeyringProtector. Real saved transcript finals are replayed locally through the typed topic, with derived interim text; no transcript contents are committed. Measurements describe this UI workload, not audio decoding, network STT or media decoding. The first library run overlapped fixture startup and produced no complete frame report: discarded. No numbers have been invented for missing cases.

### gpui-fast-switch — dependency and API compatibility (2026-11-15)

`gpui-kit = { version = "=0.9.1", features = ["gpui-fast"] }` is the workspace dependency; `ui-web` enables the corresponding feature on `gpui-webview =0.9.1`. Every application crate continues importing Kit; a literal scan found no direct gpui-pre/gpui_pre references in application/crate Rust or manifests. Main has a compile-time assertion on the facade's documented `GPUI_FAST` constant, and WebView 0.9.1 already asserts that its own feature agrees with Kit.

Cargo feature trees are saved under `evidence/gpui-fast/`. `cargo tree -i gpui-pre -e features` still reports gpui-pre 0.3.12: Kit/base/component/platform/reqwest's pinned manifests have **nonoptional normal dependencies** on it. These are compiled dependencies, not merely build-script edges; the pinned Kit README calls this additive-feature compatibility setup and explicitly says upstream packages remain build dependencies while only Fast types are exposed. No misleading claim of an empty Cargo graph is made, and no registry sources or `[patch]` shims are introduced. Application runtime selection is through the single Fast facade; release-symbol verification is recorded with the gate results.

No existing version was moved: taffy remains 0.15.0 and unicode-properties remains 0.1.3 (070's exact-pin resolution); gpui-fast and all its platform crates remain 0.1.14, gpui-kit and gpui-webview remain 0.9.1. The profiler adds hdrhistogram 7.6.0 to the lockfile. Feature resolution adds the Kit Fast-web/reqwest edges and WebView's optional objc2/objc2-foundation edges, all to already locked packages. Resolving offline once updated those feature edges; subsequent --locked checks work.

API checks: Fast platform::linux still accepts WindowingModes, so the 103 X11-only startup call is unchanged. Fast Window::drop_image(Arc<RenderImage>) -> Result<()> and App::drop_image(image, window) match current callers: LivePreview's FrameSwap/atlas eviction and media PreviewPlayer need no signature edits. Fast request_animation_frame schedules cx.notify(current_view), preserving existing launch, onboarding and coach animations. Application custom drawing is Kit canvas rather than a raw Element implementation; no trait adaptation was needed. Kit's test-support and #[gpui_kit::test] use its backend-aware facade and macros; existing text/IME handler tests are retained. The macOS workspace clippy gate passed with -D warnings after the switch without any API replacement or suppression.

Composition: gpui-webview 0.9.1's feature automatically calls enable_window_composition and attaches its native surface on macOS and Linux, with a fallback if attaching fails. The pinned Windows adapter still uses an ordinary WebView2 child window; newer upstream README claims are not a reason to assume this pinned adapter supports composition. Keep 089's shell-overlay hiding and pointer clearing until the platform is actually exercised; keep preview pausing/hiding off-route for reduced work. Windows and Linux X11/Wayland composition are not yet verified. No workaround is removed merely because a feature compiled.

### gpui-fast-switch — remove the linked legacy core (2026-11-15)

Release `nm` inspection disproved a stronger interpretation of the published feature setup: besides the Fast core, 222 symbols still carried the original gpui-pre crate identity, including NoAction/Unbind inventory constructors. No old App::new was found, but these are linked legacy code, not merely a build-only dependency. The initial core-symbols.json check therefore reported false; it is a diagnostic finding, not success evidence.

Decision before implementation: use the GPUI Fast repository's documented compatibility-shim pattern (compat/README.md) locally for gpui-pre and gpui-pre-platform. Each package retains the exact 0.3.12 snapshot package/version that pinned Kit expects and only re-exports its corresponding real Fast 0.1.14 crate, forwarding existing features. This replaces the legacy implementation and backend dependency edges rather than altering registry sources, bumping Kit/WebView, or pretending that an unused runtime dependency is build-only. Application imports remain gpui_kit; the shim package names exist solely to satisfy published upstream dependency identities. They contain no replacement behavior or test fake. Other gpui-pre-* utility/macro packages remain where Fast itself uses them. Repeat the core-symbol check and all affected gates after this change.

Compatibility result: both local shim packages compile with their forwarded features. Final release nm finds only the Fast GPUI identity (`Cs1H6vbwbRB0T`), with no baseline identity (`Cs1gJGNQfaVmb`); core-symbols.json records true. The old core/platform registry entries and unused legacy backend packages leave Cargo.lock; source identities change to the local, same-version re-exports, not new framework versions. Workspace clippy and the 2052/0/44 desktop-access test run pass after this change. The existing golden/UI tests, image eviction paths, canvas elements and EntityInputHandler editor tests compile and pass unchanged. Mac web composition is enabled by the adapter but not manually qualified, so no 089 workaround removal is claimed.

## 2026-11-15 — 104 — Release pipeline

Decision: one entry point, `.github/workflows/release-native.yml`, manual (`workflow_dispatch`) only until prompt 112. It calls `ci.yml` (fmt, clippy, tests on macOS, Windows and Ubuntu) and the three platform workflows as reusable workflows on the release tag, and its `publish` job `needs` all of them, so a failed or cancelled job skips publishing (the Electron rule: "a partial matrix can never publish"). The platform workflows lost their `v*` tag trigger; they now run from the release workflow or by hand, and never publish. `fail_job` fails one platform on purpose, at its first step, for the dry run.
Decision: publishing. Until 112 every native release is created with `--draft --prerelease --latest=false` on `CodeLawd/kairo-presenter`, where deployed apps (electron-updater, `kairo-updater`) and the website (`/api/download/[platform]`, `releases/latest`) read releases. `releases/latest` skips drafts and pre-releases, so Electron keeps publishing normally. After creating it the job checks the release is a draft pre-release with every asset and that `releases/latest` did not change. A published release with the same tag stops the run; an earlier draft of the same tag is deleted and replaced. The token is `KAIRO_PRESENTER_RELEASE_TOKEN` in the `release` environment: a fine-grained token for that one repository with Contents read/write only (owner to create). A dry run may instead target this repository (`CodeLawd/kairo-v2`) with the workflow's own token, and only with a test tag `v<version>-test.<n>`; a test tag cannot target the release repository. Release notes come from the What's New entry, or say "internal only".
Decision: version sequence. Electron stays on 1.x. Native releases are plain `MAJOR.MINOR.PATCH` from 2.0.0 (the Windows file version, the deb version and the packaging scripts need numeric versions, so no `-beta`). `kairo-release check-tag` refuses a native version below 2.0.0 or not above the latest `CodeLawd/kairo-presenter` release, which is Electron's until cutover. Every native version is therefore newer than every Electron install, and an Electron release that strayed onto 2.x would block the native one rather than collide. Electron's `bun run release major` would produce 2.0.0 and must not be used before cutover (kairo-presenter is read-only here, so this is recorded, not enforced there). A draft version is never reused once anyone has installed it; the next attempt bumps the patch.
Decision: the workspace version is the release version and moves only through `cargo run -p kairo-release -- prepare <patch|minor|major|X.Y.Z> [--skip-changelog]`, the port of `scripts/release.mjs`: clean tree, on a branch, nothing upstream it lacks, tag free, version above Electron's latest, changelog entry, DeckLink helper, a y/N confirmation, then `cargo fmt --check`, clippy `-D warnings` and `cargo test --workspace` before anything leaves the machine; then it bumps `[workspace.package] version` and `Cargo.lock`, commits `Release v<v>`, tags `v<v>`, pushes the branch then the tag, and starts the workflow on the tag. A failed bump is undone. It stays at 0.1.0 in the repository until the first native release.
Decision: CHANGELOG requirement. A release needs an entry for its version in `crates/ui-shell/data/changelog.json` (What's New), unless it is internal-only (`--skip-changelog` / `skip_changelog`), as in Electron. The entry is still written in kairo-presenter's `website/src/lib/changelog.ts` (the public page reads it; `draft-changelog.mjs` drafts it there) and embedded by `embed-changelog`. Pre-cutover drafts are internal by definition and use `skip_changelog`: an unreleased 2.x entry on the public changelog would also become the website's fallback download version (`changelog[0].version`) and point at files that do not exist yet.
Decision: `latest-mac.yml` is written by the publish job (`kairo-release mac-feed`), not merged: the macOS packaging (101) makes no feed. It lists the arm64 zip, arm64 DMG, x64 zip and x64 DMG with base64 SHA-512 and size, top-level `path`/`sha512` from the arm64 zip, `releaseDate`, and `minimumSystemVersion: 22.0.0` (DECISIONS 101). No macOS blockmaps are made, so electron-updater downloads the whole zip (its fallback). Windows `latest.yml` and Linux `latest-linux.yml` come from electron-builder (102, 103).
Decision: `kairo-release verify` runs before anything is created: exactly the expected asset names (`Kairo-<v>-arm64.dmg`, `Kairo-<v>.dmg`, `Kairo-<v>-arm64-mac.zip`, `Kairo-<v>-mac.zip`, `Kairo-Setup-<v>.exe` (+ optional `.blockmap`), `Kairo-<v>.AppImage`, `kairo-presenter_<v>_amd64.deb`, the three feeds, `sha256sums.txt`), no stray or empty file, each feed for this version with every listed file present and matching its SHA-512 and size, `kairo-updater`'s own pickers choosing the right file per platform and architecture, the website's four matchers each finding exactly the expected file, and the admin console's installer pattern counting the seven installers. Feeds are read with `kairo_updater::feed::parse_feed`, the parser the app uses.
Decision: `kairo-updater` now reads `files` items at column 0, as PyYAML writes them. The live Electron 1.0.33 `latest-mac.yml` (merged with PyYAML) failed with "unexpected line: - sha512:…" (found in 101). Fixture `latest-mac-1.0.33-merged.yml` is that file.
Decision: DeckLink helper prebuilt verification stays, as `kairo-release decklink`: the SHA-256 of the seven source paths and their LF-normalised text, as `prepare-mac-natives.mjs` computes it, must equal `native/decklink/prebuilt/SOURCE_SHA256`, and on macOS `lipo -archs` must list arm64 and x86_64. `bundle.sh` runs it before bundling, and the release's prepare job runs it.
Decision: build-time API URLs come from the same variables as Electron (`PROD_API_URL`, `PROD_WEB_URL`, plus `PROD_AUDIENCE_API_URL`, `LICENSE_PUBLIC_KEY`) through the platform workflows, as `KAIRO_*`; empty values stop the job and a release build refuses local or non-https URLs at compile time (`build_config.rs`). `KAIRO_ELECTRON_ARCHIVE` and the Electron archive download retire.
Decision: caching. Each platform caches its FFmpeg prefix (keyed on `packaging/common.sh`, its `build-ffmpeg.sh` and `config.sh`; the script's stamp skips a matching build) and Cargo's registry and git downloads (keyed on `Cargo.lock`). Cargo `target` is not cached: four release targets would exceed the 10 GB repository cache and evict each other.
Decision: `scripts/dev-processes.mjs` and its test start the API server and audience service for development. They stay in kairo-presenter with the server; this repository has no server to start. No port.
Edge cases from the sources and where they are handled: bad manual tag (prepare job, `version_from_tag`); tag ≠ manifest (`check-tag`); run not on the tag (prepare job); unknown bump, version not newer, tag exists, dirty tree, detached HEAD, upstream ahead, cancelled confirmation, failed gates (`prepare`); missing or stale changelog entry, entry not dated today (warning) (`check-tag`, `prepare`); empty endpoint variables (platform workflows); no signing certificate (ad-hoc or unsigned with a warning, unchanged from 101–102); signature checks (platform verify scripts); missing upload (`if-no-files-found: error`); arch version mismatch and a single-arch Mac feed (generated feed, `verify`); duplicate file across platforms (stage step); incomplete matrix (`needs` plus `verify`); feed/file hash or size mismatch, stale assets, empty files (`verify`); published release already there (prepare and publish); earlier draft (replaced); missing token (publish); Electron latest unreadable (prepare fails rather than guessing); network blips (download and notarization retries already in the scripts); macOS 11–12 Electron installs (`minimumSystemVersion`); website fallback to `changelog[0].version` (draft releases skip the changelog).
Affects prompts: 105, 111, 112.

## 2026-11-15 — 105 — Owner — Start before 104 is done

Decision: prompt 105 starts while 104 is `in-progress` (its dry run has not run), at the owner's direction. Work that needs 104's published draft release (a real GitHub-hosted upgrade, Windows and Linux artifacts from CI) is recorded as not verified rather than faked; the local macOS test uses a private feed instead.
Affects prompts: 105.

## 2026-11-15 — 106 — Accessibility and keyboard pass

Decision: the audit reads the AccessKit tree GPUI sends to VoiceOver and Narrator (`Window::debug_a11y_tree_json`). `.cargo/config.toml` sets `GPUI_A11Y_ACTIVE=1` for every process cargo runs, which GPUI honours only in `test-support` builds, so every UI test window builds the tree. `kairo_ui_kit::a11y_audit::check` runs in each UI crate's test draw helper (18 harnesses), so every state an existing UI test reaches is audited. Rules: every control (pressable, checkable, editable or clickable node) has a name (an edit field may use its placeholder, as Chromium does); every pressable role offers `Click` unless Kit reports it disabled. Sibling-name uniqueness is an opt-in check (`twin_names`), because Electron repeats per-row names ("Reply" on each question) too. `KAIRO_A11Y_AUDIT_LOG=<file>` logs violations instead of failing, for surveys.
Decision: GPUI does not name a node from its child text as Chromium does, so a custom `div` button with visible text was announced as "button". Every custom control that the audit found gets an explicit `aria_label` with the visible text (or the Electron `aria-label` where one exists), plus `aria_selected` or `aria_toggled` where it shows a state.
Decision: Kit `Popover` and `DropdownMenu` open on a mouse press on the trigger. A Kit `Button` without a click handler offers no `Click`, so VoiceOver's VO-Space and Narrator's Invoke did nothing on 79 triggers. `PopoverTriggerExt::popover_trigger` adds an empty click handler. The node then offers `Click`, and GPUI answers a press that has no listener with a synthesized mouse press and release at the node's centre, which the popover handles as it handles the pointer (checked: the rail popovers still open with the mouse). `PopoverButtonTriggerExt::button_trigger` does the same for `Popover::trigger`. The shell's `ToolbarButton` popovers (Live, Screen Themes) instead register an accessibility `Click` that toggles the shell's open state, and report `expanded`.
Decision (deviation): a custom `div` button that is busy or unavailable (import busy, no search results, no page to bookmark, locked screen, the open song, settings Back/Forward at the end of history) keeps an empty click handler. A press from assistive technology does nothing, as the pointer does. GPUI has no public `aria_disabled` (DECISIONS 070), so these do not read as dimmed. Kit buttons keep Kit's disabled flag.
Decision: canvas objects that select on pointer press (props stage objects, the theme canvas delete handle) register an accessibility `Click` that selects or deletes directly, and report `selected`.
Decision: modal dialogs follow Radix `Dialog`: focus moves in, Tab cycles inside, Escape closes (already handled), and focus goes back to the trigger on close. `kairo_ui_kit::components::ModalFocus` does this for dialogs a view draws itself: `open` where a `Window` is at hand, `sync` from `render` otherwise. Its handle goes to the dialog's `focus_trap`. Applied to What's New, the quit question, the lyrics Import Song and Song usage dialogs, the Media Inspector, the documents import and converter dialogs, the Engage dialogs, and the account gate (trap only; it covers the app). `MovableDialog` already did this. The quit question gives initial focus to Keep Kairo open (DESIGN.md). The screen-themes picker and slideshow menu are popover content, and the coach card is non-modal, as in Electron.
API difference: `FocusTrapContainer` replaces its element's id with the trap id, which changed the AccessKit node id and broke id lookups in tests. Each trap reuses its element's own id. Kit's component `Button` implements only `InteractiveElement`, so it has no `on_a11y_action` or `aria_expanded`. Kit's `ColorPicker` swatch button opens on a mouse press and takes no handler from outside, so it offers no `Click`. The audit excludes that one source location (`a11y_audit::kit_limit`) and still requires its name. Fixing it needs a Kit change.
Decision: reduced transparency and increased contrast use solid chrome (`index.css:686`, DESIGN.md). Native has no window vibrancy or backdrop blur (DECISIONS 070, with its 074 amendment); the window background is the solid `surface`, so the header, rail and page sidebars were already solid, which is what Electron's media query produces. The remaining translucent tones (`.glass` 55%, `.modal-glass` 78%, the unused 62% material tint) are drawn opaque under `KairoColors::solid_chrome`. This goes beyond Electron, whose query did not touch `.glass`: the native glass has no blur behind it, so its transparency only lowers contrast. The app reads `accessibilityDisplayShouldReduceTransparency` and `accessibilityDisplayShouldIncreaseContrast` at launch and on window focus, with reduced motion; `KAIRO_SOLID_CHROME=1|0` overrides them. Windows and Linux are not read yet, as for reduced motion (072).
Affects prompts: 107, 109.

## 2026-11-16 — 105 — Bridge validation completion

Decision: finish the bridge preparation while 104 remains in progress, continuing the owner's 2026-11-15 exception. A missing selected feed, a skipped required signing check, or invalid CLI arguments must not produce a successful validation result. Default validation requires all platform feeds; the platform selectors deliberately limit the scope. The native zip must contain exactly `Kairo.app` with id `com.kairo.app`.
Decision: regression tests use the deployed electron-updater 6.8.9 parser and pickers, with synthetic package bytes that are never executed. CI uses a throwaway checkout of the read-only Electron source at `af978c5159f4d433dc93b2de7a8f92f92765b74e`; it installs only the updater's test runtime, without Electron or its app dependencies. These tests prove feed validation, not OS installation or data migration.
Release choices (staging percentage and the final macOS 11/12 Electron message) remain open. No public release, owner acceptance, or unperformed OS upgrade is implied by this preparation.
Affects prompts: 104, 105, 112.

## 2026-11-16 — 107 — Performance budgets and startup fixes

Decision: scripture search budget is p95 under 16 ms (one 60 Hz frame) through `ScriptureService::search`; 06 had none and the Electron baseline measured only the SQL.
Decision: every HTTP client shares one rustls `ClientConfig` (`kairo_net::transport`, via `use_preconfigured_tls`): native roots loaded once, unparseable roots skipped, ring provider, TLS 1.2/1.3, SNI, HTTP/1.1 ALPN, matching what reqwest 0.12.28 built per client. TLS options set on a builder afterwards are ignored; none are used. `rustls` 0.23 and `rustls-native-certs` 0.8 are direct dependencies with reqwest's own features, so no new crates are built.
Decision: performance tests assert budgets as written in optimised builds or with `KAIRO_PERF_STRICT=1`, and 4× in unoptimised runs (gross-regression guard). CI runs them strict in release on macos-15 only.
Waiver (owner, 2026-11-16): the idle-memory target (at most 40% of Electron) is waived for 107. Measured 224,560 KiB with 0 outputs, 55% of Electron. Reduction (32 MB CG image, heap, window drawables in pinned GPUI) is deferred to later work. Lyrics/media scroll draw p95 and CPU are above the 2026-11-15 figures; not confirmed as a regression.
Affects prompts: 108, 109.

## 2026-11-16 — 113 — UI parity cleanup and owner design changes

Deviation (owner, 2026-11-16): the Documents page has no large live-page preview above the slide grid. The live page is marked in the grid (red border and number). The "Preview only · No output connected" status line stays.
Deviation (owner, 2026-11-16): scripture verse cards draw the projected theme's background (the main output's scripture theme: image, else colour; the checkerboard only when transparent). Cards have square corners, no border unless live (red) or picked (white), and a 34 px black reference strip.
Deviation (owner, 2026-11-16): a display output is called "External display" (Electron `af978c5`: "Kairo screen"). The add menu lists External display, NDI feed, Syphon, Video card. Outputs already saved keep their stored names.
Decision: the Live monitor previews an output whose sink is missing. When a video card or display is absent, the engine still composes that output offscreen (`compose_without_sink`), and pushes go to every output the engine composes (`OutputEngine::composes`). The output still reports its sink missing, and dispatch health still counts such a push as not taken.
Decision: Kit's `ButtonCustomVariant` paints its colour at 20% (`mix_oklab(transparent, 0.2)`), so every custom-coloured button gets an explicit solid `bg` with the same colour. Shared controls take Electron's sizes through `kairo_ui_kit::ControlSize`: buttons 32 px tall with 14 px side padding, inputs at least 36 px with 12 px side padding.
Decision: a measuring canvas that needs another frame calls `window.on_next_frame(|w, _| w.refresh())`. `Window::refresh` is ignored during a draw, which left the lyrics, scripture, documents and media grids sized for a stale width. Coach anchors keep their last bounds without a time limit, because cached layers skip repainting unchanged controls (the tour flickered).
Decision: overlays enter with `kairo_ui_kit::motion::enter`: opacity only, over `spring-in`'s 0.2 s curve, skipped under reduced motion. Applied to the shell overlays, What's New, the quit question, `MovableDialog`, the Documents dialogs and slideshow menu, the lyrics import, quick open and song usage dialogs, the Media Inspector, scripture review, the Engage dialog and the coach card. Kit's own popovers, menus and selects are not animated: they are inside the pinned `gpui-kit =0.9.1`.
Deviation (owner, 2026-11-16): lyric and scripture cards default to 240 px minimum width at 100 % (Electron: 180). Scripture card artwork carries a 40% black scrim, and resting cards a 0.5 px border. Lit Live-rail layer buttons get an accent fill and foot bar.
Decision: a measuring canvas notifies its own view on the next frame. `Window::refresh` alone does not redraw a view that gpui-fast keeps cached, which left grids at a stale width. The engine handle forwards content to a video-card surface without its card (`surface_composes`); before, it dropped every slide there, so the monitor stayed empty.
Deviation (owner, 2026-11-16): every clickable control shows a pointer cursor. Kit buttons use the arrow by design (`cursor_default`), so each `Button` sets `cursor_pointer`, and custom clickable rows do the same.
Deviation (owner, 2026-11-16): the New engagement start and end dates use Kit's `DatePicker` (Electron: text boxes). Kit's picker takes no accessible name, so the audit exempts that one source location (`a11y_audit::kit_unnamed`); the visible label sits beside it. The engagement Settings dialog has a fixed 640 px height.
Deviation (owner, 2026-11-16): scripture cards lay out the verse and reference from the main output's scripture theme (frames, font, size, weight, colour, alignment, auto-fit), as the output shows them. Documents and lyrics dialogs use a solid `surface_elevated` panel; the modal glass let the page show through without the blur Electron puts behind it.
Decision: the rail's Audio and Account popovers follow `RailPopovers.tsx`. Audio lists the inputs from the settings backend, saves the choice to the `audio` section, and shows the core's `AudioLevel` while Kairo listens. Account shows the church and role, status, last sync, Show tips again and Sign out (`ShellRequest::ShowTips`, `ShellRequest::SignOut`). The leftover "arrives with prompt" placeholder texts are gone.
Decision: scripture cards take the verse and reference sizes from the engine's own layout (`kairo_scene::analyse` on the slide's `SlideScene` at 1920 × 1080, with auto-fit), scaled to the card and cached per slide, so a card's text matches the Live monitor.
Deviation (owner, 2026-11-16): importing slides first asks which library they go into (default: the library in view) when user libraries exist; the new document is assigned there. Song search sits in the window's centre, 560 px wide, with 40 px rows and an accent-filled selection. The engagement Settings dialog keeps its section list fixed while the pane scrolls.
Deviation (owner, 2026-11-16): the scripture dock keeps only zoom and Previous/Next. Go live (a card click sends), Clear (the search box clears), Save to playlist or library, the verse count and the Verse/Chapter switch are gone from it. The slide-import library choice now comes after the file is imported. Engagement Settings uses the app Settings layout (sidebar panel, header naming the section, scrolling pane). The Video tab's layout preview is 16:9. Card text uses the theme's font face (CSS generic names mapped to macOS faces) and box padding.
Decision: the Video tab's "Allow video access" asks AVFoundation for camera access and lists inputs through `kairo_camera` (`BoothAnswer::VideoInputs`), labelled as the engine opens them. Before, it only answered with a message, so no input could be chosen.
Decision: the sermon notes review uses the dark app chrome with the notes on light paper. A reference written another way (`Mat 18:28` for Matthew 18:28, Electron's `normalizedReferenceLabel`) gets a wavy accent underline in the notes, and every such reference is named (`→ Matthew 18:28`) in a 220 px right margin of the notes page, one label per line, placed at `EditorState::range_to_bounds`. The Kit editor cannot reflow text around Electron's inline chip, so the margin keeps labels clear of the text. The list follows the notes: clicking a reference selects it, and scrolling selects the reference at the reading line (28% down), as Electron does; the review observes the editor because clicks and scrolls raise no event. The header and playlist name share one compact row.
Decision: a theme save announces a settings change (`theme.rs::persist`), so pages that draw output themes, such as the scripture cards, follow the edit; the store itself still does not broadcast `overlay` or `themeLibrary` writes, matching Electron.
Deviation (owner, 2026-11-16): an image or video picked for a theme is copied into the profile's `theme-media` folder (an APFS clone on macOS) and the theme stores the copy, so deleting or moving the original does not break the theme. Media chosen before this change keeps its original path.
Deviation (owner, 2026-11-17): lyric cards take the scripture card style: the main output's lyrics-theme background under a scrim, a 34 px label strip filled with the section's colour, square corners, a hairline border at rest. The Configure Screens setup step is centred vertically.
Decision: an empty `stt.llmModel` means no override, so scripture AI detection uses the provider default (`deepseek-flash`), as Electron does; sending `"model": ""` was rejected by the server's managed-request policy ("Unsupported or unbounded managed request").
Affects prompts: 110, 111, 112.

Decision (owner plan, 2026-11-16): onboarding deliberately departs from Electron parity. AccountGate and setup share an edge-to-edge stage, draggable top bar and 22.5rem form column; setup has no scrim or modal card. Kit Buttons provide command focus, keyboard and pressed states. The six-slide tour remains before account creation; kairo-domain still owns Welcome → Account → Output and backend writes. Interactive movement uses caller-owned springs (0.4 response, critical damping; 0.8 damping on thrown release), drag velocity, momentum projection and edge resistance. Reduced motion snaps navigation. Only mock-up stories and the welcome letter rise retain scripted clocks. Wizard dismissal fades the retained stage into the booth. Real trackpad phase handling and macOS window dragging require hands-on verification.

Decision (onboarding follow-up, 2026-11-16): account/setup now share AccountPage and tour motion lives in TourPage. Preserve signup drafts when walking Back, but suspend hidden account transitions. SetupState derives page and actions without cloning progress; display/church transforms live in kairo-domain. Setup effects run sequentially, reject duplicate user commands, and coalesce automatic account reconciliation instead of queuing stale navigation. Identity/reset generations reject stale effects; display saves additionally check settings generations. A failed automatic account write exposes Try again. Motion uses executor time; scripted entry requests bounded frames, then caret/countdown use deadline timers. Shared command divs are Kit Buttons with the owner's pointer cursor preference preserved. No glow bake: headless CPU/allocation measurements do not establish a GPU bottleneck.
