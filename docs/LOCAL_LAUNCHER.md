# Stream Archive local launcher

Stream Archive includes a Windows launcher for the default local-use workflow.

## Normal use

1. Double-click `RUN.bat` or `soop-launcher.exe`.
2. If Stream Archive is not running, the launcher starts `soop-server.exe` in its own console window.
3. The launcher waits until the local web UI is ready and opens `http://127.0.0.1:8787/` in the default browser.
4. If the server is already running, a second launcher start only opens the existing web UI.

The local launcher does not register a Windows service and does not configure OS auto-start.

## Stopping

The server console remains visible on purpose. Press `Ctrl+C` in that console to stop Stream Archive cleanly. The Rust server then stops only its owned LIVE/VOD child processes through the existing Recorder/VOD lifecycle.

Do not use broad process-name termination such as `taskkill /IM ffmpeg.exe` or `taskkill /IM streamlink.exe`.

## Port conflict

The launcher verifies that the local listener on the configured port is actually Stream Archive. If another program is using the port, it shows an error instead of opening the wrong application.

The default endpoint is `127.0.0.1:8787`. If `SOOP_WEB_BIND` uses another port, the launcher uses that port while still opening the local loopback address.

## Direct server mode

For troubleshooting or advanced use, run `RUN_SERVER_CONSOLE.bat` to launch `soop-server.exe` directly without the launcher.

## Remote access

Remote access remains optional. The launcher does not install or start Caddy, modify Windows Firewall, or configure router port forwarding. Keep the Rust server on loopback and use the existing reverse-proxy documentation only when remote access is intentionally configured.
