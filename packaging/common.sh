# Pins shared by every platform's package (prompts 101 to 103). Sourced by
# packaging/<platform>/config.sh, which sets KAIRO_LOG_PREFIX first.
# Change a pin only together with its checksum.
# shellcheck shell=bash disable=SC2034

KAIRO_PRODUCT_NAME="Kairo"
KAIRO_APP_ID="com.kairo.app"
KAIRO_COPYRIGHT="Copyright © 2026 Kairo"

# Bundled data and icons come from a pinned commit of the Electron repository.
# They are fetched at package time and never committed here (bible.db is 43 MB).
KAIRO_PRESENTER_REPO="CodeLawd/kairo-presenter"
KAIRO_PRESENTER_COMMIT="af978c5159f4d433dc93b2de7a8f92f92765b74e"
BIBLE_DB_SHA256="aabe5bd76f6e73c953366a7547fa4eeec0b3ba40cc6401b2d0971ae03ccbc6cd"
ICON_PNG_SHA256="7c1e22a35c89414699868a797d86ddf59b2fa456b308244eb53818ebc3a6ee23"
ICON_ICNS_SHA256="d7e289121d0f68a3bfda6ad6b27e376eb40a77dff2886df45dc825cd69d2912e"
ICON_ICO_SHA256="7025520f8b8d2af224f35df95cca543b3182c671b13c05e54b687239b241c1b7"

# FFmpeg: LGPL shared build from source (prompts 101 to 103, amendment from
# 043). The major versions must match what ffmpeg-next =9.0.0 was built
# against: libavutil 61, libavcodec 63, libavformat 63, libswscale 10.
# libswresample 7 is not linked by Kairo, but FFmpeg's Opus decoder needs it.
FFMPEG_VERSION="9.0.2"
FFMPEG_URL="https://ffmpeg.org/releases/ffmpeg-${FFMPEG_VERSION}.tar.xz"
FFMPEG_SHA256="8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e"

# libvpx (BSD-3-Clause), decoders only, linked statically into libavcodec.
# FFmpeg's own VP9 decoder drops the alpha channel that WebM keeps in Matroska
# BlockAdditional; only libvpx reads it (decision 055), and Chromium played
# transparent WebM. Checksum as pinned by Homebrew's libvpx formula.
LIBVPX_VERSION="1.17.0"
LIBVPX_URL="https://github.com/webmproject/libvpx/archive/refs/tags/v${LIBVPX_VERSION}.tar.gz"
LIBVPX_SHA256="1020f184046187baa2985dbde38e0691f49c44088bca7a1842b0236c6081dc0a"

libvpx_configure_flags() {
  printf '%s\n' \
    --enable-static --disable-shared --enable-pic \
    --disable-vp8-encoder --disable-vp9-encoder \
    --disable-examples --disable-tools --disable-docs --disable-unit-tests \
    --disable-webm-io --disable-libyuv --enable-vp9-highbitdepth
}

# Every container and codec the media dock (mp4, webm, m4v, mov, ogv) and the
# tracks library (mp3, wav, m4a, aac, ogg, flac) accept. FFmpeg's own decoders,
# plus libvpx for VP8 and VP9 with alpha.
FFMPEG_DEMUXERS="mov,matroska,ogg,mp3,wav,aac,flac,aiff"
FFMPEG_VIDEO_DECODERS="h264,hevc,vp8,vp9,libvpx_vp8,libvpx_vp9,av1,prores,theora,mpeg4,mjpeg,png,qtrle,rawvideo,dnxhd"
FFMPEG_AUDIO_DECODERS="aac,aac_fixed,mp3,mp3float,opus,vorbis,flac,alac,ac3,eac3,pcm_s16le,pcm_s16be,pcm_s24le,pcm_s24be,pcm_s32le,pcm_s32be,pcm_f32le,pcm_f32be,pcm_u8"
FFMPEG_PARSERS="h264,hevc,vp8,vp9,av1,aac,mpegaudio,opus,vorbis,flac,ac3,mjpeg,png"
FFMPEG_BSFS="vp9_superframe_split,h264_mp4toannexb,hevc_mp4toannexb"

