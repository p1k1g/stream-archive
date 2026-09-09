# SOOP Downloader Operations

Phase 6 operational guidance for the Rust/SQLite runtime.

## Source of truth

The primary database is `data/soop.db` unless `SOOP_DATA_DIR` is set. Settings, channels, encrypted secrets, LIVE history, and VOD history are stored there.

The files under `backend/` are compatibility mirrors generated from SQLite. Do not use them as the primary backup after the Phase 5.2 cutover.

## Backup

For a consistent portable backup, stop the SOOP server first with `Ctrl+C`. Do not kill unrelated `streamlink`, `ffmpeg`, or `yt-dlp` processes.

From the repository root or portable package root:

```powershell
powershell -ExecutionPolicy Bypass -File .\maintenance\Backup-SoopData.ps1
```

The script:

- refuses to run while `soop-server` or `soop-web` is active;
- validates the SQLite header;
- writes a timestamped `.db` backup under `data/backups` by default;
- writes a companion JSON file with size and SHA-256;
- keeps the newest 10 backups by default.

Custom retention:

```powershell
powershell -ExecutionPolicy Bypass -File .\maintenance\Backup-SoopData.ps1 -Keep 30
```

Custom data directory:

```powershell
powershell -ExecutionPolicy Bypass -File .\maintenance\Backup-SoopData.ps1 -DataDir D:\SOOP_DATA
```

## Restore

Stop the server before restore. Restore replaces the authoritative SQLite database, so it should never run concurrently with the server.

```powershell
powershell -ExecutionPolicy Bypass -File .\maintenance\Restore-SoopData.ps1 -BackupFile .\data\backups\soop_YYYYMMDD_HHMMSS.db
```

The restore script:

- refuses to run while the SOOP server is active;
- validates the SQLite header;
- verifies SHA-256 when the companion JSON exists;
- makes a `pre_restore_*.db` safety copy of the current database;
- removes stale `-wal` and `-shm` sidecars;
- copies through a temporary file before replacing `soop.db`.

After restore, launch the server and verify settings, channel list, LIVE history, and VOD history before resuming unattended operation.

## Backup retention

`Backup-SoopData.ps1 -Keep N` controls local database-backup retention. `0` disables pruning. This is separate from LIVE/VOD history retention inside SQLite.

## Upgrade procedure

1. Stop the running server with `Ctrl+C`.
2. Run a database backup.
3. Keep the previous portable package until the new version has been exercised.
4. Replace the executable/package files, but keep the existing `data` directory.
5. Start the new server and verify `/api/status`, settings, channels, history, LIVE start/stop, and VOD analyze/download.
6. Roll back by stopping the new server, restoring the previous package, and restoring the pre-upgrade database backup if necessary.

## Incident notes

The process-lifecycle invariant remains unchanged: the application may terminate only child-process trees it owns. Never use broad `taskkill /IM ffmpeg.exe`, `taskkill /IM streamlink.exe`, or similar commands as operational cleanup.


## Phase 12 backup / retention

- Web UI: Settings -> Backup.
- Default backup directory: sibling `soop-recorder-backups` next to the portable application folder, not inside `data`.
- Override with `SOOP_BACKUP_DIR`.
- Automatic defaults: enabled, every 24 hours, keep 10, remove backups older than 30 days.
- SQLite backups use the online backup API and may be created while LIVE recording is active.
- Restore requires Watcher and VOD to be stopped. A `pre_restore` safety backup is created first.
- Restore invalidates all browser sessions so an old database cannot resurrect a previously valid session.
- `BACKUP_DATA.bat` remains available for offline/manual maintenance and uses the same external backup location.
