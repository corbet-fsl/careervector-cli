@echo off
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0cv.ps1" %*
exit /b %ERRORLEVEL%
