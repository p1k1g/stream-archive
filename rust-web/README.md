# SOOP Rust Web — Phase 6

Phase 6 keeps the Rust/SQLite runtime from Phase 5.2 and hardens build, packaging, backup/restore, and reverse-proxy deployment.

## Runtime

```text
Browser
  -> Axum server
     -> SQLite data/soop.db
        -> settings / channels / encrypted secrets / history
     -> NativeWatcherManager
        -> RecorderManager -> streamlink
     -> VodManager -> yt-dlp / ffmpeg
```

The server is manually launched. It is not registered as a Windows Service or systemd unit.

## Run

From the repository root:

```powershell
.\RUN_RUST_WEB.bat
```

Default endpoint:

```text
http://127.0.0.1:8787
```

The management token is printed at startup and stored under `backend/.rust-web/web-token.txt` unless `SOOP_WEB_TOKEN` is supplied.

Press `Ctrl+C` to stop the server. Graceful shutdown cancels VOD work and stops the native watcher/owned recorder child processes.

## SQLite primary configuration

Default database:

```text
data/soop.db
```

Override the data directory with `SOOP_DATA_DIR`.

At the first Phase 5.2-or-later startup, the server performs a one-time import from the existing compatibility files. A `meta.sqlite_primary_bootstrap=1` marker prevents later startups from importing those files again. From then on, Web reads and writes go to SQLite first.

The compatibility files remain because the current native watcher still consumes the established text format internally. They are generated projections from SQLite and are not authoritative.

## Secret storage

`SOOP_PASSWORD` and `CLOUDFLARE_API_KEY` are protected with Windows CurrentUser DPAPI before they are written into SQLite. Web secret APIs expose configured/not-configured status rather than plaintext values.

## History

LIVE and VOD history are persisted in SQLite. Rows that were in progress when the server previously stopped are recovered as `INTERRUPTED` on startup.

VOD lifecycle state is synchronized server-side, so final VOD history does not depend on the browser continuing to poll `/api/vod/status`.

## Build

```powershell
.\BUILD_RUST_WEB.bat
```

The release build uses the tracked lockfile:

```text
cargo build --locked --release
```

`SOOP_NO_PAUSE=1` suppresses interactive pauses for package/CI execution.

## Portable package

```powershell
.\PACKAGE_RUST_WEB.bat
```

Phase 6 output:

```text
dist/soop-recorder/
  soop-server.exe
  RUN.bat
  BACKUP_DATA.bat
  RESTORE_DATA.bat
  RELEASE_INFO.txt
  SHA256SUMS.txt
  backend/
    SOOP_LIVE_SETTING.example.ini
    SOOP_LIVE_CHANNELS.example.txt
    vod/
      SOOP_VOD_SETTING.example.ini
  maintenance/
    Backup-SoopData.ps1
    Restore-SoopData.ps1
  docs/
    OPERATIONS.md
    REVERSE_PROXY.md
  data/
```

External `streamlink`, `yt-dlp`, and `ffmpeg` binaries are not bundled.

## Backup and restore

For a consistent SQLite backup, stop the server first.

```powershell
.\BACKUP_DATA.bat
```

The backup script validates the SQLite header, records SHA-256 metadata, and keeps the newest 10 backups by default.

Restore:

```powershell
.\RESTORE_DATA.bat -BackupFile .\data\backups\soop_YYYYMMDD_HHMMSS.db
```

Restore refuses to run while the SOOP server is active and creates a pre-restore safety copy of the current DB. See `docs/OPERATIONS.md` for the full upgrade/rollback procedure.

## Reverse proxy

Keep Axum on `127.0.0.1:8787` and terminate HTTPS at Caddy or Nginx when remote access is required. See `docs/REVERSE_PROXY.md` for examples and firewall/access-control notes.

## Security

- SOOP password and Cloudflare API key use Windows CurrentUser DPAPI.
- Secrets are never returned in clear text by the Web API.
- Use a reverse proxy with HTTPS for traffic leaving the local host.
- The recommended default bind remains `127.0.0.1:8787`.
- Child process cleanup targets only processes owned by this server/manager.
- Backup/restore scripts never kill processes; they fail closed if the SOOP server is still running.

## Environment variables

- `SOOP_WEB_BIND`: bind address, default `127.0.0.1:8787`
- `SOOP_WEB_TOKEN`: optional fixed management token
- `SOOP_START_WATCHER`: start the native watcher after the manually launched server starts
- `SOOP_DATA_DIR`: SQLite data directory override
- `SOOP_NO_PAUSE`: suppress build-script pause when set to `1`

## Tests and release workflow

```powershell
cargo test --locked --manifest-path rust-web/Cargo.toml
cargo check --locked --manifest-path rust-web/Cargo.toml
```

Normal CI runs Rust unit tests and the Windows native compile check. The manual portable-release workflow additionally tests with the lockfile, builds the package, verifies the packaged executable checksum, archives the package, and emits a ZIP SHA-256 file.

## Phase boundary

Phase 6 completes the planned migration/productization sequence. Future work can be treated as smaller maintenance releases for runtime bugs, UX improvements, optional history-retention policy, or packaging enhancements rather than another mandatory migration phase.

## Runtime platform boundary

OS-specific process termination and private-directory permission behavior is centralized in `rust-web/src/platform_runtime.rs`; provider modules must not add process-name-wide termination or inline OS ACL logic.
