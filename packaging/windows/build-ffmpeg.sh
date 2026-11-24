#!/usr/bin/env bash
# Build the LGPL shared FFmpeg that the Windows package ships (prompt 102,
# amendment from 043), in an MSYS2 UCRT64 shell with MinGW-w64 GCC. Never a
# package-manager FFmpeg (vcpkg defaults, MSYS2's mingw-w64-ffmpeg): those are
# GPL or pull in other libraries.
#
#   packaging/windows/build-ffmpeg.sh <work-dir>
#
# Result in <work-dir>/ffmpeg-x64, in the layout ffmpeg-sys-next reads on
# MSVC (the same as the BtbN builds CI already links): bin/*.dll,
# lib/<name>.lib import libraries, include/, plus licenses/ffmpeg and
# licenses/libvpx. libvpx (pinned in ../common.sh) is built first, statically.
# The DLLs link libgcc statically and use Win32 threads, so they need only
# Windows' own DLLs (the Universal C Runtime is part of Windows 10 and 11).
# Reuses an earlier build when its recorded configure line is unchanged.
#
# Needs: gcc, make, nasm, pkgconf, diffutils, tar, xz (UCRT64 packages
# mingw-w64-ucrt-x86_64-gcc, make, mingw-w64-ucrt-x86_64-nasm,
# mingw-w64-ucrt-x86_64-pkgconf, diffutils).
#
# KAIRO_MINGW_CROSS_PREFIX=x86_64-w64-mingw32- cross-compiles from Linux or
# macOS instead, to check this script and the configure line without Windows.
# Such a build links msvcrt.dll rather than the UCRT and is not for release.
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
vpx_target="x86_64-win64-gcc"

cross="${KAIRO_MINGW_CROSS_PREFIX:-}"
if [[ -z "$cross" ]]; then
  case "${MSYSTEM:-}" in
    UCRT64) ;;
    *) die "run this in an MSYS2 UCRT64 shell (MSYSTEM=UCRT64); MSYSTEM is '${MSYSTEM:-unset}'" ;;
  esac
fi
for tool in "${cross}gcc" make nasm pkg-config "${cross}dlltool" "${cross}objdump"; do
  command -v "$tool" >/dev/null || die "missing $tool"
done

vpx_flags=()
while IFS= read -r flag; do vpx_flags+=("$flag"); done < <(libvpx_configure_flags)
flags=()
while IFS= read -r flag; do flags+=("$flag"); done < <(ffmpeg_configure_flags)
toolchain=(--arch=x86_64 --target-os=mingw32
  --extra-cflags="-I$vpx_prefix/include"
  --extra-ldflags="-L$vpx_prefix/lib -static-libgcc"
  --extra-libs="-Wl,-Bstatic -lwinpthread -Wl,-Bdynamic")
if [[ -n "$cross" ]]; then
  toolchain+=(--enable-cross-compile --cross-prefix="$cross")
fi
stamp="$(printf '%s\n' "$FFMPEG_VERSION" "${flags[@]}" "${toolchain[@]}" "$LIBVPX_VERSION" "${vpx_flags[@]}" "$vpx_target")"

if [[ -f "$prefix/.kairo-stamp" ]] && [[ "$(cat "$prefix/.kairo-stamp")" == "$stamp" ]]; then
  note "FFmpeg $FFMPEG_VERSION (x64) already built at $prefix"
  exit 0
fi

download "$FFMPEG_URL" "$tarball" "$FFMPEG_SHA256"
download "$LIBVPX_URL" "$vpx_tarball" "$LIBVPX_SHA256"
jobs="$(nproc 2>/dev/null || echo 4)"

