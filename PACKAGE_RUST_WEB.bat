@echo off
setlocal
cd /d "%~dp0"

set "SOOP_NO_PAUSE=1"
call BUILD_RUST_WEB.bat || exit /b 1

set "OUT=dist\soop-recorder"
set "PRESERVE=dist\.soop-recorder-preserve"
set "PRESERVE_RUNTIME=1"
if /I "%GITHUB_ACTIONS%"=="true" set "PRESERVE_RUNTIME=0"

if "%PRESERVE_RUNTIME%"=="1" (
    if exist "%PRESERVE%" rmdir /s /q "%PRESERVE%"
    if exist "%OUT%\data" (
        mkdir "%PRESERVE%" || exit /b 1
        powershell -NoProfile -ExecutionPolicy Bypass -Command "Copy-Item -LiteralPath '.\%OUT%\data' -Destination '.\%PRESERVE%\data' -Recurse -Force"
        if errorlevel 1 (
            echo ERROR: Failed to preserve existing runtime data. Stop soop-server.exe and retry.
            exit /b 1
        )
    )
    if exist "%OUT%\backend\.rust-web" (
        if not exist "%PRESERVE%" mkdir "%PRESERVE%" || exit /b 1
        mkdir "%PRESERVE%\backend" 2>nul
        powershell -NoProfile -ExecutionPolicy Bypass -Command "Copy-Item -LiteralPath '.\%OUT%\backend\.rust-web' -Destination '.\%PRESERVE%\backend\.rust-web' -Recurse -Force"
        if errorlevel 1 (
            echo ERROR: Failed to preserve local management token data. Stop soop-server.exe and retry.
            exit /b 1
        )
    )
)

if exist "%OUT%" rmdir /s /q "%OUT%"
if exist "%OUT%" (
    echo ERROR: Existing portable package could not be removed.
    echo Stop the SOOP server/launcher and retry PACKAGE_RUST_WEB.bat.
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

copy /y "rust-web\target\release\soop-web.exe" "%OUT%\soop-server.exe" >nul || exit /b 1
copy /y "rust-web\target\release\soop-launcher.exe" "%OUT%\soop-launcher.exe" >nul || exit /b 1
copy /y "backend\SOOP_LIVE_SETTING.example.ini" "%OUT%\backend\SOOP_LIVE_SETTING.example.ini" >nul || exit /b 1
copy /y "backend\SOOP_LIVE_CHANNELS.example.txt" "%OUT%\backend\SOOP_LIVE_CHANNELS.example.txt" >nul || exit /b 1
if exist "backend\vod\SOOP_VOD_SETTING.example.ini" copy /y "backend\vod\SOOP_VOD_SETTING.example.ini" "%OUT%\backend\vod\SOOP_VOD_SETTING.example.ini" >nul
copy /y "maintenance\Backup-SoopData.ps1" "%OUT%\maintenance\Backup-SoopData.ps1" >nul || exit /b 1
copy /y "maintenance\Restore-SoopData.ps1" "%OUT%\maintenance\Restore-SoopData.ps1" >nul || exit /b 1
copy /y "docs\OPERATIONS.md" "%OUT%\docs\OPERATIONS.md" >nul || exit /b 1
copy /y "docs\REVERSE_PROXY.md" "%OUT%\docs\REVERSE_PROXY.md" >nul || exit /b 1
copy /y "docs\LOCAL_LAUNCHER.md" "%OUT%\docs\LOCAL_LAUNCHER.md" >nul || exit /b 1
copy /y "deploy\Caddyfile.example" "%OUT%\Caddyfile.example" >nul || exit /b 1

if "%PRESERVE_RUNTIME%"=="1" (
    if exist "%PRESERVE%\data" (
        rmdir /s /q "%OUT%\data"
        powershell -NoProfile -ExecutionPolicy Bypass -Command "Copy-Item -LiteralPath '.\%PRESERVE%\data' -Destination '.\%OUT%\data' -Recurse -Force"
        if errorlevel 1 exit /b 1
    )
    if exist "%PRESERVE%\backend\.rust-web" (
        powershell -NoProfile -ExecutionPolicy Bypass -Command "Copy-Item -LiteralPath '.\%PRESERVE%\backend\.rust-web' -Destination '.\%OUT%\backend\.rust-web' -Recurse -Force"
        if errorlevel 1 exit /b 1
    )
    if exist "%PRESERVE%" rmdir /s /q "%PRESERVE%"
)

>"%OUT%\RUN.bat" echo @echo off
>>"%OUT%\RUN.bat" echo cd /d "%%~dp0"
>>"%OUT%\RUN.bat" echo start "" "soop-launcher.exe"

>"%OUT%\RUN_SERVER_CONSOLE.bat" echo @echo off
>>"%OUT%\RUN_SERVER_CONSOLE.bat" echo cd /d "%%~dp0"
>>"%OUT%\RUN_SERVER_CONSOLE.bat" echo soop-server.exe

>"%OUT%\BACKUP_DATA.bat" echo @echo off
>>"%OUT%\BACKUP_DATA.bat" echo cd /d "%%~dp0"
>>"%OUT%\BACKUP_DATA.bat" echo powershell -NoProfile -ExecutionPolicy Bypass -File ".\maintenance\Backup-SoopData.ps1" %%*

>"%OUT%\RESTORE_DATA.bat" echo @echo off
>>"%OUT%\RESTORE_DATA.bat" echo cd /d "%%~dp0"
>>"%OUT%\RESTORE_DATA.bat" echo powershell -NoProfile -ExecutionPolicy Bypass -File ".\maintenance\Restore-SoopData.ps1" %%*

powershell -NoProfile -ExecutionPolicy Bypass -File ".\maintenance\Write-ReleaseMetadata.ps1" -OutputPath ".\%OUT%\RELEASE_INFO.txt" -ManifestPath ".\rust-web\Cargo.toml"
if errorlevel 1 exit /b 1

powershell -NoProfile -ExecutionPolicy Bypass -Command "$names=@('soop-server.exe','soop-launcher.exe'); $lines=foreach($n in $names){$h=(Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path '.\%OUT%' $n)).Hash.ToLowerInvariant(); $h+'  '+$n}; $lines | Set-Content -LiteralPath '.\%OUT%\SHA256SUMS.txt' -Encoding ASCII"
if errorlevel 1 exit /b 1

echo.
echo Portable package created: %OUT%
if "%PRESERVE_RUNTIME%"=="1" echo Existing local data/history and management token were preserved when present.
echo Default launch: RUN.bat ^> soop-launcher.exe ^> local server ^> default browser.
echo Direct troubleshooting: RUN_SERVER_CONSOLE.bat
echo Included: launcher, maintenance scripts, operations docs, Caddy template, release metadata, SHA256 checksums.
echo External tools are not bundled. Configure Streamlink, yt-dlp and ffmpeg paths or install them in PATH.
endlocal
