#!/usr/bin/env bash
# Gather the pinned, checksummed inputs that are not built here (prompt 103):
#
#   packaging/linux/fetch-inputs.sh <work-dir>
#
# - bible.db, icon.png and the Linux icon set (build/icons) from
#   CodeLawd/kairo-presenter at KAIRO_PRESENTER_COMMIT (../common.sh).
#   Never committed here.
# - PDFium (libpdfium.so) from bblanchon/pdfium-binaries.
# - The NDI runtime (libndi.so.6) and its licence from @grandi/linux-x64.
#
# Local overrides (no download; checksums still apply):
#   KAIRO_PRESENTER_DIR  an Electron checkout to read the pinned commit from
#   KAIRO_PDFIUM_TGZ     a pdfium-linux-x64.tgz already on disk
#   KAIRO_GRANDI_TGZ     a linux-x64-<version>.tgz already on disk
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=config.sh
source "$here/config.sh"

work="${1:?usage: fetch-inputs.sh <work-dir>}"
mkdir -p "$work/inputs/resources/icons"
work="$(cd "$work" && pwd)"
inputs="$work/inputs"

# 1. Electron resources at the pinned commit.
presenter_resource resources/bible.db "$inputs/resources/bible.db" "$BIBLE_DB_SHA256"
presenter_resource resources/icon.png "$inputs/resources/icon.png" "$ICON_PNG_SHA256"
for size in "${LINUX_ICON_SIZES[@]}"; do
  presenter_resource "build/icons/${size}x${size}.png" "$inputs/resources/icons/${size}x${size}.png" \
    "$(icon_sha256_for "$size")"
done
note "resources from ${KAIRO_PRESENTER_REPO}@${KAIRO_PRESENTER_COMMIT:0:10}"

# 2. PDFium.
pdfium_tgz="$inputs/pdfium-linux-x64.tgz"
if [[ -n "${KAIRO_PDFIUM_TGZ:-}" && ! -f "$pdfium_tgz" ]]; then
  cp "$KAIRO_PDFIUM_TGZ" "$pdfium_tgz"
fi
download "$PDFIUM_URL_BASE/pdfium-linux-x64.tgz" "$pdfium_tgz" "$PDFIUM_SHA256_x64"
rm -rf "$inputs/pdfium-x64"
mkdir -p "$inputs/pdfium-x64"
tar -xzf "$pdfium_tgz" -C "$inputs/pdfium-x64"
check_sha256 "$inputs/pdfium-x64/lib/libpdfium.so" "$PDFIUM_SO_SHA256"
[[ -f "$inputs/pdfium-x64/LICENSE" ]] || die "PDFium archive has no LICENSE"
note "PDFium $PDFIUM_RELEASE (x64)"

# 3. NDI runtime.
ndi="$inputs/ndi"
rm -rf "$ndi"
mkdir -p "$ndi"
grandi_tgz="$inputs/grandi-linux-x64-$GRANDI_VERSION.tgz"
if [[ -n "${KAIRO_GRANDI_TGZ:-}" && ! -f "$grandi_tgz" ]]; then
  cp "$KAIRO_GRANDI_TGZ" "$grandi_tgz"
fi
if [[ ! -f "$grandi_tgz" ]]; then
  curl -fL --retry 3 -o "$grandi_tgz.part" \
    "https://registry.npmjs.org/@grandi/linux-x64/-/linux-x64-$GRANDI_VERSION.tgz"
  mv "$grandi_tgz.part" "$grandi_tgz"
fi
tar -xzf "$grandi_tgz" -C "$ndi" --strip-components=1 package/libndi.so.6 package/libndi_licenses.txt
check_sha256 "$ndi/libndi.so.6" "$NDI_SO_SHA256"
check_sha256 "$ndi/libndi_licenses.txt" "$NDI_LINUX_LICENSES_SHA256"
note "NDI runtime from @grandi/linux-x64 $GRANDI_VERSION"
