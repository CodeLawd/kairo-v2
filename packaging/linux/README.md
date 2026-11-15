# Linux packaging

Builds `Kairo-<version>.AppImage`, `kairo-presenter_<version>_amd64.deb` and
`latest-linux.yml` for x64 (prompt 103). Choices and reasons:
`native-migration/DECISIONS.md`, entry 103.

The packages are made by electron-builder's AppImage and deb targets from a folder we stage
(`--prepackaged`), with the Electron release's identity:
- deb package `kairo-presenter`, installed in `/opt/Kairo` with `/usr/bin/kairo-presenter`;
- desktop entry `Kairo.desktop`, category `AudioVideo`;
- `latest-linux.yml` for the AppImage updater.

So the deb upgrades an Electron install in place, and the user's data in
`~/.config/Kairo` stays. New here is the `application/x-kairo` MIME type for `*.kairo`.

## Steps

On Ubuntu 22.04 (the oldest supported release), with the build dependencies the CI job
installs:

```sh
packaging/linux/build-ffmpeg.sh target/package-linux
packaging/linux/fetch-inputs.sh target/package-linux
KAIRO_API_URL=https://… KAIRO_WEB_URL=https://… packaging/linux/package.sh target/package-linux
```

The CI job is `.github/workflows/package-linux.yml`. It builds on Ubuntu 22.04, then runs
`install-test.sh` in clean `ubuntu:22.04` and `ubuntu:24.04` containers. Each container
runs it twice: once on a clean system, and once over the Electron 1.0.33 deb.

## Scripts

| Script | Does |
|---|---|
| `config.sh` | Linux pins: FFmpeg sonames and VA-API, PDFium, NDI and icon checksums, deb dependencies `dpkg-shlibdeps` cannot see |
| `build-ffmpeg.sh` | libvpx (static, decoders only), then LGPL shared FFmpeg with sonames and `$ORIGIN` runpaths |
| `fetch-inputs.sh` | `bible.db`, `icon.png` and the icon set at the pinned Electron commit; PDFium; the NDI runtime. All checksummed |
| `package.sh` | `cargo build`, stages the app folder, sets runpaths, works out deb `Depends` with `dpkg-shlibdeps`, runs electron-builder, verifies |
| `verify-package.sh` | The port of Electron's `verify-package.mjs` for the folder, the AppImage and the deb |
| `install-test.sh` | Clean install from apt, upgrade over Electron, launch, error messages, AppImage, purge |
| `electron-builder.yml` | Package identity |

## Install folder (`/opt/Kairo`, or the AppImage root)

```
kairo-presenter  kairo-engine          runpath $ORIGIN/lib
lib/libavutil.so.61  lib/libswresample.so.7  lib/libavcodec.so.63
lib/libavformat.so.63  lib/libswscale.so.10      runpath $ORIGIN
libpdfium.so  libndi.so.6              found beside the executable
resources/bible.db  resources/icon.png
licenses/ffmpeg  licenses/libvpx  licenses/pdfium  licenses/ndi  licenses/fonts  licenses/icons
```

The deb and the AppImage share this layout. FFmpeg goes in `lib/` beside the executables,
not in `/usr/lib/kairo`, so both formats load it the same way.

## What the system provides

Everything else comes from the system. The deb declares it:
- `dpkg-shlibdeps` reads it from the binaries (WebKitGTK 4.1, GTK 3, ALSA, Avahi for NDI,
  VA-API, …);
- plus the libraries opened at run time: `libvulkan1`, `libegl1`, `xdg-utils`;
- recommended: `mesa-vulkan-drivers` (software Vulkan when no GPU driver is installed) and
  `zenity` (the startup error dialog).

The AppImage expects the same libraries on the system, as Electron's did for GTK. Ubuntu
22.04 and 24.04 desktops have them, except possibly WebKitGTK 4.1 on 22.04
(`libwebkit2gtk-4.1-0`).

## Startup on Linux

Kairo starts on X11 only (XWayland in a Wayland session), because its WebKitGTK views need
it. Two failures show a message (zenity or kdialog, else a notification, and always on
stderr) and exit with status 1, instead of crashing or exiting silently:
- no `DISPLAY`;
- no graphics driver that Vulkan or OpenGL can use, so the window cannot open.

A missing Vulkan alone is not an error: drawing falls back to OpenGL.

## Not covered by CI

- A real desktop session: a real GPU, Wayland with XWayland, and the look of the window.
- A receiver on the network for the NDI output (CI only checks the runtime loads).
- A video thumbnail in the Media dock.
