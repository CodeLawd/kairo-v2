#!/usr/bin/env bash
# Build, stage and wrap Kairo for Linux x64: AppImage and deb (prompt 103).
#
#   packaging/linux/package.sh [work-dir]
#
# Run on Ubuntu 22.04 (the oldest supported release, so glibc and library
# versions hold on 24.04) after, with the same work directory:
#   build-ffmpeg.sh <work>   -> <work>/ffmpeg-x64
#   fetch-inputs.sh <work>   -> <work>/inputs
# Needs: cargo, node 22+, npm, patchelf, dpkg-shlibdeps (dpkg-dev), readelf,
# and the build dependencies CI installs (WebKitGTK 4.1, GTK 3, ALSA, …).
#
# Steps:
#   1. cargo build --release against the bundled FFmpeg.
#   2. Stage <work>/app as electron-builder's linux-unpacked folder:
#      kairo-presenter (the app), kairo-engine, lib/ with FFmpeg, PDFium and
#      the NDI runtime beside the executables (where kairo-documents,
#      kairo-scripture and kairo-sinks look), resources/bible.db and icon.png,
#      licences. The executables get an $ORIGIN/lib rpath.
#   3. deb depends: dpkg-shlibdeps over every binary, plus DEB_RUNTIME_DEPENDS.
#   4. electron-builder --prepackaged: Kairo-<v>.AppImage,
#      kairo-presenter_<v>_amd64.deb and latest-linux.yml.
#   5. verify-package.sh on the staged folder, the AppImage and the deb.
#
# The release endpoints (KAIRO_API_URL, KAIRO_WEB_URL) must be set: a release
# build refuses local or non-https URLs.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
# shellcheck source=config.sh
source "$here/config.sh"

work="${1:-$repo/target/package-linux}"
mkdir -p "$work"
work="$(cd "$work" && pwd)"
ffmpeg="$work/ffmpeg-x64"
inputs="$work/inputs"
[[ -f "$ffmpeg/.kairo-stamp" ]] || die "no FFmpeg build at $ffmpeg; run build-ffmpeg.sh $work"
[[ -f "$inputs/resources/bible.db" ]] || die "no inputs at $inputs; run fetch-inputs.sh $work"
for tool in cargo node npm patchelf dpkg-shlibdeps readelf; do
  command -v "$tool" >/dev/null || die "missing $tool"
done
for var in KAIRO_API_URL KAIRO_WEB_URL; do
  [[ -n "${!var:-}" ]] || die "$var is not set"
done

version="$(cd "$repo" && cargo pkgid -p kairo | sed 's/.*[#@]//')"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "cannot read the kairo package version (got '$version')"
note "Kairo $version for Linux x64"

# 1. Build.
cargo_target="$work/target"
(
  cd "$repo"
  export FFMPEG_DIR="$ffmpeg"
  export PKG_CONFIG_PATH="$ffmpeg/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
  export CARGO_TARGET_DIR="$cargo_target"
  cargo build --release --locked -p kairo -p kairo-engine
)
bin="$cargo_target/release"

# 2. Stage.
app="$work/app"
rm -rf "$app"
mkdir -p "$app/lib" "$app/resources" "$app/licenses"
cp "$bin/kairo" "$app/$KAIRO_EXECUTABLE"
# kairo looks for the engine beside its own executable (apps/kairo/src/main.rs).
cp "$bin/kairo-engine" "$app/kairo-engine"
for so in "${FFMPEG_SONAMES[@]}"; do
  cp -L "$ffmpeg/lib/$so" "$app/lib/$so"
done
cp "$inputs/pdfium-x64/lib/libpdfium.so" "$app/libpdfium.so"
cp "$inputs/ndi/libndi.so.6" "$app/libndi.so.6"
cp "$inputs/resources/bible.db" "$inputs/resources/icon.png" "$app/resources/"
chmod 755 "$app/$KAIRO_EXECUTABLE" "$app/kairo-engine"
chmod 644 "$app/lib/"*.so.* "$app/libpdfium.so" "$app/libndi.so.6"

# The executables find FFmpeg in lib/; each FFmpeg library its siblings.
for exe in "$app/$KAIRO_EXECUTABLE" "$app/kairo-engine"; do
  patchelf --set-rpath '$ORIGIN/lib' "$exe"
done
for so in "$app/lib/"*.so.*; do
  patchelf --set-rpath '$ORIGIN' "$so"
done

