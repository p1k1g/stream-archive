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

## 19.7 runtime contract guards

The five historical phase/process/release entry points and three obsolete phase-only workflows were
replaced by one permanent CI entry point, `maintenance/Test-RuntimeContracts.ps1`. The entry point
only orchestrates focused modules under `maintenance/guards/`: shared helpers, architecture,
providers, process lifecycle, storage ownership, security, and release safety. No transitional
wrappers were retained because repository workflows were the only callers.

The former implementation-location assumptions now follow the Phase 19 boundary. CHZZK VOD is
required to call `restrict_private_dir` and `terminate_owned`; the platform runtime is independently
required to implement Windows PID/tree termination, forbid `/IM`, apply the Windows ACL, and provide
Unix mode 0700. LIVE and SOOP VOD also route owned termination through that common boundary.

Behavioral assertions remain Rust tests: destination collision and reusable claims, no-clobber and
cancellable publication, stale job cleanup, progress parsing, completed-versus-late-cancel,
configuration-cache commits, queue serialization/retry, status transitions, and cookie cleanup.
PowerShell contracts require those tests to exist while concentrating on dependency direction,
forbidden patterns, OS boundaries, secrets, workflow triggers, and portable package contents.

`rust-web-check.yml` is now the only pull-request runtime workflow. It invokes the consolidated guard,
JavaScript syntax validation, whole-crate rustfmt, Rust tests/check/clippy, Windows compilation, and
portable package smoke/verification. Its path filter covers provider code, `platform_runtime.rs`,
recorder, queue, main, backend, web assets, all maintenance guards, and workflow changes.
