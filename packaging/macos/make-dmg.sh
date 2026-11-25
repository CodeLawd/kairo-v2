#!/usr/bin/env bash
# Make the DMG with Electron's layout (prompt 101): a 540×380 window, the app
# at (140, 200), an Applications link at (400, 200), titled
# "<product> <version>", with the app icon as the volume icon.
#
#   packaging/macos/make-dmg.sh <Kairo.app> <output.dmg> <version>
#
# The layout is written by Finder through AppleScript, which needs a logged-in
# session (GitHub's macOS runners have one). KAIRO_DMG_LAYOUT=0 skips it and
# makes a plain DMG, for headless machines only; a release must not set it.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=config.sh
source "$here/config.sh"

app="${1:?usage: make-dmg.sh <Kairo.app> <output.dmg> <version>}"
output="${2:?usage: make-dmg.sh <Kairo.app> <output.dmg> <version>}"
version="${3:?usage: make-dmg.sh <Kairo.app> <output.dmg> <version>}"
[[ -d "$app" ]] || die "no app at $app"
title="$KAIRO_PRODUCT_NAME $version"
[[ ! -d "/Volumes/$title" ]] || die "/Volumes/$title is already mounted; eject it first"

stage="$(mktemp -d "${TMPDIR:-/tmp}/kairo-dmg.XXXXXX")"
rw="$stage/rw.dmg"
device=""
cleanup() {
  [[ -n "$device" ]] && hdiutil detach -quiet "$device" >/dev/null 2>&1 || true
  rm -rf "$stage"
}
trap cleanup EXIT

mkdir "$stage/root"
ditto "$app" "$stage/root/$KAIRO_PRODUCT_NAME.app"
ln -s /Applications "$stage/root/Applications"
cp "$app/Contents/Resources/icon.icns" "$stage/root/.VolumeIcon.icns"

# Room for the app plus Finder's metadata.
kib="$(du -sk "$stage/root" | awk '{print $1}')"
hdiutil create -quiet -volname "$title" -srcfolder "$stage/root" -fs HFS+ \
  -format UDRW -size "$((kib + 20480))k" -ov "$rw"
device="$(hdiutil attach -readwrite -noverify -noautoopen "$rw" | awk '/Apple_HFS/{print $1; exit}')"
[[ -n "$device" ]] || die "could not attach $rw"
mount="/Volumes/$title"
[[ -d "$mount" ]] || die "the DMG did not mount at $mount"
SetFile -a C "$mount" 2>/dev/null || xattr -wx com.apple.FinderInfo \
  "0000000000000000040000000000000000000000000000000000000000000000" "$mount"

if [[ "${KAIRO_DMG_LAYOUT:-1}" != 0 ]]; then
  osascript <<EOF || die "Finder could not lay out the DMG (KAIRO_DMG_LAYOUT=0 skips this on headless machines)"
tell application "Finder"
  tell disk "$title"
    open
    set current view of container window to icon view
    set toolbar visible of container window to false
    set statusbar visible of container window to false
    set the bounds of container window to {200, 120, 740, 500}
    set viewOptions to the icon view options of container window
    set arrangement of viewOptions to not arranged
    set icon size of viewOptions to 96
    set position of item "$KAIRO_PRODUCT_NAME.app" of container window to {140, 200}
    set position of item "Applications" of container window to {400, 200}
    update without registering applications
    delay 2
    close
  end tell
end tell
EOF
  [[ -f "$mount/.DS_Store" ]] || die "Finder wrote no layout (.DS_Store) to the DMG"
else
  note "KAIRO_DMG_LAYOUT=0: plain DMG without the window layout"
fi

chmod -Rf go-w "$mount" || true
sync
hdiutil detach -quiet "$device"
device=""
mkdir -p "$(dirname "$output")"
rm -f "$output"
hdiutil convert -quiet "$rw" -format UDZO -imagekey zlib-level=6 -o "$output"
note "made $output"
