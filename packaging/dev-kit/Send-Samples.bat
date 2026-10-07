@echo off
rem OPTIONAL. Packs the few small sound clips that Probe-SM2.bat kept from YOUR Space Marine 2 into one zip on your Desktop,
rem so they can be used to test the sound converter. Nothing is sent anywhere by this: you decide whether to attach the zip.
cd /d "%~dp0"
if not exist "ashenmarine\probe-sm2\samples\index.txt" (
  echo Run Probe-SM2.bat first - there are no sound clips yet.
  pause
  exit /b 1
)
echo.
echo  These are the files that would go into the zip (all are short Space Marine 2 sound effects from your own install):
echo.
powershell -NoProfile -ExecutionPolicy Bypass -Command ^
 "Get-ChildItem 'ashenmarine\probe-sm2\samples' -File | Format-Table Name, @{n='KB';e={[math]::Round($_.Length/1KB)}} -AutoSize;" ^
 "$out = Join-Path ([Environment]::GetFolderPath('Desktop')) 'AshenMarine-sound-samples.zip'; if (Test-Path $out) { Remove-Item $out -Force };" ^
 "Compress-Archive -Path 'ashenmarine\probe-sm2\samples\*' -DestinationPath $out;" ^
 "Write-Host ('Made ' + $out + '  (' + [math]::Round((Get-Item $out).Length/1KB) + ' KB). Attach it to the chat only if you are happy to.')"
echo.
pause
