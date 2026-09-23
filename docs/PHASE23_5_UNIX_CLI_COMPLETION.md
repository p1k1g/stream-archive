# Phase 23.5 — Unix CLI Completion

Phase 23.5 promotes the Phase 20 Unix CLI from bootstrap/tool discovery into a
daily-use Linux/macOS headless management surface.

## Architecture

The new management commands call StreamArchiveCore. They do not add a second
provider, SQLite, Queue, History, Backup, secret or media-process
implementation.

~~~text
stream-archive-cli
        |
        +-- bootstrap: init / tools / doctor
        |
        +-- daily-use management
                |
                +--> StreamArchiveCore
                         |
                         +-- settings / protected secrets
                         +-- channels / watcher
                         +-- VOD / Queue / History
                         +-- Backup / Storage / Logs
                         +-- process ownership
~~~

The compatibility stream-archive-server binary remains supported. Its former
entry logic is now a shared headless runner used by both the server binary and
CLI serve path.

No localhost HTTP, Axum, browser launcher or second GUI is restored.

## CLI completion

Phase 23.5 adds:

- status
- settings show/set
- provider status/non-secret setting/secret-stdin/auth test
- channel list/add/remove/enable/disable/action/password-stdin
- watcher status/start/stop
- VOD analyze/download/status/cancel
- persistent Queue list/add/cancel/retry/remove
- History filters
- Backup status/create/restore
- Storage
- runtime logs

Major read commands support JSON.

## Secret handling

Provider secret values are accepted from stdin only. No CLI flag accepts a
SOOP password, Cloudflare API key, NID_AUT or NID_SES value directly.

The underlying security contract is unchanged:

- Linux: Secret Service
- macOS: Keychain
- SQLite: opaque native-secret reference
- native store unavailable: fail closed
- plaintext fallback: none

Protected LIVE broadcast passwords also use stdin and stay memory-only.

## Foreground lifecycle

Continuous Unix work remains foreground-owned.

~~~text
stream-archive-cli serve --watch
stream-archive-cli watcher start
stream-archive-cli vod download ...
~~~

Unix SIGINT and SIGTERM converge on shared cancellation/shutdown.

The compatibility stream-archive-server uses the same signal path.

Phase 23.5 deliberately does not add a cross-process HTTP/socket control plane.
Therefore active watcher/VOD LogBuffer objects are process-local. Queue,
settings, channels, history and backup state remain persistent in SQLite.

## Unix integration harness

rust-runtime/tests/unix_cli.rs executes real built binaries on Linux/macOS.

It uses isolated backend/data/tool paths containing:

- whitespace
- Korean text
- non-ASCII emoji

The smoke verifies initialization, tool configuration, JSON-only management
reads, safe provider status, settings/channel persistence and persistent
Queue/History/Backup/Storage access.

A lifecycle test sends real SIGTERM to both:

- stream-archive-cli serve
- stream-archive-server

and verifies an unrelated headless runtime survives.

The existing Phase 23.3 media-process and Phase 23.4 provider E2E suites continue
to verify media descendant cleanup, cancellation and unrelated-process
protection at the owned media-process level.

## CI

The existing cross-platform core matrix remains.

Linux and macOS receive an explicit Unix CLI integration smoke step in addition
to the normal full Rust test suite.

Windows still runs:

- Slint compile/tests
- strict clippy
- runtime contract guards
- source archive metadata smoke
- BUILD_PORTABLE.bat
- portable package verification

## Runtime contract guard

maintenance/guards/UnixCli.ps1 protects the Phase 23.5 boundaries:

- management routing exists
- StreamArchiveCore is used
- shared settings/provider/channel/VOD/Queue/Backup/log services are used
- secret stdin contract is retained
- no provider/Web control plane is added
- no direct provider-tool spawn is added to daily-use management
- SIGTERM and shared shutdown remain present
- compatibility server and CLI share the headless runner
- real Unix binary integration/SIGTERM coverage remains present

## Manual Linux/macOS smoke

~~~bash
cargo build --locked --release --manifest-path rust-runtime/Cargo.toml

export STREAM_ARCHIVE_BACKEND_DIR="/tmp/Stream Archive/backend"
export STREAM_ARCHIVE_DATA_DIR="/tmp/Stream Archive/data"

rust-runtime/target/release/stream-archive-cli init
rust-runtime/target/release/stream-archive-cli tools configure
rust-runtime/target/release/stream-archive-cli doctor --active-tools
rust-runtime/target/release/stream-archive-cli status --json
rust-runtime/target/release/stream-archive-cli channels list --json
rust-runtime/target/release/stream-archive-cli serve --watch
~~~

From another terminal:

~~~bash
kill -TERM <pid>
~~~

Expected result: graceful core shutdown and cleanup of owned process groups only.

## Phase 23.6 handoff

Phase 23.6 Packaging / Release Readiness should consume this completed CLI and
headless lifecycle rather than changing their application semantics.

Expected next concerns include Unix distributable layout/install guidance,
release artifacts, checksums/version metadata and platform release readiness.
System service installers, signing/notarization and package-manager integration
should be scoped explicitly rather than mixed back into Phase 23.5.
