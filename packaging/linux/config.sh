# Pinned inputs for the Linux package (prompt 103). Shared pins are in
# ../common.sh.
# shellcheck shell=bash disable=SC2034

KAIRO_LOG_PREFIX="package-linux"
# shellcheck source=../common.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/common.sh"

# electron-builder names the executable after package.json `name`, as the
# Electron release did: /opt/Kairo/kairo-presenter, /usr/bin/kairo-presenter.
KAIRO_EXECUTABLE="kairo-presenter"

# FFmpeg shared objects by soname; bundled in <app>/lib.
FFMPEG_SONAMES=(libavutil.so.61 libswresample.so.7 libavcodec.so.63 libavformat.so.63 libswscale.so.10)
# kairo-video asks for AV_HWDEVICE_TYPE_VAAPI. VA-API is libva (MIT), which the
# deb declares and every Ubuntu desktop has.
FFMPEG_HWACCELS="h264_vaapi,hevc_vaapi,vp9_vaapi,av1_vaapi"

# The configure line, without --prefix and toolchain flags, which
# build-ffmpeg.sh adds. Written verbatim into the shipped source notice.
ffmpeg_configure_flags() {
  ffmpeg_common_flags
  printf '%s\n' --enable-vaapi --enable-pthreads
}

PDFIUM_SHA256_x64="d6b1e28950a0392590d79bbdbb7eb19686872b688f8a36f170acb19e41473113"
PDFIUM_SO_SHA256="655e831208783f38dc187ad424bdfdae39858cfbfcf3a9a2cb9dd0694b5abcad"

# NDI runtime from @grandi/linux-x64 (needs Avahi's client libraries).
NDI_SO_SHA256="1c70a0068bb8ad1c894e556bc7c0e790dbd2c8229167d808d137a4b944355ae3"
NDI_LINUX_LICENSES_SHA256="bbec10a5f5caf19b134dd6e617f898357a26c1df20a14de636b8ea29f82888d6"

# The Electron release's Linux icons (build/icons), by size.
LINUX_ICON_SIZES=(16 32 64 128 256 512)
icon_sha256_for() {
  case "$1" in
    16) echo ff55f7772928ebcdc5e19cb80179eb8e93cd6613c76e30eaa4e5e7695b276453 ;;
    32) echo eeb4503d0dd3ba94ad8f2114137e266d3fbe670f86d1e1ff86bb353e089a3643 ;;
    64) echo c1c86a1cdc9c320f0c9521e57096643afcbc2d141cfacbba6abc1168e266054b ;;
    128) echo b81ef1fdc197945ffb8411b30f2ade5da7b8a066396d29f1275d36e164ae8a21 ;;
    256) echo 1721d0bbf9a4a844809c07f6e5e59e3511cb387e3dceb828cd9197d22c2a6bef ;;
    512) echo 292192920af80513f0f241a96b9c049d1ef626bcdcbb9c256fb214eafb04d2c4 ;;
  esac
}

# Packages the deb needs beyond what dpkg-shlibdeps finds in the binaries:
# libraries opened at run time (Vulkan loader, EGL for the OpenGL fallback),
# xdg-utils for opening links and folders.
DEB_RUNTIME_DEPENDS=(libvulkan1 libegl1 xdg-utils)
# Not required, but what makes the experience complete on a desktop:
# software Vulkan when no GPU driver is installed, and the error dialog tool.
DEB_RECOMMENDS=(mesa-vulkan-drivers zenity)
