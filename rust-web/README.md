# SOOP Rust Web - Phase 3

Phase 3 separates recording process ownership from the watcher and removes the last PowerShell dependency from the Rust LIVE runtime.

```text
Browser
  -> Axum Web/API
  -> Rust NativeWatcher
     -> SOOP login / LIVE detection
     -> Cloudflare Worker master HLS
     -> Rust RecorderManager
        -> streamlink.exe
        -> output .ts

Secrets
  -> Rust security module
  -> Windows CurrentUser DPAPI
```

The application remains **manual-start only**. It does not register a Windows Service, scheduled task, or OS auto-start entry.

## What changed in Phase 3

### RecorderManager boundary

The watcher now owns scheduling and broadcast detection only. Streamlink lifecycle is owned through `RecorderManager`:

- exact owned Streamlink PID/process-tree management
- Streamlink stderr captured directly into Web logs
- file-growth stall monitoring
- minimum free-space monitoring
- recording finish reason and size logging
- channel disable/remove/current-broadcast-stop uses the same recorder ownership path

This keeps UI/API/Watcher recording actions on one process-management path instead of duplicating recorder lifecycle logic.

### Rust-native DPAPI

Existing Windows secrets use:

```text
dpapi:v1:<base64>
```

Phase 3 reads this format directly from Rust using Windows DPAPI (`CryptProtectData` / `CryptUnprotectData`) with the same CurrentUser scope and legacy entropy value.

**The LIVE Rust runtime no longer starts PowerShell to decrypt secrets.**

Existing encrypted values remain compatible; there is no forced migration or plaintext conversion.

### Protected secret API

```text
GET /api/secrets
PUT /api/secrets
```

`GET` returns only whether each secret is configured:

```json
{
  "SOOP_PASSWORD": true,
  "CLOUDFLARE_API_KEY": true
}
```

The secret value/ciphertext is never returned to the browser.

`PUT` accepts a newly entered secret and stores it as DPAPI ciphertext. An empty value means "leave the existing value unchanged". Writes preserve the existing configuration format and use `.bak` plus atomic replacement.

On the Web page, secret fields are password inputs and are cleared after save.

## Existing behavior retained

- SOOP login/session cookies
- direct channel page + `player_live_api.php` LIVE/BNO detection
- Windows native TLS / HTTP 1.1 / no-proxy SOOP client behavior
- Cloudflare Worker master-playlist request and retry
- channel/settings hot reload
- channel nickname lookup
- filename patterns and collision protection
- per-channel stop current broadcast / resume / immediate recheck
- legacy `SOOP_LIVE.ps1` duplicate-watcher guard
- exact owned process-tree stop; never system-wide Streamlink kill by image name

## Run

```powershell
.\RUN_RUST_WEB.bat
```

Default:

```text
http://127.0.0.1:8787
```

Optional watcher auto-start after the user manually launches the server:

```powershell
$env:SOOP_START_WATCHER="Y"
.\RUN_RUST_WEB.bat
```

This is application-level auto-start only, not OS startup registration.

For temporary LAN testing:

```powershell
$env:SOOP_WEB_BIND="0.0.0.0:8787"
.\RUN_RUST_WEB.bat
```

Use HTTPS/reverse proxy before exposing the management endpoint to the Internet.

## Phase 3 local validation

With the old WinUI/PowerShell watcher stopped:

1. Start `RUN_RUST_WEB.bat`.
2. Confirm the Web page shows the two protected secrets as `설정됨` when the existing INI has them.
3. Start Watcher and confirm logs contain:

```text
[RUST] native watcher v3 started
[RUST:AUTH] SOOP login OK : <account>
[RUST] watcher ready ... secret_backend=native-dpapi
```

4. Confirm no PowerShell child is launched merely to start the Rust watcher/decrypt secrets.
5. With a LIVE channel, confirm Streamlink starts and file size grows.
6. Test current-broadcast stop/resume and watcher stop.
7. Optional: enter a new secret in the Web security section and save. Confirm `SOOP_LIVE_SETTING.ini` contains a `dpapi:v1:` value rather than plaintext and a `.bak` exists.

## Existing PowerShell backend

Legacy files remain in the repository for regression/fallback and VOD until its own migration phase:

```text
backend/SOOP_LIVE.ps1
backend/modules/*
backend/vod/*
```

The Rust LIVE watcher does not execute `SOOP_LIVE.ps1`.

## Remaining migration work

Phase 4:

- VOD engine/auth/CloudFront short-lived authorization/merge migration to Rust

Later cleanup:

- SQLite settings/channel/recording history storage
- event bus / SSE or WebSocket if useful
- release packaging and retirement of WinUI/PowerShell after functional parity

Windows Service/systemd startup is intentionally not planned unless explicitly requested.
