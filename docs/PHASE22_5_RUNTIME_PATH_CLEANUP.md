# Phase 22.5 — Runtime Path / Workspace Naming Cleanup

## 1. Scope

Phase 22.5 removes the remaining legacy **source-directory/path** name `rust-web/` after Phase 22.4 made the compatibility boundaries explicit.

This phase is intentionally limited to repository path naming:

```text
rust-web/
→
rust-runtime/
```

It does **not** rename the Cargo package, Rust crate identity, binaries, CLI command, SQLite/config keys, provider behavior, process ownership, or Native UI behavior.

## 2. Baseline

Baseline is `main` after Phase 22.4 PR #92 merged.

The branch was created from merge commit:

```text
4c9c932855ce37fb003b764eae2293624b1065b4
```

Phase 22.4 had already established that:

- `rust-web/` was a legacy directory name rather than a Web presentation layer;
- `stream-archive-server` remained an active compatibility boundary;
- a directory/path rename could be separated safely from package/binary compatibility migration.

## 3. Directory rename

The canonical shared runtime source directory is now:

```text
rust-runtime/
```

The previous directory:

```text
rust-web/
```

does not exist in the Phase 22.5 branch tree.

The rename commit reused the existing Git tree without rewriting source files. GitHub compare recognizes all 34 tracked runtime files as zero-content-change renames, including:

- `Cargo.toml`;
- `Cargo.lock`;
- `src/lib.rs`;
- `src/main.rs`;
- `src/app_core.rs`;
- runtime/store/security/backup/history/queue modules;
- SOOP/CHZZK provider modules;
- process ownership modules;
- `src/bin/stream-archive-cli.rs`.

This preserves rename history and avoids unrelated source churn.

## 4. Cargo path dependency changes

The Native GUI dependency changed from:

```toml
stream-archive-server = { path = "../rust-web" }
```

to:

```toml
stream-archive-server = { path = "../rust-runtime" }
```

Compatibility identity is intentionally unchanged:

```text
Directory:       rust-runtime/
Cargo package:   stream-archive-server
Rust crate:      stream_archive_server
default-run:     stream-archive-server
headless binary: stream-archive-server
CLI binary:      stream-archive-cli
```

No package/dependency version changed.

The runtime `Cargo.lock` moved with the directory and has zero content changes.

## 5. CI path changes

The existing workflow filenames remain:

```text
.github/workflows/rust-web-check.yml
.github/workflows/rust-web-release.yml
```

The filenames are retained as workflow compatibility identifiers in this phase.

Active paths inside those workflows changed to `rust-runtime`, including:

- path triggers;
- Rust cache workspace paths;
- `--manifest-path` commands;
- release metadata manifest arguments.

Validation coverage was not reduced.

## 6. Build/package path changes

`BUILD_PORTABLE.bat` now builds:

```text
rust-runtime/Cargo.toml
rust-gui/Cargo.toml
```

and copies the compatibility headless binary from:

```text
rust-runtime\target\release\stream-archive-server.exe
```

The portable output contract remains unchanged:

```text
StreamArchive.exe
stream-archive-server.exe
RUN.bat
RUN_HEADLESS.bat
BACKUP_DATA.bat
RESTORE_DATA.bat
backend/
data/
maintenance/
docs/
LICENSE
THIRD_PARTY_NOTICES.md
RELEASE_INFO.txt
SHA256SUMS.txt
```

`RUN_DEV.bat` also uses the new runtime manifest path while retaining:

```text
--bin stream-archive-server
```

## 7. Maintenance/guard path changes

Current runtime guards now read canonical source files under `rust-runtime/`:

- Architecture;
- Providers;
- Security;
- ToolDiscovery;
- ProcessLifecycle;
- StorageOwnership;
- ReleaseSafety.

`maintenance/Write-ReleaseMetadata.ps1` now defaults to:

```text
.\rust-runtime\Cargo.toml
```

Its Cargo package lookup remains `stream-archive-server`.

The root `.gitignore` now ignores `rust-runtime/target/` instead of the retired `rust-web/target/`, so normal Cargo checks do not leave the renamed build tree untracked. The historical runtime-state ignore `backend/.rust-web/` is retained because it is not the Rust source directory.

ReleaseSafety additionally rejects reintroduction of an active directory reference matching:

```text
rust-web/
rust-web\
```

across current workflow/build/developer/release/current-doc/GUI-manifest inputs.

This guard deliberately does not reject the retained workflow filenames or historical `.rust-web` legacy-name safety pattern.

