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

Decision: Windows (WebView2 `PrintToPdf`, runtime check) and Linux (`WebKitPrintOperation`) printers are not written. This machine has only the `aarch64-apple-darwin` toolchain, and `webview2-com` and `webkit2gtk-sys` need their targets and system libraries, so the code could not be compiled or run. The printer is a `SlidePrinter` trait, so each platform adds one implementation. Until then, the simplified renderer has no printer on those platforms.

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
