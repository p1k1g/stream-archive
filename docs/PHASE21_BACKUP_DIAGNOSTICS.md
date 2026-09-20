# Phase 21.7 — Native Backup / Restore + Diagnostics / Log Viewer

Phase 21.7 moves maintenance operations into the Windows Slint UI while preserving the Rust/SQLite runtime, managed-backup format, exact process ownership, and current Web compatibility.

## Architecture

```text
Slint Maintenance UI
        ↓
rust-gui controller / maintenance_adapter
        ↓
StreamArchiveCore
        ↓
backup_service / diagnostics / LogBuffer
        ↓
canonical SQLite + managed backup directory
```

The GUI does not open SQLite, copy/replace database files, calculate backup hashes, call localhost HTTP, decrypt secrets, or spawn/kill media processes.

`rust-web/src/backup_service.rs` owns reusable backup policy, list/create/restore/integrity/retention behavior. `rust-web/src/backup.rs` remains the Axum/Web adapter and keeps browser session invalidation as a Web-only concern.

## Restore safety

Native restore is rejected while any of these conditions are true:

- LIVE watcher is running or has active recordings;
- a direct VOD operation is running;
- VOD Queue has pending or active work;
- the selected managed backup does not have `integrity == OK`.

A successful restore creates a `pre_restore` safety backup before replacing canonical SQLite state. Settings, Channels, Queue, History, Backup and Diagnostics snapshots are reloaded afterward. The watcher is not automatically restarted.

## Runtime logs

The Logs section reads the existing bounded `LogBuffer` through `StreamArchiveCore::runtime_logs`. The view requests at most 200 recent lines, uses non-overlapping polling only while Maintenance / Logs is active, and does not persist a second log history.

## Manual Windows QA

1. Launch the native GUI and open **Maintenance**.
2. Verify the effective backup directory is shown.
3. Verify Automatic backup, Interval hours, Keep count and Retention days are populated.
4. Click **Create backup**.
5. Verify a new managed backup appears immediately.
6. Verify its integrity is **OK**.
7. Verify filename, kind, local creation time, size and SHA256 are displayed.
8. When directory editing is enabled, use **Browse...** and select another backup directory.
9. Cancel the native picker and verify the previous draft path remains.
10. With `STREAM_ARCHIVE_BACKUP_DIR` set, verify the effective directory is read-only and the UI explains why.
11. Start the LIVE watcher and verify Restore is rejected.
12. During an actual LIVE recording, verify Restore is rejected.
13. Start a direct VOD operation and verify Restore is rejected.
14. Leave Queue work pending/running and verify Restore is rejected.
15. When runtime is idle, click Restore and verify the confirmation panel shows the exact filename and safety-backup warning.
16. Click **Cancel** and verify no data changes.
17. Create backup A, change a harmless setting/channel, then confirm Restore of backup A.
18. Verify a `pre_restore` backup appears.
19. Verify Settings / Channels / Queue / History / Backup state reloads from restored canonical SQLite.
20. Verify the watcher remains stopped after restore.
21. Open **Diagnostics** and verify canonical backend/database/data directory/tool checks.
22. Verify Backup directory / policy diagnostics are present.
23. Confirm Diagnostics Refresh does not create the missing diagnostic directories or execute media tools.
24. Open **Logs** and verify recent runtime log lines appear.
25. Turn auto refresh on and verify updates arrive without UI freezes.
26. Leave Logs open while runtime activity occurs and verify memory/UI remain responsive.
27. Verify passwords, API keys, NID cookies, management tokens or provider cookies are not shown in logs.
28. Recheck LIVE watcher start/stop and actual recording.
29. Recheck SOOP/CHZZK VOD Analyze + direct Download + Cancel.
30. Recheck Add to Queue / Cancel / Retry / Remove / persistence and History filters.
31. Recheck Phase 21.6 PART semantics: Analyze selects all; first PART click selects only that PART; later clicks form a subset; Direct Download and Queue use the same subset.
32. Exit the application while it owns media children and verify exact owned-child cleanup still works.

## Automated validation

Run from repository root:

```powershell
cargo fmt --manifest-path rust-web\Cargo.toml -- --check
cargo test --locked --manifest-path rust-web\Cargo.toml
cargo check --locked --manifest-path rust-web\Cargo.toml

cargo fmt --manifest-path rust-gui\Cargo.toml -- --check
cargo test --locked --manifest-path rust-gui\Cargo.toml
cargo check --locked --manifest-path rust-gui\Cargo.toml

.\maintenance\Test-RuntimeContracts.ps1
```

The repository CI additionally runs Linux/macOS core regression and the current Windows portable-package smoke/verification. Phase 21.7 does not switch portable startup to the Slint executable; that belongs to Phase 21.8.
