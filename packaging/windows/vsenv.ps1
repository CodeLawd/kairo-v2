# Dot-source to get MSVC, dumpbin and the Windows SDK on PATH (prompt 102):
#   . (Join-Path $PSScriptRoot 'vsenv.ps1')
# Does nothing when dumpbin is already on PATH (a developer PowerShell).
# Otherwise finds Visual Studio (or its Build Tools) with the C++ x64 tools
# through vswhere and enters its x64 developer shell, as Microsoft documents.
if (-not (Get-Command dumpbin -ErrorAction SilentlyContinue)) {
  $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
  if (-not (Test-Path $vswhere)) {
    throw 'package-windows: Visual Studio or its Build Tools (C++ x64 tools) are not installed; vswhere.exe is missing'
  }
  $vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
  if (-not $vs) { throw 'package-windows: no Visual Studio installation has the C++ x64 tools' }
  Import-Module (Join-Path $vs 'Common7\Tools\Microsoft.VisualStudio.DevShell.dll')
  Enter-VsDevShell -VsInstallPath $vs -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64' | Out-Null
  if (-not (Get-Command dumpbin -ErrorAction SilentlyContinue)) { throw 'package-windows: dumpbin is still not on PATH' }
}
