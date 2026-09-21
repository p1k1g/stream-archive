# Stream Archive Operations

Operational guidance for the current Rust/SQLite runtime.

## Source of truth

The primary database is `data/stream-archive.db` unless `STREAM_ARCHIVE_DATA_DIR` is set. Settings, channels, encrypted secrets, VOD queue state, LIVE history, VOD history, and backup policy are stored there.

Runtime INI/TXT mirrors are retired. Do not create or edit `SOOP_LIVE_SETTING.ini`, `SOOP_LIVE_CHANNELS.txt`, or `SOOP_VOD_SETTING.ini` as application configuration.

For users upgrading from an earlier private build, startup performs only a bounded database filename migration: if `stream-archive.db` does not exist but `soop.db` does, the database is copied into the new canonical filename and the old database file is removed after the SQLite backup completes successfully.

## Online backup

The native Slint UI keeps administration under one top-level **설정** page. **설정 -> 일반** contains provider/runtime configuration, while **설정 -> 관리** owns the complete backup surface: automatic-backup policy, managed backup creation/list/integrity/restore, Diagnostics, and Runtime Logs. These views use the same BackupManager/StreamArchiveCore service and canonical SQLite policy. The retained Web UI exposes the same managed backup behavior through its compatibility adapter.

Defaults:

- backup enabled;
- every 24 hours;
- keep at most 10 managed backups;
- remove managed backups older than 3 days;
- default directory is the sibling `stream-archive-backups` folder outside the replaceable portable package directory.

Set `STREAM_ARCHIVE_BACKUP_DIR` to force a specific backup directory. When the environment override is present the directory field in the Web UI is read-only, while retention settings remain editable.

Managed backup names use the `stream_archive_*.db` prefix and include companion `.db.json` metadata. Files without valid metadata are not automatically pruned.

## Native storage status

The Native LIVE page reads storage diagnostics through `StreamArchiveCore` and the shared `storage_service`; it does not call the Web API or probe filesystems from Slint. The snapshot includes the configured `OUTPUT_DIR`, per-channel output-directory overrides, and the canonical SQLite data directory. Paths on the same Windows volume are collapsed into one row.

Storage state follows the existing Web meaning:

- **정상 / OK**: free space is greater than twice `MIN_FREE_SPACE_GB`;
- **주의 / WARN**: free space is at or below twice the threshold but above the threshold;
- **공간 부족 / CRITICAL**: free space is at or below `MIN_FREE_SPACE_GB`.

The recorder's actual low-space stop/start boundary remains `MIN_FREE_SPACE_GB`; the warning band is presentation only. Storage refresh happens on initial Native load, when returning to LIVE, on explicit refresh, and on a bounded LIVE-only timer.

## CHZZK destination claim sidecars

CHZZK VOD publication uses a `.stream-archive.claim` sidecar beside the intended final media pathname as a reusable file-lock anchor. The claim pathname intentionally survives job completion; deleting it immediately after unlock can let concurrent contenders lock different file identities and weaken no-clobber guarantees.

On Windows, Stream Archive marks these internal claim files with the Hidden attribute so they do not normally appear in Explorer while hidden items are disabled. The lock location and reuse semantics are unchanged. Existing claim files are hidden the next time that destination is claimed. `.stream-archive.finalizing` remains a temporary publication artifact and is reclaimed/removed by the existing completion, cancellation, and stale-recovery paths.

## Offline manual backup

For an offline maintenance backup, close `StreamArchive.exe` and stop any Web compatibility `stream-archive-server.exe` cleanly first. Do not kill unrelated `streamlink`, `ffmpeg`, or `yt-dlp` processes.

From the repository root or portable package root:

```powershell
powershell -ExecutionPolicy Bypass -File .\maintenance\Backup-StreamArchiveData.ps1
```

The script:

- refuses to run while `StreamArchive.exe` or `stream-archive-server.exe` is active;
- validates the SQLite header;
- backs up `stream-archive.db`;
- writes a timestamped `stream_archive_manual_*.db` file;
- writes companion JSON metadata with size and SHA-256;
- uses the sibling `stream-archive-backups` directory by default;
- keeps the newest 10 managed backups by default.