licenses="$app/licenses"
mkdir -p "$licenses/pdfium" "$licenses/ndi" "$licenses/fonts" "$licenses/icons"
cp -R "$ffmpeg/licenses/." "$licenses/"
cp "$inputs/pdfium-x64/LICENSE" "$licenses/pdfium/LICENSE"
cp "$inputs/pdfium-x64/licenses/"* "$licenses/pdfium/"
cp "$inputs/ndi/libndi_licenses.txt" "$licenses/ndi/libndi_licenses.txt"
cat >"$licenses/ndi/NOTICE.txt" <<'TXT'
NDI® is a registered trademark of Vizrt NDI AB.
The NDI runtime (libndi.so.6) is distributed under the NDI SDK licence; see
libndi_licenses.txt beside this file and https://ndi.video.
TXT
cp "$repo/crates/kairo-ui-kit/assets/licenses/source-sans-3-OFL.txt" "$licenses/fonts/"
cp "$repo/crates/kairo-ui-kit/assets/licenses/hugeicons-LICENSE.md" "$licenses/icons/"
cp "$repo/crates/kairo-ui-kit/assets/licenses/phosphor-LICENSE" "$licenses/icons/"

# 3. deb depends from the binaries themselves. Bundled libraries are private
# (-l), so they need no shlibs entry.
shlib="$work/shlibdeps"
rm -rf "$shlib"
mkdir -p "$shlib/debian"
printf 'Source: kairo-presenter\n\nPackage: kairo-presenter\nArchitecture: amd64\n' >"$shlib/debian/control"
(
  cd "$shlib"
  dpkg-shlibdeps --ignore-missing-info -l"$app/lib" -l"$app" -O \
    "$app/$KAIRO_EXECUTABLE" "$app/kairo-engine" "$app/libpdfium.so" "$app/libndi.so.6" "$app/lib/"*.so.* \
    >"$shlib/substvars"
)
found="$(sed -n 's/^shlibs:Depends=//p' "$shlib/substvars")"
[[ -n "$found" ]] || die "dpkg-shlibdeps found no dependencies"
depends=()
while IFS= read -r dep; do
  dep="${dep#"${dep%%[![:space:]]*}"}"
  [[ -n "$dep" ]] && depends+=("$dep")
done < <(tr ',' '\n' <<<"$found")
depends+=("${DEB_RUNTIME_DEPENDS[@]}")
for dep in "${depends[@]}"; do
  case "$dep" in
    # libavahi-* is Avahi, which the NDI runtime needs; it is not FFmpeg.
    libavutil* | libavcodec* | libavformat* | libswscale* | libswresample* | libpdfium* | libndi*)
      die "a bundled library leaked into depends: $dep" ;;
  esac
done
note "deb depends: ${depends[*]}"

# 4. AppImage and deb.
build_res="$work/build-resources"
rm -rf "$build_res"
mkdir -p "$build_res/icons"
cp "$inputs/resources/icons/"*.png "$build_res/icons/"
generated="$work/electron-builder.linux.yml"
{
  echo "extends: $here/electron-builder.yml"
  echo "directories:"
  echo "  buildResources: $build_res"
  echo "  output: $work/dist"
  echo "extraMetadata:"
  echo "  version: $version"
  echo "deb:"
  echo "  depends:"
  for dep in "${depends[@]}"; do printf '    - "%s"\n' "$dep"; done
  echo "  recommends:"
  for dep in "${DEB_RECOMMENDS[@]}"; do printf '    - "%s"\n' "$dep"; done
} >"$generated"
rm -rf "$work/dist"
(
  cd "$here"
  npm ci --ignore-scripts --no-audit --no-fund
  for attempt in 1 2 3; do
    npx electron-builder --linux AppImage deb --x64 --prepackaged "$app" --publish never \
      --config "$generated" && exit 0
    echo "package-linux: electron-builder failed (attempt $attempt of 3)" >&2
    [[ "$attempt" -lt 3 ]] && sleep 30
  done
  exit 1
)

appimage="$work/dist/$KAIRO_PRODUCT_NAME-$version.AppImage"
deb="$work/dist/${KAIRO_EXECUTABLE}_${version}_amd64.deb"
[[ -f "$appimage" ]] || die "electron-builder made no $appimage"
[[ -f "$deb" ]] || die "electron-builder made no $deb"

# 5. Verify.
"$here/verify-package.sh" --app "$app" --appimage "$appimage" --deb "$deb" --version "$version"

for f in "$appimage" "$deb" "$work/dist/latest-linux.yml"; do
  note "$(basename "$f") $(stat -c %s "$f") bytes sha256 $(sha256_of "$f")"
done
