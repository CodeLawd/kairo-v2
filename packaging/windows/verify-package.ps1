<#
.SYNOPSIS
  Package checks for Kairo on Windows (prompt 102), ported from the Electron
  scripts/verify-package.mjs and extended for the native build.

.DESCRIPTION
  verify-package.ps1 -AppDir <staged or installed folder> [-Installer <Kairo-Setup-x.y.z.exe>]
                     [-Version x.y.z] [-Signed]

  Runs on any Windows with PowerShell 5.1 or later (pe.ps1 reads the PE
  headers; no Visual Studio needed). Checks:
  - layout: Kairo.exe, kairo-engine.exe, every FFmpeg DLL, pdfium.dll and the
    NDI runtime beside them, resources\bible.db (a SQLite file) and icon.png,
    licence files; no repository or build junk;
  - every .exe and .dll is x64; every DLL it imports is either beside it or a
    Windows system DLL; no VC++ runtime from System32 and no FFmpeg from
    anywhere but the install folder (the imports `dumpbin /dependents` lists,
    amendment from 043);
  - FFmpeg is the LGPL build with the expected majors;
  - version information and icon on Kairo.exe;
  - with -Signed: valid Authenticode signatures on the app's files and the
    installer; the NDI runtime keeps Vizrt's signature either way;
  - the installer and electron-builder's latest.yml and blockmap; size.
  Exits non-zero on the first failure.
#>
param(
  [Parameter(Mandatory = $true)][string]$AppDir,
  [string]$Installer,
  [string]$Version,
  [switch]$Signed
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Fail([string]$Message) { Write-Host "verify-package FAIL: $Message"; exit 1 }
function Ok([string]$Message) { Write-Host "verify-package ok: $Message" }

. (Join-Path $PSScriptRoot 'pe.ps1')
if (-not (Test-Path $AppDir)) { Fail "no folder at $AppDir" }
$app = (Resolve-Path $AppDir).Path

$ffmpegDlls = 'avutil-61.dll', 'swresample-7.dll', 'avcodec-63.dll', 'avformat-63.dll', 'swscale-10.dll'
$ownFiles = @('Kairo.exe', 'kairo-engine.exe') + $ffmpegDlls + 'pdfium.dll'
$required = $ownFiles + 'Processing.NDI.Lib.x64.dll', 'resources\bible.db', 'resources\icon.png'

# 1. Layout.
foreach ($file in $required) {
  if (-not (Test-Path (Join-Path $app $file) -PathType Leaf)) { Fail "missing $file" }
}
$licences = 'licenses\ffmpeg\COPYING.LGPLv2.1', 'licenses\ffmpeg\FFMPEG-SOURCE.txt', 'licenses\libvpx\LICENSE',
  'licenses\libvpx\PATENTS', 'licenses\pdfium\LICENSE', 'licenses\ndi\libndi_licenses.txt', 'licenses\ndi\NOTICE.txt',
  'licenses\fonts\source-sans-3-OFL.txt', 'licenses\icons\hugeicons-LICENSE.md', 'licenses\icons\phosphor-LICENSE'
foreach ($file in $licences) {
  $path = Join-Path $app $file
  if (-not (Test-Path $path -PathType Leaf) -or (Get-Item $path).Length -eq 0) { Fail "missing or empty $file" }
}
$notice = Get-Content -Raw (Join-Path $app 'licenses\ffmpeg\FFMPEG-SOURCE.txt')
if ($notice -notmatch 'Written offer' -or $notice -notmatch 'https://ffmpeg.org/releases/') { Fail 'the FFmpeg source notice lacks the written offer or the source link' }
Ok 'layout: both processes, FFmpeg, PDFium and NDI DLLs, resources, licences'

$junk = Get-ChildItem $app -Recurse -Force | Where-Object {
  $_.Name -match '^\.env(\..*)?$' -or $_.Name -match '\.(pdb|rlib|d|rs|db-shm|db-wal)$' -or $_.Name -in 'Cargo.toml', 'Cargo.lock'
}
if ($junk) { Fail "junk in the package: $($junk.FullName -join ', ')" }
Ok 'no repository or build junk'

# 2. bible.db.
$bible = Join-Path $app 'resources\bible.db'
$size = (Get-Item $bible).Length
if ($size -lt 1024) { Fail "bible.db suspiciously small ($size bytes)" }
$header = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($bible), 0, 15)
if ($header -ne 'SQLite format 3') { Fail 'bible.db is not a SQLite database' }
Ok ("bible.db exists ({0:N1} MB)" -f ($size / 1MB))

