# SOOP Rust Web - Phase 5

Phase 5 is the productization stage after LIVE and VOD orchestration moved to Rust.

```text
Browser
  -> Axum Web/API
     -> Rust NativeWatcher -> RecorderManager -> Streamlink
     -> Rust VodManager -> yt-dlp / ffmpeg
     -> SQLite Store -> data/soop.db
```

The application remains **manual-start only**. No Windows Service, scheduled task, systemd unit, or OS auto-start entry is created.

## SQLite persistence

Phase 5 creates `data/soop.db` by default. Set `SOOP_DATA_DIR` to override the data directory.

SQLite stores:

- safe setting snapshots
- channel snapshots
- LIVE recording history
- VOD job history
- migration metadata

The database uses WAL mode and marks unfinished LIVE/VOD rows as `INTERRUPTED` on the next server start.

### Safe migration strategy

Phase 5 intentionally uses a compatibility period instead of removing the working INI/TXT flow immediately:

```text
SOOP_LIVE_SETTING.ini ----\
SOOP_LIVE_CHANNELS.txt ----+--> idempotent startup snapshot --> SQLite
SOOP_VOD_SETTING.ini -----/

Web save
  -> existing file write
  -> SQLite sync
```

The NativeWatcher continues reading the legacy-compatible files, so hot reload and manual editing remain available while SQLite history is validated. DPAPI secrets are not copied into the normal settings table.

## History

The Web UI now has a History section backed by:

```text
GET /api/history
```

It shows recent LIVE recordings and VOD jobs, including status, start time, duration, file size/output path and stop/failure reason.

LIVE history is written directly by `RecorderManager` when Streamlink starts and finishes. VOD status is persisted by the API status lifecycle.

## VOD tool settings

`YT_DLP_PATH` and `FFMPEG_PATH` are shown in the normal Settings area. They remain stored in `backend/vod/SOOP_VOD_SETTING.ini` during the compatibility period and are also mirrored into SQLite.

Resolution order remains:

```text
saved YT_DLP_PATH / FFMPEG_PATH
  -> backend/vod local executable
  -> PATH
```

## Portable package

Build a local portable directory with:

```powershell
.\PACKAGE_RUST_WEB.bat
```

Output:

```text
dist/soop-recorder/
  soop-server.exe
  RUN.bat
  backend/
    SOOP_LIVE_SETTING.example.ini
    SOOP_LIVE_CHANNELS.example.txt
    vod/SOOP_VOD_SETTING.example.ini
  data/
```

Streamlink, yt-dlp and ffmpeg are intentionally not bundled. Configure their paths in Settings or install them in PATH.

## Run

Development/repository run:

```powershell
.\RUN_RUST_WEB.bat
```

Portable package:

```powershell
.\RUN.bat
```

Default address:

```text
http://127.0.0.1:8787
```

Optional watcher auto-start after manually starting the server:

```powershell
$env:SOOP_START_WATCHER="Y"
.\RUN_RUST_WEB.bat
```

For temporary LAN testing:

```powershell
$env:SOOP_WEB_BIND="0.0.0.0:8787"
.\RUN_RUST_WEB.bat
```

Use HTTPS/reverse proxy before exposing the management endpoint to the Internet.

## Phase 5 validation

Before removing legacy source files, validate:

1. Existing INI/TXT settings and channels migrate into `data/soop.db` without changing runtime behavior.
2. Settings and channel saves still hot-reload correctly.
3. Start/stop a LIVE recording and confirm a History row records duration, size and reason.
4. Complete and cancel VOD jobs and confirm History state updates.
5. Restart the server during an active job and confirm the previous row becomes `INTERRUPTED`.
6. Build `PACKAGE_RUST_WEB.bat` and run the produced `soop-server.exe` with the packaged backend examples.

## Legacy retirement boundary

The old PowerShell/WinUI implementation remains in the repository for one final regression window. The Rust LIVE/VOD paths do not execute those PowerShell scripts.

After Phase 5 runtime/history validation, the next cleanup can remove:

```text
backend/SOOP_LIVE.ps1
backend/modules/*
backend/vod PowerShell modules
overlay/*
legacy WinUI build workflow
```

The compatibility INI/TXT files can then become import/export files instead of the primary watcher configuration source.
