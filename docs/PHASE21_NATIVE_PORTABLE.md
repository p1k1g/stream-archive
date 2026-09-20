# Phase 21.8 — Windows Native Portable / Startup Cutover

## Goal

The Windows portable package now uses the Slint desktop as the default user-facing entry point while retaining the Axum/browser launcher as an explicit compatibility fallback.

Default flow:

```text
RUN.bat
  -> StreamArchive.exe
  -> StreamArchiveCore
  -> data/stream-archive.db
```

Compatibility flow:

```text
RUN_WEB.bat
  -> stream-archive-launcher.exe
  -> stream-archive-server.exe
  -> browser
```

`StreamArchive.exe` resolves the portable `backend` directory beside its own executable before falling back to the process working directory. The canonical database remains `data/stream-archive.db` unless `STREAM_ARCHIVE_DATA_DIR` overrides it.

## Manual Windows QA

Build from the repository root:

```powershell
.\BUILD_PORTABLE.bat
```

Perform the following checks against `dist\stream-archive`, not the raw Cargo target directories.

1. Confirm `StreamArchive.exe`, `RUN.bat`, `RUN_WEB.bat`, `backend\`, and `data\` exist.
2. Double-click `StreamArchive.exe` directly from Explorer.
3. Confirm the native UI opens without requiring a repository-root PowerShell working directory.
4. Confirm Settings shows the expected backend and canonical SQLite database locations.
5. Close the app, then double-click `RUN.bat`; confirm it opens the same native UI.
6. Confirm existing settings, channels, secrets/configured-state indicators, Queue, and History are loaded from the existing `data\stream-archive.db`.
7. On LIVE, verify the channel list and Watcher start/stop behavior.
8. On VOD, analyze a supported URL and verify quality/PART selection.
9. Verify Queue and History refresh/action behavior.
10. Verify Maintenance Backup, Restore safety gating, Diagnostics, and bounded Runtime Logs.
11. If a LIVE/VOD process was started during QA, close the app cleanly and verify only application-owned child processes are cleaned up.
12. Restart `StreamArchive.exe` and verify SQLite-backed state remains intact.
13. Run `RUN_WEB.bat` and confirm the retained Web launcher/server path still opens the browser UI.
14. If used, verify `RUN_SERVER_CONSOLE.bat` still starts the Web compatibility server directly.

## Portable rebuild/data preservation

A local `BUILD_PORTABLE.bat` rebuild preserves the existing package `data` directory and `backend\.stream-archive` Web-management state before replacing package files, then restores them. GitHub Actions uses a clean package.

Do not copy legacy INI/TXT configuration files into the package. SQLite remains the only runtime configuration authority.

## Deferred

This phase does not remove Axum/browser code, bundle Streamlink/yt-dlp/FFmpeg, add Linux/macOS GUI packaging, or add new native UX features such as LIVE storage-capacity display.
