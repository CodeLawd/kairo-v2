#!/usr/bin/env bash
# Package integrity checks for Kairo.app, ported from the Electron
# scripts/verify-package.mjs and extended for the native bundle (prompt 101).
#
#   packaging/macos/verify-package.sh <Kairo.app|Kairo-*.dmg> <arm64|x86_64> \
#     [--signing adhoc|developer-id] [--notarized]
#
# Checks: bundle layout and Info.plist; no repository junk; bible.db present
# and a SQLite file; every Mach-O built for exactly this architecture and for
# macOS 11 or older; no absolute non-system library path (no /opt/homebrew,
# /usr/local or build-machine FFmpeg) and no absolute rpath; every @rpath
# library present in Contents/Frameworks; FFmpeg is the LGPL build with the
# expected majors; signatures (strict, inside-out, hardened runtime, team,
# timestamp, entitlements); NDI runtime and every licence file; size.
# Exit 0 on success; 1 with diagnostics on the first failure; 2 on usage.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=config.sh
source "$here/config.sh"

usage() { echo "usage: verify-package.sh <Kairo.app|Kairo.dmg> <arm64|x86_64> [--signing adhoc|developer-id] [--notarized]" >&2; exit 2; }
target_path="${1:-}"; arch="${2:-}"
[[ -n "$target_path" && -n "$arch" ]] || usage
shift 2
signing=""; notarized=0
while (($#)); do
  case "$1" in
    --signing) signing="${2:-}"; shift 2 ;;
    --notarized) notarized=1; shift ;;
    *) usage ;;
  esac
done
case "$arch" in arm64|x86_64) ;; *) usage ;; esac
case "$signing" in ""|adhoc|developer-id) ;; *) usage ;; esac
[[ -e "$target_path" ]] || { echo "verify-package FAIL: target does not exist: $target_path" >&2; exit 1; }

fail() { echo "verify-package FAIL: $*" >&2; exit 1; }
ok() { echo "verify-package ok: $*"; }

mount_point=""
cleanup() { [[ -n "$mount_point" ]] && hdiutil detach -quiet "$mount_point" >/dev/null 2>&1 || true; }
trap cleanup EXIT

if [[ "$target_path" == *.dmg ]]; then
  [[ "$notarized" == 1 ]] && { xcrun stapler validate "$target_path" >/dev/null 2>&1 || fail "DMG has no stapled notarization ticket"; ok "DMG ticket stapled"; }
  if [[ "$signing" == developer-id ]]; then
    codesign --verify --strict "$target_path" 2>&1 || fail "DMG signature does not verify"
    ok "DMG signed"
  fi
  mount_point="$(mktemp -d "${TMPDIR:-/tmp}/kairo-verify.XXXXXX")"
  hdiutil attach -readonly -nobrowse -noautoopen -mountpoint "$mount_point" "$target_path" >/dev/null \
    || { rmdir "$mount_point"; mount_point=""; fail "cannot mount $target_path"; }
  app="$mount_point/$KAIRO_PRODUCT_NAME.app"
  [[ -d "$app" ]] || fail "DMG has no $KAIRO_PRODUCT_NAME.app"
  [[ -L "$mount_point/Applications" && "$(readlink "$mount_point/Applications")" == /Applications ]] \
    || fail "DMG has no Applications link"
  ok "DMG holds $KAIRO_PRODUCT_NAME.app and the Applications link"
else
  app="$(cd "$target_path" && pwd)"
fi
contents="$app/Contents"
[[ -d "$contents" ]] || fail "not an app bundle: $app"

