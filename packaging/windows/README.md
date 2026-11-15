# Windows packaging

Builds the Windows x64 installer `Kairo-Setup-<version>.exe`, its `latest.yml` and
blockmap (prompt 102). Choices and reasons: `native-migration/DECISIONS.md`, entry 102.

The installer is made by electron-builder's NSIS target from a folder we stage
(`--prepackaged`). It has the same identity as the Electron installer. That gives the same
GUID (`440e67a7-8e87-5084-89a4-da16aeff9350`, from `com.kairo.app`), registry keys,
per-user default folder (`%LOCALAPPDATA%\Programs\Kairo`), choosable folder, shortcuts with
AppUserModelID `com.kairo.app`, `.kairo` association and uninstall entry. It also removes the
old version the same way and handles the in-app updater's `--updated`, `/S` and
`--force-run` the same way. Installing over the Electron app therefore upgrades it in place,
and the user data in `%APPDATA%\Kairo` stays.

## Steps

On Windows with Visual Studio 2022 or its Build Tools (C++ x64 tools), Rust, Node 22 or
later and MSYS2:

1. In an MSYS2 UCRT64 shell, from the repository root, after
   `pacman -S --needed mingw-w64-ucrt-x86_64-gcc mingw-w64-ucrt-x86_64-nasm mingw-w64-ucrt-x86_64-pkgconf make diffutils perl`:

   ```sh
   packaging/windows/build-ffmpeg.sh target/package-windows
   packaging/windows/fetch-inputs.sh target/package-windows
   ```

2. In PowerShell:

   ```powershell
   $env:KAIRO_API_URL = 'https://…'; $env:KAIRO_WEB_URL = 'https://…'
   $env:KAIRO_UNSIGNED = '1'   # or WIN_CSC_LINK and WIN_CSC_KEY_PASSWORD to sign
   ./packaging/windows/package.ps1
   ```

Output: `target/package-windows/dist`. The CI job is `.github/workflows/package-windows.yml`.

## Scripts

| Script | Does |
|---|---|
| `config.sh` | Windows pins: FFmpeg DLL names, D3D11VA hwaccels, PDFium and NDI checksums. Shared pins are in `../common.sh` |
| `build-ffmpeg.sh` | libvpx (static, decoders only), then LGPL shared FFmpeg with MinGW-w64 UCRT64, plus MSVC import libraries. The DLLs need only Windows' own DLLs |
| `fetch-inputs.sh` | `bible.db`, `icon.png`, `icon.ico` at the pinned Electron commit; PDFium; the NDI runtime. All checksummed |
| `package.ps1` | `cargo build` (static C runtime), stages the app folder, icon and version info (`edit-resources.mjs`), signs, gets the WebView2 bootstrapper, runs electron-builder, verifies |
| `edit-resources.mjs` | Icon and version information on `Kairo.exe` and `kairo-engine.exe` (resedit) |
| `verify-package.ps1` | The port of Electron's `verify-package.mjs`; runs on any Windows (no Visual Studio) |
| `install-test.ps1` | Clean install, upgrade over Electron, launch with no FFmpeg on PATH, uninstall |
| `pe.ps1`, `vsenv.ps1` | PE import reader; Visual Studio developer shell |
| `electron-builder.yml`, `build/installer.nsh` | Installer identity; WebView2 runtime when missing |

## Install folder

```
Kairo.exe  kairo-engine.exe
avutil-61.dll  swresample-7.dll  avcodec-63.dll  avformat-63.dll  swscale-10.dll
pdfium.dll  Processing.NDI.Lib.x64.dll
resources\bible.db  resources\icon.png  resources\elevate.exe (electron-builder)
licenses\ffmpeg  licenses\libvpx  licenses\pdfium  licenses\ndi  licenses\fonts  licenses\icons
Uninstall Kairo.exe
```

No VC++ redistributable: the Rust binaries link the C runtime statically, PDFium and NDI
do too, and the MinGW FFmpeg uses the Universal C Runtime that Windows 10 and 11 include.
`package.ps1` copies a VC++ DLL beside the binaries only if one is still imported, and
`verify-package.ps1` fails if one would come from System32.

## Testing on clean Windows 10 and 11 VMs

GitHub's runners are Windows Server, so client Windows is tested by hand. Copy the
installer and the `packaging/windows` folder to the VM. In PowerShell:

```powershell
Set-ExecutionPolicy -Scope Process Bypass
./install-test.ps1 -Installer .\Kairo-Setup-<version>.exe -Version <version>
```

For the upgrade, download `Kairo-Setup-1.0.33.exe` from the Electron release and add
`-ElectronInstaller .\Kairo-Setup-1.0.33.exe`. Then check by hand:
- the installer's own window (folder choice, finish page);
- a video thumbnail in the Media dock;
- web pages with the WebView2 runtime absent (Windows 10 without Edge's runtime), so the
  installer installs it.
