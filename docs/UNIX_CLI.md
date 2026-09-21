# Unix / headless CLI

Linux/macOS use a CLI-oriented headless path without adding a second GUI stack. Windows uses the Slint Native UI as the default product surface.

The Unix CLI binary is:

```text
stream-archive-cli
```

It is intentionally separate from the future Windows `stream-archive` Slint application name.

## Build

From the repository root:

```bash
cargo build --locked --release --manifest-path rust-web/Cargo.toml
```

Relevant release binaries:

```text
rust-web/target/release/stream-archive-cli
rust-web/target/release/stream-archive-server
```

## First-run layout

Create the backend/data layout and the SQLite settings table:

```bash
./rust-web/target/release/stream-archive-cli init
```

Default layout:

```text
./backend/
  vod/
./data/
  stream-archive.db
```

The same canonical runtime overrides are honored:

```bash
export STREAM_ARCHIVE_BACKEND_DIR=/srv/stream-archive/backend
export STREAM_ARCHIVE_DATA_DIR=/srv/stream-archive/data
```

## Media tool discovery

Stream Archive requires:

- Streamlink
- yt-dlp
- FFmpeg

Inspect discovery results:

```bash
stream-archive-cli tools
```

Machine-readable output:

```bash
stream-archive-cli tools --json
```

Discovery order is:

1. existing SQLite path settings (`STREAMLINK_PATH`, `STREAMLINK_FALLBACK`, `YT_DLP_PATH`, `FFMPEG_PATH`)
2. Stream Archive backend/bundled layout
3. the current `PATH`
4. common Unix paths such as `/usr/local/bin`, `/usr/bin`, `~/.local/bin`, `/opt/homebrew/bin` and `/opt/local/bin`

Persist the resolved absolute paths back to SQLite atomically:

```bash
stream-archive-cli tools configure
```

This writes the canonical runtime keys with source `cli-tool-discovery`. It does not create an alternative config file or plaintext mirror.

## Doctor

Run the headless readiness check:

```bash
stream-archive-cli doctor
```

It reports:

- operating system / architecture
- backend and SQLite paths
- Streamlink / yt-dlp / FFmpeg discovery
- Linux `secret-tool` availability for Secret Service
- macOS Keychain or Windows DPAPI native secret boundary

A missing required tool causes a non-zero exit code so the command can also be used in scripts.

## Start the headless runtime

Run the shared Rust headless runtime in the foreground:

```bash
stream-archive-cli serve
```

To start the LIVE watcher automatically:

```bash
stream-archive-cli serve --watch
```

The CLI locates the compatibility-named `stream-archive-server` headless runtime next to itself first and then searches `PATH`. It passes the canonical backend path through `STREAM_ARCHIVE_BACKEND_DIR`; no HTTP listener or browser launcher is started.

Use `Ctrl+C` for normal shutdown. The headless runtime remains responsible for owned LIVE/VOD process-group cleanup.

## Linux secret requirement

Persisted Linux secrets use Secret Service through `secret-tool` and fail closed if a usable Secret Service session is unavailable. There is no automatic plaintext fallback.

On headless Linux hosts without Secret Service, secret injection/configuration needs a separate explicit workflow; this remains part of Phase 20 integration work rather than weakening the native secret boundary.

## Phase boundary

Phase 20 is responsible for cross-platform runtime readiness, Unix/headless operation, tool discovery, process ownership, native secret storage and integration coverage.

The Unix CLI remains free of Windows GUI dependencies such as PowerShell dialogs, WinForms or WebView-specific behavior.