# 1. Info.plist.
plist="$contents/Info.plist"
[[ -f "$plist" ]] || fail "missing Contents/Info.plist"
plutil -lint "$plist" >/dev/null || fail "Info.plist is not a valid property list"
key() { /usr/libexec/PlistBuddy -c "Print :$1" "$plist" 2>/dev/null; }
expect_key() { local v; v="$(key "$1")" || fail "Info.plist has no $1"; [[ "$v" == "$2" ]] || fail "Info.plist $1 is '$v', expected '$2'"; }
expect_key CFBundleIdentifier "$KAIRO_BUNDLE_ID"
expect_key CFBundlePackageType APPL
expect_key CFBundleExecutable "$KAIRO_PRODUCT_NAME"
expect_key CFBundleIconFile icon.icns
expect_key LSMinimumSystemVersion "$KAIRO_MIN_MACOS"
expect_key NSBonjourServices:0 _ndi._tcp
expect_key CFBundleDocumentTypes:0:CFBundleTypeExtensions:0 kairo
expect_key CFBundleDocumentTypes:0:CFBundleTypeRole Editor
version="$(key CFBundleShortVersionString)" || fail "Info.plist has no CFBundleShortVersionString"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+ ]] || fail "version '$version' is not semver"
expect_key CFBundleVersion "$version"
for usage_key in NSMicrophoneUsageDescription NSCameraUsageDescription NSLocalNetworkUsageDescription; do
  value="$(key "$usage_key")" || fail "Info.plist has no $usage_key"
  [[ "$value" == Kairo* ]] || fail "$usage_key does not describe Kairo's use: '$value'"
done
ok "Info.plist: $KAIRO_BUNDLE_ID $version, macOS $KAIRO_MIN_MACOS, usage strings, .kairo, _ndi._tcp"

# 2. Layout.
need() { [[ -e "$contents/$1" ]] || fail "missing Contents/$1"; }
for f in "MacOS/$KAIRO_PRODUCT_NAME" MacOS/kairo-engine PkgInfo \
  Resources/bible.db Resources/icon.png Resources/icon.icns Resources/decklink/decklink-helper \
  Frameworks/libndi.dylib Frameworks/libpdfium.dylib; do
  need "$f"
done
for lib in "${FFMPEG_LIBS[@]}"; do need "Frameworks/$lib.dylib"; done
for exe in "MacOS/$KAIRO_PRODUCT_NAME" MacOS/kairo-engine Resources/decklink/decklink-helper; do
  [[ -x "$contents/$exe" ]] || fail "Contents/$exe is not executable"
done
ok "bundle layout: both processes, DeckLink helper, Frameworks, resources"

# No repository or build junk (the Electron check's equivalent).
while IFS= read -r entry; do
  case "$entry" in
    Info.plist|PkgInfo|MacOS|Frameworks|Resources|_CodeSignature|CodeResources) ;;
    *) fail "unexpected Contents/$entry" ;;
  esac
done < <(ls -A "$contents")
junk="$(find "$app" \( -name '.env' -o -name '.env.*' -o -name '*.db-shm' -o -name '*.db-wal' \
  -o -name '*.dSYM' -o -name '*.rlib' -o -name '*.d' -o -name 'Cargo.toml' -o -name '*.rs' -o -name '.DS_Store' \) -print)"
[[ -z "$junk" ]] || fail "junk in the bundle:"$'\n'"$junk"
ok "no repository or build junk"

# 3. bible.db.
bible="$contents/Resources/bible.db"
size="$(stat -f %z "$bible")"
((size >= 1024)) || fail "bible.db suspiciously small ($size bytes)"
[[ "$(head -c 15 "$bible")" == "SQLite format 3" ]] || fail "bible.db is not a SQLite database"
ok "bible.db exists ($(awk -v b="$size" 'BEGIN{printf "%.1f", b/1048576}') MB)"

# 4. Mach-O files: architecture, deployment target, linkage.
macho=()
while IFS= read -r -d '' f; do
  if file -b "$f" | grep '^Mach-O' >/dev/null; then macho+=("$f"); fi
