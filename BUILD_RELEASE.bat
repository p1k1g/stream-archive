@echo off
setlocal
cd /d "%~dp0"

where cargo.exe >nul 2>nul
if errorlevel 1 (
  echo [ERROR] Rust cargo.exe was not found in PATH.
  echo Install Rust from https://rustup.rs/ and reopen this terminal.
  if /I not "%STREAM_ARCHIVE_NO_PAUSE%"=="1" pause
  exit /b 1
)

cargo build --locked --release --manifest-path ".\rust-web\Cargo.toml"
if errorlevel 1 (
  echo.
  echo [ERROR] Stream Archive release build failed.
  if /I not "%STREAM_ARCHIVE_NO_PAUSE%"=="1" pause
  exit /b 1
)

set "RELEASE_DIR=rust-web\target\release"
if not exist "%RELEASE_DIR%\backend\vod" mkdir "%RELEASE_DIR%\backend\vod" || exit /b 1
if not exist "%RELEASE_DIR%\data" mkdir "%RELEASE_DIR%\data" || exit /b 1

echo.
echo ========================================
echo  STREAM ARCHIVE BUILD COMPLETE
echo ========================================
echo.
echo Runnable release layout:
echo   %RELEASE_DIR%\stream-archive-server.exe
echo   %RELEASE_DIR%\stream-archive-launcher.exe
echo   %RELEASE_DIR%\backend\
echo   %RELEASE_DIR%\data\
echo.
echo stream-archive-launcher.exe can be launched directly from this directory.
echo For the distributable Windows package, use BUILD_PORTABLE.bat.
echo.
if /I not "%STREAM_ARCHIVE_NO_PAUSE%"=="1" pause
endlocal
