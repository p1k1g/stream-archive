# Phase 19 Runtime Hardening Audit

## 19.1 resource lifetime

The runtime task/process/channel audit covered every `tokio::spawn`, process spawn, SSE producer, watcher command channel, VOD worker, backup timer, job directory, destination claim, and SQLite connection site. SSE uses a bounded channel and its producer exits as soon as the receiver closes. Watcher commands are bounded and watcher/VOD task handles are reaped.

CHZZK VOD external-tool readers were hardened from unbounded/detached readers to bounded channels with explicit join/abort behavior. Success, cancellation, downstream setup failure, and process-failure paths all retain ownership long enough to drain or terminate the work they created.

Job directories, owner locks, cookie files, destination claims, and finalizing paths are scoped resources. Owned children remain protected by exact process ownership and cancellation waits for the owned process tree. SQLite connections/statements remain lexical and the committed settings cache is a read-through snapshot of committed database state, not a second authority.

## 19.2 dead code and dependencies

Removed unused backend config writers/readers, unused provider/session identity helpers, redundant queue wrappers, obsolete compatibility code, and redundant CHZZK metadata state. Phase 19.5 later removed the now-unreferenced INI-based VOD tool settings module as well.

Streamlink, FFmpeg, and yt-dlp remain intentional external media tools. No dependency is removed merely because it is not visible in orchestration code; dependency removal must still be demonstrated by build/tests.

## 19.3 Rust runtime inventory

Watcher orchestration, LIVE/VOD lifecycles, queue, canonical configuration, backup/restore, authentication, cleanup, and maintenance run in Rust. Browser JavaScript, Windows launch/build/package batch files, CI, and regression PowerShell scripts remain outside the service runtime.

The localhost-only native Windows picker still uses a Windows-specific implementation boundary. Cross-platform picker replacement belongs to Phase 20 and must not leak Windows assumptions into provider/orchestration code.

## 19.5 state and source of truth

SQLite is the sole persistent application authority. The canonical database is `data/stream-archive.db` unless `STREAM_ARCHIVE_DATA_DIR` overrides the data directory. Settings, channels, encrypted secrets, VOD queue state, LIVE/VOD history, and backup policy are stored there.

Earlier Phase 19 builds retained INI/TXT compatibility mirrors during migration. Phase 19.5 retires that compatibility path before public release: runtime INI/TXT import/materialization and the tracked example config files are removed. The only retained transition bridge is a bounded filename migration from an existing private-build `soop.db` to `stream-archive.db` when the new filename does not yet exist.

In-memory watcher, queue, VOD, notification, and SSE data are ephemeral delivery/process state. Broadcast receivers and bounded SSE producers remain connection scoped and release on disconnect.

## 19.6 cross-platform boundary and Phase 20

`platform_runtime` owns external CLI environment setup, exact-owned process-tree termination, process identity validation, and private-directory permissions. Windows uses retained process ownership/Job Object logic with exact PID/process identity and ACL hardening; Unix currently has a minimal child/process permission implementation.

Phase 20 should add Unix process-group ownership/termination, non-DPAPI secret storage, native cross-platform picker behavior, launcher/packaging support, executable discovery without Windows suffix assumptions, and Linux/macOS integration coverage.

## Manual regression checklist

- SOOP LIVE start, normal stop, forced stop, stall and restart.
- CHZZK public/authenticated LIVE start and stop; confirm zero-based MPEG-TS.
- SOOP VOD analyze, download, merge and cancel.
- CHZZK public and age-restricted VOD analyze/download/cancel; confirm cookie deletion and final TS.
- Queue retry/cancel, server shutdown with active work, destination collision, stale job scavenging.
- SSE disconnect/reconnect and session revocation.
- Windows picker, DPAPI round trip, native compile, portable package smoke and verification.

## 19.7 runtime contract guards

Historical phase-specific process/release entry points were consolidated into the permanent CI entry point `maintenance/Test-RuntimeContracts.ps1`. It orchestrates focused modules under `maintenance/guards/`: shared helpers, architecture, providers, process lifecycle, storage ownership, security, and release safety.

Behavioral assertions remain Rust tests: destination collision and reusable claims, no-clobber/cancellable publication, stale job cleanup, progress parsing, completed-versus-late-cancel, configuration-cache commits, queue serialization/retry, status transitions, and cookie cleanup. PowerShell contracts concentrate on dependency direction, forbidden patterns, OS boundaries, secrets, workflow triggers, and portable package contents.

`.github/workflows/rust-web-check.yml` is the pull-request runtime workflow. It runs runtime contracts, JavaScript syntax validation, whole-crate rustfmt, Rust tests/check/clippy, Windows compilation, and portable package smoke/verification.

## 19.8 repository and module diet

The tracked repository was inventoried across runtime, frontend, maintenance, workflows, packaging, and documentation. Obsolete WinUI/cloud-era documents were removed and phase-numbered Rust modules were renamed by responsibility: `phase8.rs` became `history_storage.rs`, and `phase9_1.rs` became `local_picker.rs`.

Phase-numbered browser assets were intentionally retained because they form a working layered UI with cross-file globals; renaming them would create broad frontend churn without reducing runtime complexity. `backend/worker.js` was retained because it is still the deployable Cloudflare Worker used by the SOOP provider.

The provider trees, common LIVE/VOD facades, `platform_runtime`, process ownership, security, store, backup, queue, and publication boundaries remain separate. The small root VOD facade remains intentionally thin rather than duplicating provider logic.

### Permanent multi-platform persistence contract

SQLite is the canonical multi-platform source of truth, and persisted channel identity remains `(platform, account)`. Pre-multiplatform rows are upgraded with platform `SOOP`; older compatible `live_recordings`, `vod_jobs`, and `vod_queue` rows likewise receive the SOOP platform identity during schema upgrade.

Backup restore runs the same schema upgrade before runtime caches are refreshed, so restoring an older compatible database preserves the same platform migration semantics as normal startup. No provider is projected into a legacy text-file mirror after the Phase 19.5 cleanup.

## 19.5 namespace and pre-public legacy cleanup

Before public release, product-wide names were moved from historical SOOP Downloader/Recorder identifiers to the `Stream Archive` namespace:

- Cargo package/default binary: `stream-archive-server`;
- Windows launcher: `stream-archive-launcher`;
- portable directory: `dist/stream-archive`;
- canonical database: `stream-archive.db`;
- runtime token directory: `backend/.stream-archive`;
- managed backup prefix/folder: `stream_archive_*` / `stream-archive-backups`;
- product-wide environment variables: `STREAM_ARCHIVE_*`;
- developer/release/package entry points: `RUN_DEV.bat`, `BUILD_RELEASE.bat`, and `BUILD_PORTABLE.bat`.

Provider-specific identifiers such as `SOOP_USERNAME`, `SOOP_PASSWORD`, SOOP URLs, CHZZK cookie names, and provider module names intentionally remain provider-scoped. They are not product branding and should not be renamed into generic application keys.

The cleanup is guarded so obsolete app-wide executable/package names and retired INI/TXT configuration paths do not silently return. Phase 20 can therefore start from a cleaner product namespace and SQLite-only runtime rather than carrying private-development compatibility layers forward.
