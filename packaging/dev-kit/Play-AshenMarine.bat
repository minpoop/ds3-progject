@echo off
rem Starts Dark Souls III as the Ashen Marine OFFLINE COPY. Your real save is backed up first and checked afterwards.
rem --probe        private test kits: a read-only report about the running game (nothing is changed)
rem --sounds       play the Space Marine 2 sounds that Prepare-AshenMarine.bat made
rem --experiments  F8 in the game: the test weapons (changes the OFFLINE COPY of your save only)
cd /d "%~dp0"
if not exist "ashenmarine\ashenmarine-launcher.exe" (
  echo Cannot find ashenmarine\ashenmarine-launcher.exe - did you unzip the whole folder?
  pause
  exit /b 1
)
start "" "ashenmarine\ashenmarine-launcher.exe" --probe --sounds --experiments
