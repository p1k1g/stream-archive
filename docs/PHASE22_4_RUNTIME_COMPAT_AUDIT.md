# Phase 22.4 — Runtime/Core Compatibility Cleanup & Legacy Naming Audit

## 1. Scope

Phase 22.4 audits the compatibility surface left after Phase 22.3 removed the browser/Axum presentation layer.

This phase intentionally keeps:

- the `rust-web/` directory;
- Cargo package/binary name `stream-archive-server`;
- `stream-archive-cli serve`;
- `StreamArchive.exe`, `RUN.bat`, and `RUN_HEADLESS.bat`;
- `data/stream-archive.db`;
- existing SQLite schema, persisted setting names, provider behavior, process ownership, and packaging behavior.

No user-facing feature is added or intentionally changed.

## 2. Baseline

Baseline is main after Phase 22.3 PR #91 merged.

Phase 22.3 established:

- Slint Native UI as the default Windows presentation;
- a presentation-neutral `StreamArchiveCore`;
- `stream-archive-server` as a compatibility-named headless runtime with no HTTP listener;
- browser/Axum/auth/session/SSE/local-picker/launcher removal;
- Native + optional headless portable packaging.

The Phase 22.3 validation baseline was GitHub Actions run #759 with Linux/macOS/Windows core checks and the Windows package/runtime-contract job passing.

## 3. Compatibility inventory

### A. Canonical shared runtime — retained

The following remain canonical runtime/core code:

- `StreamArchiveCore`;
- `Store`, canonical SQLite migrations and settings cache;
- provider configuration and protected secrets;
- SOOP and CHZZK LIVE/VOD providers;
- `NativeWatcherManager` and `RecorderManager`;
- VOD, Queue, History, Backup/Restore and Storage services;
- diagnostics/runtime logs;
- tool discovery;
- Windows Job Object and Unix process-group ownership;
- CHZZK job/destination ownership locks;
- retained process-lifecycle safety boundaries.

### B. Required compatibility boundaries — retained

- `stream-archive-server` package/binary name;
- `stream-archive-cli serve` sibling/PATH lookup;
- `RUN_HEADLESS.bat`;
- `soop.db` -> `stream-archive.db` bounded filename migration;
- provider/settings persistence keys already used by existing databases;
- process compatibility helpers protected by runtime contracts.

### C. Dead compatibility shims — removed

The following `StreamArchiveCore` public facades had no production, Native, CLI, headless, test-contract or guard caller and were removed:

- `watcher()`;
- `vod()`;
- `queue()`;
- `backups()`;
- `config_write_lock()`;
- `lifecycle_lock()`;
- `update_settings()`;
- `update_secrets()`;
- `update_vod_tool_settings()`;
- `storage_check()`.

These removals do not remove the underlying manager/lock/service. Internal ownership remains in `StreamArchiveCore`.

### D. Legacy naming only — retained

- `rust-web/`;
- Cargo package/binary `stream-archive-server`.

These names are legacy, but the code behind them is active shared runtime code.

### E. Historical documentation — retained

Phase 19/20/21/22.1/22.2/22.3 documents remain historical records and are not rewritten to look like the current architecture.

### F. Stale current source terminology — cleaned

Current runtime comments referring to values loaded from the old Web path or the old Web settings adapter were rewritten in presentation-neutral terms.

## 4. Removed compatibility code

Phase 22.4 removes only unreferenced `StreamArchiveCore` facade methods.

It does **not** remove:

- process ownership compatibility functions;
- Store/global-store compatibility;
- SQLite migrations;
- serialization fields;
- provider abstractions;
- Queue lifecycle locking;
- restore lifecycle locking;
- secret-storage boundaries.

The architecture guard now rejects reintroduction of the removed public manager/lock and generic configuration facades.

## 5. Retained compatibility boundaries

The following areas were deliberately treated as architecture contracts rather than compiler dead code:

- `terminate_owned`;
- `terminate_owned_checked`;
- Windows owned-tree termination;
- `OwnedTreeJob`;
- `OwnedProcessGroup`;
- process snapshot/identity helpers;
- Unix process-group adoption rules;
- CHZZK job/destination locks;
- no-clobber/finalizing behavior;
- canonical Store migration and settings persistence.

