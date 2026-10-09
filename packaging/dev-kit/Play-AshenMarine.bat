@echo off
rem Starts Dark Souls III as the Ashen Marine OFFLINE COPY. Your real save is backed up first and checked afterwards.
rem --probe        private test kits: read-only reports about the running game, and a collector that looks in the game's
rem                memory for what is needed to read its archives (it changes nothing in the game)
rem --sounds       play the Space Marine 2 sounds that Prepare-AshenMarine.bat made
rem --experiments  F8 in the game: the test weapons (changes the OFFLINE COPY of your save only)
cd /d "%~dp0"
if not exist "ashenmarine\ashenmarine-launcher.exe" (
  echo Cannot find ashenmarine\ashenmarine-launcher.exe - did you unzip the whole folder?
  pause
  exit /b 1
)
echo.
echo  ASHEN MARINE - play
echo  -------------------
echo  Dark Souls III starts as the offline copy. This window stays open while you play; quit the game from its own
echo  menu when you are done. Right after that, the weapon names are prepared for the NEXT time you press Play.
echo.
"ashenmarine\ashenmarine-launcher.exe" --probe --sounds --experiments
if exist "ashenmarine\mod\ashenmarine-msg.json" goto done
echo.
echo  ===== Dark Souls III is closed. Making the item names (Chainsword / Bolt Pistol / Bolt Rounds) ... =====
"ashenmarine\ashenmarine-setup.exe" ds3-prepare
echo.
echo  If that says the names are written, they show the next time you press Play-AshenMarine.bat.
echo  If it says it could not: run Send-Logs.bat and send me the zip.
:done
echo.
pause
