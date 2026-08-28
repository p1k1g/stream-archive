@echo off
chcp 65001 >nul
setlocal
cd /d "%~dp0"
title SOOP LIVE Watcher - Cloudflare

echo.
echo ========================================
echo  SOOP LIVE Watcher - Cloudflare
echo ========================================
echo.

if not exist "%~dp0SOOP_LIVE_SETTING.ini" (
    echo SOOP_LIVE_SETTING.ini not found.
    echo Run SOOP_LIVE_SETTING.bat first.
    echo.
    pause
    exit /b 1
)

if not exist "%~dp0SOOP_LIVE_CHANNELS.txt" (
    echo SOOP_LIVE_CHANNELS.txt not found.
    echo Run SOOP_LIVE_SETTING.bat first.
    echo.
    pause
    exit /b 1
)

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0SOOP_LIVE.ps1"
set "EXITCODE=%ERRORLEVEL%"

echo.
echo ========================================
if "%EXITCODE%"=="0" (
    echo  Finished
) else (
    echo  Finished with error (%EXITCODE%^)
)
echo ========================================
echo.
pause
exit /b %EXITCODE%
