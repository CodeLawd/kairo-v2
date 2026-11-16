# 105 — Bridge from Electron to native

Status: bridge tooling and evidence prepared; live qualification and owner
review remain open. Nothing here has been published. A public bridge release needs the owner's explicit approval
(prompt 105, section 5).

## 1. How Electron installs in the field update today

Read from `../kairo-presenter` at `af978c5` (no drift in the updater files
since `de3bb54`) and from its `node_modules/electron-updater` 6.8.9, the
version 1.0.33 ships.

Every Electron install reads the GitHub provider baked into
`Resources/app-update.yml` (`CodeLawd/kairo-presenter`, `releaseType:
release`). electron-updater asks `releases/latest`, which skips drafts and
pre-releases, then fetches `latest-mac.yml`, `latest.yml` or
`latest-linux.yml` from that release's tag. `autoDownload` is off (the
operator presses Download); `autoInstallOnAppQuit` is on.

### Checks a deployed client runs before it installs

| # | Check | Where | What the native release must do |
|---|---|---|---|
| C1 | Feed parses; `version` is valid semver | `parseUpdateInfo`, `isUpdateAvailable` (`ERR_UPDATER_INVALID_VERSION`) | electron-builder layout; plain `X.Y.Z` (104: native starts at 2.0.0) |
| C2 | Feed version greater than the running one; equal is not offered, older only with `allowDowngrade` (off) | `AppUpdater.isUpdateAvailable` | 2.x is above every 1.x install |
| C3 | `minimumSystemVersion` against `os.release()` (Darwin version) | `AppUpdater.checkIfUpdateSupported` | `minimumSystemVersion: 22.0.0` in `latest-mac.yml` (DECISIONS 101), so macOS 11 and 12 keep Electron |
| C4 | `stagingPercentage` (absent: everyone) | `isStagingMatch` | Leave it out unless the owner wants a staged rollout (see 4.3) |
| C5 | macOS file pick: arm64 file on Apple silicon and under Rosetta, non-arm64 on Intel; a zip must exist | `MacUpdater.filterFilesForArch`, `findFile(files, "zip", ["pkg","dmg"])` (`ERR_UPDATER_ZIP_FILE_NOT_FOUND`) | `Kairo-<v>-arm64-mac.zip` and `Kairo-<v>-mac.zip` in the feed (104 `verify` requires both) |
| C6 | Download SHA-512 equals the feed (base64) | `DownloadedUpdateHelper` / `executeDownload` | `kairo-release mac-feed` and electron-builder write it |
| C7 | Differential download: needs `<file>.blockmap`; any failure falls back to the full file | `differentialDownloadInstaller` | No macOS blockmaps: always a full download. Logged as an error, harmless |
| C8 | Squirrel.Mac (Developer ID builds, 1.0.33 is one): the zip holds a bundle with the running app's bundle id, and that bundle satisfies the running app's designated requirement (`identifier "com.kairo.app" and anchor apple generic … leaf[subject.OU] = YK7WF42S33`) | Electron's `autoUpdater` (Squirrel.Mac) behind electron-updater's local proxy | Native bundle id `com.kairo.app`, signed with the same Developer ID team, sealed (`--strict`), zip made with `ditto -c -k --keepParent` |
| C9 | Self-update path (ad-hoc signed Electron builds only): `pickMacZip` by `/(arm64\|aarch64)/`, `releaseAssetUrl` accepts only a bare file name or a URL under `github.com/CodeLawd/kairo-presenter/releases/download/`, SHA-512 equal, `ditto -x -k`, first `*.app` in the zip, bundle id equal, `codesign --verify --deep --strict`, quarantine removed, swap script on quit; install location writable | `services/updater/index.ts`, `mac-self-update.ts` | Bare file names in the feed; one `Kairo.app` at the zip root; strict seal |
| C10 | Windows: `findFile(files, "exe")`; SHA-512; Authenticode publisher check only when `app-update.yml` has `publisherName` | `NsisUpdater.doDownloadUpdate`, `verifySignature` | 1.0.33's installer is unsigned (PE security directory is empty, checked by range request), so its `app-update.yml` has no `publisherName` and no publisher check runs. A signed native installer is accepted too |
| C11 | Windows launch: `Kairo-Setup-<v>.exe --updated` (+ `/S` on an ordinary quit, `--force-run` from Install); `elevate.exe` only if the feed sets `isAdminRightsRequired` or spawn fails with EACCES/UNKNOWN; ENOENT opens the file with the shell | `NsisUpdater.doInstall` | electron-builder's NSIS template, same GUID (102), per-user, no `isAdminRightsRequired`; the new installer runs the old uninstaller with `/S /KEEP_APP_DATA --updated` |
| C12 | Linux AppImage: `$APPIMAGE` must be set and absolute; `findFile(files, "AppImage", [...])`; embedded-blockmap differential, full download on failure; old file unlinked, new one moved beside it (new name when the old name has a version) | `AppImageUpdater` | `Kairo-<v>.AppImage` in `latest-linux.yml` (electron-builder) |
| C13 | Linux deb (resources/`package-type` = `deb`): `findFile(files, "deb", [...])`; `dpkg -i` through pkexec/gksudo/kdesudo/beesu/sudo, then `apt-get install -f -y`; error if neither dpkg nor apt | `DebUpdater`, `LinuxUpdater` | `kairo-presenter_<v>_amd64.deb`, package name `kairo-presenter`, `/opt/Kairo` (103) |
| C14 | Errors shown to the operator are generic ("Could not download the update. …") and the details logged | `errors.ts` `updateErrorMessage` | Nothing to match; every failure leaves Electron installed |