## 8. Documentation classification

Current documentation was migrated to `rust-runtime` where it gives executable commands or describes the canonical source layout:

- `README.md`;
- `AGENTS.md`;
- `CONTRIBUTING.md`;
- `docs/UNIX_CLI.md`.

The repository guide now explicitly distinguishes:

```text
source directory: rust-runtime/
Cargo package:    stream-archive-server
Rust crate:       stream_archive_server
```

## 9. Historical references retained

Historical Phase documents were intentionally not rewritten.

Known historical files containing `rust-web` references include Phase 19/21/22 audit or migration records such as:

- `docs/PHASE19_AUDIT.md`;
- `docs/PHASE21_ARCHITECTURE.md`;
- `docs/PHASE21_BACKUP_DIAGNOSTICS.md`;
- `docs/PHASE21_NATIVE_UX_POLISH.md`;
- `docs/PHASE22_1_LEGACY_DEPENDENCY_AUDIT.md`;
- `docs/PHASE22_2_WARNING_CLEANUP.md`;
- `docs/PHASE22_3_WEB_REMOVAL.md`;
- `docs/PHASE22_4_RUNTIME_COMPAT_AUDIT.md`.

These describe the repository state that existed during those phases and remain historical evidence.

## 10. stream-archive-server compatibility retained

Phase 22.5 deliberately retains:

```text
Cargo package: stream-archive-server
Rust crate: stream_archive_server
binary: stream-archive-server / stream-archive-server.exe
CLI: stream-archive-cli serve
portable launcher: RUN_HEADLESS.bat
```

The compatibility name still participates in:

- the Native GUI Cargo dependency identity;
- CLI sibling/PATH lookup;
- portable packaging;
- offline backup/restore process detection;
- release metadata;
- operational scripts that may exist outside the repository.

A directory rename is therefore separated from a binary/package compatibility migration.

## 11. Tests

Final validation requires:

```text
cargo fmt --manifest-path rust-runtime/Cargo.toml -- --check
cargo test --locked --manifest-path rust-runtime/Cargo.toml
cargo check --locked --manifest-path rust-runtime/Cargo.toml
cargo clippy --locked --manifest-path rust-runtime/Cargo.toml --all-targets --all-features -- -D warnings

cargo fmt --manifest-path rust-gui/Cargo.toml -- --check
cargo check --locked --manifest-path rust-gui/Cargo.toml
cargo test --locked --manifest-path rust-gui/Cargo.toml
cargo clippy --locked --manifest-path rust-gui/Cargo.toml --all-targets --all-features -- -D warnings

.\maintenance\Test-RuntimeContracts.ps1
.\BUILD_PORTABLE.bat
portable package verification
```

## 12. CI result

Final PR CI result: **pending**.

Required jobs:

```text
core-check (linux)
core-check (macos)
core-check (windows)
windows-check
```

This section will be updated after final PR validation.

## 13. Active rust-web references after cleanup

```text
Active rust-web directory/path references: 0
```

The branch tree contains no `rust-web/` directory.

The only non-historical current `rust-web` strings intentionally retained are:

1. workflow filenames:
   - `.github/workflows/rust-web-check.yml`
   - `.github/workflows/rust-web-release.yml`
2. references needed to open the retained workflow filename;
3. the historical runtime-state ignore `backend/.rust-web/`;
4. ReleaseSafety's historical hidden-name rejection for `.rust-web`.

None of these is an active runtime source directory dependency.

## 14. Residual naming debt

Remaining compatibility naming:

- Cargo package `stream-archive-server`;
- Rust crate import `stream_archive_server`;
- binary `stream-archive-server[.exe]`;
- CLI `serve` lookup for that binary;
- `RUN_HEADLESS.bat`;
- maintenance process detection;
- release metadata package lookup;
- workflow filenames `rust-web-check.yml` / `rust-web-release.yml`.

The binary/package identity has materially more external compatibility risk than the source directory did.

## 15. Phase 22.6 recommendation

```text
KEEP COMPATIBILITY NAME
```

for `stream-archive-server` by default.

A later phase should rename the package/binary only if there is a concrete product benefit that justifies a bounded compatibility migration.

If migration is chosen, prefer a deliberate transition such as:

```text
new canonical binary: stream-archive-runtime
temporary compatibility lookup/alias: stream-archive-server
```

with CLI lookup, portable packaging, backup/restore process detection, release metadata, documentation and external-operator compatibility addressed together.

Do not combine such a migration with SQLite/config/provider/process-lifecycle changes.
