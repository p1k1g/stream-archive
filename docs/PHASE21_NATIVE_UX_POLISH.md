# Phase 21.9 — Native UX Polish / Storage / Information Architecture

Phase 21.9 builds on the Phase 21.8 Windows Native portable cutover. It keeps the existing Web compatibility server/launcher intact while making the Slint frontend more suitable for daily Windows use.

## Scope

This slice covers:

- shared storage-capacity diagnostics surfaced in the Native LIVE page;
- backup policy in Settings and backup/restore operations in Maintenance;
- History/status UX regression protection from Phase 21.8;
- Windows destination-claim sidecar visibility without weakening no-clobber locking;
- a Native/Web parity audit to define the later Phase 22 cleanup boundary.

SQLite remains the canonical source of truth. Slint calls `StreamArchiveCore`; it does not call localhost HTTP, open a second SQLite authority, or manage media processes directly.

## Native storage

Storage probing lives in `rust-web/src/storage_service.rs` and is exposed through `StreamArchiveCore::storage_snapshot`. The existing Web storage route delegates to the same service.

The snapshot includes:

- configured `OUTPUT_DIR`;
- channel-specific output-directory overrides;
- canonical SQLite data directory;
- total/free bytes and used percentage;
- `MIN_FREE_SPACE_GB`;
- one row per collapsed Windows volume.

Status meaning is unchanged from the retained Web UI:

| Canonical | Native label | Rule |
|---|---|---|
| `OK` | 정상 | free > 2 × threshold |
| `WARN` | 주의 | threshold < free <= 2 × threshold |
| `CRITICAL` | 공간 부족 | free <= threshold |
| `ERROR` | 확인 불가 | path/probe failed |

The recorder still uses `MIN_FREE_SPACE_GB` as its real low-space boundary. WARN is presentation-only.

Native refresh is bounded: initial load, explicit refresh, return to LIVE, and a 30-second timer only while the LIVE page is active.

## Backup information architecture

Native backup configuration and operations now have separate presentation responsibilities.

### Settings — backup policy

Settings owns:

- automatic backup enabled/disabled;
- interval hours;
- maximum managed backup count;
- retention days;
- backup directory.

`0` for maximum count or retention days means unlimited, matching the existing shared policy semantics.

### Maintenance — backup / restore

Maintenance owns:

- create backup now;
- managed backup list;
- kind/time/size/SHA-256/integrity;
- restore request and confirmation;
- pre-restore safety behavior.

Both presentations use the existing `MaintenanceState` callbacks and `StreamArchiveCore -> BackupManager`; no second policy store is introduced.

## History and state UX

Phase 21.8 History status behavior remains part of the Native contract. Korean aliases, canonical English states, and partial terms remain supported. Representative examples:

- `완료` -> `COMPLETED`
- `STOP` -> `STOPPED`
- `병합` -> `MERGING`
- `진행` -> active VOD states
- `취소` -> cancellation-related states
- `녹화` -> `RECORDING`

The `전체 / LIVE / VOD` controls retain fixed widths so selecting a filter cannot shift surrounding controls.

## Destination claim sidecars

CHZZK no-clobber publication uses `<final-media>.stream-archive.claim` as a reusable lock anchor. The file must not simply be deleted in `DestinationClaim::drop`: after unlock, unlinking and recreating the pathname can allow concurrent processes to lock different file identities.

Phase 21.9 therefore preserves the exact lock identity strategy and applies the Windows Hidden attribute to the sidecar. This improves normal Explorer UX while retaining:

- concurrent destination ownership;
- reusable claim path;
- late external collision detection;
- `_02`, `_03` collision suffix behavior;
- crash recovery;
- existing `.stream-archive.finalizing` cleanup.

Hiding is best-effort. A failure to change the Explorer attribute must not make an otherwise safe VOD download fail. Existing visible sidecars become hidden when that destination is claimed again.

## Native/Web parity audit

### A — Native general-user parity

The Native application is expected to own normal local-user workflows:

- Settings and native path pickers;
- provider/channel configuration;
- LIVE watcher and recording controls;
- storage-capacity status;
- SOOP/CHZZK VOD analyze/download;
- Queue and History;
- backup policy;
- backup/restore actions;
- diagnostics and runtime logs.

### B — Web compatibility-specific concerns

These remain valid Web-only or advanced-use concerns for now:

- browser session/cookie/CSRF handling;
- local Web management/recovery token;
- reverse-proxy / HTTPS remote access;
- Axum listener/bind behavior;
- browser-specific presentation.

They should not be copied into Slint merely for numeric parity.

### C — Phase 22 candidates

After Phase 21.9 regression/manual QA, Phase 22 can decide whether and how to remove:

- browser frontend assets;
- launcher dependency;
- Web presentation-only routes;
- local picker Web bridge;
- compatibility packaging that is no longer required.

Phase 21.9 does **not** remove `stream-archive-server.exe`, `stream-archive-launcher.exe`, `RUN_WEB.bat`, Axum routes, or browser authentication/session code.

## Automated checks

Run:

```powershell
cargo fmt --manifest-path .\rust-web\Cargo.toml -- --check
cargo fmt --manifest-path .\rust-gui\Cargo.toml -- --check

cargo check --locked --manifest-path .\rust-web\Cargo.toml
cargo check --locked --manifest-path .\rust-gui\Cargo.toml

cargo test --locked --manifest-path .\rust-web\Cargo.toml
cargo test --locked --manifest-path .\rust-gui\Cargo.toml

Set-ExecutionPolicy -Scope Process Bypass
.\maintenance\Test-RuntimeContracts.ps1
```

On Windows use the same serialized test conditions already encoded by CI when process-lifecycle tests require them.

## Portable manual QA

Build and test the assembled product rather than raw Cargo binaries:

```powershell
.\BUILD_PORTABLE.bat
```

Then use `dist\stream-archive`:

1. Double-click `StreamArchive.exe`.
2. Open LIVE and verify storage rows show the actual configured drive, free/total capacity, usage, and Korean status.
3. Verify multiple LIVE paths on the same drive collapse to one volume row while their roles/paths remain visible.
4. Refresh storage manually and return to LIVE from another page.
5. Check Settings -> 백업 정책; edit/save policy and restart to verify persistence.
6. Check 관리 -> 백업 / 복원; create a backup and inspect time/kind/size/SHA-256/integrity.
7. Confirm restore remains gated while Watcher/VOD/Queue work is active and uses the pre-restore safety backup.
8. In History test `STOP`, `병합`, `진행`, `취소`, and `녹화`.
9. Switch `전체 / LIVE / VOD` and confirm the filter row does not shift.
10. Exercise long titles, channel names, URLs, file paths, empty/loading/error states, and normal/maximized window sizes.
11. Complete a CHZZK VOD download. With normal Explorer hidden-file settings, the `.stream-archive.claim` sidecar should not be visible; with hidden items enabled it may be inspected and should remain as the reusable lock anchor.
12. Confirm no stale `.stream-archive.finalizing` file remains after normal completion/cancellation.
13. Close/reopen the Native app and verify canonical SQLite state remains.
14. Run `RUN_WEB.bat` and verify the retained browser fallback still starts.

## Deferred

Phase 22 decides the actual legacy Web/launcher cleanup boundary after this Native parity and manual QA completes. New providers, media formats, media-tool bundling, Linux/macOS GUI packaging, database authority changes, and large provider algorithm refactors remain out of scope.
