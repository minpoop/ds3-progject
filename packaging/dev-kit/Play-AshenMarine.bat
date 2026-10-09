@echo off
rem Starts Dark Souls III as the Ashen Marine OFFLINE COPY. Your real save is backed up first and checked afterwards.
rem --probe        private test kits: read-only reports about the running game, and a collector that looks in the game's
rem                memory for what is needed to read its archives (it changes nothing in the game)
rem --sounds       play the Space Marine 2 sounds that Prepare-AshenMarine.bat made
rem --experiments  F8 in the game: the test weapons (changes the OFFLINE COPY of your save only)
rem --rename       write the new names of the test weapons over the old ones in the game's memory (logs\rename.txt)
cd /d "%~dp0"
if not exist "ashenmarine\ashenmarine-launcher.exe" (
  echo Cannot find ashenmarine\ashenmarine-launcher.exe - did you unzip the whole folder?
  pause
  exit /b 1
)
rem Take over the key cache of an older kit folder next to this one, so that the game does not have to be started once more.
for %%F in (ds3-keys.pem keys-seen.pem) do (
  if not exist "ashenmarine\cache\%%F" (
    for /d %%D in ("..\AshenMarine-*") do (
      if exist "%%~fD\ashenmarine\cache\%%F" if not exist "ashenmarine\cache\%%F" (
        if not exist "ashenmarine\cache" mkdir "ashenmarine\cache"
        copy /y "%%~fD\ashenmarine\cache\%%F" "ashenmarine\cache\%%F" >nul
        echo  Took over %%F of an older kit folder.
      )
    )
  )
)
echo.
echo  ASHEN MARINE - play
echo  -------------------
echo  Dark Souls III starts as the offline copy. This window stays open while you play; quit the game from its own
echo  menu when you are done. Please stay in the game for at least 4 minutes after your character has loaded: the
echo  helpers that collect the archive keys and write the new weapon names into the game need that time.
echo  Right after you quit, the program looks at the game's files again (a minute or two).
echo.
"ashenmarine\ashenmarine-launcher.exe" --probe --sounds --experiments --rename
echo.
echo  ===== Dark Souls III is closed. Looking at its files again (now with everything the helper collected) ... =====
"ashenmarine\ashenmarine-setup.exe" ds3-prepare
echo.
"ashenmarine\ashenmarine-setup.exe" ds3-probe
echo.
echo  Done. If the weapons had their new names and looks, tell me; either way: run Send-Logs.bat and send me the zip.
echo.
pause
