@echo off
setlocal
cd /d "%~dp0"

where cargo.exe >nul 2>nul
if errorlevel 1 (
  echo [ERROR] Rust cargo.exe was not found in PATH.
  echo Install Rust from https://rustup.rs/ and reopen this terminal.
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
        "rust-gui\target\release\stream-archive-gui.exe" --copy-dir ".\%OUT%\data" ".\%PRESERVE%\data"
        if errorlevel 1 (
            echo ERROR: Failed to preserve existing runtime data. Stop StreamArchive.exe, then retry.
            exit /b 1
        )
    )
)

if exist "%OUT%" rmdir /s /q "%OUT%"
if exist "%OUT%" (
    echo ERROR: Existing portable package could not be removed.
    echo Stop StreamArchive.exe, then retry BUILD_PORTABLE.bat.
    if "%PRESERVE_RUNTIME%"=="1" if exist "%PRESERVE%\data" (
        if not exist "%OUT%\data" mkdir "%OUT%\data" 2>nul
        "rust-gui\target\release\stream-archive-gui.exe" --copy-dir ".\%PRESERVE%\data" ".\%OUT%\data" >nul 2>nul
    )
    exit /b 1
)

mkdir "%OUT%\backend\vod" || exit /b 1
mkdir "%OUT%\data" || exit /b 1
mkdir "%OUT%\docs" || exit /b 1

copy /y "rust-gui\target\release\stream-archive-gui.exe" "%OUT%\StreamArchive.exe" >nul || exit /b 1
copy /y "docs\OPERATIONS.md" "%OUT%\docs\OPERATIONS.md" >nul || exit /b 1
copy /y "LICENSE" "%OUT%\LICENSE" >nul || exit /b 1
copy /y "THIRD_PARTY_NOTICES.md" "%OUT%\THIRD_PARTY_NOTICES.md" >nul || exit /b 1

if "%PRESERVE_RUNTIME%"=="1" (
    if exist "%PRESERVE%\data" (
        rmdir /s /q "%OUT%\data"
        "%OUT%\StreamArchive.exe" --copy-dir ".\%PRESERVE%\data" ".\%OUT%\data"
        if errorlevel 1 exit /b 1
    )
    if exist "%PRESERVE%" rmdir /s /q "%PRESERVE%"
)

>"%OUT%\RUN.bat" echo @echo off
>>"%OUT%\RUN.bat" echo cd /d "%%~dp0"
>>"%OUT%\RUN.bat" echo start "" "StreamArchive.exe"

>"%OUT%\BACKUP_DATA.bat" echo @echo off
>>"%OUT%\BACKUP_DATA.bat" echo cd /d "%%~dp0"
>>"%OUT%\BACKUP_DATA.bat" echo "StreamArchive.exe" --backup %%*

>"%OUT%\RESTORE_DATA.bat" echo @echo off
>>"%OUT%\RESTORE_DATA.bat" echo cd /d "%%~dp0"
>>"%OUT%\RESTORE_DATA.bat" echo "StreamArchive.exe" --restore %%*

"%OUT%\StreamArchive.exe" --write-release-metadata --output-path ".\%OUT%\RELEASE_INFO.txt" --manifest-path ".\rust-runtime\Cargo.toml" --repository-root "."
if errorlevel 1 exit /b 1

"%OUT%\StreamArchive.exe" --write-sha256sums --root ".\%OUT%"
if errorlevel 1 exit /b 1

echo.
echo Portable package created: %OUT%
for %%F in ("%OUT%\StreamArchive.exe") do echo StreamArchive.exe bytes: %%~zF
if "%PRESERVE_RUNTIME%"=="1" echo Existing local SQLite data/history were preserved when present.
echo Default launch: RUN.bat ^> StreamArchive.exe ^> shared Rust core ^> canonical SQLite.
echo Native direct launch: StreamArchive.exe
echo Windows package surface: Native GUI only.
echo Included: native GUI, offline backup/restore launchers, operations docs, license notices, release metadata, SHA256 checksums.
echo Backups: default to a sibling stream-archive-backups folder outside the replaceable portable package directory.
echo External tools are not bundled. Configure Streamlink, yt-dlp and ffmpeg paths or install them in PATH.
endlocal
