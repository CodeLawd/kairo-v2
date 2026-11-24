<#
.SYNOPSIS
  Install, upgrade and uninstall checks for the Kairo Windows installer
  (prompt 102). For CI runners and the clean Windows 10 and 11 VMs.

.DESCRIPTION
  install-test.ps1 -Installer <Kairo-Setup-x.y.z.exe> -Version x.y.z
                   [-ElectronInstaller <Kairo-Setup-1.0.33.exe>] [-SkipLaunch]

  Silent, per-user, as the in-app updater runs it. Changes this user's
  install of Kairo, so run it only on a test machine or VM.

  1. With -ElectronInstaller: install the Electron release first, leave a
     marker in its user data, then install over it. The native build must take
     the same folder and registry entries, remove the Electron files (app.asar),
     leave one uninstall entry at the new version, and keep the user data.
  2. Without it: a clean install.
  Then: install folder and registry (electron-builder's GUID for
  com.kairo.app), Start menu and desktop shortcuts with AppUserModelID
  com.kairo.app, the .kairo association, verify-package.ps1 on the installed
  folder, and a launch with no FFmpeg on PATH: Kairo.exe must keep running and
  start kairo-engine.exe from the install folder.
  3. Uninstall: the folder, shortcuts and registry entries go; the user data
  (%APPDATA%\Kairo) stays.
