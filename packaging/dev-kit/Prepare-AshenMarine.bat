@echo off
rem Reads YOUR Space Marine 2 and Dark Souls III installs (read-only) and makes what the mashup needs. Nothing in either game is changed.
rem The only thing put where Dark Souls III loads it is the new weapon models in ashenmarine\mod\parts - and only if you say yes in step 5.
cd /d "%~dp0"
if not exist "ashenmarine\ashenmarine-setup.exe" (
  echo Cannot find ashenmarine\ashenmarine-setup.exe - did you unzip the whole folder?
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
echo  ASHEN MARINE - prepare
echo  ----------------------
echo  This READS your Space Marine 2 and Dark Souls III files (on your PC only; nothing is uploaded, nothing in either
echo  game is changed) and makes:
echo    1. the Space Marine 2 chainsword and bolt pistol sounds, as ordinary .wav files in  ashenmarine\assets
echo    2. if it can: a copy of Dark Souls III's item text with the new names, in  ashenmarine\mod
echo       (If it cannot, that is OK: the names are also written into the running game by Play-AshenMarine.bat.)
echo    3. a report that lists where Dark Souls III keeps the files the mod needs (no copies of them)
echo    4. a report about how the Space Marine 2 weapon models are stored (no copies of the models)
echo    5. the new weapon MODELS: built in memory, checked and drawn (a report and a picture in  ashenmarine\ds3-models ).
echo       If they pass every check it asks whether to put them into the game; Remove-Models.bat takes them out again.
echo  It takes a few minutes. Please wait for "Done" at the end.
echo  If Space Marine 2 is not in a normal Steam library, put its folder on the first line of a new text file called
echo  ashenmarine\sm2-folder.txt (and Dark Souls III's, the folder that contains Game\DarkSoulsIII.exe, in
echo  ashenmarine\game-folder.txt) and run this again.
echo.
echo  ===== 1 of 5: Space Marine 2 sounds =====
"ashenmarine\ashenmarine-setup.exe" prepare
echo.
echo  ===== 2 of 5: Dark Souls III item names (a file; may not be possible yet, see above) =====
"ashenmarine\ashenmarine-setup.exe" ds3-prepare
echo.
echo  ===== 3 of 5: where Dark Souls III keeps its files (report only) =====
"ashenmarine\ashenmarine-setup.exe" ds3-probe
echo.
echo  ===== 4 of 5: model reports =====
"ashenmarine\ashenmarine-setup.exe" sm2-mesh-probe
echo.
echo  ===== 5 of 5: the new weapon models =====
"ashenmarine\ashenmarine-setup.exe" ds3-models
if errorlevel 1 goto nomodels
echo.
echo  The new models passed every check (the pictures are in  ashenmarine\ds3-models ).
echo  If you put them into the game, the chainsword and the bolt pistol may look like the Space Marine 2 ones.
echo  If the game ever crashes or a weapon looks wrong: double-click Remove-Models.bat.
echo.
choice /c YN /t 30 /d Y /m " Put the new models into the game now (Y = yes, N = no; it chooses Y by itself after 30 seconds)"
if errorlevel 2 goto skipinstall
echo.
"ashenmarine\ashenmarine-setup.exe" ds3-models --install
goto afterModels
:skipinstall
echo.
echo  OK, nothing was put into the game. (Run this again, or the line  ashenmarine\ashenmarine-setup.exe ds3-models --install  , to do it later.)
goto afterModels
:nomodels
echo.
echo  No new model was made (the lines above say why). Nothing was put into the game.
:afterModels
echo.
echo  Done. You can listen to any .wav in  ashenmarine\assets\sounds  (set A) and  ashenmarine\assets\sounds-exact  (set B).
echo  Next: double-click Play-AshenMarine.bat.
echo.
pause
