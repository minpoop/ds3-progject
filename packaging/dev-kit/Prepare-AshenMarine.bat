@echo off
rem Reads YOUR Space Marine 2 install (read-only) and makes the sound files the mashup plays. Nothing in the game is changed.
cd /d "%~dp0"
if not exist "ashenmarine\ashenmarine-setup.exe" (
  echo Cannot find ashenmarine\ashenmarine-setup.exe - did you unzip the whole folder?
  pause
  exit /b 1
)
echo.
echo  ASHEN MARINE - prepare the Space Marine 2 sounds
echo  -------------------------------------------------
echo  This READS your Space Marine 2 files and turns the chainsword and bolt pistol sounds into ordinary
echo  .wav files in  ashenmarine\assets\sounds  (on your PC only; nothing is uploaded, nothing in the game is changed).
echo  It takes a minute or two. Please wait for "Done".
echo  If Space Marine 2 is not in a normal Steam library, put its folder on the first line of a new text file
echo  called  ashenmarine\sm2-folder.txt  and run this again.
echo.
"ashenmarine\ashenmarine-setup.exe" prepare
echo.
echo  When it says it prepared the sounds, you can double-click any .wav in  ashenmarine\assets\sounds  to listen.
echo  Next: double-click Play-AshenMarine.bat.
echo.
pause
