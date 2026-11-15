#!/usr/bin/env bash
# Build and assemble Kairo.app for one architecture, then sign it inside-out
# (prompt 101).
#
#   packaging/macos/bundle.sh <arm64|x86_64> <work-dir>
#
# Needs build-ffmpeg.sh and fetch-inputs.sh to have run for the same
# architecture and work directory (package.sh runs all three).
#
# Signing, decided by the environment:
#   KAIRO_SIGN_IDENTITY  a Developer ID Application identity in the keychain:
#                        hardened runtime, secure timestamp, entitlements.
#   KAIRO_ADHOC_SIGN=1   no identity yet: ad-hoc signature, as the Electron
#                        release does until the signing secrets exist. Apple
#                        Silicon reports an unsigned app as "damaged"; an
#                        ad-hoc one gets the ordinary "unidentified developer"
#                        prompt instead.
# One of the two is required. The release endpoints (KAIRO_API_URL,
# KAIRO_WEB_URL) must be set: a release build refuses local or non-https URLs.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
# shellcheck source=config.sh
source "$here/config.sh"

arch="${1:?usage: bundle.sh <arm64|x86_64> <work-dir>}"
work="${2:?usage: bundle.sh <arm64|x86_64> <work-dir>}"
target="$(rust_target_for "$arch")"
work="$(cd "$work" && pwd)"
ffmpeg="$work/ffmpeg-$arch"
inputs="$work/inputs"
[[ -f "$ffmpeg/.kairo-stamp" ]] || die "no FFmpeg build at $ffmpeg; run build-ffmpeg.sh $arch $work"
[[ -f "$inputs/resources/bible.db" ]] || die "no inputs at $inputs; run fetch-inputs.sh $arch $work"
[[ -f "$inputs/pdfium-$arch/lib/libpdfium.dylib" ]] || die "no PDFium for $arch; run fetch-inputs.sh $arch $work"

if [[ -n "${KAIRO_SIGN_IDENTITY:-}" ]]; then
  signing=developer-id
elif [[ "${KAIRO_ADHOC_SIGN:-}" == 1 ]]; then
  signing=adhoc
else
  die "set KAIRO_SIGN_IDENTITY (Developer ID Application) or KAIRO_ADHOC_SIGN=1"
fi

version="$(cd "$repo" && cargo pkgid -p kairo | sed 's/.*[#@]//')"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+ ]] || die "cannot read the kairo package version (got '$version')"
note "Kairo $version for $arch ($target), $signing signing"

# 1. Build both processes against the bundled FFmpeg, never Homebrew's.
cargo_target="$work/target"
(
  cd "$repo"
  export FFMPEG_DIR="$ffmpeg"
  export PKG_CONFIG_PATH="$ffmpeg/lib/pkgconfig"
  export PKG_CONFIG_LIBDIR="$ffmpeg/lib/pkgconfig"
  export MACOSX_DEPLOYMENT_TARGET="$KAIRO_MIN_MACOS"
  export CARGO_TARGET_DIR="$cargo_target"
  cargo build --release --locked --target "$target" -p kairo -p kairo-engine
)
bin="$cargo_target/$target/release"

# 2. Assemble.
out="$work/out-$arch"
app="$out/$KAIRO_PRODUCT_NAME.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Frameworks" \
  "$app/Contents/Resources/decklink" "$app/Contents/Resources/licenses"
contents="$app/Contents"

cp "$bin/kairo" "$contents/MacOS/$KAIRO_PRODUCT_NAME"
# kairo looks for the engine beside its own executable (apps/kairo/src/main.rs).
cp "$bin/kairo-engine" "$contents/MacOS/kairo-engine"

for lib in "${FFMPEG_LIBS[@]}"; do
  cp -L "$ffmpeg/lib/$lib.dylib" "$contents/Frameworks/$lib.dylib"
done
cp -L "$inputs/pdfium-$arch/lib/libpdfium.dylib" "$contents/Frameworks/libpdfium.dylib"
# The NDI runtime is universal; ship only this architecture.
lipo "$inputs/ndi/libndi.dylib" -thin "$arch" -output "$contents/Frameworks/libndi.dylib"

# The engine finds the helper at ../Resources/decklink (kairo-sinks decklink.rs).
helper="$repo/native/decklink/prebuilt/decklink-helper"
[[ -f "$helper" ]] || die "missing $helper (prompt 062)"
lipo "$helper" -thin "$arch" -output "$contents/Resources/decklink/decklink-helper"
chmod 755 "$contents/Resources/decklink/decklink-helper"

cp "$inputs/resources/bible.db" "$inputs/resources/icon.png" "$inputs/resources/icon.icns" "$contents/Resources/"

