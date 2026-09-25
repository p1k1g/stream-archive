# Stream Archive Operations

Operational guidance for the current Rust/SQLite runtime.

## Source of truth

The primary database is `data/stream-archive.db` unless `STREAM_ARCHIVE_DATA_DIR` is set. Settings, channels, encrypted secrets, VOD queue state, LIVE history, VOD history, and backup policy are stored there.

Runtime INI/TXT mirrors are retired. Do not create or edit `SOOP_LIVE_SETTING.ini`, `SOOP_LIVE_CHANNELS.txt`, or `SOOP_VOD_SETTING.ini` as application configuration.

For users upgrading from an earlier private build, startup performs only a bounded database filename migration: if `stream-archive.db` does not exist but `soop.db` does, the database is copied into the new canonical filename and the old database file is removed after the SQLite backup completes successfully.

## Online backup

The native Slint UI keeps administration under one top-level **설정** page. **설정 -> 일반** contains provider/runtime configuration, while **설정 -> 관리** owns the complete backup surface: automatic-backup policy, managed backup creation/list/integrity/restore, Diagnostics, and Runtime Logs. These views use the shared BackupManager/StreamArchiveCore service and canonical SQLite policy.

Defaults:

- backup enabled;
- every 24 hours;
- keep at most 10 managed backups;
- remove managed backups older than 3 days;
- default directory is the sibling `stream-archive-backups` folder outside the replaceable portable package directory.

Set `STREAM_ARCHIVE_BACKUP_DIR` to force a specific backup directory. When the environment override is present the Native backup-directory field is read-only, while retention settings remain editable.

Managed backup names use the `stream_archive_*.db` prefix and include companion `.db.json` metadata. Files without valid metadata are not automatically pruned.

## Native storage status

The Native LIVE page reads storage diagnostics through `StreamArchiveCore` and the shared `storage_service`; it does not use localhost HTTP or probe filesystems from Slint. The snapshot includes the configured `OUTPUT_DIR`, per-channel output-directory overrides, and the canonical SQLite data directory. Paths on the same Windows volume are collapsed into one row.

Storage state uses the shared runtime thresholds:

- **정상 / OK**: free space is greater than twice `MIN_FREE_SPACE_GB`;
- **주의 / WARN**: free space is at or below twice the threshold but above the threshold;
- **공간 부족 / CRITICAL**: free space is at or below `MIN_FREE_SPACE_GB`.

The recorder's actual low-space stop/start boundary remains `MIN_FREE_SPACE_GB`; the warning band is presentation only. Storage refresh happens on initial Native load, when returning to LIVE, on explicit refresh, and on a bounded LIVE-only timer.

## CHZZK destination claim sidecars

CHZZK VOD publication uses a `.stream-archive.claim` sidecar beside the intended final media pathname as a reusable file-lock anchor. The claim pathname intentionally survives job completion; deleting it immediately after unlock can let concurrent contenders lock different file identities and weaken no-clobber guarantees.

On Windows, Stream Archive marks these internal claim files with the Hidden attribute so they do not normally appear in Explorer while hidden items are disabled. The lock location and reuse semantics are unchanged. Existing claim files are hidden the next time that destination is claimed. `.stream-archive.finalizing` remains a temporary publication artifact and is reclaimed/removed by the existing completion, cancellation, and stale-recovery paths.

## Offline manual backup

For an offline maintenance backup, close `StreamArchive.exe` and stop any optional headless `stream-archive-server.exe` runtime cleanly first. Do not kill unrelated `streamlink`, `ffmpeg`, or `yt-dlp` processes.

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

Restore is destructive to the active authoritative database and therefore requires the watcher, active VOD work, and VOD queue to be stopped. `StreamArchiveCore::restore_backup` enforces these runtime conditions and creates a `pre_restore` safety backup first.

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


## Upgrade procedure

Before replacing any package:

1. Stop the foreground watcher/runtime cleanly.
2. Create a database backup.
3. Keep the previous package/archive until the new version has been exercised.
4. Verify the new archive checksum before extraction.
5. Keep the existing runtime data directory separate from the replacement package files.
6. Start the new package and run diagnostics before resuming unattended work.

### Windows portable upgrade

