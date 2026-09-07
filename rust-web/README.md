# SOOP Rust Web — Phase 5.2

Phase 5.2 makes SQLite the authoritative configuration and history store while keeping the native Rust runtime introduced in Phase 5.1.

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

At the first Phase 5.2 startup, the server performs a one-time import from the existing compatibility files:

```text
backend/SOOP_LIVE_SETTING.ini
backend/SOOP_LIVE_CHANNELS.txt
backend/vod/SOOP_VOD_SETTING.ini
```

A `meta.sqlite_primary_bootstrap=1` marker prevents later startups from importing those files again. From then on, Web reads and writes go to SQLite first.

The compatibility files remain because the current native watcher still consumes the established text format internally. They are generated projections from SQLite and are reconciled back to the database state. Manual file edits after the cutover are therefore not authoritative and may be overwritten.

If `data/soop.db` is removed, a fresh database can be bootstrapped from the remaining compatibility/example files on the next startup.

## Secret storage

`SOOP_PASSWORD` and `CLOUDFLARE_API_KEY` are protected with Windows CurrentUser DPAPI before they are written into the SQLite `settings` table. Only the `dpapi:v1:...` ciphertext is persisted. Web secret APIs expose configured/not-configured status rather than plaintext values.

## History

LIVE and VOD history are persisted in SQLite. Rows that were in progress when the server previously stopped are recovered as `INTERRUPTED` on startup.

Phase 5.2 also persists VOD lifecycle state in a server-side background task, so final VOD history no longer depends on the browser continuing to poll `/api/vod/status`.

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

## Security

- SOOP password and Cloudflare API key use Windows CurrentUser DPAPI.
- Secrets are never returned in clear text by the Web API.
- Use a reverse proxy with HTTPS for internet exposure.
- The recommended default bind remains `127.0.0.1:8787`.
- Child process cleanup targets only processes owned by this server/manager.

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

## Next boundary

Phase 6 can focus on release hardening: database backup/restore and retention, portable release finalization, reverse-proxy documentation, and reproducible release metadata.
