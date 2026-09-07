# SOOP Rust Web — Phase 5.1

Phase 5.1 establishes the Rust Web implementation as the only product runtime in this repository.

## Runtime

```text
Browser
  -> Axum server
     -> NativeWatcherManager
        -> RecorderManager -> streamlink
     -> VodManager -> yt-dlp / ffmpeg
     -> SQLite data/soop.db
```

The old WinUI and PowerShell LIVE/VOD implementations have been removed from the repository.

The server is still manually launched. It is not registered as a Windows Service or systemd unit.

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

## Build

```powershell
.\BUILD_RUST_WEB.bat
```

Portable package:

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
    vod/
      SOOP_VOD_SETTING.example.ini
  data/
```

External `streamlink`, `yt-dlp`, and `ffmpeg` binaries are not bundled.

## Persistence

SQLite database:

```text
data/soop.db
```

Override the data directory with `SOOP_DATA_DIR`.

LIVE and VOD history are persisted in SQLite. Rows that were in-progress when the server previously stopped are recovered as interrupted on startup.

## Compatibility settings

Phase 5.1 intentionally keeps the existing text configuration contract for one more migration window:

```text
backend/SOOP_LIVE_SETTING.ini
backend/SOOP_LIVE_CHANNELS.txt
backend/vod/SOOP_VOD_SETTING.ini
```

Web saves continue to dual-write the compatibility files and SQLite snapshot tables. This preserves hot reload/manual editing and the existing DPAPI secret behavior while removing the old executable implementations.

Tracked repository files contain only `.example` templates; runtime settings/secrets are ignored by Git.

## Security

- SOOP password and Cloudflare API key use Windows CurrentUser DPAPI.
- Secrets are never returned in clear text by the Web API.
- Use a reverse proxy with HTTPS for internet exposure.
- The recommended default bind remains `127.0.0.1:8787`.

## Environment variables

- `SOOP_WEB_BIND`: bind address, default `127.0.0.1:8787`
- `SOOP_WEB_TOKEN`: optional fixed management token
- `SOOP_START_WATCHER`: start the native watcher after the manually launched server starts
- `SOOP_DATA_DIR`: SQLite data directory override

## Tests

```powershell
cargo test --manifest-path rust-web/Cargo.toml
cargo check --manifest-path rust-web/Cargo.toml
```

CI runs Rust unit tests on Linux and a Windows native compile check.

## Next migration boundary

Phase 5.2 will make SQLite the primary source for settings/channels. INI/TXT will then be reduced to migration/import compatibility rather than being the active runtime authority.
