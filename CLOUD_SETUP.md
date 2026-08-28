# Codex cloud repository setup

This repository contains a Windows WinUI 3 desktop application.

## Secrets and local data

Never commit these local runtime files:

- `backend/SOOP_LIVE_SETTING.ini`
- `backend/SOOP_LIVE_CHANNELS.txt`
- logs, control commands, generated projects, or publish output

The build scripts create missing runtime files from the corresponding
`.example` files. Enter credentials and personal channels only in the ignored
local files.

## Codex cloud

Codex cloud can inspect and edit the C#, PowerShell, batch, and documentation
sources. Its hosted environment is Linux, so it must not be treated as the
final WinUI 3 build or runtime verifier.

Recommended workflow:

1. Make source changes in a Codex cloud branch.
2. Review the diff and open a pull request.
3. Pull the branch on Windows.
4. Run `PREPARE_PROJECT.bat`, then `BUILD_EXE.bat`.
5. Test `publish/SOOPLiveWinUI.exe` without committing local settings or output.

Repository-specific safety and verification rules are in `AGENTS.md`.
