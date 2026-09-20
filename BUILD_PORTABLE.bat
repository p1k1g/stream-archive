@echo off
setlocal
cd /d "%~dp0"

where cargo.exe >nul 2>nul
if errorlevel 1 (
  echo [ERROR] Rust cargo.exe was not found in PATH.
  echo Install Rust from https://rustup.rs/ and reopen this terminal.
  exit /b 1
)

cargo build --locked --release --manifest-path ".\rust-web\Cargo.toml"
if errorlevel 1 (
  echo.
  echo [ERROR] Stream Archive Web compatibility release build failed.
  exit /b 1
)

cargo build --locked --release --manifest-path ".\rust-gui\Cargo.toml"
if errorlevel 1 (
  echo.
  echo [ERROR] Stream Archive native GUI release build failed.
  exit /b 1
)

set "OUT=dist\stream-archive"
set "PRESERVE=dist\.stream-archive-preserve"
set "PRESERVE_RUNTIME=1"
if /I "%GITHUB_ACTIONS%"=="true" set "PRESERVE_RUNTIME=0"

if "%PRESERVE_RUNTIME%"=="1" (
    if exist "%PRESERVE%" rmdir /s /q "%PRESERVE%"
    if exist "%OUT%\data" (
        mkdir "%PRESERVE%" || exit /b 1
        powershell -NoProfile -ExecutionPolicy Bypass -Command "Copy-Item -LiteralPath '.\%OUT%\data' -Destination '.\%PRESERVE%\data' -Recurse -Force"
        if errorlevel 1 (
            echo ERROR: Failed to preserve existing runtime data. Stop StreamArchive.exe and any legacy Stream Archive server/launcher, then retry.
            exit /b 1
        )
    )
    if exist "%OUT%\backend\.stream-archive" (
        if not exist "%PRESERVE%" mkdir "%PRESERVE%" || exit /b 1
        mkdir "%PRESERVE%\backend" 2>nul
        powershell -NoProfile -ExecutionPolicy Bypass -Command "Copy-Item -LiteralPath '.\%OUT%\backend\.stream-archive' -Destination '.\%PRESERVE%\backend\.stream-archive' -Recurse -Force"
        if errorlevel 1 (
            echo ERROR: Failed to preserve local management token data. Stop StreamArchive.exe and any legacy Stream Archive server/launcher, then retry.
            exit /b 1
        )
    )
)

if exist "%OUT%" rmdir /s /q "%OUT%"
if exist "%OUT%" (
    echo ERROR: Existing portable package could not be removed.
    echo Stop StreamArchive.exe and any legacy Stream Archive server/launcher, then retry BUILD_PORTABLE.bat.
    if "%PRESERVE_RUNTIME%"=="1" if exist "%PRESERVE%\data" (
        if not exist "%OUT%\data" mkdir "%OUT%\data" 2>nul
        powershell -NoProfile -ExecutionPolicy Bypass -Command "Copy-Item -LiteralPath '.\%PRESERVE%\data\*' -Destination '.\%OUT%\data' -Recurse -Force" >nul 2>nul
    )
    exit /b 1
)

mkdir "%OUT%\backend\vod" || exit /b 1
mkdir "%OUT%\data" || exit /b 1
mkdir "%OUT%\maintenance" || exit /b 1
mkdir "%OUT%\docs" || exit /b 1

copy /y "rust-gui\target\release\stream-archive-gui.exe" "%OUT%\StreamArchive.exe" >nul || exit /b 1
copy /y "rust-web\target\release\stream-archive-server.exe" "%OUT%\stream-archive-server.exe" >nul || exit /b 1
copy /y "rust-web\target\release\stream-archive-launcher.exe" "%OUT%\stream-archive-launcher.exe" >nul || exit /b 1
copy /y "maintenance\Backup-StreamArchiveData.ps1" "%OUT%\maintenance\Backup-StreamArchiveData.ps1" >nul || exit /b 1
copy /y "maintenance\Restore-StreamArchiveData.ps1" "%OUT%\maintenance\Restore-StreamArchiveData.ps1" >nul || exit /b 1
copy /y "docs\OPERATIONS.md" "%OUT%\docs\OPERATIONS.md" >nul || exit /b 1
copy /y "docs\REVERSE_PROXY.md" "%OUT%\docs\REVERSE_PROXY.md" >nul || exit /b 1
copy /y "docs\LOCAL_LAUNCHER.md" "%OUT%\docs\LOCAL_LAUNCHER.md" >nul || exit /b 1
copy /y "deploy\Caddyfile.example" "%OUT%\Caddyfile.example" >nul || exit /b 1
copy /y "LICENSE" "%OUT%\LICENSE" >nul || exit /b 1
copy /y "THIRD_PARTY_NOTICES.md" "%OUT%\THIRD_PARTY_NOTICES.md" >nul || exit /b 1

