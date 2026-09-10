# Phase 15 audit / optimization baseline

Phase 15 keeps current behavior stable while reducing unnecessary work and preparing the Rust runtime for a future second platform (CHZZK).

## Rules

- Keep the canonical runtime Rust-only.
- Keep INI/TXT compatibility mirrors intact.
- Never kill processes by image name; only terminate owned PID/process trees.
- Prefer measurable hot-path reductions before large structural refactors.
- Every behavior-affecting change must keep or add regression coverage and pass Windows portable-package CI.

## Current execution paths

### LIVE

`launcher -> Axum -> SQLite settings/channels -> NativeWatcherManager -> SOOP discovery -> RecorderManager -> Streamlink -> SQLite history -> SSE/UI -> browser notification`

### VOD

`UI -> VOD queue -> VodQueueManager -> VodManager -> yt-dlp/ffmpeg -> merge -> SQLite queue/history -> SSE/UI -> browser notification`

## Audit findings

### P1: realtime snapshot amplification

`realtime.rs` emits the full watcher/VOD/queue/log snapshot every second, and log broadcast events can additionally trigger the same full snapshot. Streamlink stderr can produce bursts, so a noisy recorder can multiply process-state and SQLite reads for every connected SSE client.

Phase 15.1: coalesce log-triggered snapshots while retaining the regular one-second snapshot and five-second session revalidation.

### P1: VOD queue snapshot opens SQLite repeatedly

`VodQueueManager::snapshot()` loads queue rows and queued count through separate helper calls, each opening a SQLite connection. Because SSE calls this path regularly, connection setup/read overhead scales with connected browsers.

Target: read queue rows/count through one connection or move the queue onto an intentional shared DB access abstraction without increasing lock contention.

### P1: frontend state has multiple update paths

The browser can receive state through REST polling or SSE, while later phase scripts wrap functions such as `api`, `renderStatus`, and `applyRealtimeSnapshot`. Correctness currently depends on script load/wrapper order.

Target: one state-ingest/event pipeline consumed by dashboard, queue, history, and notifications. REST and SSE should only be transports.

### P1: platform-specific logic is mixed with orchestration

`native_watcher.rs` owns SOOP HTTP/login/live discovery as well as watcher scheduling/state transitions. `vod.rs` similarly contains a large SOOP-oriented VOD implementation.

Target before CHZZK: isolate platform discovery/auth/stream resolution behind provider boundaries while keeping recorder/process/history/notification infrastructure platform-independent.

### P2: periodic SQLite configuration reads

The native watcher checks live settings every second and channels on the configured reload interval. This is simple and robust but repeatedly reads unchanged SQLite data.

Target: add a lightweight configuration revision/generation signal or equivalent change detection so full settings/channel materialization happens only when needed. Preserve external compatibility and restart recovery.

### P2: SQLite access patterns are inconsistent

`Store` owns a mutex-protected connection, while VOD queue code opens independent connections for many operations. This works with WAL/busy timeout but makes connection lifetime and contention harder to reason about.

Target: explicitly separate short read/write connections or a shared Store API; avoid holding synchronous SQLite mutexes across `.await` points.

### P2: duplicated schema/bootstrap responsibilities

`Store::open()` and `Store::ensure_schema()` contain overlapping schema creation logic. Drift between the two is possible as tables/indexes evolve.

Target: one schema initializer/migration path covered by tests.

### P2: large modules / phase-named frontend files

Current large modules include `native_watcher.rs` and `vod.rs`; browser behavior is spread across `app.js` plus phase-numbered scripts. Phase numbers are useful history but are not useful production architecture boundaries.

Target after hot-path stabilization:

```text
Rust
  api/
  config/
  db/
  platform/
    soop/
  recorder/
  watcher/
  vod/

Web
  core/
    api
    state
    realtime
  features/
    auth
    live
    vod
    history
    notifications
    settings
  ui/
    theme
    tabs
    toast
```

This is a direction, not a requirement to rename everything in one PR.

## Planned batches

### 15.1 — hot paths / observability

- Coalesce noisy SSE snapshot triggers.
- Reduce duplicate VOD queue DB reads.
- Add focused tests around changed behavior.
- Record before/after call-frequency expectations where practical.

### 15.2 — runtime state flow

- Audit watcher/VOD state transitions and terminal-state ownership.
- Reduce unnecessary settings/channel reload reads.
- Check mutex/lifecycle lock scope and `.await` boundaries.
- Audit subprocess start/cancel/crash/retry cleanup.

### 15.3 — frontend state pipeline

- Replace wrapper-order dependencies with one explicit state/event ingest path.
- Ensure SSE and polling feed identical consumers.
- Keep Phase 14 notifications behavior identical.

### 15.4 — structural cleanup / CHZZK readiness

- Extract SOOP-specific discovery/auth/stream resolution boundaries.
- Keep RecorderManager, VOD queue, history and notifications platform-neutral where possible.
- Remove dead/duplicate phase-era code only after regression coverage exists.

### 15.5 — final verification

- `cargo fmt --check`
- `cargo test --locked`
- `cargo check --locked`
- `cargo clippy` where the runner/toolchain supports it
- Windows portable package smoke/verify
- LIVE/VOD cancellation and abnormal-process-exit scenarios
- multi-client SSE sanity check
- final secret/privacy scan before public release work

## CHZZK-ready exit criteria

Phase 15 is complete when adding CHZZK no longer requires scattering `if platform == ...` through recorder, history, notification, queue, and generic UI state code. Platform-specific code should mainly resolve platform state/metadata/stream inputs; common infrastructure should own execution and lifecycle.