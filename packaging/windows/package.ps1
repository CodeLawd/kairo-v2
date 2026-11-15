<#
.SYNOPSIS
  Build, stage, sign and wrap Kairo for Windows x64 (prompt 102).

.DESCRIPTION
  packaging/windows/package.ps1 [-WorkDir <dir>]

  Needs Visual Studio or its Build Tools with the C++ x64 tools (vsenv.ps1
  enters their developer shell), Rust, Node 22 or later, and, with the same
  work directory, first:
    build-ffmpeg.sh <work>   in MSYS2 UCRT64   -> <work>/ffmpeg-x64
    fetch-inputs.sh <work>   in any bash       -> <work>/inputs

  Steps:
    1. cargo build --release for x86_64-pc-windows-msvc against the bundled
       FFmpeg, with the C runtime linked statically (+crt-static).
    2. Stage <work>/app as electron-builder's win-unpacked folder: Kairo.exe,
       kairo-engine.exe, the FFmpeg, PDFium and NDI DLLs beside them,
       resources\bible.db and icon.png, licences. A VC++ runtime DLL is copied
       beside them only if a binary still imports one.
    3. Icon and version information (edit-resources.mjs).
    4. Sign the app's own files with WIN_CSC_LINK / WIN_CSC_KEY_PASSWORD (the
       Electron release's secrets), or build unsigned with KAIRO_UNSIGNED=1,
       as the Electron release does until a certificate exists.
    5. Fetch Microsoft's WebView2 bootstrapper and check its signature.
    6. electron-builder --prepackaged: the NSIS installer with Electron's
       identity, plus latest.yml and the blockmap. It signs the installer and
       uninstaller itself when WIN_CSC_LINK is set.
    7. verify-package.ps1 on the staged folder and the installer.

  The release endpoints (KAIRO_API_URL, KAIRO_WEB_URL) must be set: a release
  build refuses local or non-https URLs.
#>
param(
  [string]$WorkDir = (Join-Path $PSScriptRoot '..\..\target\package-windows')
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Fail([string]$Message) { throw "package-windows: $Message" }
function Note([string]$Message) { Write-Host "package-windows: $Message" }
function Invoke-Checked([string]$What, [scriptblock]$Block) {
  & $Block
  if ($LASTEXITCODE -ne 0) { Fail "$What failed with exit code $LASTEXITCODE" }
}

. (Join-Path $PSScriptRoot 'vsenv.ps1')
. (Join-Path $PSScriptRoot 'pe.ps1')
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
New-Item -ItemType Directory -Force -Path $WorkDir | Out-Null
$work = (Resolve-Path $WorkDir).Path
$ffmpeg = Join-Path $work 'ffmpeg-x64'
$inputs = Join-Path $work 'inputs'
if (-not (Test-Path (Join-Path $ffmpeg '.kairo-stamp'))) { Fail "no FFmpeg build at $ffmpeg; run build-ffmpeg.sh $work in MSYS2 UCRT64" }
if (-not (Test-Path (Join-Path $inputs 'resources\bible.db'))) { Fail "no inputs at $inputs; run fetch-inputs.sh $work" }
foreach ($tool in 'cargo', 'node', 'npm', 'link') {
  if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) { Fail "$tool is not on PATH" }
}

$signed = [bool]$env:WIN_CSC_LINK
if (-not $signed -and $env:KAIRO_UNSIGNED -ne '1') {
  Fail 'set WIN_CSC_LINK and WIN_CSC_KEY_PASSWORD to sign, or KAIRO_UNSIGNED=1 for an unsigned build'
}
foreach ($var in 'KAIRO_API_URL', 'KAIRO_WEB_URL') {
  if (-not (Get-Item "env:$var" -ErrorAction SilentlyContinue)) { Fail "$var is not set" }
}

Push-Location $repo
try {
  $pkgid = (cargo pkgid -p kairo)
  if ($LASTEXITCODE -ne 0) { Fail 'cargo pkgid failed' }
} finally { Pop-Location }
$version = ($pkgid -split '[#@]')[-1]
if ($version -notmatch '^\d+\.\d+\.\d+$') { Fail "cannot read the kairo package version (got '$version')" }
Note "Kairo $version for x64, $(if ($signed) { 'signed' } else { 'unsigned' })"

# 1. Build.
$target = 'x86_64-pc-windows-msvc'
$cargoTarget = Join-Path $work 'target'
$env:FFMPEG_DIR = $ffmpeg
$env:CARGO_TARGET_DIR = $cargoTarget
# No VC++ redistributable: the C runtime is linked into the executables.
$env:RUSTFLAGS = '-C target-feature=+crt-static'
Push-Location $repo
try {
  Invoke-Checked 'cargo build' { cargo build --release --locked --target $target -p kairo -p kairo-engine }
} finally { Pop-Location }
$bin = Join-Path $cargoTarget "$target\release"

