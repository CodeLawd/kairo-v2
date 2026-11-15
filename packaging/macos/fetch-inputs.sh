#!/usr/bin/env bash
# Gather the pinned, checksummed inputs that are not built here (prompt 101):
#
#   packaging/macos/fetch-inputs.sh <arm64|x86_64> <work-dir>
#
# - bible.db, icon.png, icon.icns from CodeLawd/kairo-presenter at
#   KAIRO_PRESENTER_COMMIT. Never committed to this repository.
# - PDFium from bblanchon/pdfium-binaries.
# - The NDI runtime and its licence from the @grandi npm package.
#
# Local overrides (no download; checksums still apply):
#   KAIRO_PRESENTER_DIR  an Electron checkout to copy resources from
#   KAIRO_PDFIUM_TGZ     a pdfium-mac-<arch>.tgz already on disk
#   KAIRO_NDI_DIR        a folder holding libndi.dylib and libndi_licenses.txt
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=config.sh
source "$here/config.sh"

arch="${1:?usage: fetch-inputs.sh <arm64|x86_64> <work-dir>}"
work="${2:?usage: fetch-inputs.sh <arm64|x86_64> <work-dir>}"
rust_target_for "$arch" >/dev/null
mkdir -p "$work/inputs/resources"
work="$(cd "$work" && pwd)"
inputs="$work/inputs"

fetch() { # fetch URL DEST
  curl -fL --retry 3 -o "$2.part" "$1"
  mv "$2.part" "$2"
}

# 1. Electron resources at the pinned commit (presenter_resource, ../common.sh).
presenter_resource resources/bible.db "$inputs/resources/bible.db" "$BIBLE_DB_SHA256"
presenter_resource resources/icon.png "$inputs/resources/icon.png" "$ICON_PNG_SHA256"
presenter_resource build/icon.icns "$inputs/resources/icon.icns" "$ICON_ICNS_SHA256"
note "resources from ${KAIRO_PRESENTER_REPO}@${KAIRO_PRESENTER_COMMIT:0:10}"

# 2. PDFium.
pdfium_sha_var="PDFIUM_SHA256_$arch"
pdfium_sha="${!pdfium_sha_var}"
pdfium_arch="$arch"
[[ "$arch" == x86_64 ]] && pdfium_arch=x64
pdfium_tgz="$inputs/pdfium-mac-$pdfium_arch.tgz"
if [[ ! -f "$pdfium_tgz" ]] || [[ "$(sha256_of "$pdfium_tgz")" != "$pdfium_sha" ]]; then
  if [[ -n "${KAIRO_PDFIUM_TGZ:-}" ]]; then
    cp "$KAIRO_PDFIUM_TGZ" "$pdfium_tgz"
  else
    fetch "$PDFIUM_URL_BASE/pdfium-mac-$pdfium_arch.tgz" "$pdfium_tgz"
  fi
fi
check_sha256 "$pdfium_tgz" "$pdfium_sha"
rm -rf "$inputs/pdfium-$arch"
mkdir -p "$inputs/pdfium-$arch"
tar -xzf "$pdfium_tgz" -C "$inputs/pdfium-$arch"
[[ -f "$inputs/pdfium-$arch/lib/libpdfium.dylib" ]] || die "PDFium archive has no lib/libpdfium.dylib"
[[ -f "$inputs/pdfium-$arch/LICENSE" ]] || die "PDFium archive has no LICENSE"
note "PDFium $PDFIUM_RELEASE ($pdfium_arch)"

# 3. NDI runtime.
ndi="$inputs/ndi"
mkdir -p "$ndi"
if [[ ! -f "$ndi/libndi.dylib" ]] || [[ "$(sha256_of "$ndi/libndi.dylib")" != "$LIBNDI_SHA256" ]]; then
  if [[ -n "${KAIRO_NDI_DIR:-}" ]]; then
    cp "$KAIRO_NDI_DIR/libndi.dylib" "$KAIRO_NDI_DIR/libndi_licenses.txt" "$ndi/"
  else
    npm_arch=arm64
    [[ "$arch" == x86_64 ]] && npm_arch=x64
    tgz="$inputs/grandi-darwin-$npm_arch-$GRANDI_VERSION.tgz"
    fetch "https://registry.npmjs.org/@grandi/darwin-$npm_arch/-/darwin-$npm_arch-$GRANDI_VERSION.tgz" "$tgz"
    tar -xzf "$tgz" -C "$ndi" --strip-components=1 package/libndi.dylib package/libndi_licenses.txt
  fi
fi
check_sha256 "$ndi/libndi.dylib" "$LIBNDI_SHA256"
check_sha256 "$ndi/libndi_licenses.txt" "$LIBNDI_LICENSES_SHA256"
note "NDI runtime from @grandi/darwin $GRANDI_VERSION"
