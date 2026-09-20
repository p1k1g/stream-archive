# Stream Archive Windows launch paths

Stream Archive's Windows portable package uses the Slint native desktop as the default user-facing application. The previous browser/Axum launcher remains available as an explicit compatibility fallback.

## Default native use

Use either of these from `dist\stream-archive`:

1. Double-click `RUN.bat`; or
2. Double-click `StreamArchive.exe` directly.

The default flow is:

```text
RUN.bat
  -> StreamArchive.exe
  -> StreamArchiveCore
  -> data/stream-archive.db
```

`RUN.bat` first changes the working directory to the portable root. Direct Explorer launch does not depend on that working directory: backend resolution prefers the `backend` directory beside `StreamArchive.exe`, then falls back to the current working directory. `STREAM_ARCHIVE_BACKEND_DIR` and `STREAM_ARCHIVE_DATA_DIR` remain explicit overrides.

## Web compatibility fallback

Use `RUN_WEB.bat` when the retained browser UI is needed for regression comparison or troubleshooting.

```text
RUN_WEB.bat
  -> stream-archive-launcher.exe
  -> stream-archive-server.exe
  -> http://127.0.0.1:8787/
```

The launcher starts the compatibility server when necessary, waits for the local Web UI, and opens the default browser. It does not register a Windows service or configure OS auto-start.

## Direct Web server mode

For Web/server troubleshooting, `RUN_SERVER_CONSOLE.bat` starts `stream-archive-server.exe` directly without the browser launcher.

## Stopping and process ownership

Close the native application normally, or press `Ctrl+C` in the legacy Web server console when using direct server mode. Stream Archive must stop only LIVE/VOD child processes that it created and owns.

Do not use broad process-name termination such as `taskkill /IM ffmpeg.exe`, `taskkill /IM streamlink.exe`, or `taskkill /IM yt-dlp.exe`.

## Web port conflicts

Port handling applies only to the Web compatibility path. The launcher verifies that the configured local listener is Stream Archive before opening it. The default endpoint is `127.0.0.1:8787`.

## Remote access

Remote access remains a Web compatibility/advanced-use concern. The launcher does not install or start Caddy, modify Windows Firewall, or configure router port forwarding. Keep the Rust Web server on loopback and use the reverse-proxy documentation only when remote access is intentionally configured.

For Phase 21.8 package verification and manual QA, see `PHASE21_NATIVE_PORTABLE.md`.
