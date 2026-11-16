# Electron → native bridge

The native packages retain Electron's updater contract. The compatibility
analysis and live macOS Squirrel evidence are in
[`105-bridge-plan.md`](../../native-migration/notes/105-bridge-plan.md).
Nothing in these tools publishes a release.

## Validate downloaded draft assets

Use Node 24 and a read-only `kairo-presenter` checkout with its deployed
`electron-updater` 6.8.9 dependencies installed. On a Mac with the signed
Electron app installed:

```bash
node packaging/bridge/check-electron-bridge.mjs \
  --dir /path/to/downloaded-draft-assets \
  --electron ../kairo-presenter --from 1.0.33 \
  --installed /Applications/Kairo.app
```

The default requires all three feeds and both Mac zips. Exit 0 means every
required check passed; exit 1 means validation failed or a required check
was skipped; exit 2 means invalid arguments or missing setup. Platform
selectors (`--mac-only`, `--win-only`, `--linux-only`) intentionally restrict
validation. A selected feed must exist. Mac bundle checks require macOS
and an installed Electron bundle for its signing requirement; running on
another OS cannot certify the Mac artifacts.

This checks the real deployed parsers and file pickers, update ordering,
Mac OS floor and architecture selection, artifact hashes, Mac zip layout,
bundle identity and signatures, and Windows elevation metadata (including
the selected file's `isAdminRightsRequired`). It does not install Windows
or Linux packages, certify notarization, or prove profile migration.

## Regression tests

```bash
node --test packaging/bridge/check-electron-bridge.test.mjs
```

Set `BRIDGE_ELECTRON` when the reference checkout is elsewhere. Tests use
temporary fixtures, never execute their fake installer bytes, and on macOS
ad-hoc sign a temporary bundle without a Developer ID or keychain access.
CI runs them on Ubuntu and macOS against pinned Electron source and updater
6.8.9. That job also gates the reusable CI workflow used by native releases.
It does not replace live upgrade qualification.

## Live upgrade acceptance

Run on disposable machines/VMs containing test accounts and test data.
A scratch `HOME` does **not** isolate Electron's profile; do not launch the
real Electron app on the owner's workstation for these checks. Do not use
personal credentials or copy them into evidence.

For each of macOS 13 (arm64 and Intel where available), Windows 10/11,
and Ubuntu 22.04 (AppImage and deb separately):

1. Install Electron 1.0.33 and create a test song, setlist and theme. Sign in
   with a test account, select a microphone, quit, and snapshot the VM.
2. Record app version, OS/architecture, draft tag and artifact SHA-512.
   Capture a directory listing and hashes of the test profile outside it.
3. Download and validate the draft's artifacts. Serve the matching feed and
   artifacts privately; point only the disposable Electron install's
   `app-update.yml` at that generic feed. Keep the draft unpublished.
4. Launch Electron, Download, then Install. Confirm the native version
   relaunches, the engine starts, migration creates its backup and marker,
   and the song, setlist, theme, sign-in and microphone selection survive.
   A missing physical microphone must produce the documented migration
   warning rather than erase the saved choice.
5. Restore the pre-upgrade snapshot and repeat with an ordinary quit after
   Download instead of Install. Launch native manually after the swap.
6. Test rollback by reinstalling Electron 1.0.33; confirm the test data remain
   readable. Retain the native migration backup for diagnosis.
7. Record actual observations and updater/install logs by platform, launch
   mode and artifact hash in `native-migration/evidence/105/`. Redact tokens,
   personal paths and account details. A missing observation stays unverified.

The Mac stand-in (`make-standin.sh`, usage in the script) can exercise real
Squirrel staging and swapping with a private profile and no relaunch. It
cannot certify Electron UI behaviour, post-relaunch profile migration,
notarization, or the other platforms. The existing evidence states that limit.

Prompt 105 remains in progress until the required live tests, workspace
gates and owner review are complete. Public rollout policy and any final
Electron release for macOS 11/12 require owner decisions; publication is the
separate prompt 112 checkpoint.
