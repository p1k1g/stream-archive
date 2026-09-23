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
cargo build --locked --release --manifest-path rust-runtime/Cargo.toml
```

Relevant release binaries:

```text
rust-runtime/target/release/stream-archive-cli
rust-runtime/target/release/stream-archive-server
```

## First-run layout

Create the backend/data layout and the SQLite settings table:

```bash
./rust-runtime/target/release/stream-archive-cli init
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

## Doctor / shared runtime preflight

Run the same shared local preflight model used by the Windows Native Diagnostics page:

```bash
stream-archive-cli doctor
```

Machine-readable output:

```bash
stream-archive-cli doctor --json
```

Phase 23.3 keeps those commands passive. To explicitly execute only local media-tool version probes:

```bash
stream-archive-cli doctor --active-tools
stream-archive-cli doctor --json --active-tools
```

Active probing resolves the same Streamlink/yt-dlp/FFmpeg paths, runs only their local version command through the shared owned-process runner, applies a bounded timeout/output capture, and performs no provider/media network request.

The preflight reports stable check IDs/categories and separates required, optional and informational checks. It covers:

- canonical backend/data/SQLite paths
- read-only SQLite `PRAGMA quick_check`
- Streamlink / yt-dlp / FFmpeg filesystem discovery
- configured LIVE output path
- native secret-store capability
- SOOP/CHZZK local configuration readiness without exposing credential values
- backup directory and policy

Exit status is based on blocking required errors:

- exit `0`: base runtime is usable; warnings/optional gaps may still need attention
- non-zero: at least one required preflight check is in Error state

The passive command is intentionally local/read-only. It does not contact SOOP/CHZZK, test logins, download media, create missing directories, rewrite settings or repair SQLite. The explicit `--active-tools` mode adds only bounded local version subprocesses; it still does not contact providers or media URLs.

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