licenses="$contents/Resources/licenses"
mkdir -p "$licenses/ffmpeg" "$licenses/pdfium" "$licenses/ndi" "$licenses/fonts" "$licenses/icons" "$licenses/syphon"
cp -R "$ffmpeg/licenses/." "$licenses/"
cp "$inputs/pdfium-$arch/LICENSE" "$licenses/pdfium/LICENSE"
[[ -d "$inputs/pdfium-$arch/licenses" ]] && cp "$inputs/pdfium-$arch/licenses/"* "$licenses/pdfium/"
cp "$inputs/ndi/libndi_licenses.txt" "$licenses/ndi/libndi_licenses.txt"
cat >"$licenses/ndi/NOTICE.txt" <<'EOF'
NDI® is a registered trademark of Vizrt NDI AB.
The NDI runtime (libndi.dylib) is distributed under the NDI SDK licence; see
libndi_licenses.txt beside this file and https://ndi.video.
EOF
cp "$repo/crates/kairo-ui-kit/assets/licenses/source-sans-3-OFL.txt" "$licenses/fonts/source-sans-3-OFL.txt"
cp "$repo/crates/kairo-ui-kit/assets/licenses/hugeicons-LICENSE.md" "$licenses/icons/hugeicons-LICENSE.md"
cp "$repo/crates/kairo-ui-kit/assets/licenses/phosphor-LICENSE" "$licenses/icons/phosphor-LICENSE"
cp "$repo/crates/kairo-native-sys/native/syphon/vendor/Syphon/License.txt" "$licenses/syphon/License.txt"

escape() { sed -e 's/[&|\\]/\\&/g' <<<"$1"; }
sed -e "s|@PRODUCT_NAME@|$(escape "$KAIRO_PRODUCT_NAME")|g" \
  -e "s|@BUNDLE_ID@|$(escape "$KAIRO_BUNDLE_ID")|g" \
  -e "s|@VERSION@|$(escape "$version")|g" \
  -e "s|@MIN_MACOS@|$(escape "$KAIRO_MIN_MACOS")|g" \
  -e "s|@COPYRIGHT@|$(escape "$KAIRO_COPYRIGHT")|g" \
  "$here/Info.plist.in" >"$contents/Info.plist"
plutil -lint "$contents/Info.plist" >/dev/null || die "generated Info.plist is invalid"
printf 'APPL????' >"$contents/PkgInfo"

# 3. Linkage: every bundled library by @rpath, found in Contents/Frameworks.
for dylib in "$contents/Frameworks/"*.dylib; do
  name="$(basename "$dylib")"
  install_name_tool -id "@rpath/$name" "$dylib"
done
relink() { # point any absolute reference to a bundled library at @rpath
  local file="$1" dep base
  while read -r dep; do
    base="$(basename "$dep")"
    if [[ -f "$contents/Frameworks/$base" && "$dep" != "@rpath/$base" ]]; then
      install_name_tool -change "$dep" "@rpath/$base" "$file"
    fi
  done < <(otool -L "$file" | tail -n +2 | awk '{print $1}')
}
drop_build_rpaths() { # remove rpaths that point into a build machine
  local file="$1" path
  while read -r path; do
    case "$path" in
      @executable_path/*|@loader_path/*) ;;
      *) install_name_tool -delete_rpath "$path" "$file" ;;
    esac
  done < <(otool -l "$file" | awk '/cmd LC_RPATH/{getline; getline; print $2}')
}
add_rpath() { # add_rpath FILE PATH, once
  otool -l "$1" | awk '/cmd LC_RPATH/{getline; getline; print $2}' | grep -x "$2" >/dev/null \
    || install_name_tool -add_rpath "$2" "$1"
}
for exe in "$contents/MacOS/$KAIRO_PRODUCT_NAME" "$contents/MacOS/kairo-engine"; do
  relink "$exe"
  drop_build_rpaths "$exe"
  add_rpath "$exe" "@executable_path/../Frameworks"
done
for dylib in "$contents/Frameworks/"*.dylib; do
  relink "$dylib"
  drop_build_rpaths "$dylib"
  add_rpath "$dylib" "@loader_path"
done

# 4. Sign inside-out: libraries, the helper, the engine, then the bundle.
sign() { # sign FILE [ENTITLEMENTS] [IDENTIFIER]
  local args=(--force)
  if [[ "$signing" == developer-id ]]; then
    args+=(--sign "$KAIRO_SIGN_IDENTITY" --timestamp --options runtime)
  else
    args+=(--sign -)
  fi
  [[ -n "${2:-}" ]] && args+=(--entitlements "$2")
  [[ -n "${3:-}" ]] && args+=(--identifier "$3")
  codesign "${args[@]}" "$1"
}
for dylib in "$contents/Frameworks/"*.dylib; do
  sign "$dylib"
done
sign "$contents/Resources/decklink/decklink-helper" "$here/entitlements/decklink-helper.plist" "$KAIRO_DECKLINK_ID"
sign "$contents/MacOS/kairo-engine" "$here/entitlements/kairo-engine.plist" "$KAIRO_ENGINE_ID"
sign "$app" "$here/entitlements/kairo.plist" "$KAIRO_BUNDLE_ID"
codesign --verify --deep --strict --verbose=2 "$app"

printf '%s\n' "$version" >"$out/VERSION"
printf '%s\n' "$signing" >"$out/SIGNING"
note "built $app"
