@echo off
rem Collects the Ashen Marine logs and reports (text only - no pictures, no game files) into one zip on your Desktop.
rem Your Windows user name and Steam id are masked in the copies.
cd /d "%~dp0"
powershell -NoProfile -ExecutionPolicy Bypass -Command ^
 "$tmp = Join-Path $env:TEMP 'ashen-logs'; if (Test-Path $tmp) { Remove-Item $tmp -Recurse -Force };" ^
 "New-Item -ItemType Directory $tmp | Out-Null;" ^
 "$srcs = @('ashenmarine\logs', 'modengine2\modengine2\logs', 'ashenmarine\probe-sm2', 'ashenmarine\prepare-sm2');" ^
 "foreach ($s in $srcs) { if (Test-Path $s) { Get-ChildItem $s -File | ForEach-Object { $t = (Get-Content $_.FullName -Raw) -replace [regex]::Escape($env:USERNAME), '<user>' -replace '\b\d{15,20}\b', '<steamid>'; Set-Content -Path (Join-Path $tmp ($s.Replace('\','_') + '__' + $_.Name)) -Value $t } } };" ^
 "if (Test-Path 'ashenmarine\FATAL.txt') { Copy-Item 'ashenmarine\FATAL.txt' $tmp };" ^
 "$out = Join-Path ([Environment]::GetFolderPath('Desktop')) 'AshenMarine-logs.zip'; if (Test-Path $out) { Remove-Item $out -Force };" ^
 "Compress-Archive -Path (Join-Path $tmp '*') -DestinationPath $out;" ^
 "Write-Host ('Done. Attach this file to the chat: ' + $out)"
pause
