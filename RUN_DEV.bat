@echo off
setlocal
cd /d "%~dp0"

where cargo.exe >nul 2>nul
if errorlevel 1 (
  echo [ERROR] Rust cargo.exe was not found in PATH.
  echo Install Rust from https://rustup.rs/ and reopen this terminal.
  pause
  exit /b 1
)

cargo run --release --bin stream-archive-server --manifest-path ".\rust-runtime\Cargo.toml"
set "RC=%ERRORLEVEL%"

echo.
echo Stream Archive exited with code %RC%.
pause
exit /b %RC%
