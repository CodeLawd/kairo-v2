#!/usr/bin/env bash
# Package checks for Kairo on Linux (prompt 103), ported from the Electron
# scripts/verify-package.mjs and extended for the native build.
#
#   packaging/linux/verify-package.sh --app <folder> [--appimage <file>]
#                                     [--deb <file>] [--version x.y.z]
#
# --app is the staged folder or an installed /opt/Kairo. Checks:
# - layout: both executables, FFmpeg in lib/, PDFium and the NDI runtime
#   beside the executables, resources/bible.db (SQLite) and icon.png,
#   licences; no repository or build junk;
# - every ELF is x86-64 and needs at most glibc 2.35 (Ubuntu 22.04);
#   executables carry the $ORIGIN/lib runpath and FFmpeg libraries $ORIGIN;
#   `ldd` resolves every library, and every FFmpeg library from inside the
#   folder (amendment from 043);
# - FFmpeg is the LGPL build with the expected majors;
# - the AppImage unpacks (no FUSE needed) to the same app, with its desktop
#   entry; the deb is kairo-presenter at this version with /opt/Kairo,
#   Kairo.desktop (AudioVideo, application/x-kairo), the MIME type for
#   *.kairo, icons and no bundled library in Depends; sizes.
# Exit 0 on success; 1 with a message on the first failure; 2 on usage.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=config.sh
source "$here/config.sh"

usage() { echo "usage: verify-package.sh --app <folder> [--appimage <file>] [--deb <file>] [--version x.y.z]" >&2; exit 2; }
app=""; appimage=""; deb=""; version=""
while (($#)); do
  case "$1" in
    --app) app="${2:-}"; shift 2 ;;
    --appimage) appimage="${2:-}"; shift 2 ;;
    --deb) deb="${2:-}"; shift 2 ;;
    --version) version="${2:-}"; shift 2 ;;
    *) usage ;;
  esac
done
[[ -n "$app" ]] || usage
fail() { echo "verify-package FAIL: $*" >&2; exit 1; }
ok() { echo "verify-package ok: $*"; }
[[ -d "$app" ]] || fail "no folder at $app"
app="$(cd "$app" && pwd)"
for tool in readelf ldd file strings; do
  command -v "$tool" >/dev/null || fail "missing $tool"
done
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
glibc_max="2.35"