# 2. Stage.
$app = Join-Path $work 'app'
if (Test-Path $app) { Remove-Item -Recurse -Force $app }
$res = Join-Path $app 'resources'
$lic = Join-Path $app 'licenses'
New-Item -ItemType Directory -Force -Path $res, $lic | Out-Null
Copy-Item (Join-Path $bin 'kairo.exe') (Join-Path $app 'Kairo.exe')
# kairo looks for the engine beside its own executable (apps/kairo/src/main.rs).
Copy-Item (Join-Path $bin 'kairo-engine.exe') (Join-Path $app 'kairo-engine.exe')
foreach ($dll in 'avutil-61.dll', 'swresample-7.dll', 'avcodec-63.dll', 'avformat-63.dll', 'swscale-10.dll') {
  $source = Join-Path $ffmpeg "bin\$dll"
  if (-not (Test-Path $source)) { Fail "FFmpeg build has no $dll" }
  Copy-Item $source $app
}
# PDFium and the NDI runtime are found beside the executable (kairo-documents,
# kairo-scripture and kairo-sinks look there).
Copy-Item (Join-Path $inputs 'pdfium-x64\bin\pdfium.dll') $app
Copy-Item (Join-Path $inputs 'ndi\Processing.NDI.Lib.x64.dll') $app
# The app reads resources\bible.db first (apps/kairo/src/services.rs), as the
# Electron package placed it.
Copy-Item (Join-Path $inputs 'resources\bible.db'), (Join-Path $inputs 'resources\icon.png') $res

foreach ($sub in 'ffmpeg', 'libvpx', 'pdfium', 'ndi', 'fonts', 'icons') {
  New-Item -ItemType Directory -Force -Path (Join-Path $lic $sub) | Out-Null
}
Copy-Item (Join-Path $ffmpeg 'licenses\ffmpeg\*') (Join-Path $lic 'ffmpeg')
Copy-Item (Join-Path $ffmpeg 'licenses\libvpx\*') (Join-Path $lic 'libvpx')
Copy-Item (Join-Path $inputs 'pdfium-x64\LICENSE') (Join-Path $lic 'pdfium')
Copy-Item (Join-Path $inputs 'pdfium-x64\licenses\*') (Join-Path $lic 'pdfium')
Copy-Item (Join-Path $inputs 'ndi\libndi_licenses.txt') (Join-Path $lic 'ndi')
Set-Content -Encoding utf8 -Path (Join-Path $lic 'ndi\NOTICE.txt') -Value @(
  'NDI® is a registered trademark of Vizrt NDI AB.',
  'The NDI runtime (Processing.NDI.Lib.x64.dll) is distributed under the NDI SDK',
  'licence; see libndi_licenses.txt beside this file and https://ndi.video.'
)
Copy-Item (Join-Path $repo 'crates\kairo-ui-kit\assets\licenses\source-sans-3-OFL.txt') (Join-Path $lic 'fonts')
Copy-Item (Join-Path $repo 'crates\kairo-ui-kit\assets\licenses\hugeicons-LICENSE.md') (Join-Path $lic 'icons')
Copy-Item (Join-Path $repo 'crates\kairo-ui-kit\assets\licenses\phosphor-LICENSE') (Join-Path $lic 'icons')

# A VC++ runtime DLL beside the binaries, only if one is still imported.
$vcDlls = @()
foreach ($pe in Get-ChildItem $app -Recurse -Include *.exe, *.dll) {
  $deps = (Get-PeInfo $pe.FullName).Imports | ForEach-Object { $_.ToLowerInvariant() }
  $vcDlls += $deps | Where-Object { $_ -match '^(vcruntime140(_1)?|msvcp140(_1|_2|_atomic_wait|_codecvt_ids)?|concrt140)\.dll$' }
}
$vcDlls = $vcDlls | Sort-Object -Unique
if ($vcDlls) {
  if (-not $env:VCToolsRedistDir) { Fail "binaries import $($vcDlls -join ', ') but VCToolsRedistDir is not set" }
  $crt = Get-ChildItem (Join-Path $env:VCToolsRedistDir 'x64') -Directory -Filter 'Microsoft.VC*.CRT' | Select-Object -First 1
  if (-not $crt) { Fail "no Microsoft.VC*.CRT folder under $env:VCToolsRedistDir\x64" }
  foreach ($dll in $vcDlls) {
    $source = Join-Path $crt.FullName $dll
    if (-not (Test-Path $source)) { Fail "$dll is imported but not in $($crt.FullName)" }
    Copy-Item $source $app
  }
  Note "copied the VC++ runtime beside the binaries: $($vcDlls -join ', ')"
}

# 3. Icon and version information.
Push-Location $PSScriptRoot
try {
  Invoke-Checked 'npm ci' { npm ci --ignore-scripts --no-audit --no-fund }
  Invoke-Checked 'edit-resources' { node edit-resources.mjs $app $version (Join-Path $inputs 'resources\icon.ico') }
} finally { Pop-Location }

