@echo off
setlocal
cd /d "%~dp0"

echo ========================================
echo  SOOP LIVE built EXE locations
echo ========================================
echo.

for /r ".\.generated\SOOPLiveWinUI\bin" %%F in (*.exe) do (
  echo %%F
)

echo.
echo Note:
echo RUN_SOURCE.bat uses dotnet run, so the executable is built under
echo .generated\SOOPLiveWinUI\bin\..., not in the project root.
echo.
pause
