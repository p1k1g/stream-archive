# Phase 21.4 — Native LIVE controls

## Scope

The Slint LIVE page now consumes the existing shared Rust LIVE runtime instead of creating a second watcher. The dependency path remains:

`Slint -> GUI controller/presentation adapter -> StreamArchiveCore -> NativeWatcherManager -> Recorder/Store`

The native frontend does not call localhost HTTP, open SQLite directly, contact providers directly, spawn Streamlink/FFmpeg, or kill processes. SOOP and CHZZK use the same provider-neutral watcher status and command path.

The Phase 21.4 follow-up also closes the minimum configuration gap required to test LIVE without opening the legacy Web UI: native channel CRUD and provider credential entry now reuse the canonical SQLite/shared-core boundaries.

## Runtime controls

The page shows watcher running/stopped state, engine, start time, channel/recording/offline/error counts, and the channel runtime rows already produced by `NativeWatcherStatus` / `ChannelRuntimeStatus`.

Watcher Start, Stop and Refresh call `StreamArchiveCore::start_watcher`, `stop_watcher` and `watcher_status`. A 1.5 second native status poll is active only while the LIVE page is selected, does not run while an explicit LIVE command is busy, and prevents overlapping poll requests. Poll failures keep the last successful snapshot visible.

Per-channel commands reuse the current watcher contract:

- **Stop current broadcast** -> existing `stop` / `WatcherCommand::StopOnce`. This suppresses the current broadcast; it is not a persistent channel disable.
- **Resume monitoring** -> existing `resume` command.
- **Recheck** -> existing `recheck` command.

All commands use the existing scoped `PLATFORM:account` target form so identical SOOP and CHZZK account strings remain unambiguous.

## Native channel configuration

The new **Channels** page reads and saves the same canonical `Channel` rows used by Web through `StreamArchiveCore::channels` and `StreamArchiveCore::update_channels`.

The native draft supports:

- add / remove
- SOOP / CHZZK platform toggle
- enabled / disabled toggle
- channel name
- account / canonical channel id
- optional per-channel output directory
- discard/reload from SQLite

Validation remains in the shared `validate_channels` path. The GUI does not duplicate provider account rules or write SQLite directly. A running watcher sees the saved canonical list through its existing reload policy rather than a GUI-specific hot-reload mechanism.

## Provider credentials

Settings now exposes the provider values required for standalone native LIVE setup:

- `SOOP_USERNAME`
- `SOOP_PASSWORD`
- `CLOUDFLARE_WORKER_URL`
- `CLOUDFLARE_API_KEY`
- `CHZZK_NID_AUT`
- `CHZZK_NID_SES`

Plain settings and secret drafts are saved together through `StreamArchiveCore::update_provider_configuration`. The shared validators remain authoritative and secret values use the existing protected storage boundary (Windows CurrentUser DPAPI or the supported native Unix secret store). Blank secret inputs preserve the stored secret. Saved secrets are never read back into Slint; the UI receives only configured/not-configured status.

The **Test SOOP + Worker** action runs through `StreamArchiveCore::test_soop_auth`, using the currently saved SOOP username/password and Cloudflare Worker URL/API key. It does not return or log plaintext credentials. CHZZK authentication remains cookie-based; the runtime does not currently expose a standalone CHZZK network test endpoint, so native Settings reports whether NID_AUT/NID_SES are configured and the cookies are consumed by normal restricted LIVE/VOD requests.

## Protected broadcasts

`PASSWORD_REQUIRED` rows expose a password entry flow. The password is passed only to `StreamArchiveCore::channel_password`, whose existing watcher implementation keeps it in the in-memory broadcast password store and triggers a recheck. The native UI clears its draft when submitted and never writes the stream password to Settings, diagnostics or SQLite.

## Presentation

`rust-gui/src/live_adapter.rs` is presentation-only. It formats byte counts, maps existing runtime status strings to display labels/tones, creates scoped channel targets and derives which existing actions are meaningful. It does not inspect files, provider APIs or recorder processes.

`rust-gui/src/channels_adapter.rs` owns only the unsaved native channel draft. Validation and persistence remain in the shared core.

The status strings remain owned by the runtime. Unknown future statuses are displayed rather than rejected or converted into new runtime state.

## Shutdown ownership

The GUI worker calls `StreamArchiveCore::shutdown()` after its request channel closes. This preserves the existing process-ownership rule: the GUI does not detach the watcher or recorder and only the shared runtime cleans up children it owns.

## Automated coverage

GUI-independent tests cover:

- SOOP/CHZZK scoped channel identity
- accepted StopOnce/Resume/Recheck action names
- password-required presentation
- running/stopped action enablement
- suppressed/resume state
- recording metadata and size formatting without filesystem probing
- channel draft add/edit/remove/platform/enabled state
- stale channel edit indices

Shared-core coverage verifies that native provider settings reuse the existing safe-setting validation boundary. Existing architecture guards scan every `rust-gui/src/*.rs` module and continue to forbid HTTP, direct SQLite and direct process-control shortcuts in the native frontend.

## Manual Windows acceptance

1. Run `cargo run --locked --manifest-path rust-gui/Cargo.toml`.
2. Open **Settings** and enter SOOP username, Cloudflare Worker URL and any required SOOP/Worker or CHZZK secret values. Save and confirm the configured indicators update while secret plaintext is not read back.
3. With saved SOOP + Worker credentials, use **Test SOOP + Worker** and confirm success or a useful authentication error.
4. Open **Channels**, add a SOOP or CHZZK channel, save, reload and confirm the row persists. Remove a test row and confirm removal persists after reload.
5. Open LIVE and confirm the canonical channel rows appear.
6. Start the watcher and confirm state/counts update automatically.
7. Confirm an offline channel remains visible and updates without manual refresh.
8. During an actual broadcast, verify title, broadcast id, output file, current size and start time.
9. Use Recheck.
10. Use **Stop current broadcast** and verify only that broadcast is suppressed; then use **Resume monitoring**.
11. If a protected SOOP broadcast is available, verify PASSWORD_REQUIRED -> password entry -> recheck and confirm the input clears after submit. If this cannot be reproduced, rely on the adapter/service-path tests and inspect the runtime log only for the non-secret acceptance message.
12. Stop the watcher and start it again.
13. Revisit Settings and Diagnostics to verify Phase 21.3 behavior remains intact.

Phase 21.4 still intentionally does not add background/tray daemon behavior, detached recorder ownership, VOD controls, full Settings cleanup, or a separate CHZZK credential probe. Those remain later Phase 21 work.
