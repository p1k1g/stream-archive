@echo off
setlocal
cd /d "%~dp0"

where cargo.exe >nul 2>nul
if errorlevel 1 (
  echo [ERROR] Rust cargo.exe was not found in PATH.
  echo Install Rust from https://rustup.rs/ and reopen this terminal.
  exit /b 1
)

cargo build --locked --release --manifest-path ".\rust-runtime\Cargo.toml"
if errorlevel 1 (
  echo.
  echo [ERROR] Stream Archive shared/headless runtime release build failed.
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
            echo ERROR: Failed to preserve existing runtime data. Stop StreamArchive.exe and the headless runtime, then retry.
            exit /b 1
        )
    )
)

if exist "%OUT%" rmdir /s /q "%OUT%"
if exist "%OUT%" (
    echo ERROR: Existing portable package could not be removed.
    echo Stop StreamArchive.exe and the headless runtime, then retry BUILD_PORTABLE.bat.
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
copy /y "rust-runtime\target\release\stream-archive-server.exe" "%OUT%\stream-archive-server.exe" >nul || exit /b 1
copy /y "maintenance\Backup-StreamArchiveData.ps1" "%OUT%\maintenance\Backup-StreamArchiveData.ps1" >nul || exit /b 1
copy /y "maintenance\Restore-StreamArchiveData.ps1" "%OUT%\maintenance\Restore-StreamArchiveData.ps1" >nul || exit /b 1
copy /y "docs\OPERATIONS.md" "%OUT%\docs\OPERATIONS.md" >nul || exit /b 1
copy /y "LICENSE" "%OUT%\LICENSE" >nul || exit /b 1
copy /y "THIRD_PARTY_NOTICES.md" "%OUT%\THIRD_PARTY_NOTICES.md" >nul || exit /b 1

if "%PRESERVE_RUNTIME%"=="1" (
    if exist "%PRESERVE%\data" (
        rmdir /s /q "%OUT%\data"
        powershell -NoProfile -ExecutionPolicy Bypass -Command "Copy-Item -LiteralPath '.\%PRESERVE%\data' -Destination '.\%OUT%\data' -Recurse -Force"
        if errorlevel 1 exit /b 1
    )
    if exist "%PRESERVE%" rmdir /s /q "%PRESERVE%"
)

>"%OUT%\RUN.bat" echo @echo off
>>"%OUT%\RUN.bat" echo cd /d "%%~dp0"
>>"%OUT%\RUN.bat" echo start "" "StreamArchive.exe"

>"%OUT%\RUN_HEADLESS.bat" echo @echo off
>>"%OUT%\RUN_HEADLESS.bat" echo cd /d "%%~dp0"
>>"%OUT%\RUN_HEADLESS.bat" echo stream-archive-server.exe

>"%OUT%\BACKUP_DATA.bat" echo @echo off
>>"%OUT%\BACKUP_DATA.bat" echo cd /d "%%~dp0"
>>"%OUT%\BACKUP_DATA.bat" echo powershell -NoProfile -ExecutionPolicy Bypass -File ".\maintenance\Backup-StreamArchiveData.ps1" %%*

>"%OUT%\RESTORE_DATA.bat" echo @echo off
>>"%OUT%\RESTORE_DATA.bat" echo cd /d "%%~dp0"
>>"%OUT%\RESTORE_DATA.bat" echo powershell -NoProfile -ExecutionPolicy Bypass -File ".\maintenance\Restore-StreamArchiveData.ps1" %%*

powershell -NoProfile -ExecutionPolicy Bypass -File ".\maintenance\Write-ReleaseMetadata.ps1" -OutputPath ".\%OUT%\RELEASE_INFO.txt" -ManifestPath ".\rust-runtime\Cargo.toml" -RepositoryRoot "."
if errorlevel 1 exit /b 1

powershell -NoProfile -ExecutionPolicy Bypass -Command "$names=@('StreamArchive.exe','stream-archive-server.exe'); $lines=foreach($n in $names){$h=(Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path '.\%OUT%' $n)).Hash.ToLowerInvariant(); $h+'  '+$n}; $lines | Set-Content -LiteralPath '.\%OUT%\SHA256SUMS.txt' -Encoding ASCII"
if errorlevel 1 exit /b 1

echo.
echo Portable package created: %OUT%
if "%PRESERVE_RUNTIME%"=="1" echo Existing local SQLite data/history were preserved when present.
echo Default launch: RUN.bat ^> StreamArchive.exe ^> shared Rust core ^> canonical SQLite.
echo Native direct launch: StreamArchive.exe
echo Optional headless runtime: RUN_HEADLESS.bat ^> stream-archive-server.exe ^> shared Rust core.
echo Included: native GUI, headless runtime, maintenance scripts, operations docs, license notices, release metadata, SHA256 checksums.
echo Backups: default to a sibling stream-archive-backups folder outside the replaceable portable package directory.
echo External tools are not bundled. Configure Streamlink, yt-dlp and ffmpeg paths or install them in PATH.
endlocal
