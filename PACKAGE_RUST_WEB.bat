@echo off
setlocal
cd /d "%~dp0"

set "SOOP_NO_PAUSE=1"
call BUILD_RUST_WEB.bat || exit /b 1

set "OUT=dist\soop-recorder"
if exist "%OUT%" rmdir /s /q "%OUT%"
mkdir "%OUT%\backend\vod" || exit /b 1
mkdir "%OUT%\data" || exit /b 1
mkdir "%OUT%\maintenance" || exit /b 1
mkdir "%OUT%\docs" || exit /b 1

copy /y "rust-web\target\release\soop-web.exe" "%OUT%\soop-server.exe" >nul || exit /b 1
copy /y "backend\SOOP_LIVE_SETTING.example.ini" "%OUT%\backend\SOOP_LIVE_SETTING.example.ini" >nul || exit /b 1
copy /y "backend\SOOP_LIVE_CHANNELS.example.txt" "%OUT%\backend\SOOP_LIVE_CHANNELS.example.txt" >nul || exit /b 1
if exist "backend\vod\SOOP_VOD_SETTING.example.ini" copy /y "backend\vod\SOOP_VOD_SETTING.example.ini" "%OUT%\backend\vod\SOOP_VOD_SETTING.example.ini" >nul
copy /y "maintenance\Backup-SoopData.ps1" "%OUT%\maintenance\Backup-SoopData.ps1" >nul || exit /b 1
copy /y "maintenance\Restore-SoopData.ps1" "%OUT%\maintenance\Restore-SoopData.ps1" >nul || exit /b 1
copy /y "docs\OPERATIONS.md" "%OUT%\docs\OPERATIONS.md" >nul || exit /b 1
copy /y "docs\REVERSE_PROXY.md" "%OUT%\docs\REVERSE_PROXY.md" >nul || exit /b 1

>"%OUT%\RUN.bat" echo @echo off
>>"%OUT%\RUN.bat" echo cd /d "%%~dp0"
>>"%OUT%\RUN.bat" echo soop-server.exe

>"%OUT%\BACKUP_DATA.bat" echo @echo off
>>"%OUT%\BACKUP_DATA.bat" echo cd /d "%%~dp0"
>>"%OUT%\BACKUP_DATA.bat" echo powershell -NoProfile -ExecutionPolicy Bypass -File ".\maintenance\Backup-SoopData.ps1" %%*

>"%OUT%\RESTORE_DATA.bat" echo @echo off
>>"%OUT%\RESTORE_DATA.bat" echo cd /d "%%~dp0"
>>"%OUT%\RESTORE_DATA.bat" echo powershell -NoProfile -ExecutionPolicy Bypass -File ".\maintenance\Restore-SoopData.ps1" %%*

powershell -NoProfile -ExecutionPolicy Bypass -Command "$m=Get-Content '.\rust-web\Cargo.toml'; $v=($m | Select-String '^version\s*=\s*\"(.+)\"' | Select-Object -First 1).Matches.Groups[1].Value; $sha='unknown'; if(Get-Command git -ErrorAction SilentlyContinue){$sha=(git rev-parse --short=12 HEAD 2>$null)}; @('product=SOOP Downloader','version='+$v,'commit='+$sha,'built_at='+(Get-Date).ToString('o')) | Set-Content -LiteralPath '.\%OUT%\RELEASE_INFO.txt' -Encoding UTF8"
if errorlevel 1 exit /b 1

powershell -NoProfile -ExecutionPolicy Bypass -Command "$h=(Get-FileHash -Algorithm SHA256 -LiteralPath '.\%OUT%\soop-server.exe').Hash.ToLowerInvariant(); ($h+'  soop-server.exe') | Set-Content -LiteralPath '.\%OUT%\SHA256SUMS.txt' -Encoding ASCII"
if errorlevel 1 exit /b 1

echo.
echo Portable package created: %OUT%
echo Included: maintenance scripts, operations docs, release metadata, SHA256 checksum.
echo External tools are not bundled. Configure Streamlink, yt-dlp and ffmpeg paths or install them in PATH.
endlocal
