# SOOP Rust Web - Phase 1

Phase 1 adds a Rust web-management layer while keeping the existing PowerShell
watcher/recording backend unchanged.

## Scope

Included:

- Axum/Tokio web server
- browser dashboard
- watcher start/stop
- watcher stdout/stderr log view
- structured channel editor
- safe settings editor
- atomic settings/channel writes
- `.bak` backup compatibility
- management-token authentication
- responsive browser UI

Not included yet:

- Rust-native SOOP live detection
- Rust-native recorder/VOD engine
- SQLite migration
- WebSocket event bus
- replacement of PowerShell
- OS service / auto-start registration

The server is intentionally **manual-start only**.

## Existing backend

Phase 1 reuses:

```text
backend/SOOP_LIVE.ps1
backend/modules/*
backend/vod/*
backend/SOOP_LIVE_SETTING.ini
backend/SOOP_LIVE_CHANNELS.txt
```

`SOOP_PASSWORD` and `CLOUDFLARE_API_KEY` are intentionally not exposed by the
Phase 1 settings API. Existing DPAPI-protected values are preserved.

## Requirements

Windows development/runtime:

- Rust stable toolchain
- PowerShell 5.1+
- the existing SOOP backend requirements
- Streamlink
- FFmpeg

The Rust web layer itself is cross-platform, but the existing Phase 1 backend
still contains Windows-specific behavior. Cross-platform recorder support is a
later phase.

## Run

From the repository root:

```powershell
cargo run --release --manifest-path .\rust-web\Cargo.toml
```

or:

```powershell
.\RUN_RUST_WEB.bat
```

Default listener:

```text
127.0.0.1:8787
```

Open:

```text
http://127.0.0.1:8787
```

At first launch the server creates:

```text
backend\.rust-web\web-token.txt
```

The management token is also printed in the console. The browser asks for this
token and stores it only in `sessionStorage`.

## Watcher start behavior

By default, starting `soop-web` starts only the web server.

The PowerShell watcher is started from the browser.

To start the watcher automatically **after the user manually starts the Rust
server**, set:

```powershell
$env:SOOP_START_WATCHER="Y"
```

This is not OS auto-start and does not register a Windows Service.

## Backend location

By default the server accepts only:

```text
<current-working-directory>\backend\SOOP_LIVE.ps1
```

or:

```text
<soop-web.exe directory>\backend\SOOP_LIVE.ps1
```

It intentionally does not search arbitrary parent directories, to avoid
accidentally running an old backend copy.

An explicit backend can be supplied with:

```powershell
$env:SOOP_BACKEND_DIR="C:\path\to\soop-downloader\backend"
```

## Remote/LAN listening

For temporary LAN testing:

```powershell
$env:SOOP_WEB_BIND="0.0.0.0:8787"
.\RUN_RUST_WEB.bat
```

Do **not** expose this plain HTTP port directly to the public Internet.

For DDNS/Internet access use an HTTPS reverse proxy:

```text
Internet
   |
 HTTPS :443
   |
Caddy / Nginx
   |
127.0.0.1:8787
   |
soop-web
```

Example Caddy concept:

```text
your-ddns.example.com {
    reverse_proxy 127.0.0.1:8787
}
```

The Phase 1 API still requires the management token even behind the reverse
proxy.

## APIs

```text
GET  /api/status

GET  /api/logs

GET  /api/settings
PUT  /api/settings

GET  /api/channels
PUT  /api/channels

POST /api/watcher/start
POST /api/watcher/stop
```

All `/api/*` requests require:

```text
Authorization: Bearer <management-token>
```

## Configuration safety

Settings and channels are not directly truncated.

The Rust bridge:

1. validates input
2. creates the existing `.bak` backup
3. writes the replacement through `atomic-write-file`
4. commits the atomic replacement

This is intentional because the PowerShell watcher hot-reloads these files.

Channel parsing accepts:

- CRLF
- LF
- CR

Channel format remains:

```text
Y|NAME|ACCOUNT|OUTDIR
```

## Build

```powershell
.\BUILD_RUST_WEB.bat
```

Output:

```text
rust-web\target\release\soop-web.exe
```

## Stop

Prefer `Ctrl+C` in the Rust server console.

During graceful shutdown the server stops the watcher it owns before exiting.

The watcher stop path on Windows targets only the exact watcher PID and its
owned process tree. It does not kill `streamlink.exe` or `python.exe`
system-wide.

## Phase 2 target

Phase 2 moves the watcher itself into Rust while still using Streamlink/FFmpeg:

```text
Browser
   |
Rust Web/API
   |
Rust Watcher
   |
Recorder Manager
   |
Streamlink / FFmpeg
```
