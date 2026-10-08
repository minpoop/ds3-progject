@echo off
rem Reads YOUR Space Marine 2 and Dark Souls III installs (read-only) and makes what the mashup needs. Nothing in either game is changed.
cd /d "%~dp0"
if not exist "ashenmarine\ashenmarine-setup.exe" (
  echo Cannot find ashenmarine\ashenmarine-setup.exe - did you unzip the whole folder?
  pause
  exit /b 1
)
echo.
echo  ASHEN MARINE - prepare
echo  ----------------------
echo  This READS your Space Marine 2 and Dark Souls III files (on your PC only; nothing is uploaded, nothing in either
echo  game is changed) and makes:
echo    1. the Space Marine 2 chainsword and bolt pistol sounds, as ordinary .wav files in  ashenmarine\assets
echo    2. a copy of Dark Souls III's item text with the names Chainsword / Bolt Pistol / Bolt Rounds, in  ashenmarine\mod
echo    3. a report about how the weapon models are stored (no copies of the models)
echo  It takes a few minutes. Please wait for "Done" at the end.
echo  If Space Marine 2 is not in a normal Steam library, put its folder on the first line of a new text file called
echo  ashenmarine\sm2-folder.txt (and Dark Souls III's, the folder that contains Game\DarkSoulsIII.exe, in
echo  ashenmarine\game-folder.txt) and run this again.
echo.
echo  ===== 1 of 3: Space Marine 2 sounds =====
"ashenmarine\ashenmarine-setup.exe" prepare
echo.
echo  ===== 2 of 3: Dark Souls III item names =====
"ashenmarine\ashenmarine-setup.exe" ds3-prepare
echo.
echo  ===== 3 of 3: model reports =====
"ashenmarine\ashenmarine-setup.exe" sm2-mesh-probe
echo.
echo  Done. You can listen to any .wav in  ashenmarine\assets\sounds  (set A) and  ashenmarine\assets\sounds-exact  (set B).
echo  Next: double-click Play-AshenMarine.bat.
echo.
pause
