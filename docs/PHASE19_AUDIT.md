# Phase 19 Runtime Hardening Audit

## 19.1 resource lifetime

The runtime task/process/channel audit covered every `tokio::spawn`, process spawn, SSE producer,
watcher command channel, VOD worker, backup timer, job directory, destination claim, and SQLite
connection site. SSE uses a bounded channel and its producer exits as soon as the receiver closes.
Watcher commands are bounded and watcher/VOD task handles are reaped. CHZZK VOD was the exception:
its external-tool line readers used unbounded channels and detached handles. They now use bounded
channels with backpressure and every success, cancellation, and process-failure path joins or aborts
and joins the readers. Capture readers are also joined after cancellation.

Job directories, owner locks, cookie files, claims, and finalizing paths remain RAII/scoped. Owned
children remain `kill_on_drop` protected and cancellation waits for the owned PID tree. SQLite
connections/statements remain lexical and the committed settings cache remains the deliberate
runtime read source.

## 19.2 dead code and dependencies

Removed unused legacy backend setting/channel writers and path/hidden-setting readers, unused
provider/session identity methods, an unused queue query wrapper, and the redundant CHZZK `adult`
field (the derived `requires_auth` invariant remains). No Cargo dependency was proven unused; all
existing dependencies remain. Production `cargo check` is warning-free on the audit host.

## 19.3 Rust runtime inventory

Watcher orchestration, LIVE/VOD lifecycles, queue, canonical configuration, backup/restore,
notifications, authentication, cleanup, and maintenance run in Rust. Streamlink, FFmpeg and yt-dlp
remain intentional external media tools. Browser JavaScript, batch launchers, packaging scripts,
CI and regression PowerShell scripts are intentionally outside the service runtime.

The localhost-only native Windows picker still invokes Windows PowerShell. Replacing that UI bridge
requires a tested COM `IFileDialog` implementation and is deferred rather than changing picker
behavior without Windows validation. It is isolated from provider and lifecycle code and is the only
known production PowerShell invocation.

## 19.5 state and source of truth

SQLite remains the persistent canonical source. The settings cache is refreshed only after committed
writes and avoids repeated parsing/queries. In-memory watcher, queue, VOD, notification and SSE data
are ephemeral delivery/process state. Legacy INI/TXT files remain compatibility mirrors as required;
no second authoritative state was introduced. Broadcast receivers and bounded SSE producers are
connection scoped and release on disconnect.

## 19.6 cross-platform boundary and Phase 20

`platform_runtime` now owns UTF-8 external CLI configuration, exact-owned process-tree termination,
and private-directory permissions. Windows uses exact PID `taskkill` and `icacls`; Unix has a minimal
child-kill and mode-0700 implementation. Provider code supplies provider-specific arguments only.

Phase 20 should add Unix process-group ownership/termination, non-DPAPI secret storage, native picker
implementations (and remove the PowerShell bridge), launcher/packaging support, executable discovery
without Windows suffix assumptions, and Windows plus Linux/macOS integration tests.

## Manual regression checklist

- SOOP LIVE start, normal stop, forced stop, stall and restart.
- CHZZK public/authenticated LIVE start and stop; confirm zero-based MPEG-TS.
- SOOP VOD analyze, download, merge and cancel.
- CHZZK public and age-restricted VOD analyze/download/cancel; confirm cookie deletion and final TS.
- Queue retry/cancel, server shutdown with active work, destination collision, stale job scavenging.
- SSE disconnect/reconnect and session revocation.
- Windows picker, DPAPI round trip, native compile, portable package smoke and verification.
