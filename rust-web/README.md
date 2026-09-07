# SOOP Rust Web - Phase 4

Phase 4 keeps the LIVE pipeline from Phase 3 and moves the VOD orchestration into Rust.

```text
Browser
  -> Axum Web/API
     -> Rust NativeWatcher
        -> SOOP LIVE detection
        -> Cloudflare Worker
        -> Rust RecorderManager -> Streamlink

     -> Rust VodManager
        -> SOOP login / private_auth / manifest probe
        -> yt-dlp metadata + PART download
        -> ffmpeg concat merge

Secrets
  -> Rust security module
  -> Windows CurrentUser DPAPI
```

The application remains **manual-start only**. No Windows Service, scheduled task, systemd unit, or OS auto-start entry is created.

## Phase 4 VOD scope

The Web page now exposes a VOD section backed by these APIs:

```text
GET  /api/vod/status
POST /api/vod/analyze
POST /api/vod/download
POST /api/vod/cancel
```

The Rust VOD engine provides:

- separate analyze and download phases
- `SOOP_LOGIN`, `FILE`, and `BROWSER` cookie modes
- Phase 3 native DPAPI credentials reused for stored SOOP login
- Rust-managed Netscape cookie jar
- subscription VOD `private_auth.php` refresh
- CloudFront Key-Pair/Policy/Signature capture and host-scope repair
- direct manifest authorization probe and available height extraction
- yt-dlp metadata extraction
- SOOP mobile API fallback when PART URL/duration is absent from yt-dlp metadata
- PART selection (`1,2,4-6` in the Web UI)
- metadata + authorization refresh on each retry
- yt-dlp progress surfaced through `/api/vod/status`
- exact owned yt-dlp/ffmpeg cancellation
- optional ffmpeg concat merge
- source PART files removed only after a successful merged output is validated
- temporary job/cookie files under ignored `backend/.rust-web/vod`

The new Rust VOD path does **not** launch PowerShell or curl. yt-dlp and ffmpeg remain external media tools.

## VOD Web flow

1. Enter a SOOP VOD URL.
2. Choose Cookie mode. Normally use `SOOP_LOGIN` when the Phase 3 SOOP credentials are configured.
3. Click **분석**.
4. Wait for title, streamer, PART count, durations, and quality choices.
5. Enter PART selection or leave blank for all.
6. Click **다운로드**.
7. Use **취소** to terminate the exact owned external process tree.

The quality selector uses the same values as the legacy backend:

```text
best
best[height<=1080]
best[height<=720]
...
```

## Subscription VOD retry behavior

For each PART attempt, Phase 4 refreshes metadata before starting the download. A later PART may otherwise inherit an expired manifest URL from the original analysis.

When a fresh short-lived authorization is needed:

```text
SOOP/browser base session
  -> private_auth.php
  -> CloudFront signed cookies
  -> manifest probe
  -> yt-dlp PART download
```

Retry backoff is capped similarly to the legacy implementation. Existing FILE-mode signed cookies can be used for the first attempt; if they are expired and the cookie file has no reusable SOOP login cookies, a fresh login/browser session is required.

## LIVE behavior retained from Phase 3

- Rust NativeWatcher
- Rust RecorderManager
- Rust-native CurrentUser DPAPI
- Streamlink ownership/stall/disk monitoring
- channel/settings hot reload
- nickname lookup
- current-broadcast stop/resume/recheck
- legacy watcher duplicate guard

## Run

```powershell
.\RUN_RUST_WEB.bat
```

Default:

```text
http://127.0.0.1:8787
```

Optional LIVE watcher auto-start after manually launching the server:

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

## Phase 4 local validation

Before merging Phase 4:

1. Start the branch with `RUN_RUST_WEB.bat` and confirm Phase 3 LIVE behavior still works.
2. Analyze a known public VOD using `SOOP_LOGIN`.
3. Confirm title, streamer, PART count, duration, and quality options appear.
4. Download one PART and confirm the MP4 is playable.
5. Download two or more PARTs with merge enabled and confirm the final merged file is valid and source PARTs are removed only after merge success.
6. Test Cancel while yt-dlp is active and confirm the owned process tree exits.
7. If available, test a subscription VOD to exercise `private_auth.php` / CloudFront refresh and retry behavior.

## Legacy files

The old PowerShell/WinUI/VOD sources remain in the repository during regression testing:

```text
backend/SOOP_LIVE.ps1
backend/modules/*
backend/vod/*
overlay/*
```

The Rust LIVE/VOD Web paths do not execute those PowerShell scripts.

## Later cleanup

After functional parity is verified:

- SQLite settings/channel/recording/VOD history storage
- event bus / SSE or WebSocket if useful
- release packaging
- retire the old WinUI/PowerShell implementation

Windows Service/systemd startup remains intentionally out of scope unless explicitly requested.
