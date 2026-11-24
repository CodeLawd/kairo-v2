# Pinned inputs for the Windows package (prompt 102), for the bash scripts in
# this folder (MSYS2 or Git Bash). Shared pins are in ../common.sh.
# shellcheck shell=bash disable=SC2034

KAIRO_LOG_PREFIX="package-windows"
# shellcheck source=../common.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/common.sh"

# FFmpeg DLLs next to Kairo.exe, and the import libraries Rust links.
FFMPEG_DLLS=(avutil-61.dll swresample-7.dll avcodec-63.dll avformat-63.dll swscale-10.dll)
FFMPEG_IMPORT_LIBS=(avutil avcodec avformat swscale)
# kairo-video asks for AV_HWDEVICE_TYPE_D3D11VA, which uses the d3d11va2
# hwaccels; the older d3d11va ones are kept for completeness.
FFMPEG_HWACCELS="h264_d3d11va,h264_d3d11va2,hevc_d3d11va,hevc_d3d11va2,vp9_d3d11va,vp9_d3d11va2,av1_d3d11va,av1_d3d11va2"

# The configure line, without --prefix and toolchain flags, which
# build-ffmpeg.sh adds. Written verbatim into the shipped source notice.
ffmpeg_configure_flags() {
  ffmpeg_common_flags
  printf '%s\n' --enable-d3d11va --disable-pthreads --enable-w32threads
}

# PDFium for Windows x64 (pdfium.dll links the C runtime statically).
PDFIUM_SHA256_x64="8d48973e3eb472dda0f5f2435e595ac043f9f2219dd71883392f549445afa8a5"
PDFIUM_DLL_SHA256="71a3e63e6ab8f5746a0fc9de9bc3d0c0dc07eb60990dfd37d9db7fc891821943"

# NDI runtime from @grandi/win32-x64 (Authenticode signed by Vizrt; static CRT).
NDI_DLL_SHA256="2b6602075868ba4401f82f417d72424805d69b11ca86078023d0d489ff45dd84"

# Microsoft Edge WebView2 Evergreen bootstrapper. Microsoft serves the current
# build at this link, so it is checked by its Authenticode signature
# (Microsoft Corporation), not a pinned hash.
WEBVIEW2_BOOTSTRAPPER_URL="https://go.microsoft.com/fwlink/p/?LinkId=2124703"
