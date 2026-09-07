@echo off
setlocal
cd /d "%~dp0"

call BUILD_RUST_WEB.bat || exit /b 1

set "OUT=dist\soop-recorder"
if exist "%OUT%" rmdir /s /q "%OUT%"
mkdir "%OUT%\backend\vod" || exit /b 1
mkdir "%OUT%\data" || exit /b 1

copy /y "rust-web\target\release\soop-web.exe" "%OUT%\soop-server.exe" >nul || exit /b 1
copy /y "backend\SOOP_LIVE_SETTING.example.ini" "%OUT%\backend\SOOP_LIVE_SETTING.example.ini" >nul || exit /b 1
copy /y "backend\SOOP_LIVE_CHANNELS.example.txt" "%OUT%\backend\SOOP_LIVE_CHANNELS.example.txt" >nul || exit /b 1
if exist "backend\vod\SOOP_VOD_SETTING.example.ini" copy /y "backend\vod\SOOP_VOD_SETTING.example.ini" "%OUT%\backend\vod\SOOP_VOD_SETTING.example.ini" >nul

>"%OUT%\RUN.bat" echo @echo off
>>"%OUT%\RUN.bat" echo cd /d "%%~dp0"
>>"%OUT%\RUN.bat" echo soop-server.exe

echo.
echo Portable package created: %OUT%
echo External tools are not bundled. Configure Streamlink, yt-dlp and ffmpeg paths or install them in PATH.
endlocal