## 6. API visibility changes

### Removed public API

- manager/lock escape hatches listed in section 3C;
- generic settings/secret/VOD-tool write facades with no caller;
- unused single-path Core storage facade.

### `pub` -> private

- `StreamArchiveCore::vod_tool_settings()`.

`StreamArchiveCore::settings()` remains public because the Native controller reads the canonical safe-settings snapshot directly. The CI compile check caught this external caller during the audit, so its visibility was retained.

`vod_tool_settings()` remains an internal helper used by canonical shared-core services/tests.

### Retained public APIs

Public APIs with actual binary/Native callers or explicit architecture contracts remain public, including:

- `StreamArchiveCore::open`;
- backend/store/log access used by runtime bootstrap;
- canonical safe-settings access used by the Native controller;
- Native environment/provider/channel services;
- watcher start/stop/status and scoped channel actions;
- VOD analyze/download/status/cancel;
- Queue operations;
- History;
- storage snapshot;
- Backup/Restore;
- runtime logs;
- auto-backup/history-sync startup;
- shutdown.

Public module visibility is not broadly collapsed in this phase because `rust-gui`, package binaries, and compatibility naming still cross the library boundary.

## 7. Environment variable audit

### Active runtime variables

| Variable | Current role |
| --- | --- |
| `STREAM_ARCHIVE_BACKEND_DIR` | Explicit backend directory for runtime/CLI resolution |
| `STREAM_ARCHIVE_DATA_DIR` | Explicit canonical data directory / SQLite location |
| `STREAM_ARCHIVE_BACKUP_DIR` | Backup directory override; Native backup directory becomes read-only |
| `STREAM_ARCHIVE_START_WATCHER` | Optional headless runtime watcher auto-start used by CLI `serve --watch` |

Provider/tool keys such as `SOOP_USERNAME`, `SOOP_PASSWORD`, `CHZZK_NID_AUT`, `CHZZK_NID_SES`, `STREAMLINK_PATH`, `YT_DLP_PATH`, and `FFMPEG_PATH` are persisted provider/runtime settings, not obsolete Web environment variables.

### Test-only

Process lifecycle tests may use `STREAM_ARCHIVE_TEST_PID_FILE`; this is test infrastructure, not a product configuration contract.

### Obsolete Web-only

`STREAM_ARCHIVE_BIND` and `STREAM_ARCHIVE_TOKEN` no longer have an active runtime read path after Phase 22.3. Current references are limited to historical documentation and/or guards that prevent retired Web state from returning.

No active environment variable was renamed or removed in Phase 22.4.

## 8. Persisted config audit

The canonical persisted setting allowlists/defaults contain no current setting key named with:

- `WEB`;
- `SERVER`;
- `BIND`;
- `TOKEN`;
- `HTTP`;
- `BROWSER`;
- `LAUNCHER`.

No SQLite migration is required for Phase 22.4.

Compatibility intentionally retained:

- `FILE_NAME_PATTERN=LEGACY` is an existing setting value/behavior, not a Web presentation key;
- `soop.db` filename migration remains bounded compatibility for existing installs;
- existing provider/tool/backup setting keys remain stable for backup/restore and existing user databases.

## 9. `rust-web` naming dependency map

Current tracked dependencies include:

- `rust-gui/Cargo.toml` path dependency `../rust-web`;
- `BUILD_PORTABLE.bat` manifest/target paths;
- `RUN_DEV.bat`;
- GitHub Actions manifest paths and Rust cache workspace paths;
- release/package guard patterns;
- contribution/operations/developer commands and current architecture documentation;
- Cargo lock/package metadata generated from the current manifest path.

A directory rename is mechanically broad but mostly repository-internal. It does not require a SQLite/config migration.

### Phase 22.5 readiness

**SAFE FOR PHASE 22.5** for a coordinated **directory/path rename only**, provided all tracked manifest/cache/script/doc/guard references are changed atomically and historical Phase documents remain historical.

This does not imply that the Cargo package/binary should be renamed in the same commit.

## 10. `stream-archive-server` naming dependency map

