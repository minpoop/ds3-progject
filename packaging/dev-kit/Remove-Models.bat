@echo off
rem Takes the new weapon models out of the mod folder again, so that Dark Souls III shows its own weapons.
rem Only the files that Prepare-AshenMarine.bat put there (listed in ashenmarine\mod\ashenmarine-models.json) are deleted.
cd /d "%~dp0"
if not exist "ashenmarine\ashenmarine-setup.exe" (
  echo Cannot find ashenmarine\ashenmarine-setup.exe - did you unzip the whole folder?
  pause
  exit /b 1
)
echo.
echo  ASHEN MARINE - take the new weapon models out again
echo  ---------------------------------------------------
echo  Dark Souls III must NOT be running. Nothing else is changed: your save, the sounds and the game's own files stay as they are.
echo.
"ashenmarine\ashenmarine-setup.exe" ds3-models --remove
echo.
pause
