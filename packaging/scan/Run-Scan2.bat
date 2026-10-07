@echo off
rem Ashen Marine - READ-ONLY scan, part 2. Nothing is changed, copied or uploaded. The report is copied to your clipboard.
cd /d "%~dp0"
echo.
echo  ASHEN MARINE - read-only scan, part 2
echo  --------------------------------------
echo  This only READS file names, sizes and a few small snippets. It changes nothing and uploads nothing.
echo  It can take a few minutes (Space Marine 2 is big). Please wait for "Done".
echo.
rem Optional: if a game is NOT in your normal Steam library, put its folder on the first line of
rem sm2-folder.txt or ds3-folder.txt next to this file.
if exist "%~dp0sm2-folder.txt" set /p ASHEN_SM2=<"%~dp0sm2-folder.txt"
if exist "%~dp0ds3-folder.txt" set /p ASHEN_DS3=<"%~dp0ds3-folder.txt"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scan2.ps1"
echo.
echo  Now go back to the chat and press Ctrl+V to paste the report.
echo.
pause
