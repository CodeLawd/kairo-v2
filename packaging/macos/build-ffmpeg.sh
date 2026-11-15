#!/usr/bin/env bash
# Build the LGPL shared FFmpeg that Kairo.app ships (prompt 101, amendment
# from 043). Never a package-manager FFmpeg: Homebrew's is GPL.
#
#   packaging/macos/build-ffmpeg.sh <arm64|x86_64> <work-dir>
#
# Result: <work-dir>/ffmpeg-<arch>/{include,lib,lib/pkgconfig} plus
# <work-dir>/ffmpeg-<arch>/licenses/ffmpeg/{COPYING.LGPLv2.1,FFMPEG-SOURCE.txt} and
# licenses/libvpx. libvpx (pinned in config.sh) is built first, statically.
# Reuses an earlier build when its recorded configure line is unchanged.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=config.sh
source "$here/config.sh"

arch="${1:?usage: build-ffmpeg.sh <arm64|x86_64> <work-dir>}"
work="${2:?usage: build-ffmpeg.sh <arm64|x86_64> <work-dir>}"
rust_target_for "$arch" >/dev/null
mkdir -p "$work"
work="$(cd "$work" && pwd)"
prefix="$work/ffmpeg-$arch"
tarball="$work/ffmpeg-${FFMPEG_VERSION}.tar.xz"
source_dir="$work/ffmpeg-${FFMPEG_VERSION}-src-$arch"

vpx_prefix="$work/libvpx-$arch"
vpx_tarball="$work/libvpx-${LIBVPX_VERSION}.tar.gz"
vpx_source="$work/libvpx-${LIBVPX_VERSION}-src-$arch"
vpx_flags=()
while IFS= read -r flag; do vpx_flags+=("$flag"); done < <(libvpx_configure_flags)
# darwin22 is macOS 13, the minimum.
vpx_target="$arch-darwin22-gcc"

flags=()
while IFS= read -r flag; do flags+=("$flag"); done < <(ffmpeg_configure_flags)
toolchain=(--arch="$arch" --target-os=darwin --cc="clang -arch $arch"
  --extra-cflags="-mmacosx-version-min=${KAIRO_MIN_MACOS} -I$vpx_prefix/include"
  --extra-ldflags="-mmacosx-version-min=${KAIRO_MIN_MACOS} -L$vpx_prefix/lib")
host="$(uname -m)"
if [[ "$host" != "$arch" ]]; then
  toolchain+=(--enable-cross-compile)
fi
if [[ "$arch" == x86_64 ]] && ! command -v nasm >/dev/null; then
  die "x86_64 FFmpeg and libvpx need nasm for their assembly (brew install nasm; a build tool, not shipped)"
fi
stamp="$(printf '%s\n' "$FFMPEG_VERSION" "${flags[@]}" "${toolchain[@]}" "$LIBVPX_VERSION" "${vpx_flags[@]}" "$vpx_target")"

if [[ -f "$prefix/.kairo-stamp" ]] && [[ "$(cat "$prefix/.kairo-stamp")" == "$stamp" ]]; then
  note "FFmpeg $FFMPEG_VERSION ($arch) already built at $prefix"
  exit 0
fi

download "$FFMPEG_URL" "$tarball" "$FFMPEG_SHA256"
download "$LIBVPX_URL" "$vpx_tarball" "$LIBVPX_SHA256"

# 1. libvpx, static and position-independent, decoders only.
rm -rf "$vpx_source" "$vpx_prefix"
mkdir -p "$vpx_source"
tar -xzf "$vpx_tarball" -C "$vpx_source" --strip-components=1
note "building libvpx $LIBVPX_VERSION for $arch"
(
  cd "$vpx_source"
  ./configure --prefix="$vpx_prefix" --target="$vpx_target" "${vpx_flags[@]}" \
    --extra-cflags="-mmacosx-version-min=${KAIRO_MIN_MACOS}"
  make -j"$(sysctl -n hw.ncpu)"
  make install
)
[[ -f "$vpx_prefix/lib/libvpx.a" ]] || die "libvpx did not build libvpx.a"

