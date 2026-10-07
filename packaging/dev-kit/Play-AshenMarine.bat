@echo off
rem Starts Dark Souls III as the Ashen Marine OFFLINE COPY. Your real save is backed up first and checked afterwards.
cd /d "%~dp0"
if not exist "ashenmarine\ashenmarine-launcher.exe" (
  echo Cannot find ashenmarine\ashenmarine-launcher.exe - did you unzip the whole folder?
  pause
  exit /b 1
)
start "" "ashenmarine\ashenmarine-launcher.exe"