Custom retention:

```powershell
powershell -ExecutionPolicy Bypass -File .\maintenance\Backup-StreamArchiveData.ps1 -Keep 30 -RetentionDays 30
```

Custom data directory:

```powershell
powershell -ExecutionPolicy Bypass -File .\maintenance\Backup-StreamArchiveData.ps1 -DataDir D:\StreamArchiveData
```

## Restore

Restore is destructive to the active authoritative database and therefore requires the watcher, active VOD work, and VOD queue to be stopped. The Web restore path enforces these runtime conditions and creates a `pre_restore` safety backup first.

Offline restore:

```powershell
powershell -ExecutionPolicy Bypass -File .\maintenance\Restore-StreamArchiveData.ps1 -BackupFile ..\stream-archive-backups\stream_archive_manual_YYYYMMDD_HHMMSS.db
```

The restore script:

- refuses to run while `StreamArchive.exe` or `stream-archive-server.exe` is active;
- validates the SQLite header;
- verifies SHA-256 when companion metadata exists;
- creates a `pre_restore_*.db` safety copy of the current database;
- removes stale `-wal` and `-shm` sidecars;
- copies through a temporary file before replacing `stream-archive.db`.

After restore, launch `StreamArchive.exe` and verify settings, channels, LIVE history, VOD history, and queue state before resuming unattended operation.

The retained Web restore path also reinitializes authentication and invalidates all browser sessions so restoring an older database cannot resurrect an old session.

## Upgrade procedure

1. Close the native application and stop any Web compatibility server.
2. Create a database backup.
3. Keep the previous portable package until the new version has been exercised.
4. Replace executable/package files while preserving the existing `data` directory.
5. Start `StreamArchive.exe` or `RUN.bat` and verify Settings (including the nested 관리 view), Channels, LIVE start/stop, VOD analyze/download, and Queue/History.
6. Run `RUN_WEB.bat` only when the compatibility browser path needs regression verification.
7. Roll back by closing the new application, restoring the previous package, and restoring the pre-upgrade database backup if necessary.

## Portable package replacement

`BUILD_PORTABLE.bat` writes to `dist\stream-archive`. The package contains `StreamArchive.exe` as the default native application, `RUN.bat` as the native launcher, and `RUN_WEB.bat` plus the existing server/launcher as the explicit Web compatibility path. For local rebuilds it preserves the existing package's `data` directory and `backend\.stream-archive` management-token directory before replacing package files, then restores them into the rebuilt package. GitHub Actions builds use a clean package instead.

Direct Explorer launch is supported: backend resolution prefers the `backend` directory beside `StreamArchive.exe`, and the default SQLite path is the sibling `data\stream-archive.db`. Environment overrides still take precedence where defined.

Do not copy old INI/TXT configuration files into a new package. SQLite is the only runtime configuration source.

## Runtime environment overrides

- `STREAM_ARCHIVE_BIND`: Axum listener, default `127.0.0.1:8787`.
- `STREAM_ARCHIVE_TOKEN`: optional fixed recovery/management token.
- `STREAM_ARCHIVE_START_WATCHER`: watcher auto-start flag.
- `STREAM_ARCHIVE_BACKEND_DIR`: explicit backend directory.
- `STREAM_ARCHIVE_DATA_DIR`: explicit SQLite data directory.
- `STREAM_ARCHIVE_BACKUP_DIR`: explicit managed backup directory.

Provider-specific account settings such as `SOOP_USERNAME`, `SOOP_PASSWORD`, `CHZZK_NID_AUT`, and `CHZZK_NID_SES` remain provider-scoped settings stored in SQLite.

## Incident notes

The process-lifecycle invariant is strict: the application may terminate only child-process trees it created and owns. Never use broad `taskkill /IM ffmpeg.exe`, `taskkill /IM streamlink.exe`, `taskkill /IM yt-dlp.exe`, or equivalent process-name cleanup.

If a package rebuild or restore fails because a file is locked, stop Stream Archive cleanly and retry rather than terminating unrelated media-tool processes.