if "%PRESERVE_RUNTIME%"=="1" (
    if exist "%PRESERVE%\data" (
        rmdir /s /q "%OUT%\data"
        powershell -NoProfile -ExecutionPolicy Bypass -Command "Copy-Item -LiteralPath '.\%PRESERVE%\data' -Destination '.\%OUT%\data' -Recurse -Force"
        if errorlevel 1 exit /b 1
    )
    if exist "%PRESERVE%\backend\.stream-archive" (
        powershell -NoProfile -ExecutionPolicy Bypass -Command "Copy-Item -LiteralPath '.\%PRESERVE%\backend\.stream-archive' -Destination '.\%OUT%\backend\.stream-archive' -Recurse -Force"
        if errorlevel 1 exit /b 1
    )
    if exist "%PRESERVE%" rmdir /s /q "%PRESERVE%"
)

>"%OUT%\RUN.bat" echo @echo off
>>"%OUT%\RUN.bat" echo cd /d "%%~dp0"
>>"%OUT%\RUN.bat" echo start "" "StreamArchive.exe"

>"%OUT%\RUN_WEB.bat" echo @echo off
>>"%OUT%\RUN_WEB.bat" echo cd /d "%%~dp0"
>>"%OUT%\RUN_WEB.bat" echo start "" "stream-archive-launcher.exe"

>"%OUT%\RUN_SERVER_CONSOLE.bat" echo @echo off
>>"%OUT%\RUN_SERVER_CONSOLE.bat" echo cd /d "%%~dp0"
>>"%OUT%\RUN_SERVER_CONSOLE.bat" echo stream-archive-server.exe

>"%OUT%\BACKUP_DATA.bat" echo @echo off
>>"%OUT%\BACKUP_DATA.bat" echo cd /d "%%~dp0"
>>"%OUT%\BACKUP_DATA.bat" echo powershell -NoProfile -ExecutionPolicy Bypass -File ".\maintenance\Backup-StreamArchiveData.ps1" %%*

>"%OUT%\RESTORE_DATA.bat" echo @echo off
>>"%OUT%\RESTORE_DATA.bat" echo cd /d "%%~dp0"
>>"%OUT%\RESTORE_DATA.bat" echo powershell -NoProfile -ExecutionPolicy Bypass -File ".\maintenance\Restore-StreamArchiveData.ps1" %%*

powershell -NoProfile -ExecutionPolicy Bypass -File ".\maintenance\Write-ReleaseMetadata.ps1" -OutputPath ".\%OUT%\RELEASE_INFO.txt" -ManifestPath ".\rust-web\Cargo.toml"
if errorlevel 1 exit /b 1

powershell -NoProfile -ExecutionPolicy Bypass -Command "$names=@('StreamArchive.exe','stream-archive-server.exe','stream-archive-launcher.exe'); $lines=foreach($n in $names){$h=(Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path '.\%OUT%' $n)).Hash.ToLowerInvariant(); $h+'  '+$n}; $lines | Set-Content -LiteralPath '.\%OUT%\SHA256SUMS.txt' -Encoding ASCII"
if errorlevel 1 exit /b 1

echo.
echo Portable package created: %OUT%
if "%PRESERVE_RUNTIME%"=="1" echo Existing local data/history and management token were preserved when present.
echo Default launch: RUN.bat ^> StreamArchive.exe ^> shared Rust core ^> canonical SQLite.
echo Native direct launch: StreamArchive.exe
echo Web compatibility fallback: RUN_WEB.bat ^> stream-archive-launcher.exe ^> local server ^> default browser.
echo Direct Web server troubleshooting: RUN_SERVER_CONSOLE.bat
echo Included: native GUI, Web compatibility launcher/server, maintenance scripts, operations docs, license notices, Caddy template, release metadata, SHA256 checksums.
echo Backups: default to a sibling stream-archive-backups folder outside the replaceable portable package directory.
echo External tools are not bundled. Configure Streamlink, yt-dlp and ffmpeg paths or install them in PATH.
endlocal