rm -rf "$source_dir" "$prefix"
mkdir -p "$source_dir"
tar -xJf "$tarball" -C "$source_dir" --strip-components=1

note "configuring FFmpeg $FFMPEG_VERSION for $arch"
(
  cd "$source_dir"
  PKG_CONFIG_PATH="$vpx_prefix/lib/pkgconfig" PKG_CONFIG_LIBDIR="$vpx_prefix/lib/pkgconfig" \
    ./configure --prefix="$prefix" "${toolchain[@]}" "${flags[@]}" | tee "$work/ffmpeg-configure-$arch.log"
)
# configure only warns about component names it does not know; a typo would
# silently drop a codec, so treat it as an error.
if grep -q "did not match anything" "$work/ffmpeg-configure-$arch.log"; then
  grep "did not match anything" "$work/ffmpeg-configure-$arch.log" >&2
  die "an FFmpeg component name in config.sh does not exist in $FFMPEG_VERSION"
fi
# Nor may a requested component be dropped for a missing dependency.
if grep -q "^WARNING: Disabled" "$work/ffmpeg-configure-$arch.log"; then
  grep "^WARNING: Disabled" "$work/ffmpeg-configure-$arch.log" >&2
  die "FFmpeg configure disabled a requested component"
fi
grep -q "^License: LGPL version 2.1 or later" "$work/ffmpeg-configure-$arch.log" \
  || die "FFmpeg configure did not report LGPL 2.1 or later"

make -C "$source_dir" -j"$(sysctl -n hw.ncpu)"
make -C "$source_dir" install

# The libraries the app links, with the majors ffmpeg-next 9.0.0 expects.
for lib in "${FFMPEG_LIBS[@]}"; do
  [[ -f "$prefix/lib/$lib.dylib" ]] || die "FFmpeg build has no $lib.dylib (major version changed?)"
  id="$(otool -D "$prefix/lib/$lib.dylib" | tail -1)"
  [[ "$id" == "@rpath/$lib.dylib" ]] || die "$lib.dylib install name is $id, expected @rpath/$lib.dylib"
done

mkdir -p "$prefix/licenses/ffmpeg" "$prefix/licenses/libvpx"
cp "$source_dir/COPYING.LGPLv2.1" "$prefix/licenses/ffmpeg/COPYING.LGPLv2.1"
cp "$vpx_source/LICENSE" "$vpx_source/PATENTS" "$vpx_source/AUTHORS" "$prefix/licenses/libvpx/"
{
  echo "FFmpeg in Kairo"
  echo "==============="
  echo
  echo "Kairo uses FFmpeg (https://ffmpeg.org) under the GNU Lesser General Public"
  echo "License, version 2.1 or later. The licence text is in COPYING.LGPLv2.1 beside"
  echo "this file. FFmpeg is linked dynamically: the libraries are the separate files"
  echo "in Kairo.app/Contents/Frameworks (libavutil, libavcodec, libavformat,"
  echo "libswscale), and you may replace them with your own build of the same"
  echo "version."
  echo
  echo "Version: FFmpeg $FFMPEG_VERSION, unmodified"
  echo "Source:  $FFMPEG_URL"
  echo "SHA-256: $FFMPEG_SHA256"
  echo
  echo "libvpx $LIBVPX_VERSION (BSD-3-Clause, licence in ../libvpx) is linked into"
  echo "libavcodec for VP8 and VP9 with alpha:"
  echo "  $LIBVPX_URL"
  echo "  SHA-256: $LIBVPX_SHA256"
  echo "  configured with: --target=$vpx_target ${vpx_flags[*]}"
  echo
  echo "Configured with:"
  printf '  %s\n' "${toolchain[@]}" "${flags[@]}"
  echo
  echo "Written offer: for at least three years after this release, Kairo will"
  echo "send the complete corresponding FFmpeg source above, with these build"
  echo "instructions, to anyone who asks, for no more than the cost of providing it."
  echo "Ask through the support address on the Kairo website."
} >"$prefix/licenses/ffmpeg/FFMPEG-SOURCE.txt"

printf '%s' "$stamp" >"$prefix/.kairo-stamp"
note "FFmpeg $FFMPEG_VERSION ($arch) installed at $prefix"
