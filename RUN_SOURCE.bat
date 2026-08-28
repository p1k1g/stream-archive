@echo off
setlocal
cd /d "%~dp0"

if not exist ".\.generated\SOOPLiveWinUI\SOOPLiveWinUI.csproj" (
  call PREPARE_PROJECT.bat
  if errorlevel 1 exit /b 1
)

powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File ".\SYNC_PROJECT.ps1"
if errorlevel 1 exit /b 1

echo ========================================
echo  SOOP LIVE WinUI 3 - UNPACKAGED fix40
echo ========================================
echo.

dotnet.exe run ^
  --project ".\.generated\SOOPLiveWinUI\SOOPLiveWinUI.csproj" ^
  -c Debug ^
  -p:WindowsPackageType=None ^
  -p:EnableWinAppRunSupport=false ^
  -p:PublishTrimmed=false

set RC=%ERRORLEVEL%
echo.
echo SOOP unpackaged exit code: %RC%
echo.
pause
