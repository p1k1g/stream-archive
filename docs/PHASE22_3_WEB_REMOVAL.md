# Phase 22.3 — Web Presentation Removal & Legacy Web Cleanup

Phase 22.3 removes the browser/Axum presentation surface identified by the Phase 22.1 audit after Phase 22.2 warning/dead-code preparation.

The `rust-web` crate is deliberately retained as the shared Rust runtime/core package.

## Removed Web presentation

Removed confirmed Web-only components:

- `rust-web/web/*` browser HTML/CSS/JavaScript assets;
- Axum presentation/router code from `rust-web/src/main.rs`;
- browser authentication/session/CSRF adapter;
- SSE/realtime browser transport;
- Web local file/folder picker bridge;
- Web backup/history/queue presentation adapters;
- Windows browser launcher;
- Caddy reverse-proxy example;
- current Web launcher and reverse-proxy documentation.

Deleted Rust presentation modules include:

- `rust-web/src/auth.rs`;
- `rust-web/src/realtime.rs`;
- `rust-web/src/local_picker.rs`;
- `rust-web/src/backup.rs`;
- `rust-web/src/history_storage.rs`;
- `rust-web/src/vod_queue.rs`;
- `rust-web/src/bin/stream-archive-launcher.rs`.

## Retained rust-web runtime/core

The crate/package remains named `stream-archive-server` for compatibility, but its default binary is now a headless shared-core runtime rather than an HTTP/Web server.

Retained shared/runtime areas include:

- `StreamArchiveCore`;
- canonical SQLite store and migrations;
- provider configuration and protected secrets;
- SOOP LIVE/VOD;
- CHZZK LIVE/VOD;
- NativeWatcher / Recorder;
- persistent Queue;
- History;
- Backup/Restore;
- storage diagnostics;
- runtime logs and diagnostics;
- tool discovery;
- Windows Job Object ownership;
- Unix process-group ownership;
- compatibility process termination boundaries.

The headless runtime starts no HTTP listener. It opens `StreamArchiveCore`, starts VOD-history sync, automatic backup and the Queue worker, optionally starts the watcher through `STREAM_ARCHIVE_START_WATCHER`, and shuts down through the shared owned-runtime boundary.

## CLI decision

`stream-archive-cli serve` is retained for Linux/macOS/headless compatibility.

It still locates the sibling compatibility-named `stream-archive-server` executable, but that executable is now the headless runtime. No browser launcher, localhost HTTP listener or Web UI is involved.

## Dependency cleanup

Direct Web-only dependencies were removed from `rust-web/Cargo.toml`, including:

- Axum;
- Tokio Stream;
- Tower HTTP;
- direct tracing;
- tracing-subscriber.

Windows launcher-only UI feature flags were also removed.

Cargo lockfiles were updated for the new feature/dependency graph. The rust-web lockfile was regenerated from Cargo's CI-produced diff after Web dependency removal.

## Packaging

`BUILD_PORTABLE.bat` remains the single Windows portable build entry.

Default path:

```text
RUN.bat
  -> StreamArchive.exe
  -> StreamArchiveCore
```

Optional headless path:

```text
RUN_HEADLESS.bat
  -> stream-archive-server.exe
  -> StreamArchiveCore
```

The package no longer includes:

- `stream-archive-launcher.exe`;
- `RUN_WEB.bat`;
- `RUN_SERVER_CONSOLE.bat`;
- Caddy reverse-proxy files;
- Web launcher/reverse-proxy documentation;
- browser static assets.

Package verification explicitly rejects those retired artifacts if they return.

## Runtime contract updates

The runtime guards were not weakened. Web-specific assertions were replaced with current architecture assertions.

The guards now explicitly verify:

- headless runtime bootstrap through `StreamArchiveCore`;
- Queue/backup/VOD-history lifecycle startup;
- owned-runtime shutdown;
- absence of Axum/listener/token presentation state;
- Native provider configuration through shared core;
- SOOP authentication test through shared core;
- CHZZK protected provider secrets;
- Native SOOP/CHZZK platform selection;
- CHZZK per-job/destination OS locking;
- retained Windows/Unix process ownership contracts;
- Native + headless package layout;
- absence of retired Web package artifacts/dependencies.

## Documentation

Current user/operations documentation was updated for Native + optional headless operation:

- `README.md`;
- `AGENTS.md`;
- `CONTRIBUTING.md`;
- `SECURITY.md`;
- `docs/OPERATIONS.md`;
- `docs/UNIX_CLI.md`.

Historical Phase documents remain as historical records.

## Compatibility intentionally retained

The following names/references may remain intentionally:

- `rust-web` directory/crate name;
- Cargo package/binary name `stream-archive-server`;
- historical Phase documentation describing the retired Web architecture;
- ordinary model/provider fields whose meaning includes a browser/user-agent concept and are unrelated to the removed UI.

These are not evidence that the browser presentation layer is still active.

## Validation gates

The final PR head must pass:

- Windows/Linux/macOS rust-web format, unit tests, compile and strict Clippy;
- Windows rust-gui format, compile, unit tests and strict Clippy;
- runtime contract guard;
- source archive release-metadata smoke;
- Windows portable package build;
- portable package verification.

No guard/test/CI coverage is removed to obtain a pass.
