@echo off
rem Looks at YOUR Space Marine 2 install (read-only) and writes a report. Nothing is changed, copied or uploaded.
cd /d "%~dp0"
if not exist "ashenmarine\ashenmarine-setup.exe" (
  echo Cannot find ashenmarine\ashenmarine-setup.exe - did you unzip the whole folder?
  pause
  exit /b 1
)
echo.
echo  ASHEN MARINE - Space Marine 2 probe (read-only)
echo  ------------------------------------------------
echo  This only READS your Space Marine 2 files. It changes nothing in the game and uploads nothing.
echo  It can take a few minutes (Space Marine 2 is big). Please wait for "Done".
echo  If Space Marine 2 is not in a normal Steam library, put its folder on the first line of a new text file
echo  called  ashenmarine\sm2-folder.txt  and run this again.
echo.
"ashenmarine\ashenmarine-setup.exe" probe --out "%~dp0ashenmarine\probe-sm2"
echo.
echo  The report is  ashenmarine\probe-sm2\probe-report.txt
echo  The texture pictures it made for you to look at are in  ashenmarine\probe-sm2\textures  (open index.html).
echo  Next: double-click Send-Logs.bat to collect everything into one zip.
echo.
pause
