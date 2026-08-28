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
echo  SOOP LIVE WinUI 3 Compact Publish
echo  framework-dependent / unpackaged
echo ========================================
echo.

if exist ".\publish" rmdir /s /q ".\publish"

dotnet.exe publish ".\.generated\SOOPLiveWinUI\SOOPLiveWinUI.csproj" ^
  -c Release ^
  -r win-x64 ^
  --self-contained false ^
  -p:WindowsPackageType=None ^
  -p:EnableWinAppRunSupport=false ^
  -p:WindowsAppSDKSelfContained=false ^
  -p:PublishTrimmed=false ^
  -o ".\publish"

if errorlevel 1 (
  echo.
  echo [ERROR] compact publish failed.
  pause
  exit /b 1
)

if exist ".\publish\backend" rmdir /s /q ".\publish\backend"
xcopy ".\backend" ".\publish\backend\" /E /I /Y >nul
if not exist ".\publish\backend\SOOP_LIVE_SETTING.ini" copy /Y ".\publish\backend\SOOP_LIVE_SETTING.example.ini" ".\publish\backend\SOOP_LIVE_SETTING.ini" >nul
if not exist ".\publish\backend\SOOP_LIVE_CHANNELS.txt" copy /Y ".\publish\backend\SOOP_LIVE_CHANNELS.example.txt" ".\publish\backend\SOOP_LIVE_CHANNELS.txt" >nul
del /q ".\publish\*.pdb" 2>nul

echo.
echo ========================================
echo  COMPACT PUBLISH COMPLETE
echo ========================================
echo.
echo Output:
dir /b ".\publish\*.exe"
echo.
echo This build requires:
echo   - .NET Desktop Runtime 8 x64
echo   - Windows App Runtime installed on the PC
echo.
echo Folder size:
powershell.exe -NoLogo -NoProfile -Command ^
  "$s=(Get-ChildItem '.\publish' -Recurse -File | Measure-Object Length -Sum).Sum; '{0:N1} MB' -f ($s/1MB)"
echo.

if not exist "%~dp0publish\backend\SOOP_LIVE.ps1" (
    echo.
    echo [ERROR] publish backend missing:
    echo %~dp0publish\backend\SOOP_LIVE.ps1
    echo.
    echo The EXE would exit immediately without this file.
    pause
    exit /b 1
)

echo [OK] publish backend verified.
echo.

pause
