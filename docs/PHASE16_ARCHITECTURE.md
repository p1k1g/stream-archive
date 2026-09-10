# Phase 16 multi-platform architecture

Phase 16 keeps existing SOOP behavior while making platform identity and provider boundaries explicit. The goal is that adding another platform does not require provider-specific branches across recorder, queue, history, notifications, or frontend state handling.

## Runtime boundaries

### Common infrastructure

The following layers are platform-neutral and should stay that way:

- `native_watcher.rs`: scheduling, channel state transitions, retry/suppression policy, recorder orchestration
- `recorder.rs`: Streamlink process ownership, file lifecycle, LIVE history lifecycle
- `vod_queue.rs`: queue persistence, claiming, retry/cancel/remove lifecycle
- `store.rs`: SQLite settings/channels/history persistence and migration
- `realtime.rs`: SSE snapshots
- frontend state bus / notifications: consume serialized platform identity without platform networking/auth logic

### Platform registry

`rust-web/src/platform/mod.rs` owns:

- `PlatformId`
- provider capabilities
- provider registry
- VOD platform detection

Legacy data defaults to `SOOP`; persisted channels use `(platform, account)` as identity.

### LIVE providers

`rust-web/src/platform/live.rs` is the common LIVE facade. It exposes platform-neutral broadcast/session/stream data to the watcher.

SOOP-specific LIVE behavior lives in:

- `rust-web/src/platform/soop/live.rs`

This module owns SOOP login/session handling, broadcast discovery, password state, CDN/playlist resolution and SOOP HTTP endpoints.

`native_watcher.rs` must not regain direct SOOP HTTP endpoint handling.

### VOD providers

`rust-web/src/platform/vod.rs` is the common VOD dispatcher used by queue/API code.

SOOP-specific VOD behavior lives in:

- `rust-web/src/platform/soop/vod.rs`

This module owns SOOP cookies, CloudFront authorization, metadata enrichment, manifest probing and SOOP-specific yt-dlp/ffmpeg arguments.

`rust-web/src/vod.rs` remains a compatibility facade so existing API/queue imports do not need provider-specific knowledge.

## Persistence

Phase 16 upgrades existing SQLite data in place:

- `channels`: `(platform, account)` composite primary key; legacy rows become `SOOP`
- `live_recordings.platform`: defaults legacy rows to `SOOP`
- `vod_jobs.platform`: defaults legacy rows to `SOOP`
- `vod_queue.platform`: defaults legacy rows to `SOOP`

Backup restore runs the same schema upgrade before refreshing runtime caches.

The legacy `SOOP_LIVE_CHANNELS.txt` compatibility mirror remains SOOP-only. SQLite is the canonical multi-platform source of truth.

## Phase 17 — CHZZK LIVE handoff

Adding CHZZK LIVE should primarily require:

1. Add `PlatformId::Chzzk` parsing/serialization.
2. Register a CHZZK provider under `platform/chzzk/`.
3. Implement the common LIVE facade session variant for CHZZK.
4. Implement CHZZK account lookup, LIVE discovery and stream resolution inside the CHZZK provider.
5. Expose CHZZK as a selectable platform in channel UI/API.

Recorder, history, SSE, browser notifications and channel lifecycle logic should not require CHZZK-specific network/auth branches.

## Phase 18 — CHZZK VOD handoff

After CHZZK LIVE is stable:

1. Register CHZZK VOD URL recognition.
2. Add CHZZK VOD manager/provider implementation.
3. Extend the common `platform/vod.rs` dispatcher to hold/route the CHZZK manager.
4. Keep queue state, cancellation, retry, history and notifications common.

Provider-specific metadata/auth/download mechanics stay under `platform/chzzk/vod.rs`.

## Architecture invariants

CI guards enforce these rules:

- SOOP LIVE endpoints stay under `platform/soop/live.rs`.
- SOOP VOD auth/endpoints stay under `platform/soop/vod.rs`.
- root VOD facade has no provider/process implementation.
- VOD queue has no provider auth/network implementation.
- LIVE/VOD owned subprocess trees continue to be terminated by recorded PID rather than image name.
- Phase 15 shared frontend state bus remains the transport-independent state path.

## Phase 16 exit criteria

Phase 16 is complete when:

- existing SOOP channels/history/queue data migrate without loss and retain `SOOP` identity;
- LIVE watcher consumes the common LIVE facade;
- SOOP LIVE network/auth/stream resolution is isolated in the SOOP provider;
- VOD API/queue consume the common VOD facade;
- SOOP VOD implementation is isolated in the SOOP provider;
- architecture/process/privacy/unit/compile/package CI is green;
- existing SOOP LIVE and VOD behavior passes a local smoke test before merge.