Official Windows release ZIPs are clean packages. They contain an empty `data\` directory and must not be extracted over the only copy of a live database.

1. Stop `StreamArchive.exe` and any optional `stream-archive-server.exe`.
2. Back up `data\stream-archive.db`.
3. Extract the new ZIP to a new package directory.
4. Preserve or explicitly point `STREAM_ARCHIVE_DATA_DIR` at the existing data directory.
5. Launch `StreamArchive.exe` / `RUN.bat`.
6. Verify Diagnostics, Channels, LIVE start/stop, VOD analyze/download, Queue and History.
7. If headless operation is used, verify `RUN_HEADLESS.bat` separately.

Local developer `BUILD_PORTABLE.bat` rebuilds may preserve an existing `dist\stream-archive\data` directory. This convenience is distinct from the official CI/release artifact contract, which requires clean runtime data.

### Linux/macOS portable upgrade

The Phase 23.6 Unix archives contain `bin/`, `backend/`, `data/`, docs, release metadata and checksums. The bundled `data/` directory is intentionally empty.

1. Stop `stream-archive-cli serve --watch` or `stream-archive-server`.
2. Back up the canonical SQLite database.
3. Verify the archive-level `.sha256` file.
4. Extract the new archive to a new directory instead of overwriting the current package in place.
5. Reuse the existing data directory with `STREAM_ARCHIVE_DATA_DIR`, or copy only after a verified backup.
6. Run `./bin/stream-archive-cli doctor --active-tools`.
7. Start the runtime and verify status/Queue/History before unattended operation.

When using package-local defaults, run commands from the extracted `stream-archive/` root so `backend/` and `data/` resolve to that package. For long-lived installs, explicitly separating package binaries, runtime data and backup directories with environment overrides is safer.

### Rollback

Close the new runtime before rollback. Restore the previous package first. Restore a pre-upgrade database backup only when required by the actual database state; do not assume arbitrary schema downgrades are supported.

## Portable package replacement

`BUILD_PORTABLE.bat` writes to `dist\stream-archive`. The package contains `StreamArchive.exe` as the default native application, `RUN.bat` as the native launcher, and `stream-archive-server.exe` plus `RUN_HEADLESS.bat` as the optional compatible headless runtime path. Browser launcher, Web static assets and reverse-proxy artifacts are not packaged. For local rebuilds the existing `data` directory is preserved before package replacement; GitHub Actions builds use a clean package.

Direct Explorer launch is supported: backend resolution prefers the `backend` directory beside `StreamArchive.exe`, and the default SQLite path is the sibling `data\stream-archive.db`. Environment overrides still take precedence where defined.

Linux/macOS release archives are built with `BUILD_UNIX_PACKAGE.sh` and contain `bin/stream-archive-cli` plus `bin/stream-archive-server`. They do not install systemd/launchd services or package-manager entries.

Do not copy old INI/TXT configuration files into a new package. SQLite is the only runtime configuration source.

## Release package verification

Phase 23.6 keeps package verification separate from package assembly.

Windows:

```powershell
.\BUILD_PORTABLE.bat
powershell -ExecutionPolicy Bypass -File .\maintenance\Verify-WindowsPackage.ps1 -Root .\dist\stream-archive -RequireCleanData
powershell -ExecutionPolicy Bypass -File .\maintenance\New-WindowsReleaseArchive.ps1 -PackageRoot .\dist\stream-archive -OutputDir .\dist\release
```

`New-WindowsReleaseArchive.ps1` also enforces the same clean-data verifier internally before it opens the output ZIP. A local `BUILD_PORTABLE.bat` tree that preserved an existing database therefore cannot be turned into an official-looking release archive until runtime data is removed from the staging tree. CI/release validation also verifies the generated ZIP plus its archive-level `.sha256`.

Linux/macOS:

```bash
./BUILD_UNIX_PACKAGE.sh
./maintenance/Verify-UnixPackage.sh ./dist/unix-<platform>-<arch>/stream-archive ./dist/release/stream-archive-<platform>-<arch>.tar.gz
```

The verifier checks required files, executable bits, clean runtime data, package-local `SHA256SUMS.txt`, archive checksum, forbidden legacy/runtime artifacts, and executes the CLI from a fresh extraction path containing whitespace and non-ASCII characters.

Streamlink, yt-dlp and FFmpeg remain external dependencies and are not redistributed in any Phase 23.6 package.

## Runtime environment overrides

- `STREAM_ARCHIVE_START_WATCHER`: optional headless-runtime watcher auto-start flag.
- `STREAM_ARCHIVE_BACKEND_DIR`: explicit backend directory.
- `STREAM_ARCHIVE_DATA_DIR`: explicit SQLite data directory.
- `STREAM_ARCHIVE_BACKUP_DIR`: explicit managed backup directory.

Provider-specific account settings such as `SOOP_USERNAME`, `SOOP_PASSWORD`, `CHZZK_NID_AUT`, and `CHZZK_NID_SES` remain provider-scoped settings stored in SQLite.

## Incident notes

The process-lifecycle invariant is strict: the application may terminate only child-process trees it created and owns. Never use broad `taskkill /IM ffmpeg.exe`, `taskkill /IM streamlink.exe`, `taskkill /IM yt-dlp.exe`, or equivalent process-name cleanup.

If a package rebuild or restore fails because a file is locked, stop Stream Archive cleanly and retry rather than terminating unrelated media-tool processes.