### Data

The bridge replaces only the program. Every Electron profile stays where it
is (`~/Library/Application Support/Kairo`, `%APPDATA%\Kairo`,
`~/.config/Kairo`); the Windows uninstaller is run with `/KEEP_APP_DATA`.
The native app's first-run migration (prompt 100) backs the profile up to
`Native Migration Backups/<time>/` before it changes anything, and leaves
Electron's files readable for rollback. electron-updater's download cache
(`~/Library/Caches/kairo-presenter-updater`, `%LOCALAPPDATA%\kairo-presenter-updater`,
`~/.cache/kairo-presenter-updater`) is left behind; it is a few hundred MB at
most and is not read by the native app.

## 2. Verdict per platform

| Platform | Can the existing updater install native? | Evidence | Remaining risk |
|---|---|---|---|
| macOS, Developer ID Electron (1.0.33 and every signed release) | Yes, by Squirrel.Mac, if the native app is signed by team YK7WF42S33 | Section 3 | Notarized release build not exercised through Squirrel; Intel package never built (101) |
| macOS, ad-hoc Electron (older self-update builds) | Yes, by the self-update path, if the zip has one sealed `Kairo.app` | Section 3 (checks C9 run against the native zip) | Older versions may carry older updater code than `af978c5`; not tested |
| macOS 11 and 12 | Not offered (kept on Electron) | Section 3 (C3) | They stay on the last Electron release; see 4.4 |
| Windows | Expected yes: unsigned Electron does no publisher check; the native installer is the same electron-builder NSIS target with the same GUID | Analysis only | Never run on Windows (102 has no CI pass); the upgrade over 1.0.33 is in `install-test.ps1` but has not run |
| Linux AppImage | Expected yes | Analysis only | Never run on Linux (103) |
| Linux deb | Expected yes (pkexec prompt, as for Electron updates) | Analysis only | Never run on Linux (103) |

No platform needs a one-time Electron guide release on the evidence so far.
Section 4.4 describes one in case the Windows or Linux tests fail.

## 3. Test evidence

Run on 2026-11-15, M2 Pro, macOS 27.2 (Darwin 27.2.0), Apple silicon only.
Files in `native-migration/evidence/105/`.

Native test build: `packaging/macos/package.sh arm64` in a throwaway worktree
of `04df31e` with the workspace version set to `1.99.0` (above 1.0.33, below
the 2.x a real release uses, so it can never collide with one), production
endpoints, Developer ID signed (team YK7WF42S33), **not notarized**
(`KAIRO_NOTARIZE=0`). `verify-package.sh` passed on the app and the DMG
(Gatekeeper "rejected" is the expected result for an un-notarized build).
Zip 69,888,773 bytes, DMG 69,817,355 bytes, app 191,712 KiB.

Private feed: `latest-mac.yml` in the layout `kairo-release mac-feed` writes,
listing only the arm64 zip and DMG (no Intel build exists), with
`minimumSystemVersion: 22.0.0`, served by `python3 -m http.server` on
127.0.0.1.

