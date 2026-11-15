#!/usr/bin/env bash
# Build the LGPL shared FFmpeg that the Linux packages ship (prompt 103,
# amendment from 043). Never the distribution's libavcodec: Ubuntu's is GPL
# and its major version differs between releases.
#
#   packaging/linux/build-ffmpeg.sh <work-dir>
#
# Run on the oldest supported release (Ubuntu 22.04), so the libraries need no
# newer glibc than the packages promise. Needs gcc, make, nasm, pkg-config and
# libva-dev (VA-API headers), patchelf. Result in <work-dir>/ffmpeg-x64: lib/*.so.N with
# sonames and an $ORIGIN rpath, include/, lib/pkgconfig, licenses/ffmpeg and
# licenses/libvpx. libvpx (pinned in ../common.sh) is built first, statically.
# Reuses an earlier build when its recorded configure line is unchanged.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=config.sh
source "$here/config.sh"

work="${1:?usage: build-ffmpeg.sh <work-dir>}"
mkdir -p "$work"
work="$(cd "$work" && pwd)"
prefix="$work/ffmpeg-x64"
tarball="$work/ffmpeg-${FFMPEG_VERSION}.tar.xz"
source_dir="$work/ffmpeg-${FFMPEG_VERSION}-src-x64"
vpx_prefix="$work/libvpx-x64"
vpx_tarball="$work/libvpx-${LIBVPX_VERSION}.tar.gz"
vpx_source="$work/libvpx-${LIBVPX_VERSION}-src-x64"
vpx_target="x86_64-linux-gcc"

[[ "$(uname -s)" == Linux && "$(uname -m)" == x86_64 ]] || die "build on Linux x86_64 (got $(uname -sm))"
for tool in gcc make nasm pkg-config readelf patchelf; do
  command -v "$tool" >/dev/null || die "missing $tool"
done
pkg-config --exists libva libva-drm || die "VA-API headers missing (apt install libva-dev)"

vpx_flags=()
while IFS= read -r flag; do vpx_flags+=("$flag"); done < <(libvpx_configure_flags)
flags=()
while IFS= read -r flag; do flags+=("$flag"); done < <(ffmpeg_configure_flags)
toolchain=(--arch=x86_64 --target-os=linux --enable-pic
  --extra-cflags="-I$vpx_prefix/include"
  --extra-ldflags="-L$vpx_prefix/lib")
stamp="$(printf '%s\n' "$FFMPEG_VERSION" "${flags[@]}" "${toolchain[@]}" "$LIBVPX_VERSION" "${vpx_flags[@]}" "$vpx_target")"

if [[ -f "$prefix/.kairo-stamp" ]] && [[ "$(cat "$prefix/.kairo-stamp")" == "$stamp" ]]; then
  note "FFmpeg $FFMPEG_VERSION (x64) already built at $prefix"
  exit 0
fi

download "$FFMPEG_URL" "$tarball" "$FFMPEG_SHA256"
download "$LIBVPX_URL" "$vpx_tarball" "$LIBVPX_SHA256"
jobs="$(nproc)"

# 1. libvpx, static and position-independent, decoders only.
rm -rf "$vpx_source" "$vpx_prefix"
mkdir -p "$vpx_source"
tar -xzf "$vpx_tarball" -C "$vpx_source" --strip-components=1
note "building libvpx $LIBVPX_VERSION"
(
  cd "$vpx_source"
  ./configure --prefix="$vpx_prefix" --target="$vpx_target" "${vpx_flags[@]}"
  make -j"$jobs"
  make install
)
[[ -f "$vpx_prefix/lib/libvpx.a" ]] || die "libvpx did not build libvpx.a"

# 2. FFmpeg.
rm -rf "$source_dir" "$prefix"
mkdir -p "$source_dir"
tar -xJf "$tarball" -C "$source_dir" --strip-components=1
note "configuring FFmpeg $FFMPEG_VERSION"
(
  cd "$source_dir"
  PKG_CONFIG_PATH="$vpx_prefix/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}" \
    ./configure --prefix="$prefix" "${toolchain[@]}" "${flags[@]}" | tee "$work/ffmpeg-configure-x64.log"
)
if grep -q "did not match anything" "$work/ffmpeg-configure-x64.log"; then
  grep "did not match anything" "$work/ffmpeg-configure-x64.log" >&2
  die "an FFmpeg component name does not exist in $FFMPEG_VERSION"
fi
if grep -q "^WARNING: Disabled" "$work/ffmpeg-configure-x64.log"; then
  grep "^WARNING: Disabled" "$work/ffmpeg-configure-x64.log" >&2
  die "FFmpeg configure disabled a requested component"
fi
grep -q "^License: LGPL version 2.1 or later" "$work/ffmpeg-configure-x64.log" \
  || die "FFmpeg configure did not report LGPL 2.1 or later"

make -C "$source_dir" -j"$jobs"
make -C "$source_dir" install

# $ORIGIN lets each library find its siblings wherever the package puts them.
for so in "${FFMPEG_SONAMES[@]}"; do
  [[ -e "$prefix/lib/$so" ]] || die "FFmpeg build has no lib/$so (major version changed?)"
  patchelf --set-rpath '$ORIGIN' "$(readlink -f "$prefix/lib/$so")"
  soname="$(readelf -d "$prefix/lib/$so" | sed -n 's/.*(SONAME).*\[\(.*\)\]/\1/p')"
  [[ "$soname" == "$so" ]] || die "$so has soname '$soname'"
  runpath="$(readelf -d "$prefix/lib/$so" | sed -n 's/.*(R\(UN\)\{0,1\}PATH).*\[\(.*\)\]/\2/p')"
  [[ "$runpath" == '$ORIGIN' ]] || die "$so has rpath '$runpath', expected \$ORIGIN"
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
  echo "this file. FFmpeg is linked dynamically: the libraries are the separate"
  echo "libavutil, libswresample, libavcodec, libavformat and libswscale files in"
  echo "Kairo's lib folder, and you may replace them with your own build of the same"
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
  echo "Built with GCC $(gcc -dumpversion) on $(. /etc/os-release && echo "$PRETTY_NAME"), configured with:"
  printf '  %s\n' "${toolchain[@]}" "${flags[@]}"
  echo
  echo "Written offer: for at least three years after this release, Kairo will"
  echo "send the complete corresponding FFmpeg source above, with these build"
  echo "instructions, to anyone who asks, for no more than the cost of providing it."
  echo "Ask through the support address on the Kairo website."
} >"$prefix/licenses/ffmpeg/FFMPEG-SOURCE.txt"

printf '%s' "$stamp" >"$prefix/.kairo-stamp"
note "FFmpeg $FFMPEG_VERSION (x64) installed at $prefix"
