# Phase 21 Architecture — Slint Native GUI

Phase 21 replaces the Windows browser-facing product UI with a Slint native Rust GUI without rewriting the recorder/VOD/storage runtime. Linux and macOS keep the Phase 20 CLI/headless path.

## Target dependency direction

```text
Windows Slint UI ─────┐
Unix CLI/headless ────┼──> StreamArchiveCore / shared Rust library
Axum Web adapter ─────┘                 │
                                        ├─ SQLite / settings / channels
                                        ├─ native secret storage
                                        ├─ NativeWatcherManager / RecorderManager
                                        ├─ provider LIVE/VOD facades
                                        ├─ VOD lifecycle / queue
                                        ├─ backup / history
                                        └─ exact process ownership
```

Presentation layers do not call each other. In particular, the Slint application must not use the localhost Axum server as its application API. It calls Rust services directly.

## Phase 21.1 — shared core boundary

`rust-web/src/app_core.rs` introduces `StreamArchiveCore`, the first UI-independent application facade. It owns or shares the canonical handles needed by future frontends:

- canonical SQLite `Store`
- runtime `LogBuffer`
- `NativeWatcherManager`
- `VodManager`
- serialized configuration and VOD lifecycle locks
- settings and native-protected secret writes
- channel validation/persistence
- LIVE watcher lifecycle and channel actions
- VOD status/analyze/download/cancel operations
- VOD history synchronization
- owned-runtime shutdown

`rust-web/src/lib.rs` exposes the reusable Rust modules required by this boundary. HTTP authentication, browser sessions, Axum routing and the current browser-only picker remain presentation concerns and are not part of `StreamArchiveCore`.

The current Axum/Web application remains available during migration. This is intentional: it remains the behavior/reference implementation until Slint reaches feature parity.

## Phase 21.2 — Slint application shell

The Windows desktop frontend lives in the independent `rust-gui/` crate. Keeping it separate from `rust-web/` prevents Slint build/runtime dependencies from becoming part of the existing server package and keeps the browser runtime available as a regression reference during migration.

The shell establishes these boundaries before feature screens are implemented:

- Slint `MainWindow` with Dashboard / LIVE / VOD / Queue / History / Settings navigation
- one exported `AppState` global for presentation state
- direct bootstrap through `backend::resolve_backend_dir()` + `StreamArchiveCore::open()`
- dashboard binding for runtime readiness, canonical backend/database paths, channel count, and configured media-tool state
- refresh callback that reads another snapshot from `StreamArchiveCore`
- no localhost HTTP calls, direct SQLite access, or media-process spawning from the GUI crate

The initial pages other than Dashboard are intentional placeholders. Feature-specific callbacks are added only when their shared service boundaries are ready in later Phase 21 slices.

`rust-gui` targets the Windows desktop product. Linux/macOS continue using `stream-archive-cli`; they do not compile or ship a second GUI as part of the product support promise.

## Invariants

### One persistence authority

`data/stream-archive.db` remains the only runtime source of truth. Slint, CLI and Web adapters must not create parallel INI/TXT/JSON configuration authorities.

### One security boundary

Frontends pass plaintext secrets only to the shared Rust service call that protects them. Storage continues to use Windows CurrentUser DPAPI, Linux Secret Service and macOS Keychain according to the existing platform boundary. No frontend may implement its own encryption or plaintext fallback.

### Exact process ownership

Frontend code never invokes broad process termination. LIVE/VOD children continue through the existing Windows Job Object / Unix process-group ownership implementation.

### Provider isolation

SOOP/CHZZK protocol and authentication details remain under `platform/<provider>/`. Slint callbacks must depend on application/provider-neutral services rather than provider internals.

### Web compatibility during migration

Do not remove Web routes, browser assets or launcher behavior merely because a Slint screen exists. A feature is retired from Web only after the corresponding Slint flow has parity and regression coverage.

## Planned migration sequence

```text
21.1  Shared core/service boundary
      ↓
21.2  Slint application shell + navigation/state binding
      ↓
21.3  Settings + native picker + diagnostics
      ↓
21.4  Channels + LIVE watcher/recording
      ↓
21.5  VOD analyze/download
      ↓
21.6  Queue + History
      ↓
21.7  Backup/restore + diagnostics/log viewer
      ↓
21.8  Windows native packaging/startup
      ↓
21.9  Web UI / browser launcher retirement after parity
```

Queue and backup currently combine reusable managers with Axum handlers in the same source files. Before Slint consumes those functions directly, their reusable manager/service logic should be separated from the Web adapter rather than copied into the GUI.

## Completion criteria for Phase 21.1

Phase 21.1 is complete when:

- the shared library exposes the runtime modules required by the application facade;
- `StreamArchiveCore` compiles and is tested on Windows, Linux and macOS;
- canonical settings/secrets/channels/LIVE/VOD operations have non-HTTP service entry points;
- runtime-contract guards prevent Axum/HTTP concerns from leaking into the shared core;
- current Web behavior remains intact;
- the next Slint shell can depend on the shared core without introducing localhost HTTP or a second storage authority.

## Completion criteria for Phase 21.2

Phase 21.2 is complete when:

- the Windows Slint crate and `.slint` shell compile in hosted Windows CI;
- navigation between Dashboard / LIVE / VOD / Queue / History / Settings is owned by Slint state;
- the Dashboard is populated from a live `StreamArchiveCore` snapshot rather than HTTP;
- the GUI remains usable enough to display a runtime bootstrap failure instead of silently exiting;
- architecture guards reject localhost HTTP, direct SQLite, or direct child-process control in the Slint bootstrap;
- the current Web UI, launcher, and portable package remain unchanged until later parity/packaging phases.
