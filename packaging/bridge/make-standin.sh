#!/usr/bin/env bash
# Builds a stand-in for a Developer ID Electron Kairo 1.0.33 install, to run
# the real Squirrel.Mac upgrade to a native build without touching anyone's
# Kairo profile (prompt 105).
#
#   packaging/bridge/make-standin.sh <kairo-presenter> <out-dir> <feed-url>
#
# It copies kairo-presenter's Electron (node_modules/electron/dist, the
# Electron 1.0.33 ships), renames it to Kairo with bundle id com.kairo.app and
# version 1.0.33, adds standin/main.js and an app-update.yml pointing at
# <feed-url> (a generic feed: a folder with latest-mac.yml and the zip served
# over HTTP), and signs it with KAIRO_SIGN_IDENTITY using Electron's
# entitlements. The designated requirement is then the same as the real
# 1.0.33's, which is what Squirrel.Mac checks the update against.
#
# Run it with its own profile folder; it exits without one:
#   BRIDGE_USERDATA=<dir> BRIDGE_LOG=<file> \
#   BRIDGE_UPDATER=<kairo-presenter>/node_modules/electron-updater \
#   <out-dir>/Kairo.app/Contents/MacOS/Kairo
# It checks, downloads, waits for Squirrel to stage the update and quits;
# ShipIt then swaps the bundle in place. It never relaunches (a relaunched app
# would open the real profile).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
electron_repo="${1:?usage: make-standin.sh <kairo-presenter> <out-dir> <feed-url>}"
out="${2:?usage: make-standin.sh <kairo-presenter> <out-dir> <feed-url>}"
feed="${3:?usage: make-standin.sh <kairo-presenter> <out-dir> <feed-url>}"
: "${KAIRO_SIGN_IDENTITY:?set KAIRO_SIGN_IDENTITY to the Developer ID Application identity of the installed Electron app}"
[[ "$(uname -s)" == Darwin ]] || { echo "make-standin: macOS only" >&2; exit 1; }

src="$electron_repo/node_modules/electron/dist/Electron.app"
[[ -d "$src" ]] || { echo "make-standin: $src not found (install kairo-presenter's dependencies)" >&2; exit 1; }
mkdir -p "$out"
app="$out/Kairo.app"
[[ -e "$app" ]] && { echo "make-standin: $app already exists" >&2; exit 1; }
ditto "$src" "$app"
contents="$app/Contents"
mv "$contents/MacOS/Electron" "$contents/MacOS/Kairo"
plist="$contents/Info.plist"
for kv in CFBundleExecutable=Kairo CFBundleIdentifier=com.kairo.app CFBundleName=Kairo \
  CFBundleShortVersionString=1.0.33 CFBundleVersion=1.0.33; do
  /usr/libexec/PlistBuddy -c "Set :${kv%%=*} ${kv#*=}" "$plist"
done
mkdir -p "$contents/Resources/app"
cp "$here/standin/main.js" "$contents/Resources/app/main.js"
printf '{ "name": "kairo-presenter", "productName": "Kairo", "version": "1.0.33", "main": "main.js" }\n' \
  >"$contents/Resources/app/package.json"
printf 'provider: generic\nurl: %s\nupdaterCacheDirName: kairo-bridge-test-updater\n' "$feed" \
  >"$contents/Resources/app-update.yml"
codesign --force --deep --options runtime \
  --entitlements "$electron_repo/build/entitlements.mac.plist" -s "$KAIRO_SIGN_IDENTITY" "$app"
codesign --verify --deep --strict "$app"
echo "make-standin: $app"
codesign -d -r- "$app" 2>&1 | grep '^designated'
