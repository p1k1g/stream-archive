# Phase 21.4 — Native LIVE controls

## Scope

The Slint LIVE page now consumes the existing shared Rust LIVE runtime instead of creating a second watcher. The dependency path remains:

`Slint -> GUI controller/presentation adapter -> StreamArchiveCore -> NativeWatcherManager -> Recorder/Store`

The native frontend does not call localhost HTTP, open SQLite directly, contact providers directly, spawn Streamlink/FFmpeg, or kill processes. SOOP and CHZZK use the same provider-neutral watcher status and command path.

## Runtime controls

The page shows watcher running/stopped state, engine, start time, channel/recording/offline/error counts, and the channel runtime rows already produced by `NativeWatcherStatus` / `ChannelRuntimeStatus`.

Watcher Start, Stop and Refresh call `StreamArchiveCore::start_watcher`, `stop_watcher` and `watcher_status`. A 1.5 second native status poll is active only while the LIVE page is selected, does not run while an explicit LIVE command is busy, and prevents overlapping poll requests. Poll failures keep the last successful snapshot visible.

Per-channel commands reuse the current watcher contract:

- **Stop current broadcast** -> existing `stop` / `WatcherCommand::StopOnce`. This suppresses the current broadcast; it is not a persistent channel disable.
- **Resume monitoring** -> existing `resume` command.
- **Recheck** -> existing `recheck` command.

All commands use the existing scoped `PLATFORM:account` target form so identical SOOP and CHZZK account strings remain unambiguous.

## Protected broadcasts

`PASSWORD_REQUIRED` rows expose a password entry flow. The password is passed only to `StreamArchiveCore::channel_password`, whose existing watcher implementation keeps it in the in-memory broadcast password store and triggers a recheck. The native UI clears its draft when submitted and never writes the stream password to Settings, diagnostics or SQLite.

## Presentation

`rust-gui/src/live_adapter.rs` is presentation-only. It formats byte counts, maps existing runtime status strings to display labels/tones, creates scoped channel targets and derives which existing actions are meaningful. It does not inspect files, provider APIs or recorder processes.

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

Existing architecture guards already scan every `rust-gui/src/*.rs` module and continue to forbid HTTP, direct SQLite and direct process-control shortcuts in the native frontend.

## Manual Windows acceptance

1. Run `cargo run --locked --manifest-path rust-gui/Cargo.toml`.
2. Open LIVE and confirm the canonical channel rows appear.
3. Start the watcher and confirm state/counts update automatically.
4. Confirm an offline channel remains visible and updates without manual refresh.
5. During an actual broadcast, verify title, broadcast id, output file, current size and start time.
6. Use Recheck.
7. Use **Stop current broadcast** and verify only that broadcast is suppressed; then use **Resume monitoring**.
8. If a protected SOOP broadcast is available, verify PASSWORD_REQUIRED -> password entry -> recheck and confirm the input clears after submit. If this cannot be reproduced, rely on the adapter/service-path tests and inspect the runtime log only for the non-secret acceptance message.
9. Stop the watcher and start it again.
10. Revisit Settings and Diagnostics to verify Phase 21.3 behavior remains intact.

Phase 21.4 intentionally does not add channel CRUD, background/tray daemon behavior, detached recorder ownership, VOD controls, or Settings cleanup. Those remain later Phase 21 work.
