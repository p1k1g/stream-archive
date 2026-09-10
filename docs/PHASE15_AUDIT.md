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

## Audit findings and results

### P1: realtime snapshot amplification — resolved in 15.1

`realtime.rs` previously emitted the full watcher/VOD/queue/log snapshot every second, while each log broadcast could additionally trigger another complete snapshot. Streamlink stderr bursts could therefore multiply process-state and SQLite reads for every connected SSE client.

Result: log-triggered snapshots are coalesced to a 250 ms minimum interval while the regular one-second snapshot and five-second authenticated-session revalidation remain intact. Regression coverage verifies the coalescing interval and multiple log subscribers.

### P1: VOD queue snapshot opened SQLite repeatedly — resolved in 15.1

`VodQueueManager::snapshot()` previously loaded queue rows and queued count through separate connection/read paths.

Result: one connection is reused for the queue snapshot, and single-item lookup uses the primary-key query directly instead of loading a larger list first.

### P1: repeated VOD history writes — resolved in 15.1

The runtime can observe an unchanged terminal VOD status repeatedly. Re-upserting identical values caused unnecessary SQLite writes and `updated_at` churn.

Result: VOD history updates are skipped when persisted values are unchanged. A regression test verifies that a no-op upsert does not change `updated_at`.

### P1: frontend state had multiple update paths — resolved in 15.3

The browser can receive state through REST polling or SSE, while later phase scripts previously wrapped functions such as `api`, `renderStatus`, and `applyRealtimeSnapshot`. Correctness depended on script load/wrapper order.

Result: `app.js` now exposes one `StreamArchiveState` event bus. REST and SSE publish transport results into the bus; Phase 13 queue handling and Phase 14 notifications consume state events instead of replacing core functions. CI architecture guards prevent legacy wrapper patterns from being reintroduced.

### P1: platform-specific logic mixed with orchestration — boundary established in 15.4

`native_watcher.rs` and `vod.rs` still contain substantial SOOP-specific implementation details. Moving all of that code in one optimization PR would create a high-risk rewrite.

Result: Phase 15 introduces an explicit `PlatformProvider` boundary for platform identity, capabilities, account validation, channel lookup request construction/response parsing, and VOD URL ownership. Existing channel-resolution behavior now routes through the provider contract while compatibility entry points remain intact.

Phase 16 should continue the extraction by moving SOOP LIVE discovery/auth/stream-resolution and SOOP VOD metadata/auth details behind provider-specific modules. Recorder/process/history/queue/frontend state should remain common infrastructure rather than being duplicated for CHZZK.

### P2: periodic SQLite configuration reads — resolved in 15.2

The native watcher checks settings and channels regularly for hot reload. The original path materialized unchanged SQLite data repeatedly.

Result: `Store` keeps committed settings/channels snapshots in memory while SQLite remains canonical. Cache refresh occurs after committed writes and database restore, preserving restart recovery and INI/TXT compatibility without repeated idle SELECT work.

### P2: SQLite access patterns — improved in 15.1/15.2

`Store` and VOD queue intentionally use different access patterns, but hot paths no longer open/read SQLite redundantly. Synchronous SQLite guards are not held across asynchronous work in the changed paths.

Further consolidation should be driven by measured contention rather than replacing the current WAL/short-operation model speculatively.

### P2: duplicated schema/bootstrap responsibilities — resolved in 15.2

`Store::open()` and schema repair previously carried overlapping DDL definitions.

Result: both paths use the same schema definition. Restore flow applies the current schema before refreshing runtime caches, so older compatible backups are repaired to the current runtime shape.

### P2: large modules / phase-named frontend files — partially resolved, structural follow-up retained

The largest Rust modules and phase-numbered browser assets are still intentionally present to avoid a broad rename/rewrite in the same PR as runtime changes.

Phase 15 removes the most dangerous coupling — frontend wrapper order — and establishes the provider boundary. Physical file/module decomposition can continue in Phase 16 as platform adapters are extracted.

Target direction remains:

```text
Rust
  api/
  config/
  db/
  platform/
    soop/
    chzzk/
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

## Completed batches

### 15.1 — hot paths / observability

- Coalesced noisy SSE snapshot triggers.
- Reduced duplicate VOD queue DB reads.
- Replaced broad queue item lookup with direct primary-key lookup.
- Skipped unchanged VOD history writes.
- Added focused regression coverage.

### 15.2 — runtime state flow

- Added committed settings/channels runtime caches while keeping SQLite canonical.
- Kept watcher hot-reload semantics without repeated idle database materialization.
- Unified schema initialization/repair and restore ordering.
- Audited mutex/lifecycle boundaries in changed paths.

### 15.3 — frontend state pipeline

- Added one explicit shared browser state bus.
- Routed REST/SSE state into the same consumer pipeline.
- Removed Phase 13/14 wrapper-order coupling.
- Preserved Phase 14 LIVE/VOD browser notification behavior through subscribers.

### 15.4 — structural cleanup / CHZZK readiness

- Added the platform provider contract and explicit capabilities.
- Routed channel validation/lookup through the provider boundary.
- Added VOD platform-recognition ownership to the provider contract for the next extraction step.
- Kept recorder, VOD queue, history, notifications, and browser state generic.

### 15.5 — final verification

Blocking CI now includes:

- Phase 15 architecture regression guard.
- Owned PID/process-tree lifecycle guard for LIVE and VOD cancellation paths.
- Public-release secret/privacy scan across tracked text/config files.
- JavaScript syntax validation using the self-hosted runner's embedded Node when Node is not on PATH.
- `rustfmt --check` for the new provider boundary files.
- `cargo test --locked`.
- `cargo check --locked`.
- Whole-repository `cargo clippy --all-targets` as an advisory baseline.
- Windows portable package smoke test and package verification.

The repository contains older pre-Phase-15 formatting/clippy style warnings. They are deliberately not converted into blocking `-D warnings` in this optimization PR because doing so would require a broad unrelated rewrite. Newly introduced Phase 15 architectural/lifecycle/privacy invariants are instead enforced by focused blocking guards.

Process lifecycle audit confirms that LIVE explicit stop targets its owned Streamlink PID tree and VOD cancellation routes yt-dlp/ffmpeg subprocesses through the owned-child stop helper. Image-name termination is prohibited by CI guard. LIVE abnormal recorder exits remain classified as failures rather than normal completion.

## CHZZK handoff / exit criteria

Phase 15 is complete when the second platform can be added without reintroducing frontend transport/wrapper coupling or duplicating recorder/history/queue/notification infrastructure.

That criterion is met at the common-infrastructure layer. Phase 16 is responsible for completing provider extraction of the remaining SOOP-specific LIVE discovery/auth/stream-resolution and VOD metadata/auth internals, then implementing the CHZZK provider alongside SOOP rather than scattering platform conditionals through shared code.