done < <(find "$app" -type f -print0)
((${#macho[@]} >= 8)) || fail "only ${#macho[@]} Mach-O files found"
rel() { echo "${1#"$app"/}"; }
min_os_le() { # is version $1 <= $2 (dotted)?
  [[ "$(printf '%s\n%s\n' "$1" "$2" | sort -t. -k1,1n -k2,2n -k3,3n | head -1)" == "$1" ]]
}
for f in "${macho[@]}"; do
  archs="$(lipo -archs "$f" 2>/dev/null)" || fail "cannot read the architecture of $(rel "$f")"
  [[ "$archs" == "$arch" ]] || fail "$(rel "$f") is [$archs], expected only $arch"
  minos="$(otool -l "$f" | awk '/LC_BUILD_VERSION/{b=1} b&&/minos/{print $2; exit} /LC_VERSION_MIN_MACOSX/{m=1} m&&/version/{print $2; exit}')"
  [[ -n "$minos" ]] || fail "$(rel "$f") declares no minimum macOS"
  min_os_le "$minos" "$KAIRO_MIN_MACOS" || fail "$(rel "$f") needs macOS $minos, above $KAIRO_MIN_MACOS"
  self="$(otool -D "$f" | tail -n +2)"
  while read -r dep; do
    [[ -z "$dep" || "$dep" == "$self" ]] && continue
    case "$dep" in
      /usr/lib/*|/System/Library/*) ;;
      @rpath/*)
        [[ -f "$contents/Frameworks/${dep#@rpath/}" ]] || fail "$(rel "$f") needs $dep, which is not in Contents/Frameworks" ;;
      *) fail "$(rel "$f") links $dep (absolute or build-machine path)" ;;
    esac
  done < <(otool -L "$f" | tail -n +2 | awk '{print $1}')
  while read -r rpath; do
    case "$rpath" in
      @executable_path/*|@loader_path|@loader_path/*) ;;
      *) fail "$(rel "$f") has rpath $rpath" ;;
    esac
  done < <(otool -l "$f" | awk '/cmd LC_RPATH/{getline; getline; print $2}')
done
for exe in "MacOS/$KAIRO_PRODUCT_NAME" MacOS/kairo-engine; do
  otool -l "$contents/$exe" | awk '/cmd LC_RPATH/{getline; getline; print $2}' | grep -x '@executable_path/../Frameworks' >/dev/null \
    || fail "Contents/$exe has no @executable_path/../Frameworks rpath"
done
if strings -a "${macho[@]}" 2>/dev/null | grep -E '/opt/homebrew/(opt|Cellar)/ffmpeg|/usr/local/(opt|Cellar)/ffmpeg' >/dev/null; then
  fail "a binary still names a Homebrew FFmpeg path"
fi
ok "${#macho[@]} Mach-O files: all $arch, macOS <= $KAIRO_MIN_MACOS, @rpath linkage, no absolute library paths"

# The engine and app must link the bundled FFmpeg majors.
for exe in "MacOS/$KAIRO_PRODUCT_NAME" MacOS/kairo-engine; do
  for lib in libavutil.61 libavcodec.63 libavformat.63; do
    otool -L "$contents/$exe" | grep "@rpath/$lib.dylib" >/dev/null || fail "Contents/$exe does not link @rpath/$lib.dylib"
  done
done

# 5. FFmpeg is the LGPL build.
config="$(strings -a "$contents/Frameworks/libavcodec.63.dylib" | grep -m1 -e '--enable-shared' || true)"
[[ -n "$config" ]] || fail "cannot read FFmpeg's configuration from libavcodec"
for bad in --enable-gpl --enable-nonfree --enable-version3 --enable-libx264 --enable-libx265; do
  [[ "$config" != *"$bad"* ]] || fail "FFmpeg was configured with $bad"
done
[[ "$config" == *--disable-gpl* ]] || fail "FFmpeg configuration lacks --disable-gpl"
strings -a "$contents/Frameworks/libavutil.61.dylib" | grep 'LGPL version 2.1 or later' >/dev/null \
  || fail "libavutil does not report the LGPL 2.1+ licence"
ok "FFmpeg is the LGPL shared build (avutil 61, avcodec 63, avformat 63, swscale 10)"

# 6. Licences and NDI.
for f in licenses/ffmpeg/COPYING.LGPLv2.1 licenses/ffmpeg/FFMPEG-SOURCE.txt licenses/libvpx/LICENSE licenses/libvpx/PATENTS licenses/pdfium/LICENSE \
  licenses/ndi/libndi_licenses.txt licenses/ndi/NOTICE.txt licenses/fonts/source-sans-3-OFL.txt \
  licenses/icons/hugeicons-LICENSE.md licenses/icons/phosphor-LICENSE licenses/syphon/License.txt; do
  [[ -s "$contents/Resources/$f" ]] || fail "missing or empty Resources/$f"
done
grep -q "Written offer" "$contents/Resources/licenses/ffmpeg/FFMPEG-SOURCE.txt" || fail "FFmpeg source notice has no written offer"
grep -q "https://ffmpeg.org/releases/" "$contents/Resources/licenses/ffmpeg/FFMPEG-SOURCE.txt" || fail "FFmpeg source notice has no source link"
nm -gU "$contents/Frameworks/libndi.dylib" 2>/dev/null | grep '_NDIlib_send_create' >/dev/null || fail "libndi.dylib does not export NDIlib_send_create"
ok "NDI runtime and every licence file present"

# 7. Signatures.
codesign --verify --deep --strict --verbose=1 "$app" 2>&1 | sed 's/^/  /' || true
codesign --verify --deep --strict "$app" >/dev/null 2>&1 || fail "codesign --verify --deep --strict failed"
info() { codesign -dvv "$1" 2>&1; }
entitlements() { codesign -d --entitlements - --xml "$1" 2>/dev/null | plutil -convert xml1 -o - - 2>/dev/null || true; }
detected=adhoc
info "$app" | grep '^Signature=adhoc' >/dev/null || detected=developer-id
[[ -z "$signing" || "$signing" == "$detected" ]] || fail "expected a $signing signature, found $detected"
team="$(info "$app" | awk -F= '/^TeamIdentifier=/{print $2}')"
for f in "${macho[@]}"; do
  codesign --verify --strict "$f" >/dev/null 2>&1 || fail "$(rel "$f") is not signed or its signature is broken"
  if [[ "$detected" == developer-id ]]; then
    i="$(info "$f")"
    grep -q 'flags=.*runtime' <<<"$i" || fail "$(rel "$f") is not signed with the hardened runtime"
    grep -q '^Timestamp=' <<<"$i" || fail "$(rel "$f") has no secure timestamp"
    [[ "$(awk -F= '/^TeamIdentifier=/{print $2}' <<<"$i")" == "$team" ]] || fail "$(rel "$f") is signed by another team"
  fi
  e="$(entitlements "$f")"
  for forbidden in com.apple.security.cs.allow-jit com.apple.security.cs.allow-unsigned-executable-memory com.apple.security.get-task-allow; do
    [[ "$e" != *"$forbidden"* ]] || fail "$(rel "$f") has $forbidden"
  done
  if [[ "$f" != "$contents/Resources/decklink/decklink-helper" && "$e" == *disable-library-validation* ]]; then
    fail "$(rel "$f") disables library validation (only the DeckLink helper may)"
  fi
done
app_e="$(entitlements "$app")"
for needed in com.apple.security.device.audio-input com.apple.security.device.camera; do
  [[ "$app_e" == *"$needed"* ]] || fail "Kairo is missing the $needed entitlement"
done
[[ "$(info "$contents/MacOS/kairo-engine" | awk -F= '/^Identifier=/{print $2}')" == "$KAIRO_ENGINE_ID" ]] \
  || fail "kairo-engine is not signed as $KAIRO_ENGINE_ID"
if [[ "$detected" == developer-id ]]; then
  spctl --assess --type execute --verbose=2 "$app" 2>&1 | sed 's/^/  /' || true
  if [[ "$notarized" == 1 ]]; then
    spctl --assess --type execute "$app" >/dev/null 2>&1 || fail "Gatekeeper rejects the app"
    xcrun stapler validate "$app" >/dev/null 2>&1 || fail "the app has no stapled notarization ticket"
    ok "Gatekeeper accepts the notarized app"
  fi
  ok "signed by team $team with the hardened runtime and timestamps, inside-out"
else
  ok "ad-hoc signed (KAIRO_ADHOC_SIGN; no hardened runtime until a Developer ID exists)"
fi

# 8. Size, against the Electron baseline (baseline.md: 301,828 KiB, 1.0.27 arm64).
kib="$(du -sk "$app" | awk '{print $1}')"
echo "verify-package info: installed size ${kib} KiB ($(awk -v k="$kib" 'BEGIN{printf "%.1f", k/1024}') MiB); Electron baseline 301828 KiB"
((kib <= 600 * 1024)) || fail "bundle is ${kib} KiB, over the 600 MiB ceiling"
echo
echo "verify-package PASS: $target_path [$arch${signing:+, $signing}]"
