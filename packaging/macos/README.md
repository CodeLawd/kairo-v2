# macOS packaging

Builds `Kairo.app`, the updater zip and the DMG for one architecture (prompt 101).
Choices and their reasons are in `native-migration/DECISIONS.md`, entry 101.

```sh
KAIRO_ADHOC_SIGN=1 \
KAIRO_API_URL=https://… KAIRO_WEB_URL=https://… \
packaging/macos/package.sh arm64
```

Output goes to `target/package-macos/dist`:
`Kairo-<version>-arm64.dmg` and `Kairo-<version>-arm64-mac.zip` for Apple Silicon, or
`Kairo-<version>.dmg` and `Kairo-<version>-mac.zip` for Intel (`x86_64`). These are
electron-builder's names, which the updater (`pick_mac_zip`) and the website expect.
The version is the `kairo` package version from `Cargo.toml`.

## Scripts

| Script | Does |
|---|---|
| `config.sh` | macOS pins: bundle ids, minimum macOS, FFmpeg dylib names and VideoToolbox, PDFium and NDI checksums. Shared pins (FFmpeg and libvpx versions, checksums and configure flags, the Electron commit for `bible.db` and icons) are in `../common.sh` |
| `build-ffmpeg.sh` | libvpx (static, decoders only) then LGPL shared FFmpeg from the pinned sources, `@rpath` install names, licences and source notice |
| `fetch-inputs.sh` | `bible.db`, `icon.png`, `icon.icns` from `CodeLawd/kairo-presenter` at the pinned commit; PDFium; NDI runtime. All checksummed |
| `bundle.sh` | `cargo build --release` against the bundled FFmpeg, assembles the bundle, rewrites linkage, signs inside-out |
| `notarize.sh` | `notarytool submit --wait`, then staples. Retries transient failures; prints Apple's log on rejection |
| `make-dmg.sh` | DMG with Electron's layout (540×380, app at 140,200, Applications at 400,200) |
| `verify-package.sh` | The port of Electron's `verify-package.mjs`; run on the app or the DMG |
| `import-certificate.sh` | CI: puts the Developer ID certificate in a temporary keychain |
| `package.sh` | All of the above in order |

The CI job is `.github/workflows/package-macos.yml`.

## Bundle layout

```
Kairo.app/Contents/
  Info.plist  PkgInfo
  MacOS/Kairo                 UI process (com.kairo.app)
  MacOS/kairo-engine          output process, accessory (com.kairo.app.engine)
  Frameworks/                 libavutil.61, libswresample.7, libavcodec.63,
                              libavformat.63, libswscale.10, libpdfium, libndi
  Resources/bible.db  icon.icns  icon.png
  Resources/decklink/decklink-helper   (com.kairo.app.decklink-helper)
  Resources/licenses/         ffmpeg (LGPL text, source and written offer), libvpx,
                              pdfium, ndi, fonts, icons, syphon
```

Every library is `@rpath/<name>`; both executables carry `@executable_path/../Frameworks`
and each library `@loader_path`. Everything is built for one architecture only.

## Signing

| Environment | Result |
|---|---|
| `KAIRO_SIGN_IDENTITY="Developer ID Application: …"` | Hardened runtime, secure timestamps, entitlements from `entitlements/`, then notarized and stapled (app and DMG). `KAIRO_NOTARIZE=0` skips notarizing, for local checks only |
| `KAIRO_ADHOC_SIGN=1` | Ad-hoc signature with no hardened runtime, as the Electron release does until the secrets exist. Gatekeeper shows the "unidentified developer" prompt |

Signing goes inside-out: the libraries, the DeckLink helper, the engine, then the bundle.
Notarization reads `APPLE_ID`, `APPLE_APP_SPECIFIC_PASSWORD` and `APPLE_TEAM_ID`. CI
reads the certificate from `CSC_LINK` and `CSC_KEY_PASSWORD`. These are the Electron
release's secret names (`kairo-presenter/docs/signing-setup.md`).

## Local inputs

`fetch-inputs.sh` downloads by default. To use local copies (checksums still apply):

```sh
KAIRO_PRESENTER_DIR=../kairo-presenter \
KAIRO_PDFIUM_TGZ=.cache/pdfium/pdfium.tgz \
KAIRO_NDI_DIR=../kairo-presenter/node_modules/@grandi/darwin-arm64 \
packaging/macos/fetch-inputs.sh arm64 target/package-macos
```

With `KAIRO_PRESENTER_DIR`, the files come from the pinned commit through `git show`,
not from the working copy.

An Intel package needs `nasm` to build FFmpeg (`brew install nasm`). It is a build tool
and is not bundled. Never point the build at a Homebrew FFmpeg: Homebrew's is GPL.
`verify-package.sh` fails if any binary names one.

## Changing a pin

Update the value and its checksum together, in `../common.sh` for shared pins or `config.sh` for macOS ones. For FFmpeg, the majors in
`FFMPEG_LIBS` must match what `ffmpeg-next` links (`otool -L` on a development build).
`build-ffmpeg.sh` fails when configure ignores a component name or drops one for a
missing dependency.

## Known limits

- The minimum is macOS 13, because the PDFium build and the NDI runtime need it.
  The Electron releases declared 11. The release feed must carry
  `minimumSystemVersion: 22.0.0` (the Darwin version of macOS 13, which
  electron-updater compares with `os.release()`), so that Electron installs on
  macOS 11 and 12 are not offered a build they cannot open (DECISIONS.md 101).
- The ad-hoc build has no hardened runtime and is not notarized. Gatekeeper shows the
  "unidentified developer" prompt for it.
