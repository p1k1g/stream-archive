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

echo.
echo ========================================
echo  STREAM ARCHIVE BUILD COMPLETE
echo ========================================
echo.
echo Output:
echo   rust-web\target\release\stream-archive-server.exe
echo   rust-web\target\release\stream-archive-launcher.exe
echo.
if /I not "%STREAM_ARCHIVE_NO_PAUSE%"=="1" pause
endlocal
