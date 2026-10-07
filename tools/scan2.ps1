# Ashen Marine - READ-ONLY scan, part 2 (targeted). Reads Dark Souls III basics and the files of ONE chainsword and
# ONE bolt pistol inside your Space Marine 2 install. It changes, copies and uploads nothing: it prints file NAMES,
# SIZES, a few hundred bytes of a handful of files (hex) and the first lines of a few small TEXT files, so the
# converter can be written for the real formats. Your Windows user name and Steam ID are masked.
# The report goes to your clipboard and to %TEMP%\ashen-scan2.txt - paste it back to Claude.
#
# Optional overrides (non-Steam or unusual installs):  $env:ASHEN_SM2 = '...' ; $env:ASHEN_DS3 = '...'
& {
  $ErrorActionPreference = 'Continue'
  Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
  $out = New-Object System.Collections.Generic.List[string]
  function Say([string]$s) { if ($s.Length -gt 230) { $s = $s.Substring(0, 230) + '...' }; $out.Add($s) }
  $me = if ($env:USERNAME) { [regex]::Escape($env:USERNAME) } else { $null }
  function Mask($s) {
    if ($null -eq $s) { return '' }
    $t = [string]$s
    if ($me) { $t = $t -replace $me, '<user>' }
    return ($t -replace '\b\d{15,20}\b', '<steamid>')
  }
  function Mb($n) { return [math]::Round(([double]$n) / 1MB, 1) }
  function HexLines([byte[]]$b, [int]$n, [string]$indent) {
    for ($i = 0; $i -lt $n; $i += 16) {
      $k = [math]::Min(16, $n - $i)
      $part = $b[$i..($i + $k - 1)]
      $hex = (($part | ForEach-Object { $_.ToString('x2') }) -join ' ').PadRight(47)
      $asc = -join ($part | ForEach-Object { if ($_ -ge 32 -and $_ -lt 127) { [char]$_ } else { '.' } })
      Say ($indent + $i.ToString('x4') + ': ' + $hex + ' | ' + $asc)
    }
  }
  function Section([string]$title, [scriptblock]$body) {
    Say ''; Say ('== ' + $title + ' ==')
    try { & $body } catch { Say ('  (this section failed: ' + $_.Exception.Message + ')') }
  }
  function ReadBytes($entry, [int]$max) {
    $s = $entry.Open()
    try { $buf = New-Object byte[] $max; $got = 0; while ($got -lt $max) { $r = $s.Read($buf, $got, $max - $got); if ($r -le 0) { break }; $got += $r } } finally { $s.Dispose() }
    return ,@($buf, $got)
  }

  # ---------------------------------------------------------------- Steam
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
    if (Test-Path $vdf) { foreach ($line in (Get-Content -Path $vdf -ErrorAction SilentlyContinue)) { if ($line -match '"path"\s+"([^"]+)"') { $libs.Add(($Matches[1] -replace '\\\\', '\')) } } }
  }
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

  # ---------------------------------------------------------------- DARK SOULS III (first, so it can never be cut off)
  Section 'DARK SOULS III' {
    $ds3 = $null
    if ($env:ASHEN_DS3) { $ds3 = @{ Path = $env:ASHEN_DS3; Build = 'override' } } else { $ds3 = Find-Game '374320' }
    if (-not $ds3 -or -not (Test-Path -LiteralPath $ds3.Path)) { Say 'NOT FOUND (looked for Steam app 374320). Put its folder in $env:ASHEN_DS3 and run again.'; return }
    Say ('Path: ' + (Mask $ds3.Path))
    Say ('Steam build id: ' + $ds3.Build)
    $exe = Get-ChildItem -LiteralPath $ds3.Path -Recurse -Filter 'DarkSoulsIII.exe' -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($exe) { Say ('Exe: ' + (Mask $exe.FullName.Substring($ds3.Path.Length)) + '  version ' + $exe.VersionInfo.FileVersion + '  ' + (Mb $exe.Length) + ' MB') } else { Say 'DarkSoulsIII.exe not found under that folder' }
    foreach ($i in (Get-ChildItem -LiteralPath $ds3.Path -Force -Recurse -Depth 2 -ErrorAction SilentlyContinue | Select-Object -First 45)) {
      Say ('  ' + (Mask $i.FullName.Substring($ds3.Path.Length)) + $(if ($i.PSIsContainer) { '\' } else { '  (' + (Mb $i.Length) + ' MB)' }))
    }
    $appdata = if ($env:ASHEN_APPDATA) { $env:ASHEN_APPDATA } elseif ($env:APPDATA) { $env:APPDATA } else { [Environment]::GetFolderPath('ApplicationData') }
    $savedir = Join-Path $appdata 'DarkSoulsIII'
    Say ('Save folder: ' + (Mask $savedir) + $(if (Test-Path -LiteralPath $savedir) { '' } else { '  (MISSING - never played, or saved elsewhere)' }))
    foreach ($f in (Get-ChildItem -LiteralPath $savedir -Recurse -File -ErrorAction SilentlyContinue | Select-Object -First 20)) {
      Say ('  ' + (Mask $f.FullName.Substring($savedir.Length)) + '  ' + (Mb $f.Length) + ' MB  modified ' + $f.LastWriteTime.ToString('yyyy-MM-dd'))
    }
    Say 'Melty folders (names only):'
    foreach ($base in $env:LOCALAPPDATA, $env:APPDATA) {
      if (-not $base) { continue }
      foreach ($d in (Get-ChildItem -LiteralPath $base -Directory -Filter '*melty*' -ErrorAction SilentlyContinue)) {
        Say ('  ' + (Mask $d.FullName))
        foreach ($s in (Get-ChildItem -LiteralPath $d.FullName -Directory -ErrorAction SilentlyContinue | Select-Object -First 12)) { Say ('     ' + $s.Name + '\') }
      }
    }
  }

  # ---------------------------------------------------------------- SPACE MARINE 2: basics
  $sm2 = $null
  if ($env:ASHEN_SM2) { $sm2 = @{ Path = $env:ASHEN_SM2; Build = 'override' } } else { $sm2 = Find-Game '2183900' }
  if (-not $sm2 -or -not (Test-Path -LiteralPath $sm2.Path)) {
    Section 'SPACE MARINE 2' { Say 'NOT FOUND (looked for Steam app 2183900).' }
  } else {
    $root = $sm2.Path
    Section 'SPACE MARINE 2 (basics)' {
      Say ('Path: ' + (Mask $root) + '   build ' + $sm2.Build)
      $exe = Get-ChildItem -LiteralPath $root -Filter '*.exe' -File -ErrorAction SilentlyContinue | Where-Object { $_.Name -like 'Warhammer*' } | Select-Object -First 1
      if ($exe) { Say ('Exe: ' + $exe.Name + '  version ' + $exe.VersionInfo.FileVersion) }
      foreach ($sub in 'client_pc\root\mods', 'client_pc\root\local', 'client_pc\root\loadconfig', 'client_pc\root\prebuild') {
        $d = Join-Path $root $sub
        if (Test-Path -LiteralPath $d) {
          Say ($sub + '\ contains:')
          foreach ($i in (Get-ChildItem -LiteralPath $d -Force -ErrorAction SilentlyContinue | Select-Object -First 12)) { Say ('   ' + $i.Name + $(if ($i.PSIsContainer) { '\' } else { '  (' + (Mb $i.Length) + ' MB)' })) }
        } else { Say ($sub + '\ (missing)') }
      }
    }

    # ---------------------------------------------------------------- one pass over every client pak
    Write-Host 'Scanning Space Marine 2 (read-only; this can take a few minutes)...'
    $paksDir = Join-Path $root 'client_pc\root\paks\client'
    $paks = @(Get-ChildItem -LiteralPath $paksDir -Recurse -Filter '*.pak' -File -ErrorAction SilentlyContinue)
    $wpn = New-Object System.Collections.Generic.List[object]     # tpl/wpn_*chainsword*|*pistol* (every file)
    $tex = New-Object System.Collections.Generic.List[object]     # pct/wpn_*chainsword*|*pistol*
    $cls = New-Object System.Collections.Generic.List[object]     # ssl/weapons/**.cls with chainsword|pistol
    $res = New-Object System.Collections.Generic.List[object]     # resources.pak entries with chainsword|bolt_pistol
    $snd = New-Object System.Collections.Generic.List[object]     # entries of default_sound_*.pak
    $bnk = New-Object System.Collections.Generic.List[object]     # every .bnk
    $csv = $null
    $ord = [StringComparison]::Ordinal
    foreach ($pak in $paks) {
      $isRes = ($pak.Name -eq 'resources.pak'); $isSnd = ($pak.Name -like 'default_sound_*')
      try {
        $zip = [System.IO.Compression.ZipFile]::OpenRead($pak.FullName)
        try {
          foreach ($e in $zip.Entries) {
            $n = $e.FullName
            if ($n.StartsWith('tpl/wpn_', $ord) -and ($n.Contains('chainsword') -or $n.Contains('pistol'))) { $wpn.Add([pscustomobject]@{ Pak = $pak.FullName; Name = $n; Len = $e.Length }) }
            elseif ($n.StartsWith('pct/wpn_', $ord) -and ($n.Contains('chainsword') -or $n.Contains('pistol'))) { $tex.Add([pscustomobject]@{ Pak = $pak.FullName; Name = $n; Len = $e.Length }) }
            elseif ($n.EndsWith('.cls', $ord) -and $n.StartsWith('ssl/weapons/', $ord) -and ($n.Contains('chainsword') -or $n.Contains('pistol'))) { $cls.Add([pscustomobject]@{ Pak = $pak.FullName; Name = $n; Len = $e.Length }) }
            if ($isRes -and ($n.Contains('chainsword') -or $n.Contains('bolt_pistol'))) { $res.Add([pscustomobject]@{ Pak = $pak.FullName; Name = $n; Len = $e.Length }) }
            if ($isSnd -and $n.Length -gt 0 -and -not $n.EndsWith('/')) { $snd.Add([pscustomobject]@{ Pak = $pak.FullName; PakName = $pak.Name; Name = $n; Len = $e.Length; Packed = $e.CompressedLength }) }
            if ($n.EndsWith('.bnk', $ord)) { $bnk.Add([pscustomobject]@{ Pak = $pak.FullName; PakName = $pak.Name; Name = $n; Len = $e.Length }) }
            if (-not $csv -and $n.StartsWith('sounds/stats/', $ord) -and $n.EndsWith('.csv', $ord)) { $csv = [pscustomobject]@{ Pak = $pak.FullName; Name = $n; Len = $e.Length } }
          }
        } finally { $zip.Dispose() }
      } catch { }
    }

    # ---- choose ONE chainsword and ONE bolt pistol template
    $tplNames = @($wpn | Where-Object { $_.Name -match '^tpl/(wpn_[^/]+)\.tpl/\1\.tpl$' } | ForEach-Object { $Matches[1] } | Sort-Object -Unique)
    $chainNames = @($tplNames | Where-Object { $_ -like '*chainsword*' })
    $pistolNames = @($tplNames | Where-Object { $_ -like '*pistol*' })
    function Pick($names, $preferred, $likeBolt) {
      if ($names -contains $preferred) { return $preferred }
      $c = @($names); if ($likeBolt) { $b = @($names | Where-Object { $_ -like '*bolt_pistol*' }); if ($b.Count) { $c = $b } }
      if (-not $c.Count) { return $null }
      return ($c | Sort-Object { $_.Length }, { $_ } | Select-Object -First 1)
    }
    $chain = Pick $chainNames 'wpn_chainsword_01' $false
    $pistol = Pick $pistolNames 'wpn_bolt_pistol_01' $true

    Section ('WEAPON TEMPLATES FOUND (chainsword: ' + $chainNames.Count + ', pistol: ' + $pistolNames.Count + ')') {
      Say ('Chosen chainsword: ' + $chain + '     Chosen pistol: ' + $pistol)
      Say 'All chainsword templates:'; foreach ($n in ($chainNames | Select-Object -First 25)) { Say ('   ' + $n) }
      Say 'All pistol templates:'; foreach ($n in ($pistolNames | Select-Object -First 25)) { Say ('   ' + $n) }
    }

    foreach ($w in @($chain, $pistol)) {
      if (-not $w) { continue }
      Section ('FILES OF ' + $w) {
        Say 'template folder:'
        foreach ($e in ($wpn | Where-Object { $_.Name.StartsWith("tpl/$w.tpl/", $ord) })) { Say ('   ' + $e.Name.Substring(4) + '  ' + $e.Len + ' bytes  [' + (Split-Path $e.Pak -Leaf) + ']') }
        Say 'textures (pct):'
        foreach ($e in ($tex | Where-Object { $_.Name.StartsWith("pct/$w", $ord) } | Select-Object -First 14)) { Say ('   ' + $e.Name + '  ' + $e.Len + ' bytes  [' + (Split-Path $e.Pak -Leaf) + ']') }
        Say 'descriptors in resources.pak:'
        foreach ($e in ($res | Where-Object { $_.Name.Contains($w) } | Select-Object -First 8)) { Say ('   ' + $e.Name + '  ' + $e.Len + ' bytes') }
      }
    }

    # ---- text samples (.resource descriptors, weapon .cls)
    function ShowText($item, [int]$lines) {
      $zip = [System.IO.Compression.ZipFile]::OpenRead($item.Pak)
      try {
        $e = $zip.GetEntry($item.Name)
        if (-not $e) { Say ('   (entry not found: ' + $item.Name + ')'); return }
        $rb = ReadBytes $e 6000
        $text = [System.Text.Encoding]::UTF8.GetString($rb[0], 0, $rb[1])
        Say ('--- ' + $item.Name + '  (' + $item.Len + ' bytes, first ' + $lines + ' lines)')
        foreach ($l in ($text -split "`r?`n" | Select-Object -First $lines)) { Say ('   ' + $l) }
      } finally { $zip.Dispose() }
    }
    Section 'TEXT SAMPLES' {
      if ($chain) { foreach ($it in ($res | Where-Object { $_.Name.Contains($chain) -and $_.Name -match 'pct' } | Sort-Object Name | Select-Object -First 1)) { ShowText $it 22 } }
      foreach ($pat in 'chainsword', 'bolt_pistol') {
        $c = $cls | Where-Object { $_.Name.Contains($pat) } | Sort-Object Len -Descending | Select-Object -First 1
        if ($c) { ShowText $c 26 } else { Say ('   (no .cls found for ' + $pat + ')') }
      }
      Say ('.cls files mentioning chainsword/pistol: ' + $cls.Count + ' (first 12 names)')
      foreach ($c in ($cls | Sort-Object Name | Select-Object -First 10)) { Say ('   ' + $c.Name + '  ' + $c.Len) }
    }

    # ---- binary heads
    function Head($item, [int]$bytes) {
      $zip = [System.IO.Compression.ZipFile]::OpenRead($item.Pak)
      try {
        $e = $zip.GetEntry($item.Name)
        if (-not $e) { Say ('   (entry not found: ' + $item.Name + ')'); return }
        $rb = ReadBytes $e $bytes
        Say ('--- ' + $item.Name + '  (' + $e.Length + ' bytes)')
        HexLines $rb[0] $rb[1] '   '
      } finally { $zip.Dispose() }
    }
    Section 'BINARY HEADS (hex)' {
      foreach ($w in @($chain, $pistol)) {
        if (-not $w) { continue }
        foreach ($it in ($wpn | Where-Object { $_.Name.StartsWith("tpl/$w.tpl/", $ord) })) {
          if ($w -eq $chain -or $it.Name -match '\.(tpl|tpl_data)$') { Head $it 64 }
        }
        foreach ($it in ($tex | Where-Object { $_.Name.EndsWith('.pct_mip', $ord) -and $_.Name.StartsWith("pct/$w", $ord) } | Select-Object -First 1)) { Head $it 48 }
      }
    }

    # ---- audio structure
    Section ('AUDIO: entries of default_sound_*.pak (' + $snd.Count + ' total)') {
      foreach ($e in ($snd | Where-Object { $_.PakName -notmatch 'chinese|english|french|german|japanese|russian|spanish' } | Select-Object -First 30)) { Say ('   ' + $e.PakName + ' :: ' + $e.Name + '  ' + (Mb $e.Len) + ' MB  (packed ' + (Mb $e.Packed) + ')') }
      Say '   language packs (one line each):'
      foreach ($g in ($snd | Where-Object { $_.PakName -match 'chinese|english|french|german|japanese|russian|spanish' } | Group-Object PakName)) { Say ('   ' + $g.Name + ': ' + (($g.Group | ForEach-Object { $_.Name + ' ' + (Mb $_.Len) + 'MB' }) -join ', ')) }
    }
    Section ('AUDIO: sound banks (' + $bnk.Count + ' .bnk files)') {
      foreach ($e in ($bnk | Sort-Object Name | Select-Object -First 20)) { Say ('   ' + $e.Name + '  ' + $e.Len + ' bytes  [' + $e.PakName + ']') }
      $wb = $bnk | Where-Object { $_.Name -like '*wpn*' } | Select-Object -First 1
      if ($wb) { Head $wb 128 }
      if ($csv) { ShowText $csv 8 }
    }
    Section 'AUDIO: inside one sound archive' {
      $zipEntry = $snd | Where-Object { $_.Name.EndsWith('.zip', $ord) -and $_.PakName -notmatch 'chinese|english|french|german|japanese|russian|spanish' } | Sort-Object Len | Select-Object -First 1
      if (-not $zipEntry) { Say '   (no .zip entries found in the sound paks)'; return }
      Say ('   sample: ' + $zipEntry.PakName + ' :: ' + $zipEntry.Name + '  (' + (Mb $zipEntry.Len) + ' MB, packed ' + (Mb $zipEntry.Packed) + ' MB)')
      $pz = [System.IO.Compression.ZipFile]::OpenRead($zipEntry.Pak)
      try {
        $oe = $pz.GetEntry($zipEntry.Name)
        $os = $oe.Open()
        try {
          if (-not $os.CanSeek) {
            if ($zipEntry.Len -gt 300MB) { Say '   (nested archive is compressed and too big to inspect here)'; return }
            $ms = New-Object System.IO.MemoryStream; $os.CopyTo($ms); $os.Dispose(); $os = $ms; [void]$os.Seek(0, 'Begin')
          }
          $inner = New-Object System.IO.Compression.ZipArchive($os, [System.IO.Compression.ZipArchiveMode]::Read)
          try {
            Say ('   inner archive entries: ' + $inner.Entries.Count)
            foreach ($ie in ($inner.Entries | Select-Object -First 15)) { Say ('      ' + $ie.FullName + '  ' + $ie.Length + ' bytes') }
            $first = $inner.Entries | Where-Object { $_.Length -gt 0 } | Select-Object -First 1
            if ($first) { $rb = ReadBytes $first 64; Say ('   head of ' + $first.FullName + ':'); HexLines $rb[0] $rb[1] '      ' }
          } finally { $inner.Dispose() }
        } catch { Say ('   cannot open it as a nested archive: ' + $_.Exception.Message) } finally { $os.Dispose() }
        $hdrName = [System.IO.Path]::ChangeExtension($zipEntry.Name, '.header')
        $he = $pz.GetEntry($hdrName)
        if ($he) { $rb = ReadBytes $he 96; Say ('   head of ' + $hdrName + ' (' + $he.Length + ' bytes):'); HexLines $rb[0] $rb[1] '      ' } else { Say ('   (no sidecar named ' + $hdrName + ')') }
      } finally { $pz.Dispose() }
    }
  }

  # ---------------------------------------------------------------- finish
  $lines = @($out | ForEach-Object { Mask $_ })
  if ($lines.Count -gt 520) { $lines = $lines[0..510] + @('... (cut at 510 lines; the full report is in the file named below)') }
  $report = $lines -join "`r`n"
  $file = Join-Path ([System.IO.Path]::GetTempPath()) 'ashen-scan2.txt'
  try { Set-Content -Path $file -Value $report -Encoding UTF8 } catch {}
  $clip = 'could not reach the clipboard - open the file and copy it'
  try { Set-Clipboard -Value $report; $clip = 'copied to your clipboard' } catch {}
  Write-Host ''
  Write-Host ('Done: ' + $lines.Count + ' lines, ' + $clip + '. Also saved to ' + (Mask $file))
  Write-Host 'Paste it back into the chat (Ctrl+V).'
}
