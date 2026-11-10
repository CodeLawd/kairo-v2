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
