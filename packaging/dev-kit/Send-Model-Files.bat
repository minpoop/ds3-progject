@echo off
rem OPTIONAL. Copies a few MODEL FILES of YOUR Space Marine 2 and YOUR Dark Souls III, the small index files (.bhd) of the Dark
rem Souls III archives that do not open yet and the public keys the helper saw, into one zip on your Desktop, so that you can choose
rem to send them for building the model converter. Nothing is uploaded by this file; nothing in either game is changed.
cd /d "%~dp0"
if not exist "ashenmarine\ashenmarine-setup.exe" (
  echo Cannot find ashenmarine\ashenmarine-setup.exe - did you unzip the whole folder?
  pause
  exit /b 1
)
echo.
echo  ASHEN MARINE - model files (OPTIONAL)
echo  -------------------------------------
echo  This copies these files into ONE zip on your Desktop:  AshenMarine-model-files.zip
echo    - the chainsword and bolt pistol MODEL files of your Space Marine 2, and the pictures (textures) they use
echo      (a few MB, taken from the game's own files),
echo    - five WEAPON MODEL files of your Dark Souls III: the Shortsword, the Avelyn and three related ones
echo      (a few MB, taken from the game's own archives),
echo    - the small INDEX files (.bhd, 2 KB to 250 KB; only an index of an archive: names, sizes, places - no content) of the
echo      Dark Souls III archives that this program cannot open yet, and the PUBLIC keys it saw in the running game
echo      (they are not secrets: they sit in the game's own memory).
echo  Both games are only read.
echo.
echo  NOTHING IS UPLOADED. I only get the zip if you attach it to the chat yourself, and I only use it to write the
echo  converter that will run on your own PC and to work out how those archives are laid out. The finished mashup will
echo  never contain any Space Marine 2 or Dark Souls III file. You do not have to do this: the rest of the kit works without it.
echo.
choice /c YN /m " Make the zip now (Y = yes, N = no)"
if errorlevel 2 goto :eof
echo.
echo  ===== 1 of 2: Space Marine 2 =====
"ashenmarine\ashenmarine-setup.exe" sm2-export-models
set SM2_RESULT=%errorlevel%
echo.
echo  ===== 2 of 2: Dark Souls III =====
"ashenmarine\ashenmarine-setup.exe" ds3-export-models
set DS3_RESULT=%errorlevel%
echo.
if not "%SM2_RESULT%"=="0" if not "%DS3_RESULT%"=="0" (
  echo  Neither copy worked - the messages above say why. Nothing was put on your Desktop.
  pause
  exit /b 1
)
if not "%SM2_RESULT%"=="0" echo  The Space Marine 2 part did not work (see above); the zip will hold the Dark Souls III part only.
if not "%DS3_RESULT%"=="0" echo  The Dark Souls III part did not work (see above); the zip will hold the Space Marine 2 part only.
powershell -NoProfile -ExecutionPolicy Bypass -Command ^
 "$out = Join-Path ([Environment]::GetFolderPath('Desktop')) 'AshenMarine-model-files.zip'; if (Test-Path $out) { Remove-Item $out -Force };" ^
 "Compress-Archive -Path 'ashenmarine\model-files\*' -DestinationPath $out;" ^
 "Write-Host ('Done. If you want to send it, attach this file to the chat: ' + $out)"
pause
