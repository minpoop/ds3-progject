# Ashen Marine - READ-ONLY scan of your Space Marine 2 and Dark Souls III installs.
# It never changes, copies or uploads anything. It reads file NAMES, SIZES and the first 32 bytes of a
# few files so the converter can be written for the real formats. Your Windows user name is masked.
# The report goes to your clipboard and to %TEMP%\ashen-scan.txt - paste it back to Claude.
#
# Optional overrides (for non-Steam or unusual installs, and for testing):
#   $env:ASHEN_SM2 = 'D:\Games\Space Marine 2' ; $env:ASHEN_DS3 = 'D:\Games\DARK SOULS III'
& {
  $ErrorActionPreference = 'Continue'
  Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
  $out = New-Object System.Collections.Generic.List[string]
  function Say([string]$s) { $out.Add($s) }
  $me = if ($env:USERNAME) { [regex]::Escape($env:USERNAME) } else { $null }
  function Mask($s) {
    if ($null -eq $s) { return '' }
    $t = [string]$s
    if ($me) { $t = $t -replace $me, '<user>' }
    return ($t -replace '\b\d{15,20}\b', '<steamid>')
  }
  function Mb($n) { return [math]::Round(([double]$n) / 1MB, 1) }
  function Hex([byte[]]$b, [int]$n) {
    if ($n -le 0) { return '(empty)' }
    $part = $b[0..($n - 1)]
    $hex = ($part | ForEach-Object { $_.ToString('x2') }) -join ' '
    $asc = -join ($part | ForEach-Object { if ($_ -ge 32 -and $_ -lt 127) { [char]$_ } else { '.' } })
    return "$hex | $asc"
  }

  Say '== ENV =='
  Say ('OS: ' + [System.Environment]::OSVersion.VersionString)
  Say ('PowerShell: ' + $PSVersionTable.PSVersion)

  # ---------- Steam libraries ----------
  $steam = $null
  foreach ($k in 'HKCU:\Software\Valve\Steam', 'HKLM:\SOFTWARE\WOW6432Node\Valve\Steam', 'HKLM:\SOFTWARE\Valve\Steam') {
    if ($steam) { break }
    try {
      $p = Get-ItemProperty -Path $k -ErrorAction Stop
      foreach ($n in 'SteamPath', 'InstallPath') { $v = $p.$n; if ($v -and (Test-Path $v)) { $steam = ($v -replace '/', '\'); break } }
    } catch {}
  }
  $libs = New-Object System.Collections.Generic.List[string]
  if ($steam) {
    $libs.Add($steam)
    $vdf = Join-Path $steam 'steamapps\libraryfolders.vdf'
    if (Test-Path $vdf) {
      foreach ($line in (Get-Content -Path $vdf -ErrorAction SilentlyContinue)) {
        if ($line -match '"path"\s+"([^"]+)"') { $libs.Add(($Matches[1] -replace '\\\\', '\')) }
      }
    }
  }
  Say ''
  Say '== STEAM =='
  Say ('Steam: ' + (Mask $steam))
  foreach ($l in ($libs | Select-Object -Unique)) { Say ('Library: ' + (Mask $l)) }

  function Find-Game([string]$appId) {
    foreach ($lib in ($libs | Select-Object -Unique)) {
      $acf = Join-Path $lib "steamapps\appmanifest_$appId.acf"
      if (Test-Path $acf) {
        $txt = Get-Content -Path $acf -Raw -ErrorAction SilentlyContinue
        $dir = $null; if ($txt -match '"installdir"\s+"([^"]+)"') { $dir = $Matches[1] }
        $bid = '?'; if ($txt -match '"buildid"\s+"([^"]+)"') { $bid = $Matches[1] }
        if ($dir) { return @{ Path = (Join-Path $lib "steamapps\common\$dir"); Build = $bid } }
      }
    }
    return $null
  }

  # ---------- Space Marine 2 ----------
  Say ''
  Say '== SPACE MARINE 2 =='
  $sm2 = $null
  if ($env:ASHEN_SM2) { $sm2 = @{ Path = $env:ASHEN_SM2; Build = 'override' } } else { $sm2 = Find-Game '2183900' }
  if (-not $sm2 -or -not (Test-Path -LiteralPath $sm2.Path)) {
    Say 'NOT FOUND (looked for Steam app 2183900). If you have it elsewhere, set $env:ASHEN_SM2 and run again.'
  } else {
    $root = $sm2.Path
    Say ('Path: ' + (Mask $root))
    Say ('Steam build id: ' + $sm2.Build)
    Write-Host 'Scanning Space Marine 2 (read-only; this can take a minute)...'
    Say 'Top level:'
    foreach ($i in (Get-ChildItem -LiteralPath $root -Force -ErrorAction SilentlyContinue | Select-Object -First 40)) {
      $tag = if ($i.PSIsContainer) { '\' } else { ' (' + (Mb $i.Length) + ' MB)' }
      Say ('  ' + $i.Name + $tag)
    }
    Say 'Folders (depth 4):'
    foreach ($d in (Get-ChildItem -LiteralPath $root -Directory -Recurse -Depth 3 -ErrorAction SilentlyContinue | Select-Object -First 70)) {
      Say ('  ' + (Mask $d.FullName.Substring($root.Length)))
    }
    Say 'Largest files:'
    $all = @(Get-ChildItem -LiteralPath $root -File -Recurse -Force -ErrorAction SilentlyContinue)
    Say ('Total files: ' + $all.Count)
    foreach ($f in ($all | Sort-Object Length -Descending | Select-Object -First 25)) {
      Say ('  ' + (Mb $f.Length) + ' MB  ' + (Mask $f.FullName.Substring($root.Length)))
    }
    $paks = @($all | Where-Object { $_.Extension -ieq '.pak' })
    Say ''
    Say ('== SM2 PAKS (' + $paks.Count + ') ==')
    $extCount = @{}; $extBytes = @{}; $heads = @{}; $hits = New-Object System.Collections.Generic.List[object]
    $rx = '(?i)chain|bolt|pistol|wpn|weapon|melee|titus'
    foreach ($pak in $paks) {
      $rel = Mask $pak.FullName.Substring($root.Length)
      try {
        $zip = [System.IO.Compression.ZipFile]::OpenRead($pak.FullName)
        try {
          $stored = 0; $defl = 0; $n = 0
          foreach ($e in $zip.Entries) {
            if ([string]::IsNullOrEmpty($e.Name)) { continue }
            $n++
            $ext = [System.IO.Path]::GetExtension($e.Name).ToLowerInvariant(); if (-not $ext) { $ext = '(none)' }
            $extCount[$ext] = 1 + [int]$extCount[$ext]
            $extBytes[$ext] = [long]$extBytes[$ext] + $e.Length
            if ($e.CompressedLength -eq $e.Length) { $stored++ } else { $defl++ }
            $isHit = ($e.FullName -match $rx)
            if ($isHit) { $hits.Add([pscustomobject]@{ Pak = $pak.Name; Name = $e.FullName; Ext = $ext; Size = $e.Length }) }
            if ((-not $heads.ContainsKey($ext)) -or ($isHit -and -not $heads.ContainsKey($ext + '#hit'))) {
              $key = if ($isHit) { $ext + '#hit' } else { $ext }
              try {
                $s = $e.Open()
                try { $buf = New-Object byte[] 32; $got = $s.Read($buf, 0, 32) } finally { $s.Dispose() }
                $heads[$key] = ($e.FullName + '  [' + $e.Length + ' bytes]  ' + (Hex $buf $got))
              } catch { $heads[$key] = ($e.FullName + '  (cannot open: ' + $_.Exception.Message + ')') }
            }
          }
          Say ('  ' + $rel + '  ' + (Mb $pak.Length) + ' MB  entries=' + $n + ' stored=' + $stored + ' compressed=' + $defl)
        } finally { $zip.Dispose() }
      } catch {
        Say ('  ' + $rel + '  ' + (Mb $pak.Length) + ' MB  NOT readable as a zip: ' + $_.Exception.Message)
        try {
          $fs = [System.IO.File]::Open($pak.FullName, 'Open', 'Read', 'ReadWrite')
          try {
            $b = New-Object byte[] 32; $g = $fs.Read($b, 0, 32); Say ('     first bytes: ' + (Hex $b $g))
            if ($fs.Length -gt 64) { [void]$fs.Seek(-64, 'End'); $b2 = New-Object byte[] 64; $g2 = $fs.Read($b2, 0, 64); Say ('     last bytes:  ' + (Hex $b2 $g2)) }
          } finally { $fs.Dispose() }
        } catch {}
      }
    }
    Say ''
    Say '== FILE TYPES INSIDE THE PAKS (top 45 by count) =='
    foreach ($k in ($extCount.Keys | Sort-Object { -$extCount[$_] } | Select-Object -First 45)) {
      Say ('  ' + $k.PadRight(14) + ([string]$extCount[$k]).PadLeft(8) + ' files  ' + (Mb $extBytes[$k]) + ' MB')
    }
    Say ''
    Say ('== NAMES MATCHING chain|bolt|pistol|wpn|weapon|melee|titus (' + $hits.Count + ' total, max 8 per type) ==')
    foreach ($g in ($hits | Group-Object Ext | Sort-Object Count -Descending | Select-Object -First 16)) {
      Say ('  [' + $g.Name + '] ' + $g.Count + ' matches')
      foreach ($h in ($g.Group | Sort-Object Size -Descending | Select-Object -First 8)) { Say ('     ' + $h.Name + '  (' + $h.Size + ' bytes, ' + $h.Pak + ')') }
    }
    Say ''
    Say '== FIRST 32 BYTES OF ONE SAMPLE PER FILE TYPE =='
    foreach ($k in ($heads.Keys | Sort-Object | Select-Object -First 60)) { Say ('  ' + $k + ' :: ' + $heads[$k]) }
  }

  # ---------- Dark Souls III ----------
  Say ''
  Say '== DARK SOULS III =='
  $ds3 = $null
  if ($env:ASHEN_DS3) { $ds3 = @{ Path = $env:ASHEN_DS3; Build = 'override' } } else { $ds3 = Find-Game '374320' }
  if (-not $ds3 -or -not (Test-Path -LiteralPath $ds3.Path)) {
    Say 'NOT FOUND (looked for Steam app 374320).'
  } else {
    Say ('Path: ' + (Mask $ds3.Path))
    Say ('Steam build id: ' + $ds3.Build)
    $exe = Get-ChildItem -LiteralPath $ds3.Path -Recurse -Filter 'DarkSoulsIII.exe' -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($exe) { Say ('Exe: ' + (Mask $exe.FullName.Substring($ds3.Path.Length)) + '  version ' + $exe.VersionInfo.FileVersion + '  ' + (Mb $exe.Length) + ' MB') } else { Say 'DarkSoulsIII.exe not found under that folder' }
    foreach ($i in (Get-ChildItem -LiteralPath $ds3.Path -Force -Recurse -Depth 1 -ErrorAction SilentlyContinue | Select-Object -First 50)) {
      Say ('  ' + (Mask $i.FullName.Substring($ds3.Path.Length)) + $(if ($i.PSIsContainer) { '\' } else { '  (' + (Mb $i.Length) + ' MB)' }))
    }
  }
  $appdata = if ($env:ASHEN_APPDATA) { $env:ASHEN_APPDATA } elseif ($env:APPDATA) { $env:APPDATA } else { [Environment]::GetFolderPath('ApplicationData') }
  $savedir = Join-Path $appdata 'DarkSoulsIII'
  Say ('Save folder: ' + (Mask $savedir) + $(if (Test-Path -LiteralPath $savedir) { '' } else { '  (missing - never played, or saved elsewhere)' }))
  foreach ($f in (Get-ChildItem -LiteralPath $savedir -Recurse -File -ErrorAction SilentlyContinue | Select-Object -First 20)) {
    Say ('  ' + (Mask $f.FullName.Substring($savedir.Length)) + '  ' + (Mb $f.Length) + ' MB  modified ' + $f.LastWriteTime.ToString('yyyy-MM-dd'))
  }

  # ---------- Melty (folder names only) ----------
  Say ''
  Say '== MELTY (folder names only) =='
  foreach ($base in $env:LOCALAPPDATA, $env:APPDATA) {
    if (-not $base) { continue }
    foreach ($d in (Get-ChildItem -LiteralPath $base -Directory -Filter '*melty*' -ErrorAction SilentlyContinue)) {
      Say ('  ' + (Mask $d.FullName))
      foreach ($s in (Get-ChildItem -LiteralPath $d.FullName -Directory -ErrorAction SilentlyContinue | Select-Object -First 15)) { Say ('     ' + $s.Name + '\') }
    }
  }

  # ---------- finish ----------
  $lines = @($out | ForEach-Object { Mask $_ })
  if ($lines.Count -gt 480) { $lines = $lines[0..470] + @('... (cut at 470 lines; the full report is in the file below)') }
  $report = $lines -join "`r`n"
  $file = Join-Path ([System.IO.Path]::GetTempPath()) 'ashen-scan.txt'
  try { Set-Content -Path $file -Value $report -Encoding UTF8 } catch {}
  $clip = 'could not reach the clipboard - open the file and copy it'
  try { Set-Clipboard -Value $report; $clip = 'copied to your clipboard' } catch {}
  Write-Host ''
  Write-Host ('Done: ' + $lines.Count + ' lines, ' + $clip + '. Also saved to ' + (Mask $file))
  Write-Host 'Paste it back into the chat (Ctrl+V).'
}