# 3. Every PE: x64, and every import resolvable on a clean Windows.
$system32 = Join-Path $env:SystemRoot 'System32'
$vcRuntime = '^(vcruntime140(_1)?|msvcp140(_\w+)?|concrt140|vccorlib140)\.dll$'
$pes = Get-ChildItem $app -Recurse -File -Include *.exe, *.dll
foreach ($pe in $pes) {
  $rel = $pe.FullName.Substring($app.Length + 1)
  # electron-builder's elevate.exe and the NSIS uninstaller are 32-bit stubs.
  $installerStub = $rel -eq 'resources\elevate.exe' -or $rel -like 'Uninstall *.exe'
  $peInfo = Get-PeInfo $pe.FullName
  if (-not $installerStub -and $peInfo.Machine -ne 'x64') { Fail "$rel is $($peInfo.Machine), not x64" }
  $deps = $peInfo.Imports
  foreach ($dep in $deps) {
    $lower = $dep.ToLowerInvariant()
    $beside = Test-Path (Join-Path $pe.DirectoryName $dep)
    if ($lower -match '^(avutil|swresample|avcodec|avformat|swscale)-\d+\.dll$') {
      if (-not $beside) { Fail "$rel imports $dep, which is not in the install folder" }
      if ($ffmpegDlls -notcontains $lower) { Fail "$rel imports $dep; expected majors are $($ffmpegDlls -join ', ')" }
      continue
    }
    if ($beside) { continue }
    if ($lower -match $vcRuntime) { Fail "$rel imports $dep, which is not beside it (a clean Windows has no VC++ runtime)" }
    if ($lower -like 'api-ms-win-*' -or $lower -like 'ext-ms-win-*') { continue }
    if (-not (Test-Path (Join-Path $system32 $dep))) { Fail "$rel imports $dep, found neither beside it nor in System32" }
  }
}
Ok "$($pes.Count) PE files: all x64; FFmpeg resolves from the install folder; other imports are Windows' own"

# 4. FFmpeg is the LGPL build.
$avcodec = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes((Join-Path $app 'avcodec-63.dll')))
if ($avcodec -notmatch '--disable-gpl') { Fail 'FFmpeg configuration lacks --disable-gpl' }
foreach ($bad in '--enable-gpl', '--enable-nonfree', '--enable-version3', '--enable-libx264', '--enable-libx265') {
  if ($avcodec.Contains($bad)) { Fail "FFmpeg was configured with $bad" }
}
$avutil = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes((Join-Path $app 'avutil-61.dll')))
if (-not $avutil.Contains('LGPL version 2.1 or later')) { Fail 'avutil does not report the LGPL 2.1+ licence' }
Ok 'FFmpeg is the LGPL shared build (avutil 61, avcodec 63, avformat 63, swscale 10)'

# 5. Identity.
$info = (Get-Item (Join-Path $app 'Kairo.exe')).VersionInfo
if ($info.ProductName -ne 'Kairo') { Fail "Kairo.exe ProductName is '$($info.ProductName)'" }
if ($Version -and $info.ProductVersion -ne $Version) { Fail "Kairo.exe ProductVersion is '$($info.ProductVersion)', expected $Version" }
Add-Type -AssemblyName System.Drawing
$icon = [Drawing.Icon]::ExtractAssociatedIcon((Join-Path $app 'Kairo.exe'))
if (-not $icon) { Fail 'Kairo.exe has no icon' }
Ok "Kairo.exe $($info.ProductVersion) with version information and an icon"

# 6. Signatures.
$ndi = Get-AuthenticodeSignature (Join-Path $app 'Processing.NDI.Lib.x64.dll')
if ($ndi.Status -ne 'Valid') { Fail "the NDI runtime's signature is $($ndi.Status)" }
if ($Signed) {
  $publisher = $null
  foreach ($file in $ownFiles) {
    $sig = Get-AuthenticodeSignature (Join-Path $app $file)
    if ($sig.Status -ne 'Valid') { Fail "$file signature is $($sig.Status)" }
    if (-not $sig.TimeStamperCertificate) { Fail "$file has no timestamp" }
    if ($publisher -and $sig.SignerCertificate.Subject -ne $publisher) { Fail "$file is signed by another publisher" }
    $publisher = $sig.SignerCertificate.Subject
  }
  Ok "app files signed and timestamped by $publisher"
} else {
  Ok 'unsigned build (KAIRO_UNSIGNED); the NDI runtime keeps its Vizrt signature'
}

# 7. Installer and update metadata.
if ($Installer) {
  if (-not (Test-Path $Installer)) { Fail "no installer at $Installer" }
  $setup = Get-Item $Installer
  if ($Signed) {
    $sig = Get-AuthenticodeSignature $setup.FullName
    if ($sig.Status -ne 'Valid') { Fail "installer signature is $($sig.Status)" }
  }
  $latest = Join-Path $setup.DirectoryName 'latest.yml'
  if (-not (Test-Path $latest)) { Fail 'electron-builder wrote no latest.yml' }
  $yml = Get-Content -Raw $latest
  if ($Version -and $yml -notmatch "(?m)^version: $([regex]::Escape($Version))\s*$") { Fail "latest.yml does not name version $Version" }
  if ($yml -notmatch [regex]::Escape($setup.Name)) { Fail "latest.yml does not name $($setup.Name)" }
  if (-not (Test-Path "$($setup.FullName).blockmap")) { Fail 'no blockmap for differential updates' }
  Write-Host ("verify-package info: installer {0} bytes" -f $setup.Length)
}
$appBytes = (Get-ChildItem $app -Recurse -File | Measure-Object -Sum Length).Sum
Write-Host ("verify-package info: installed size {0:N0} KiB" -f ($appBytes / 1KB))
if ($appBytes -gt 600MB) { Fail 'the app folder is over the 600 MiB ceiling' }

Write-Host ''
Write-Host "verify-package PASS: $app"
