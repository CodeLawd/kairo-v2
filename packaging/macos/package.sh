#!/usr/bin/env bash
# Kairo macOS package, end to end (prompt 101):
#
#   packaging/macos/package.sh <arm64|x86_64> [work-dir]
#
# 1. build-ffmpeg.sh   LGPL shared FFmpeg from pinned source
# 2. fetch-inputs.sh   bible.db and icons at the pinned Electron commit,
#                      PDFium, NDI runtime (all checksummed)
# 3. bundle.sh         cargo build, Kairo.app, @rpath linkage, inside-out signing
# 4. notarize.sh       the app (Developer ID builds only)
# 5. the updater zip   Kairo-<version>[-arm64]-mac.zip
# 6. make-dmg.sh       Kairo-<version>[-arm64].dmg, signed and notarized
# 7. verify-package.sh on the app and on the DMG
#
# Signing follows the Electron release: with KAIRO_SIGN_IDENTITY it signs and
# notarizes (KAIRO_NOTARIZE=0 skips notarizing, for local checks only); without
# it, KAIRO_ADHOC_SIGN=1 builds the ad-hoc fallback. Artifacts land in
# <work-dir>/dist (default target/package-macos).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
# shellcheck source=config.sh
source "$here/config.sh"

[[ "$(uname -s)" == Darwin ]] || die "macOS packaging runs on macOS"
arch="${1:?usage: package.sh <arm64|x86_64> [work-dir]}"
rust_target_for "$arch" >/dev/null
work="${2:-$repo/target/package-macos}"
mkdir -p "$work"
work="$(cd "$work" && pwd)"

if [[ -z "${KAIRO_SIGN_IDENTITY:-}" && "${KAIRO_ADHOC_SIGN:-}" != 1 ]]; then
  die "no signing identity: set KAIRO_SIGN_IDENTITY, or KAIRO_ADHOC_SIGN=1 for an unsigned (ad-hoc) build"
fi
notarize=0
if [[ -n "${KAIRO_SIGN_IDENTITY:-}" && "${KAIRO_NOTARIZE:-1}" != 0 ]]; then
  notarize=1
fi

"$here/build-ffmpeg.sh" "$arch" "$work"
"$here/fetch-inputs.sh" "$arch" "$work"
"$here/bundle.sh" "$arch" "$work"

out="$work/out-$arch"
app="$out/$KAIRO_PRODUCT_NAME.app"
version="$(cat "$out/VERSION")"
signing="$(cat "$out/SIGNING")"
verify_flags=(--signing "$signing")
((notarize)) && verify_flags+=(--notarized)

if ((notarize)); then
  "$here/notarize.sh" "$app"
fi
"$here/verify-package.sh" "$app" "$arch" "${verify_flags[@]}"

dist="$work/dist"
mkdir -p "$dist"
suffix="$(artifact_suffix_for "$arch")"
zip="$dist/$KAIRO_PRODUCT_NAME-$version$suffix-mac.zip"
dmg="$dist/$KAIRO_PRODUCT_NAME-$version$suffix.dmg"
rm -f "$zip" "$dmg"

# The updater unpacks this with ditto and swaps the bundle (decision 048).
ditto -c -k --sequesterRsrc --keepParent "$app" "$zip"

"$here/make-dmg.sh" "$app" "$dmg" "$version"
if [[ "$signing" == developer-id ]]; then
  codesign --force --sign "$KAIRO_SIGN_IDENTITY" --timestamp "$dmg"
  ((notarize)) && "$here/notarize.sh" "$dmg"
fi
"$here/verify-package.sh" "$dmg" "$arch" "${verify_flags[@]}"

for f in "$zip" "$dmg"; do
  echo "package-macos: $(basename "$f") $(stat -f %z "$f") bytes sha256 $(sha256_of "$f")"
done
