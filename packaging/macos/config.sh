# Pinned inputs for the macOS package (prompt 101). Sourced by the other
# scripts in this folder. Shared pins (FFmpeg, libvpx, the Electron resources
# commit, PDFium release, NDI package) are in ../common.sh.
# shellcheck shell=bash disable=SC2034

KAIRO_LOG_PREFIX="package-macos"
# shellcheck source=../common.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/common.sh"

KAIRO_BUNDLE_ID="$KAIRO_APP_ID"
KAIRO_ENGINE_ID="com.kairo.app.engine"
KAIRO_DECKLINK_ID="com.kairo.app.decklink-helper"
# macOS 13 (owner, 2026-11-15, DECISIONS 101; supersedes 008's 11): the PDFium
# build and the NDI 6.3.2 runtime both require 13.
KAIRO_MIN_MACOS="13.0"

FFMPEG_LIBS=(libavutil.61 libswresample.7 libavcodec.63 libavformat.63 libswscale.10)
FFMPEG_HWACCELS="h264_videotoolbox,hevc_videotoolbox,vp9_videotoolbox,prores_videotoolbox,av1_videotoolbox"

# The configure line, without --prefix, --arch and toolchain flags, which
# build-ffmpeg.sh adds. Written verbatim into the shipped source notice.
ffmpeg_configure_flags() {
  ffmpeg_common_flags
  printf '%s\n' --enable-videotoolbox "--install-name-dir=@rpath"
}

PDFIUM_SHA256_arm64="21a1567b397b571f4f77f21ca91eac7b892774219c3c715a5127a20950ccb18b"
PDFIUM_SHA256_x86_64="72662ac8eff7bf5381ef82ceebd0a0ad5bddb051b303981115114e6972fd201c"

# libndi.dylib from @grandi/darwin-* (universal).
LIBNDI_SHA256="af53789fcb8fc1737e6827a54d7ccea09736dc759e00ef02d53f4ed64d9abbd5"

# Architectures: one package per architecture, as the Electron release does,
# so the updater's latest-mac.yml keeps one zip per arch.
rust_target_for() {
  case "$1" in
    arm64) echo aarch64-apple-darwin ;;
    x86_64) echo x86_64-apple-darwin ;;
    *) echo "unknown architecture: $1" >&2; return 1 ;;
  esac
}

# electron-builder's artifact names, which the updater's pick_mac_zip and the
# website's download links rely on: arm64 carries "-arm64", x64 carries none.
artifact_suffix_for() {
  case "$1" in
    arm64) echo "-arm64" ;;
    x86_64) echo "" ;;
  esac
}
