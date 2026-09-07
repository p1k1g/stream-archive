# SOOP Rust Web - Phase 2

Phase 2 moves the LIVE watcher and recorder lifecycle into Rust. Streamlink remains an external recorder process; the existing PowerShell/VOD code is kept in the repository for fallback and later migration, but `Watcher Start` in the Rust Web UI no longer launches `SOOP_LIVE.ps1`.

## Runtime architecture

```text
Browser
  -> Axum Web/API
  -> Rust native watcher
     -> SOOP channel/live API (reqwest)
     -> Cloudflare Worker master HLS request
     -> Rust recorder manager
        -> streamlink.exe
        -> output .ts
```

The application remains **manual-start only**. It does not register a Windows Service or OS auto-start entry.

## Included in Phase 2

- Rust-native channel polling and hot reload
- Rust-native SOOP login/session cookies
- direct channel-page + `player_live_api.php` LIVE/BNO detection
- Cloudflare Worker master-playlist request and retry
- Streamlink process launch/ownership
- output filename collision protection and existing filename patterns
- recording file-growth stall detection
- minimum free-space checks
- exact owned recorder process-tree stop on Windows
- channel disable/remove -> owned recorder stop
- per-channel stop current broadcast / resume / recheck
- runtime channel dashboard in the browser
- settings hot reload
- existing `.ini`, channel file, `.bak`, and atomic-write compatibility

## DPAPI compatibility during migration

Existing secrets use the legacy format:

```text
dpapi:v1:<base64>
```

On Windows, Phase 2 decrypts those values through a **short-lived PowerShell DPAPI compatibility bootstrap at watcher start/settings reload**. PowerShell is no longer the persistent watcher or recorder manager.

This temporary bootstrap is intentionally left for Phase 3, where secret storage will be moved fully into Rust/cross-platform storage.

On non-Windows systems, DPAPI-protected settings cannot be decrypted in Phase 2. Cross-platform runtime testing therefore requires secrets stored by the future Phase 3 secret backend or temporary plaintext test values.

## Existing PowerShell backend

These files remain for fallback/regression comparison and VOD migration:

```text
backend/SOOP_LIVE.ps1
backend/modules/*
backend/vod/*
```

Phase 2 `Watcher Start` does not execute `SOOP_LIVE.ps1`.

## Run

```powershell
.\RUN_RUST_WEB.bat
```

Default:

```text
http://127.0.0.1:8787
```

For temporary LAN testing:

```powershell
$env:SOOP_WEB_BIND="0.0.0.0:8787"
.\RUN_RUST_WEB.bat
```

Use HTTPS/reverse proxy before exposing the management page to the Internet.

## Watcher behavior

By default, manually starting `soop-web` starts the web server only. Start the Rust watcher from the browser.

Optional auto-start **after manually launching the Rust server**:

```powershell
$env:SOOP_START_WATCHER="Y"
.\RUN_RUST_WEB.bat
```

This is not OS auto-start.

## New channel control API

```text
POST /api/watcher/channel/{account}/stop
POST /api/watcher/channel/{account}/resume
POST /api/watcher/channel/{account}/recheck
```

`stop` suppresses the current BNO so the same broadcast is not immediately restarted. `resume` clears the suppression and schedules an immediate check.

## Remaining migration work

Phase 2 intentionally does not yet move:

- VOD engine to Rust
- SQLite settings/channel storage
- fully Rust-native secret storage
- WebSocket/SSE event bus
- reverse-proxy/HTTPS provisioning
- Windows Service/systemd integration (not planned unless explicitly requested)

## Safety / fallback

Test Phase 2 with the old WinUI/PowerShell watcher stopped. The Rust watcher owns only the Streamlink processes it launches and does not kill Streamlink/Python processes by name.
