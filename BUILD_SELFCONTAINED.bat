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
echo  SOOP LIVE WinUI 3 Self-contained Publish
echo  LARGE build - runtime included
echo ========================================
echo.

if exist ".\publish_selfcontained" rmdir /s /q ".\publish_selfcontained"

dotnet.exe publish ".\.generated\SOOPLiveWinUI\SOOPLiveWinUI.csproj" ^
  -c Release ^
  -r win-x64 ^
  --self-contained true ^
  -p:WindowsPackageType=None ^
  -p:EnableWinAppRunSupport=false ^
  -p:WindowsAppSDKSelfContained=true ^
  -p:PublishTrimmed=false ^
  -o ".\publish_selfcontained"

if errorlevel 1 (
  echo.
  echo [ERROR] self-contained publish failed.
  pause
  exit /b 1
)

if exist ".\publish_selfcontained\backend" rmdir /s /q ".\publish_selfcontained\backend"
xcopy ".\backend" ".\publish_selfcontained\backend\" /E /I /Y >nul
if not exist ".\publish_selfcontained\backend\SOOP_LIVE_SETTING.ini" copy /Y ".\publish_selfcontained\backend\SOOP_LIVE_SETTING.example.ini" ".\publish_selfcontained\backend\SOOP_LIVE_SETTING.ini" >nul
if not exist ".\publish_selfcontained\backend\SOOP_LIVE_CHANNELS.txt" copy /Y ".\publish_selfcontained\backend\SOOP_LIVE_CHANNELS.example.txt" ".\publish_selfcontained\backend\SOOP_LIVE_CHANNELS.txt" >nul
del /q ".\publish_selfcontained\*.pdb" 2>nul

echo.
echo ========================================
echo  SELF-CONTAINED PUBLISH COMPLETE
echo ========================================
echo.
echo Output:
dir /b ".\publish_selfcontained\*.exe"
echo.
echo Folder size:
powershell.exe -NoLogo -NoProfile -Command ^
  "$s=(Get-ChildItem '.\publish_selfcontained' -Recurse -File | Measure-Object Length -Sum).Sum; '{0:N1} MB' -f ($s/1MB)"
echo.
pause
