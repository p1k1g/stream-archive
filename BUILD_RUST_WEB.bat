@echo off
setlocal
cd /d "%~dp0"

where cargo.exe >nul 2>nul
if errorlevel 1 (
  echo [ERROR] Rust cargo.exe was not found in PATH.
  echo Install Rust from https://rustup.rs/ and reopen this terminal.
  if /I not "%SOOP_NO_PAUSE%"=="1" pause
  exit /b 1
)

cargo build --locked --release --manifest-path ".\rust-web\Cargo.toml"
if errorlevel 1 (
  echo.
  echo [ERROR] Rust web build failed.
  if /I not "%SOOP_NO_PAUSE%"=="1" pause
  exit /b 1
)

echo.
echo ========================================
echo  SOOP RUST WEB BUILD COMPLETE
echo ========================================
echo.
echo Output:
echo   rust-web\target\release\soop-web.exe
echo   rust-web\target\release\soop-launcher.exe
echo.
if /I not "%SOOP_NO_PAUSE%"=="1" pause
endlocal