# The platform-neutral configure flags. Each platform adds its hardware
# decoding (FFMPEG_HWACCELS and the API switch) and toolchain flags. Written
# verbatim into the shipped source notice.
ffmpeg_common_flags() {
  printf '%s\n' \
    --enable-shared --disable-static \
    --disable-gpl --disable-nonfree --disable-version3 \
    --disable-autodetect --enable-zlib --enable-libvpx \
    --disable-programs --disable-doc --disable-debug \
    --disable-avdevice --disable-avfilter \
    --disable-network --disable-everything \
    --enable-protocol=file \
    "--enable-demuxer=${FFMPEG_DEMUXERS}" \
    "--enable-decoder=${FFMPEG_VIDEO_DECODERS},${FFMPEG_AUDIO_DECODERS}" \
    "--enable-parser=${FFMPEG_PARSERS}" \
    "--enable-hwaccel=${FFMPEG_HWACCELS:?FFMPEG_HWACCELS is per platform}" \
    "--enable-bsf=${FFMPEG_BSFS}"
}

# PDFium (decisions 042, 045): bblanchon/pdfium-binaries, chromium/8138.
PDFIUM_RELEASE="chromium/8138"
PDFIUM_URL_BASE="https://github.com/bblanchon/pdfium-binaries/releases/download/${PDFIUM_RELEASE}"

# NDI runtime (decision 061): the runtime the Electron release ships, from
# the @grandi prebuilt packages (NDI SDK 6.3.2). Each file is checked by its
# own hash, so the npm tarball's packaging cannot change what ships.
GRANDI_VERSION="2.0.2"
LIBNDI_LICENSES_SHA256="9f18e17e95324b1907c4de45f676ce2fedbc0a5c4ff6ed451b31c3fcb1229b8d"

die() { echo "${KAIRO_LOG_PREFIX:-package}: $*" >&2; exit 1; }
note() { echo "${KAIRO_LOG_PREFIX:-package}: $*"; }

sha256_of() {
  if command -v sha256sum >/dev/null; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

# check_sha256 FILE EXPECTED: fail unless the file has that hash.
check_sha256() {
  local actual
  [[ "$2" == @*@ || -z "$2" ]] && die "no pinned checksum for $1 yet (got $(sha256_of "$1")); pin it"
  actual="$(sha256_of "$1")"
  [[ "$actual" == "$2" ]] || die "checksum mismatch for $1: expected $2, got $actual"
}

# download URL FILE SHA256: fetch once, then always check the hash.
download() {
  if [[ ! -f "$2" ]]; then
    note "downloading $1"
    curl -fL --retry 3 -o "$2.part" "$1"
    mv "$2.part" "$2"
  fi
  check_sha256 "$2" "$3"
}

# presenter_resource REPO_PATH DEST SHA256: a file from the Electron repository
# at the pinned commit; from KAIRO_PRESENTER_DIR's git history when set (never
# its working copy, which the owner may be editing), else from GitHub.
presenter_resource() {
  if [[ -f "$2" ]] && [[ "$(sha256_of "$2")" == "$3" ]]; then return; fi
  if [[ -n "${KAIRO_PRESENTER_DIR:-}" ]]; then
    git -C "$KAIRO_PRESENTER_DIR" show "${KAIRO_PRESENTER_COMMIT}:$1" >"$2.part" \
      || die "$KAIRO_PRESENTER_DIR has no $1 at $KAIRO_PRESENTER_COMMIT"
    mv "$2.part" "$2"
  else
    curl -fL --retry 3 -o "$2.part" \
      "https://raw.githubusercontent.com/${KAIRO_PRESENTER_REPO}/${KAIRO_PRESENTER_COMMIT}/$1"
    mv "$2.part" "$2"
  fi
  check_sha256 "$2" "$3"
}
