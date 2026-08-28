@echo off
chcp 65001 >nul
cd /d "%~dp0"
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0SOOP_LIVE_SETTING.ps1"
if errorlevel 1 (
    echo.
    echo ========================================
    echo  Settings script finished with error
    echo ========================================
    echo.
    pause
)