# 1. libvpx, static, decoders only.
rm -rf "$vpx_source" "$vpx_prefix"
mkdir -p "$vpx_source"
tar -xzf "$vpx_tarball" -C "$vpx_source" --strip-components=1
note "building libvpx $LIBVPX_VERSION"
(
  cd "$vpx_source"
  CROSS="$cross" ./configure --prefix="$vpx_prefix" --target="$vpx_target" "${vpx_flags[@]}"
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
  PKG_CONFIG_PATH="$vpx_prefix/lib/pkgconfig" PKG_CONFIG_LIBDIR="$vpx_prefix/lib/pkgconfig" \
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

# 3. The DLLs, and MSVC import libraries named as ffmpeg-sys-next links them
# (lib/avcodec.lib). FFmpeg writes a .def per library; dlltool turns each into
# an import library naming the versioned DLL.
mkdir -p "$prefix/lib"
for dll in "${FFMPEG_DLLS[@]}"; do
  [[ -f "$prefix/bin/$dll" ]] || die "FFmpeg build has no bin/$dll (major version changed?)"
  name="${dll%%-*}"
  def="$(find "$source_dir/lib$name" -maxdepth 1 -name "$name-*.def" ! -name '*.orig.def' | head -1)"
  [[ -n "$def" ]] || die "no .def file for $dll"
  "${cross}dlltool" -m i386:x86-64 -d "$def" -l "$prefix/lib/$name.lib" -D "$dll"
done
for lib in "${FFMPEG_IMPORT_LIBS[@]}"; do
  [[ -s "$prefix/lib/$lib.lib" ]] || die "no import library lib/$lib.lib"
done

# Each DLL may import only Windows itself and its FFmpeg siblings.
for dll in "${FFMPEG_DLLS[@]}"; do
  while read -r dep; do
    case "${dep,,}" in
      kernel32.dll|user32.dll|advapi32.dll|bcrypt.dll|ole32.dll|shell32.dll|ws2_32.dll|secur32.dll|d3d11.dll|dxgi.dll|mfplat.dll) ;;
      api-ms-win-crt-*.dll|api-ms-win-core-*.dll|ucrtbase.dll) ;;
      msvcrt.dll) [[ -n "$cross" ]] || die "$dll imports msvcrt.dll: build in UCRT64, not MINGW64" ;;
      avutil-*.dll|swresample-*.dll|avcodec-*.dll|avformat-*.dll|swscale-*.dll) ;;
      *) die "$dll imports $dep, which a clean Windows install does not have" ;;
    esac
  done < <("${cross}objdump" -p "$prefix/bin/$dll" | awk '/DLL Name:/{print $3}')
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
  echo "avutil, swresample, avcodec, avformat and swscale DLLs in Kairo's install"
  echo "folder, and you may replace them with your own build of the same version."
  echo
  echo "Version: FFmpeg $FFMPEG_VERSION, unmodified"
  echo "Source:  $FFMPEG_URL"
  echo "SHA-256: $FFMPEG_SHA256"
  echo
  echo "libvpx $LIBVPX_VERSION (BSD-3-Clause, licence in ../libvpx) is linked into"
  echo "avcodec for VP8 and VP9 with alpha:"
  echo "  $LIBVPX_URL"
  echo "  SHA-256: $LIBVPX_SHA256"
  echo "  configured with: --target=$vpx_target ${vpx_flags[*]}"
  echo
  echo "Built with MinGW-w64 GCC $("${cross}gcc" -dumpversion) (MSYS2 UCRT64), configured with:"
  printf '  %s\n' "${toolchain[@]}" "${flags[@]}"
  echo
  echo "Written offer: for at least three years after this release, Kairo will"
  echo "send the complete corresponding FFmpeg source above, with these build"
  echo "instructions, to anyone who asks, for no more than the cost of providing it."
  echo "Ask through the support address on the Kairo website."
} >"$prefix/licenses/ffmpeg/FFMPEG-SOURCE.txt"

printf '%s' "$stamp" >"$prefix/.kairo-stamp"
note "FFmpeg $FFMPEG_VERSION (x64) installed at $prefix"