#>
param(
  [Parameter(Mandatory = $true)][string]$Installer,
  [Parameter(Mandatory = $true)][string]$Version,
  [string]$ElectronInstaller,
  [switch]$SkipLaunch
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Fail([string]$Message) { Write-Host "install-test FAIL: $Message"; exit 1 }
function Ok([string]$Message) { Write-Host "install-test ok: $Message" }

# electron-builder's UUID v5 of com.kairo.app in its namespace
# (50e065bc-3134-11e6-9bab-38c9862bdaf3), as the Electron installer used.
$guid = '440e67a7-8e87-5084-89a4-da16aeff9350'
$installKey = "HKCU:\Software\$guid"
$uninstallKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\$guid"
$userData = Join-Path $env:APPDATA 'Kairo'
$startMenuLink = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Kairo.lnk'
$desktopLink = Join-Path ([Environment]::GetFolderPath('Desktop')) 'Kairo.lnk'

function Install([string]$Setup) {
  $process = Start-Process -FilePath $Setup -ArgumentList '/S' -Wait -PassThru
  if ($process.ExitCode -ne 0) { Fail "$(Split-Path -Leaf $Setup) exited with $($process.ExitCode)" }
}
function InstallLocation {
  $key = Get-Item -Path $installKey -ErrorAction SilentlyContinue
  if ($key) { $key.GetValue('InstallLocation') }
}
function LinkAppId([string]$Link) {
  $shell = New-Object -ComObject Shell.Application
  $item = $shell.NameSpace((Split-Path $Link)).ParseName((Split-Path -Leaf $Link))
  $item.ExtendedProperty('System.AppUserModel.ID')
}

foreach ($exe in 'ffmpeg.exe', 'avcodec-63.dll') {
  $found = Get-Command $exe -ErrorAction SilentlyContinue
  if ($found) { Fail "$exe is on PATH ($($found.Source)); the launch check needs a machine without FFmpeg" }
}
if (InstallLocation) { Fail "Kairo is already installed at $(InstallLocation); use a clean machine" }

# 1. Electron first, when asked.
$marker = $null
if ($ElectronInstaller) {
  Install $ElectronInstaller
  $electronDir = InstallLocation
  if (-not $electronDir -or -not (Test-Path (Join-Path $electronDir 'resources\app.asar'))) { Fail 'the Electron release did not install' }
  New-Item -ItemType Directory -Force -Path $userData | Out-Null
  $marker = Join-Path $userData 'kairo-install-test-marker.txt'
  Set-Content -Path $marker -Value "left by install-test.ps1 before the upgrade"
  Ok "Electron $((Get-ItemProperty $uninstallKey).DisplayVersion) installed at $electronDir"
}

# 2. The native installer.
Install $Installer
$dir = InstallLocation
if (-not $dir) { Fail "no InstallLocation under $installKey" }
$expected = Join-Path $env:LOCALAPPDATA 'Programs\Kairo'
if ($dir.TrimEnd('\') -ne $expected) { Fail "installed at $dir, expected $expected (per-user default)" }
if ($ElectronInstaller -and $dir -ne $electronDir) { Fail "the upgrade moved the app from $electronDir to $dir" }
if (Test-Path (Join-Path $dir 'resources\app.asar')) { Fail 'Electron files remain after the upgrade (resources\app.asar)' }
if ((Get-Item (Join-Path $dir 'Kairo.exe')).VersionInfo.ProductVersion -ne $Version) { Fail 'Kairo.exe is not the new version' }
$uninstall = Get-ItemProperty $uninstallKey
if ($uninstall.DisplayVersion -ne $Version) { Fail "uninstall entry says $($uninstall.DisplayVersion), expected $Version" }
$entries = Get-ChildItem 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall' |
  Where-Object { (Get-ItemProperty $_.PSPath -Name DisplayName -ErrorAction SilentlyContinue).DisplayName -like 'Kairo*' }
if (@($entries).Count -ne 1) { Fail "$(@($entries).Count) Kairo uninstall entries, expected 1" }
if ($marker -and -not (Test-Path $marker)) { Fail 'the upgrade removed the user data' }
Ok "installed $Version at $dir$(if ($ElectronInstaller) { ' over Electron, user data kept' })"

foreach ($link in $startMenuLink, $desktopLink) {
  if (-not (Test-Path $link)) { Fail "no shortcut at $link" }
  $appId = LinkAppId $link
  if ($appId -ne 'com.kairo.app') { Fail "$link has AppUserModelID '$appId', expected com.kairo.app" }
}
Ok 'Start menu and desktop shortcuts with AppUserModelID com.kairo.app'

$progId = (Get-ItemProperty 'HKCU:\Software\Classes\.kairo' -ErrorAction SilentlyContinue).'(default)'
if (-not $progId) { Fail '.kairo is not associated' }
$command = (Get-ItemProperty "HKCU:\Software\Classes\$progId\shell\open\command" -ErrorAction SilentlyContinue).'(default)'
if ($command -notlike "*$dir\Kairo.exe*") { Fail ".kairo opens with '$command'" }
Ok ".kairo opens with Kairo ($progId)"

& (Join-Path $PSScriptRoot 'verify-package.ps1') -AppDir $dir -Version $Version
if (-not $?) { Fail 'verify-package.ps1 failed on the installed folder' }

if (-not $SkipLaunch) {
  $kairo = Start-Process -FilePath (Join-Path $dir 'Kairo.exe') -PassThru
  $engine = $null
  $deadline = (Get-Date).AddSeconds(60)
  while (-not $kairo.HasExited -and -not $engine -and (Get-Date) -lt $deadline) {
    $engine = Get-CimInstance Win32_Process -Filter "Name = 'kairo-engine.exe'" |
      Where-Object { $_.ExecutablePath -eq (Join-Path $dir 'kairo-engine.exe') }
    if (-not $engine) { Start-Sleep -Seconds 1 }
  }
  if (-not $engine -or $kairo.HasExited) {
    Get-ChildItem (Join-Path $userData 'logs') -Filter 'kairo.log*' -ErrorAction SilentlyContinue |
      Sort-Object LastWriteTime -Descending | Select-Object -First 1 |
      ForEach-Object { Get-Content $_.FullName -Tail 80 }
    if ($kairo.HasExited) { Fail "Kairo.exe exited with $($kairo.ExitCode) before engine validation completed" }
    Fail 'kairo-engine.exe did not start from the install folder within 60 s'
  }
  Ok "Kairo.exe running with kairo-engine.exe (pid $($engine.ProcessId)), no FFmpeg on PATH"
  Stop-Process -Id $kairo.Id -Force
  Start-Sleep -Seconds 12
  Get-Process kairo-engine -ErrorAction SilentlyContinue | Stop-Process -Force
}

# 3. Uninstall.
$uninstaller = Join-Path $dir 'Uninstall Kairo.exe'
if (-not (Test-Path $uninstaller)) { Fail 'no uninstaller' }
New-Item -ItemType Directory -Force -Path $userData | Out-Null
Start-Process -FilePath $uninstaller -ArgumentList '/S' -Wait | Out-Null
# The uninstaller copies itself away and returns; wait for it to finish.
for ($i = 0; $i -lt 60 -and (Test-Path (Join-Path $dir 'Kairo.exe')); $i++) { Start-Sleep -Seconds 1 }
if (Test-Path (Join-Path $dir 'Kairo.exe')) { Fail "Kairo.exe is still in $dir after uninstalling" }
if (Test-Path $uninstallKey) { Fail 'the uninstall entry remains' }
foreach ($link in $startMenuLink, $desktopLink) { if (Test-Path $link) { Fail "$link remains" } }
if (-not (Test-Path $userData)) { Fail 'uninstalling removed the user data' }
if ($marker -and -not (Test-Path $marker)) { Fail 'uninstalling removed the user data marker' }
Ok 'uninstalled: app, shortcuts and registry entries gone; user data kept'

Write-Host ''
Write-Host 'install-test PASS'