1. **Harness, native feed** (`harness-native-1.99.0.txt`): 14 passed, 2
   failed. Passed: C1–C3 (hidden on Darwin 20 and 21, offered on 22 and 25),
   C5 for Apple silicon and Rosetta, C9 file pick and URL rule, C6 hash and
   size, one `Kairo.app` at the zip root, bundle id `com.kairo.app`,
   `codesign --verify --deep --strict`, and the installed 1.0.33's designated
   requirement (C8). Failed: the two Intel picks, because the feed has no
   Intel zip. That is a real gap until an x86_64 package exists (101); 104's
   `verify` refuses a release without it.
2. **Harness, negative** (`harness-adhoc-negative.txt`): the same zip with
   the bundle re-signed ad hoc fails the designated-requirement check, as
   Squirrel.Mac would refuse it, while still passing the self-update path's
   checks. A native release signed by any other team would be refused by
   every Developer ID Electron install.
3. **Harness, Electron's own 1.0.33 feeds** (`harness-electron-1.0.33-feeds.txt`,
   from 1.0.31): the pickers agree with what Electron shipped; the macOS 11
   and 12 checks fail because Electron's feed has no `minimumSystemVersion`,
   which shows the check tells the two apart. File checks fail only because
   the files were not downloaded.
4. **Live Squirrel.Mac upgrade** (`squirrel-standin-1.0.33-to-1.99.0.log`):
   `make-standin.sh` turns kairo-presenter's Electron 33.4.11 into a
   `com.kairo.app` 1.0.33 signed by the same identity (designated requirement
   byte-identical to `/Applications/Kairo.app`'s), running electron-updater
   6.8.9 from kairo-presenter's `node_modules` with 1.0.33's settings
   (`autoDownload` off, `autoInstallOnAppQuit` on) and a private profile
   folder. Result: check found 1.99.0, full download (no previous zip, no
   blockmap: C7 fallback as expected), SHA-512 accepted, the local proxy
   served the zip to Squirrel.Mac, Squirrel accepted and staged it
   (`nativeUpdater.update-downloaded`), the stand-in quit, and ShipIt had
   replaced the bundle in place by the time the process had exited:
   `CFBundleShortVersionString` 1.99.0, `Contents/MacOS/Kairo` and
   `kairo-engine`, `codesign --verify --deep --strict` valid, no quarantine
   attribute. The profile file's SHA-256 was unchanged. ShipIt's own log
   (`shipit-stderr.log`) ends "Installation completed successfully", about
   1.5 s after the quit (21:53:46.712 to 21:53:48.169). Its "Couldn't remove
   owned bundle" line is about its temporary copy and appears on Electron
   updates too.
5. **First launch after the swap**: the swapped bundle started with an empty
   `HOME`, spawned `kairo-engine` from `Contents/MacOS`, created its profile,
   and the native updater read its own designated requirement (same team).
   The Electron-format data path was not exercised here (see 7).

### Not verified

- A real Electron 1.0.33 install upgrading itself. Two attempts to run a
  copy of `/Applications/Kairo.app` with a scratch `HOME` failed to isolate
  it: Electron ignores `HOME` and `CFFIXED_USER_HOME` for `userData`, and
  opened the owner's real profile (see section 8). The stand-in replaces it;
  it differs from 1.0.33 only in app code, not in Electron, electron-updater
  or signing.
- Install from the operator's Install button (`quitAndInstall`, relaunch):
  not run, because the relaunched app would get the real `HOME`. Squirrel
  installs the same way; only the relaunch differs.
- The self-update path running live (an ad-hoc signed Electron build).
  Only its checks were run (harness, C9).
- A notarized build, a real GitHub-hosted draft (needs 104), Intel, macOS 13
  as the oldest supported system.
- Data preservation on upgrade: the native first-run migration over a real
  Electron profile was not run on this Mac, because it would read the owner's
  keychain. Prompt 100's tests cover the migration itself; the owner's VM run
  (section 7) covers it end to end.
- Windows and Linux: nothing run. There are no native artifacts yet (102 and
  103 have no passing CI).

## 4. Release plan (owner decisions marked)

1. 104 publishes a native `2.0.0` as a draft pre-release. Deployed Electron
   apps cannot see it (`releases/latest` skips drafts).
2. Owner runs the live upgrades below on clean machines from that draft,
   using a private feed (generic provider in a copy of the installed app's
   `app-update.yml`, or the stand-in in 6.2), never the public release.
3. **Owner decision: staged rollout.** electron-updater honours
   `stagingPercentage`; `kairo-updater` ignores it (DECISIONS 048). A first
   public release could carry `stagingPercentage: 10` in all three feeds, then
   be raised by editing the feeds on the release. Without it every Electron
   install is offered native at once.
4. **Owner decision: macOS 11 and 12.** They will never be offered native.
   Options: (a) do nothing, they keep 1.0.x; (b) a last Electron release
   (1.0.34) whose What's New says that Kairo 2 needs macOS 13. (b) needs an
   approved edit in `../kairo-presenter` and is not done here.
5. Publishing for real is prompt 112's cutover: make the draft a normal
   release with `--latest`. That flips every Electron install's
   `releases/latest`. Rollback: delete or re-draft the release; installs that
   already moved stay on native (`kairo-updater` will not offer 1.x, which is
   older), and Electron 1.0.33 can be reinstalled from its release page over
   the native app, reading the same, backed-up profile.

## 5. Checks a release must pass before it is published

`node packaging/bridge/check-electron-bridge.mjs --dir <downloaded draft
assets> --electron ../kairo-presenter` on a Mac with an installed
Developer ID Electron Kairo. It runs electron-updater 6.8.9's own parser and
pickers, kairo-presenter's `mac-self-update.ts`, and the codesign checks
behind C8 and C9, for whatever feeds are in the folder.

## 6. Edge cases from the sources

| Path in the Electron source | Effect on the bridge | Handled by |
|---|---|---|
| Unpackaged / dev run: updater disabled | Not in the field | — |
| No `app-update.yml` / repo with no releases (error is "normal") | Unchanged | — |
| `checkForUpdates` network failure, 5xx, 404 feed (`ERR_UPDATER_CHANNEL_FILE_NOT_FOUND`) | Electron shows the generic message and stays on Electron | Every platform workflow uploads its feed; 104 `verify` |
| Invalid feed version (`ERR_UPDATER_INVALID_VERSION`) | No update | Plain semver from `kairo-release` |
| Same or older version | `update-not-available` | 2.x |
| OS below `minimumSystemVersion` | `update-not-available` | `22.0.0` (harness checks Darwin 20/21/22/25) |
| No zip for this Mac (`ERR_UPDATER_ZIP_FILE_NOT_FOUND`, self-update "no update file for this Mac") | Error, stays on Electron | Both macOS zips required by 104 `verify` (Intel package still missing) |
| SHA-512 mismatch ("did not match its published checksum") | Error, file removed | Hashes from the published bytes |
| Differential download fails | Falls back to full download | Expected (no macOS blockmap) |
| Zip without an app ("did not contain an app") / other bundle id ("The update is not Kairo") | Error | One `Kairo.app`, `com.kairo.app` (harness) |
| `codesign --verify` fails / Squirrel signature mismatch | Error, stays on Electron | Developer ID, team YK7WF42S33, strict seal (harness, negative test) |
| `releaseAssetUrl` refuses unexpected addresses or names | Error | Bare file names in feeds |
| Install location not writable (self-update) | Error telling the operator to download from the website | Unchanged; the website serves native once 112 publishes |
| Quit while downloaded (`will-quit` swap, `autoInstallOnAppQuit`) | Installs on ordinary quit | Live test 4 |
| Before-install hook failing | Ignored, quit continues | Unchanged |
| Windows unsigned installer vs `publisherName` | No check (no `publisherName` in 1.0.33) | Either signed or unsigned native installer works |
| Windows spawn EACCES/UNKNOWN → `elevate.exe`; ENOENT → shell open | Electron behaviour | No `isAdminRightsRequired` (harness) |
| Linux `APPIMAGE` unset or relative (`ERR_UPDATER_OLD_FILE_NOT_FOUND`) | Error | Unchanged |
| Linux deb with neither dpkg nor apt | Error | Unchanged |
| macOS 11/12 left on Electron | Never offered native | Owner decision 4.4 |
| Electron updater cache left behind | Disk only | Recorded; not removed |
| Native app on first run offered a 1.x feed (downgrade) | `kairo-updater` offers only greater versions | 048 |

## 7. Owner run on the macOS 13 VM (still to do)

On a clean UTM VM with macOS 13:

1. Install Electron 1.0.33 from its DMG, sign in, create a song, a setlist and
   a theme, pick a microphone. Quit.
2. Build a 2.x draft through 104 (or the 1.99.0 test zip above), serve a
   private feed (`latest-mac.yml` + zip) on the VM, and point the installed
   app at it by replacing `Kairo.app/Contents/Resources/app-update.yml` with
   `provider: generic` / `url: http://127.0.0.1:8765/`.
3. Open Kairo, press Download, then Install. Expect: relaunch into native,
   `Native Migration Backups/<time>/` created, songs, setlist, theme,
   sign-in and microphone present.
4. Repeat with an ordinary quit instead of Install.
5. Repeat on Windows 10/11 over Electron 1.0.33 (`Kairo-Setup-1.0.33.exe`)
   and Ubuntu 22.04 over the 1.0.33 AppImage and deb, once 102 and 103
   produce artifacts.

## 8. Incident during this run

Two attempts on this Mac launched Electron Kairo 1.0.33 against the owner's
real profile (`~/Library/Application Support/Kairo`), for about a minute and
about thirty seconds: once a copy with a scratch `HOME`, once
`/Applications/Kairo.app/Contents/MacOS/Kairo --version`, which starts the
app. Both did a normal app start (settings, lyrics database, usage and
licence-cache files rewritten, token refresh). Both were stopped by signal.
The first stop added one renderer "killed" entry dated
2026-11-15T20:31:31Z to `kairo-crash-reports.json`. Nothing was deleted, and
the native app never ran on that profile. The owner may want to remove that
crash entry.


## 9. Implementation follow-up — 2026-11-16

The checker and regression suite are in `packaging/bridge/`; the operator
runbook is [`packaging/bridge/README.md`](../../packaging/bridge/README.md).
The updater sources still have no drift since `de3bb54`, at Electron
`af978c5159f4d433dc93b2de7a8f92f92765b74e`, and were not edited.

Fixed validation gaps:

- Missing selected feeds fail. Default validation requires all platforms;
  a partial check requires an explicit platform selector.
- Skipped required checks return failure, including an absent installed
  signing reference or running Mac bundle checks on another OS.
- Mac zips must contain exactly `Kairo.app`, id `com.kairo.app`; a differently
  named bundle can no longer bypass the signature checks. Zip checksums run
  even when Mac extraction tools are unavailable. The OS boundary is tested
  at Darwin `22.0.0`, the actual macOS 13 floor.
- Windows elevation is checked on the selected file's metadata, where
  `DownloadedUpdateHelper` actually reads it, as well as at the feed root.
- Unknown, duplicate, missing-value and conflicting CLI options are rejected.

`node --test packaging/bridge/check-electron-bridge.test.mjs`: 19 passed,
0 failed, 0 skipped on this Mac. The regression evidence is
[`bridge-regressions-2026-11-16.txt`](../evidence/105/bridge-regressions-2026-11-16.txt).
The initial four validation regressions and the per-file elevation regression
were observed failing before their fixes. Both Mac regressions were also
reproduced by restoring the old behaviour temporarily, then the fixed suite
was rerun green. Test packages are synthetic and never installed; the Mac
fixtures are temporary ad-hoc signed copies of `/usr/bin/true`, not native
release artifacts or evidence of Intel compatibility.

The reusable CI workflow now runs these tests on Ubuntu and macOS, against
pinned Electron source and updater 6.8.9, which also gates release CI. Workflow
YAML and script syntax were checked locally; the new CI job has not run on
GitHub. The release pipeline still produces drafts only.

Outstanding acceptance remains sections 3 and 7: release artifacts from 104,
real install/relaunch and data-preservation checks on the supported OSes,
and owner evidence review. Rollout percentage and the final macOS 11/12
message remain undecided. This implementation does not authorize publication
or mark prompt 105 done.

Local workspace gates: formatting and Clippy `-D warnings` passed;
`cargo test --workspace --no-fail-fast` passed with 2078 passed, 0 failed, 44 ignored. The preliminary sandbox-restricted run was stopped and superseded by
the full passing run with desktop/local-server access. Aggregate results:
[`verification-2026-11-16.txt`](../evidence/105/verification-2026-11-16.txt).
No shipped Rust or Electron source was changed in this follow-up.

## 10. Owner's UTM test — sign-in issue under investigation

On 2026-11-16 the owner reported UTM macOS 27.2.1 arm64, prepared the
Electron test profile and connected it to the private 1.99.0 feed using a
replacement `app-update.yml`. The owner's subsequent report was “it worked
but it logged me out”. This is an unresolved sign-in-preservation issue,
not upgrade acceptance. Version, install mode, other data retention and the
migration report still need confirmation. See
[`vm-owner-observation-2026-11-16.md`](../evidence/105/vm-owner-observation-2026-11-16.md).
The private feed uses the original checksum-verified test artifacts, not a
newly rebuilt release. This VM does not qualify the macOS 13 floor.