Current production/packaging dependencies include:

- `rust-web/Cargo.toml` package name and `default-run`;
- Rust library crate import name `stream_archive_server`;
- `rust-gui/Cargo.toml` dependency package name;
- `stream-archive-cli serve` sibling/PATH binary lookup;
- `RUN_DEV.bat`;
- `BUILD_PORTABLE.bat`;
- `RUN_HEADLESS.bat` generated package entry;
- release checksum/package verification;
- `maintenance/Write-ReleaseMetadata.ps1` package lookup;
- offline backup/restore scripts that check for a running `stream-archive-server` process;
- Cargo lockfiles and current operational documentation.

External user/operator scripts may also invoke the binary name directly.

### CLI impact

A hard binary rename would break the current `stream-archive-cli serve` lookup unless the CLI gains dual-name/compatibility lookup.

### Portable package impact

The binary filename, generated `RUN_HEADLESS.bat`, checksums, package verification and maintenance process checks must migrate together.

### Phase 22.5 readiness

**NOT RECOMMENDED YET** as an unconditional hard rename.

If Phase 22.5 chooses to rename it, the safer design is a deliberate compatibility migration, for example:

1. introduce the new runtime name;
2. allow CLI lookup of new and old names during a bounded transition;
3. update maintenance process checks/package verification/release metadata atomically;
4. decide whether an alias/wrapper is required for existing operator scripts.

Candidate names for a later decision include `stream-archive-runtime` and `stream-archive-headless`.

## 11. Packaging / CI dependencies

Phase 22.4 does not change the package shape:

```text
RUN.bat
  -> StreamArchive.exe
  -> StreamArchiveCore

RUN_HEADLESS.bat
  -> stream-archive-server.exe
  -> StreamArchiveCore
```

The package must continue to reject retired Web artifacts.

CI continues to require:

- rust-web fmt/test/check/strict Clippy on Windows/Linux/macOS;
- rust-gui fmt/check/test/strict Clippy on Windows;
- runtime contracts;
- release safety;
- source metadata smoke;
- Windows portable build and verification.

## 12. Historical vs current documentation

Historical Phase documents are intentionally left unchanged.

Current source comments that implied an active Web settings path were updated to presentation-neutral wording.

Current product documentation may still state that Web/Axum/browser paths are retired; those are current architecture statements, not stale dependencies.

## 13. Tests

Required final validation:

```text
rust-web fmt
rust-web test
rust-web check
rust-web strict Clippy -D warnings

rust-gui fmt
rust-gui check
rust-gui tests
rust-gui strict Clippy -D warnings

runtime contracts
source archive metadata smoke
portable package build
portable package verify
```

## 14. CI result

GitHub Actions run **#763** validated the Phase 22.4 compatibility fix baseline:

```text
core-check (linux)    PASS
core-check (macos)    PASS
core-check (windows)  PASS
windows-check         PASS
```

The Windows job also passed the runtime contract guard, source archive metadata smoke, Windows portable package smoke, and portable package verification.

Run #761 exposed a Windows Slint cross-crate visibility regression after `StreamArchiveCore::settings()` was made private. The Native GUI still requires that read facade, so it was restored to public visibility and the audit was corrected before run #763.

## 15. Remaining risks

- the `rust-web` directory name is embedded in many developer/CI paths;
- the `stream-archive-server` package/binary name has stronger runtime/operator compatibility than the directory name;
- external scripts are not discoverable from repository search;
- SQLite/persisted settings must remain stable independently of naming cleanup;
- process lifecycle helpers must not be deleted based only on internal caller counts.

## 16. Phase 22.5 readiness

Recommended Phase 22.5 scope:

1. rename the **directory/path** `rust-web/` to a runtime/core-oriented name if desired;
2. update repository-internal path dependencies, CI cache/workflow paths, scripts, guards and current docs atomically;
3. keep `stream-archive-server` package/binary name unless Phase 22.5 explicitly implements a compatibility migration;
4. do not combine naming cleanup with SQLite/config/schema changes.

Summary:

```text
rust-web directory/path rename:   SAFE FOR PHASE 22.5
stream-archive-server hard rename: NOT RECOMMENDED YET
```
