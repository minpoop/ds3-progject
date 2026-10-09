@echo off
rem OPTIONAL. Copies the chainsword and bolt pistol MODEL FILES of YOUR Space Marine 2 into one zip on your Desktop, so that
rem you can choose to send them for building the model converter. Nothing is uploaded by this file; nothing in the game is changed.
cd /d "%~dp0"
if not exist "ashenmarine\ashenmarine-setup.exe" (
  echo Cannot find ashenmarine\ashenmarine-setup.exe - did you unzip the whole folder?
  pause
  exit /b 1
)
echo.
echo  ASHEN MARINE - model files (OPTIONAL)
echo  -------------------------------------
echo  This copies the chainsword and bolt pistol MODEL FILES of your Space Marine 2 (about 3 MB, taken from the game's own
echo  files; the game is only read) into ONE zip on your Desktop:  AshenMarine-model-files.zip
echo.
echo  NOTHING IS UPLOADED. I only get the zip if you attach it to the chat yourself, and I only use it to write the
echo  converter that will run on your own PC. The finished mashup will never contain any Space Marine 2 file.
echo  You do not have to do this: the rest of the kit works without it.
echo.
choice /c YN /m " Make the zip now (Y = yes, N = no)"
if errorlevel 2 goto :eof
echo.
"ashenmarine\ashenmarine-setup.exe" sm2-export-models
if errorlevel 1 (
  echo.
  echo  The copy did not work - the message above says why. Nothing was put on your Desktop.
  pause
  exit /b 1
)
powershell -NoProfile -ExecutionPolicy Bypass -Command ^
 "$out = Join-Path ([Environment]::GetFolderPath('Desktop')) 'AshenMarine-model-files.zip'; if (Test-Path $out) { Remove-Item $out -Force };" ^
 "Compress-Archive -Path 'ashenmarine\model-files\*' -DestinationPath $out;" ^
 "Write-Host ('Done. If you want to send it, attach this file to the chat: ' + $out)"
pause
