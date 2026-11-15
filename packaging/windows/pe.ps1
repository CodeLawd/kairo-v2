# Dot-source for Get-PeInfo (prompt 102): the machine and the imported DLLs of
# a Windows PE file, read from its headers, so the package checks need no
# Visual Studio (dumpbin) on the machine being checked.
#   . (Join-Path $PSScriptRoot 'pe.ps1')
#   Get-PeInfo C:\path\Kairo.exe  ->  @{ Machine = 'x64'; Imports = @('KERNEL32.dll', ...) }
# Imports include delay-loaded DLLs, as `dumpbin /dependents` lists them.

function Get-PeInfo([string]$Path) {
  $bytes = [IO.File]::ReadAllBytes($Path)
  function U16([int]$At) { [BitConverter]::ToUInt16($bytes, $At) }
  function U32([int]$At) { [BitConverter]::ToUInt32($bytes, $At) }
  if ($bytes.Length -lt 64 -or (U16 0) -ne 0x5A4D) { throw "$Path is not a PE file (no MZ header)" }
  $pe = [int](U32 0x3C)
  if ($pe + 24 -gt $bytes.Length -or (U32 $pe) -ne 0x00004550) { throw "$Path is not a PE file (no PE signature)" }
  $machine = switch (U16 ($pe + 4)) {
    0x8664 { 'x64' } 0x014C { 'x86' } 0xAA64 { 'arm64' } default { 'other' }
  }
  $sections = U16 ($pe + 6)
  $optionalSize = U16 ($pe + 20)
  $optional = $pe + 24
  $magic = U16 $optional
  $dataDirs = switch ($magic) { 0x20B { $optional + 112 } 0x10B { $optional + 96 } default { throw "$Path has an unknown optional header ($magic)" } }
  $sectionTable = $optional + $optionalSize
  $map = for ($i = 0; $i -lt $sections; $i++) {
    $s = $sectionTable + 40 * $i
    [pscustomobject]@{ Va = U32 ($s + 12); Size = [Math]::Max((U32 ($s + 8)), (U32 ($s + 16))); Raw = U32 ($s + 20) }
  }
  function Offset([uint32]$Rva) {
    foreach ($s in $map) { if ($Rva -ge $s.Va -and $Rva -lt $s.Va + $s.Size) { return [int]($Rva - $s.Va + $s.Raw) } }
    return -1
  }
  function Name([uint32]$Rva) {
    $at = Offset $Rva
    if ($at -lt 0) { return $null }
    $end = [Array]::IndexOf($bytes, [byte]0, $at)
    [Text.Encoding]::ASCII.GetString($bytes, $at, $end - $at)
  }
  $imports = New-Object System.Collections.Generic.List[string]
  # Data directory 1: import table, 20-byte descriptors, name RVA at +12.
  # Data directory 13: delay-load table, 32-byte descriptors, name RVA at +4.
  foreach ($dir in @(@{ Index = 1; Stride = 20; NameAt = 12 }, @{ Index = 13; Stride = 32; NameAt = 4 })) {
    $rva = U32 ($dataDirs + 8 * $dir.Index)
    if ($rva -eq 0) { continue }
    $at = Offset $rva
    if ($at -lt 0) { continue }
    while ($at + $dir.Stride -le $bytes.Length) {
      $nameRva = U32 ($at + $dir.NameAt)
      if ($nameRva -eq 0) { break }
      $name = Name $nameRva
      if ($name) { $imports.Add($name) }
      $at += $dir.Stride
    }
  }
  [pscustomobject]@{ Machine = $machine; Imports = @($imports | Sort-Object -Unique) }
}