# 4. Sign the app's own files. The NDI runtime keeps Vizrt's signature.
$signtool = $null
$pfx = $null
if ($signed) {
  $signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" -ErrorAction SilentlyContinue |
    Sort-Object FullName | Select-Object -Last 1
  if (-not $signtool) { Fail 'signtool.exe not found in the Windows 10 SDK' }
  if (Test-Path -LiteralPath $env:WIN_CSC_LINK -ErrorAction SilentlyContinue) {
    $pfx = (Resolve-Path -LiteralPath $env:WIN_CSC_LINK).Path
  } else {
    $pfx = Join-Path ([IO.Path]::GetTempPath()) "kairo-$([guid]::NewGuid()).pfx"
    [IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:WIN_CSC_LINK))
  }
  try {
    $files = @('Kairo.exe', 'kairo-engine.exe', 'avutil-61.dll', 'swresample-7.dll', 'avcodec-63.dll',
      'avformat-63.dll', 'swscale-10.dll', 'pdfium.dll') + $vcDlls | Where-Object { $_ }
    foreach ($file in $files) {
      $path = Join-Path $app $file
      if ($file -in $vcDlls -and (Get-AuthenticodeSignature $path).Status -eq 'Valid') { continue }
      for ($attempt = 1; $attempt -le 3; $attempt++) {
        & $signtool.FullName sign /fd sha256 /f $pfx /p $env:WIN_CSC_KEY_PASSWORD `
          /tr http://timestamp.digicert.com /td sha256 /d Kairo $path | Out-Host
        if ($LASTEXITCODE -eq 0) { break }
        if ($attempt -eq 3) { Fail "signing $file failed three times" }
        Start-Sleep -Seconds 15
      }
    }
  } finally {
    if ($pfx -and $pfx.StartsWith([IO.Path]::GetTempPath())) { Remove-Item -Force $pfx }
  }
} else {
  Write-Warning 'package-windows: no Windows certificate (WIN_CSC_LINK); building unsigned'
}

# 5. Build resources for the installer, with the WebView2 bootstrapper.
$buildRes = Join-Path $work 'build-resources'
if (Test-Path $buildRes) { Remove-Item -Recurse -Force $buildRes }
New-Item -ItemType Directory -Force -Path $buildRes | Out-Null
Copy-Item (Join-Path $PSScriptRoot 'build\installer.nsh') $buildRes
Copy-Item (Join-Path $inputs 'resources\icon.ico') $buildRes
$bootstrapper = Join-Path $buildRes 'MicrosoftEdgeWebview2Setup.exe'
$webview2Url = 'https://go.microsoft.com/fwlink/p/?LinkId=2124703'
for ($attempt = 1; $attempt -le 3; $attempt++) {
  try { Invoke-WebRequest -UseBasicParsing -Uri $webview2Url -OutFile $bootstrapper; break }
  catch { if ($attempt -eq 3) { throw }; Start-Sleep -Seconds 15 }
}
$signature = Get-AuthenticodeSignature $bootstrapper
if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'O=Microsoft Corporation') {
  Fail "the WebView2 bootstrapper is not validly signed by Microsoft ($($signature.Status), $($signature.SignerCertificate.Subject))"
}
Note "WebView2 bootstrapper $((Get-Item $bootstrapper).VersionInfo.FileVersion), signed by Microsoft"

# 6. The installer.
$dist = Join-Path $work 'dist'
if (Test-Path $dist) { Remove-Item -Recurse -Force $dist }
$env:CSC_IDENTITY_AUTO_DISCOVERY = 'false'
Push-Location $PSScriptRoot
try {
  for ($attempt = 1; $attempt -le 3; $attempt++) {
    npx electron-builder --win nsis --x64 --prepackaged $app --publish never `
      "-c.directories.buildResources=$buildRes" "-c.directories.output=$dist" "-c.extraMetadata.version=$version"
    if ($LASTEXITCODE -eq 0) { break }
    if ($attempt -eq 3) { Fail 'electron-builder failed three times' }
    Write-Warning "package-windows: electron-builder failed (attempt $attempt of 3)"
    Start-Sleep -Seconds 30
  }
} finally { Pop-Location }
$installer = Join-Path $dist "Kairo-Setup-$version.exe"
if (-not (Test-Path $installer)) { Fail "electron-builder made no $installer" }

# 7. Verify.
$verifyArgs = @{ AppDir = $app; Installer = $installer; Version = $version }
if ($signed) { $verifyArgs.Signed = $true }
& (Join-Path $PSScriptRoot 'verify-package.ps1') @verifyArgs
if (-not $?) { Fail 'verify-package.ps1 failed' }

foreach ($file in Get-ChildItem $dist -File | Where-Object { $_.Extension -in '.exe', '.yml', '.blockmap' }) {
  Note "$($file.Name) $($file.Length) bytes sha256 $((Get-FileHash -Algorithm SHA256 $file.FullName).Hash.ToLowerInvariant())"
}