check_app() { # check_app FOLDER LABEL
  local dir="$1" label="$2" f
  for f in "$KAIRO_EXECUTABLE" kairo-engine libpdfium.so libndi.so.6 resources/bible.db resources/icon.png; do
    [[ -f "$dir/$f" ]] || fail "$label: missing $f"
  done
  for f in "${FFMPEG_SONAMES[@]}"; do
    [[ -f "$dir/lib/$f" ]] || fail "$label: missing lib/$f"
  done
  for f in "$KAIRO_EXECUTABLE" kairo-engine; do
    [[ -x "$dir/$f" ]] || fail "$label: $f is not executable"
  done
  for f in licenses/ffmpeg/COPYING.LGPLv2.1 licenses/ffmpeg/FFMPEG-SOURCE.txt licenses/libvpx/LICENSE \
    licenses/libvpx/PATENTS licenses/pdfium/LICENSE licenses/ndi/libndi_licenses.txt licenses/ndi/NOTICE.txt \
    licenses/fonts/source-sans-3-OFL.txt licenses/icons/hugeicons-LICENSE.md licenses/icons/phosphor-LICENSE; do
    [[ -s "$dir/$f" ]] || fail "$label: missing or empty $f"
  done
  grep -q "Written offer" "$dir/licenses/ffmpeg/FFMPEG-SOURCE.txt" || fail "$label: FFmpeg notice has no written offer"
  grep -q "https://ffmpeg.org/releases/" "$dir/licenses/ffmpeg/FFMPEG-SOURCE.txt" || fail "$label: FFmpeg notice has no source link"

  local junk
  junk="$(find "$dir" \( -name '.env' -o -name '.env.*' -o -name '*.db-shm' -o -name '*.db-wal' \
    -o -name '*.rlib' -o -name '*.d' -o -name 'Cargo.toml' -o -name '*.rs' \) -print)"
  [[ -z "$junk" ]] || fail "$label: junk in the package:"$'\n'"$junk"

  [[ "$(head -c 15 "$dir/resources/bible.db")" == "SQLite format 3" ]] || fail "$label: bible.db is not a SQLite database"
  (($(stat -c %s "$dir/resources/bible.db") >= 1024)) || fail "$label: bible.db suspiciously small"

  # ELF files: architecture, glibc, runpath, resolution.
  local count=0 elf rel need runpath ldd_out line
  while IFS= read -r -d '' elf; do
    file -b "$elf" | grep '^ELF' >/dev/null || continue
    count=$((count + 1))
    rel="${elf#"$dir"/}"
    file -b "$elf" | grep 'x86-64' >/dev/null || fail "$label: $rel is not x86-64"
    need="$( (readelf -V --wide "$elf" 2>/dev/null || true) | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -t. -k1,1n -k2,2n | tail -1)"
    if [[ -n "$need" ]] && [[ "$(printf '%s\n%s\n' "$need" "$glibc_max" | sort -t. -k1,1n -k2,2n | tail -1)" != "$glibc_max" ]]; then
      fail "$label: $rel needs glibc $need, above Ubuntu 22.04's $glibc_max"
    fi
    runpath="$(readelf -d "$elf" | sed -n 's/.*(R\(UN\)\{0,1\}PATH).*\[\(.*\)\]/\2/p')"
    case "$rel" in
      "$KAIRO_EXECUTABLE" | kairo-engine)
        [[ "$runpath" == '$ORIGIN/lib' ]] || fail "$label: $rel has runpath '$runpath', expected \$ORIGIN/lib" ;;
      lib/*)
        [[ "$runpath" == '$ORIGIN' ]] || fail "$label: $rel has runpath '$runpath', expected \$ORIGIN" ;;
      *)
        [[ -z "$runpath" || "$runpath" == '$ORIGIN'* ]] || fail "$label: $rel has runpath '$runpath'" ;;
    esac
    ldd_out="$(env -u LD_LIBRARY_PATH ldd "$elf" 2>&1 || true)"
    if grep 'not found' <<<"$ldd_out" >/dev/null; then
      # electron-builder adds these optional GTK tray shims to every AppImage.
      # Kairo's executables are checked independently and do not load them.
      case "$label:$rel" in
        AppImage:usr/lib/libappindicator.so.1|AppImage:usr/lib/libappindicator3.so.1|\
        AppImage:usr/lib/libindicator.so.7|AppImage:usr/lib/libindicator3.so.7|\
        AppImage:usr/lib/libdbusmenu-gtk.so.4|AppImage:usr/lib/libdbusmenu-gtk3.so.4) ;;
        *) fail "$label: $rel has unresolved libraries:"$'\n'"$(grep 'not found' <<<"$ldd_out")" ;;
      esac
    fi
    while IFS= read -r line; do
      case "$line" in
        *libavutil.so* | *libswresample.so* | *libavcodec.so* | *libavformat.so* | *libswscale.so*)
          [[ "$line" == *"=> $dir/"* ]] || fail "$label: $rel loads FFmpeg from outside the package: $line" ;;
      esac
    done <<<"$ldd_out"
  done < <(find "$dir" -type f -print0)
  ((count >= 9)) || fail "$label: only $count ELF files"
  for f in "$KAIRO_EXECUTABLE" kairo-engine; do
    env -u LD_LIBRARY_PATH ldd "$dir/$f" | grep "libavcodec.so.63 => $dir/lib/" >/dev/null \
      || fail "$label: $f does not load libavcodec.so.63 from $dir/lib"
  done
  ok "$label: layout, $count ELF files (x86-64, glibc <= $glibc_max, runpaths), FFmpeg resolved inside the package"

  local config
  config="$(strings -a "$dir/lib/libavcodec.so.63" | grep -m1 -e '--enable-shared' || true)"
  [[ -n "$config" ]] || fail "$label: cannot read FFmpeg's configuration"
  for f in --enable-gpl --enable-nonfree --enable-version3 --enable-libx264 --enable-libx265; do
    [[ "$config" != *"$f"* ]] || fail "$label: FFmpeg was configured with $f"
  done
  [[ "$config" == *--disable-gpl* ]] || fail "$label: FFmpeg configuration lacks --disable-gpl"
  strings -a "$dir/lib/libavutil.so.61" | grep 'LGPL version 2.1 or later' >/dev/null || fail "$label: libavutil is not LGPL 2.1+"
  ok "$label: FFmpeg is the LGPL shared build"
}

check_desktop() { # check_desktop FILE LABEL
  local desktop="$1" label="$2"
  [[ -f "$desktop" ]] || fail "$label: no desktop entry at $desktop"
  grep -x 'Name=Kairo' "$desktop" >/dev/null || fail "$label: desktop entry Name is not Kairo"
  grep '^Categories=AudioVideo;' "$desktop" >/dev/null || fail "$label: desktop entry category is not AudioVideo"
  grep '^MimeType=.*application/x-kairo' "$desktop" >/dev/null || fail "$label: desktop entry lacks application/x-kairo"
  grep '^Exec=.*%U' "$desktop" >/dev/null || fail "$label: desktop entry does not pass files (%U)"
}

check_app "$app" "folder"

if [[ -n "$appimage" ]]; then
  [[ -f "$appimage" ]] || fail "no AppImage at $appimage"
  chmod +x "$appimage"
  (cd "$scratch" && "$appimage" --appimage-extract >/dev/null)
  check_app "$scratch/squashfs-root" "AppImage"
  check_desktop "$scratch/squashfs-root/Kairo.desktop" "AppImage"
  [[ -x "$scratch/squashfs-root/AppRun" ]] || fail "AppImage: no AppRun"
  rm -rf "$scratch/squashfs-root"
  latest="$(dirname "$appimage")/latest-linux.yml"
  [[ -f "$latest" ]] || fail "no latest-linux.yml beside the AppImage"
  grep -F "$(basename "$appimage")" "$latest" >/dev/null || fail "latest-linux.yml does not name $(basename "$appimage")"
  [[ -z "$version" ]] || grep -x "version: $version" "$latest" >/dev/null || fail "latest-linux.yml is not version $version"
  echo "verify-package info: AppImage $(stat -c %s "$appimage") bytes"
fi

if [[ -n "$deb" ]]; then
  [[ -f "$deb" ]] || fail "no deb at $deb"
  command -v dpkg-deb >/dev/null || fail "missing dpkg-deb"
  [[ "$(dpkg-deb -f "$deb" Package)" == "$KAIRO_EXECUTABLE" ]] || fail "deb package is not $KAIRO_EXECUTABLE"
  [[ -z "$version" || "$(dpkg-deb -f "$deb" Version)" == "$version" ]] || fail "deb version is not $version"
  depends="$(dpkg-deb -f "$deb" Depends)"
  for need in libwebkit2gtk-4.1-0 libvulkan1 libegl1 libavahi-client3; do
    [[ "$depends" == *"$need"* ]] || fail "deb Depends lacks $need: $depends"
  done
  [[ "$depends" == *libasound2* ]] || fail "deb Depends lacks libasound2: $depends"
  [[ "$depends" != *libavcodec* && "$depends" != *libnss3* ]] || fail "deb Depends names a library it must not: $depends"
  mkdir -p "$scratch/deb"
  dpkg-deb -x "$deb" "$scratch/deb"
  check_app "$scratch/deb/opt/Kairo" "deb"
  check_desktop "$scratch/deb/usr/share/applications/Kairo.desktop" "deb"
  grep -l 'application/x-kairo' "$scratch"/deb/usr/share/mime/packages/*.xml >/dev/null 2>&1 \
    || fail "deb: no shared-mime-info entry for application/x-kairo"
  grep -h 'pattern="\*.kairo"' "$scratch"/deb/usr/share/mime/packages/*.xml >/dev/null \
    || fail "deb: the MIME type does not match *.kairo"
  ls "$scratch"/deb/usr/share/icons/hicolor/*/apps/"$KAIRO_EXECUTABLE".png >/dev/null 2>&1 \
    || fail "deb: no hicolor icons"
  ok "deb: $KAIRO_EXECUTABLE $(dpkg-deb -f "$deb" Version), /opt/Kairo, Kairo.desktop, *.kairo MIME type, icons"
  echo "verify-package info: deb $(stat -c %s "$deb") bytes; Depends: $depends"
fi

echo "verify-package info: installed size $(du -sk "$app" | cut -f1) KiB"
echo
echo "verify-package PASS"
