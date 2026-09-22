# Phase 22.6 — Runtime Workflow Naming Cleanup

## 1. Scope

Phase 22.6 removes the remaining repository-internal **Web-era workflow filename debt** left intentionally after Phase 22.5.

This phase is limited to workflow naming and references:

```text
.github/workflows/rust-web-check.yml
→ .github/workflows/rust-runtime-check.yml

.github/workflows/rust-web-release.yml
→ .github/workflows/rust-runtime-release.yml
```

It does **not** rename the Cargo package, Rust crate identity, binaries, CLI command, SQLite/config keys, provider behavior, process ownership, packaging contract, or Native UI behavior.

## 2. Baseline

Baseline is `main` after Phase 22.5 PR #93 merged.

The baseline merge commit is:

```text
a4f3202bbb22cbc5ef67d4f68cfc827bdb4b3fb7
```

Phase 22.5 had already established:

- canonical shared runtime source directory: `rust-runtime/`;
- browser/Web presentation removed in Phase 22.3;
- active runtime source-path references migrated away from `rust-web/`;
- `stream-archive-server` retained as a compatibility identity;
- workflow filenames were the remaining current repository-internal Web-era naming debt.

## 3. Workflow filename migration

The canonical PR/runtime validation workflow is now:

```text
.github/workflows/rust-runtime-check.yml
```

The canonical manual portable release workflow is now:

```text
.github/workflows/rust-runtime-release.yml
```

The retired workflow filenames no longer exist in the current tree:

```text
.github/workflows/rust-web-check.yml
.github/workflows/rust-web-release.yml
```

The workflow bodies retain the existing validation and packaging behavior. This phase changes naming, not validation coverage.

## 4. Active reference migration

Current references were migrated to the canonical runtime workflow filenames.

Updated active locations include:

- `README.md`;
- `maintenance/guards/ReleaseSafety.ps1`;
- `maintenance/guards/StorageOwnership.ps1`.

`ReleaseSafety.ps1` now:

- reads `rust-runtime-check.yml` and `rust-runtime-release.yml`;
- rejects reintroduction of either retired `rust-web-*.yml` workflow filename;
- retains the existing runtime path, portable package, security, and compatibility assertions;
- verifies that the runtime release workflow retains manual `workflow_dispatch`.

Historical Phase documentation is not rewritten merely to erase accurate historical references.

## 5. Compatibility boundary retained

Phase 22.6 deliberately keeps the compatibility identity established in Phase 22.4/22.5:

```text
Directory:       rust-runtime/
Cargo package:   stream-archive-server
Rust crate:      stream_archive_server
default-run:     stream-archive-server
headless binary: stream-archive-server
CLI binary:      stream-archive-cli
GUI package:     stream-archive-gui
GUI binary:      StreamArchive.exe
```

The workflow rename is repository-internal and does not justify changing externally observable package/binary names.

## 6. Architecture boundary retained

The supported architecture remains:

```text
Windows Slint Native GUI ─┐
Unix CLI/headless ─────────┼──> StreamArchiveCore / shared Rust runtime
Headless runtime binary ───┘             │
                                         ├─ SQLite
                                         ├─ LIVE/VOD services
                                         ├─ SOOP/CHZZK providers
                                         └─ Streamlink / yt-dlp / FFmpeg
```

Phase 22.6 does not reintroduce browser/Web presentation, localhost HTTP application APIs, direct GUI SQLite ownership, or retired Web packaging artifacts.

## 7. Validation contract

The final Phase 22.6 branch must preserve the Phase 22.5 validation baseline.

Required runtime checks:

```text
cargo fmt --manifest-path rust-runtime/Cargo.toml -- --check
cargo test --locked --manifest-path rust-runtime/Cargo.toml
cargo check --locked --manifest-path rust-runtime/Cargo.toml
cargo clippy --locked --manifest-path rust-runtime/Cargo.toml --all-targets --all-features -- -D warnings
```

Required Native GUI checks:

```text
cargo fmt --manifest-path rust-gui/Cargo.toml -- --check
cargo check --locked --manifest-path rust-gui/Cargo.toml
cargo test --locked --manifest-path rust-gui/Cargo.toml
cargo clippy --locked --manifest-path rust-gui/Cargo.toml --all-targets --all-features -- -D warnings
```

Required Windows/runtime contract checks:

```powershell
.\maintenance\Test-RuntimeContracts.ps1
.\BUILD_PORTABLE.bat
```

The PR workflow must validate:

```text
core-check (linux)
core-check (macos)
core-check (windows)
windows-check
```

The Windows job also remains responsible for runtime contract guards, source archive metadata smoke coverage, portable package build smoke, and portable package verification.

## 8. Historical references

Historical documents may still contain the previous workflow filenames when they describe the repository state that existed at that phase.

In particular, `docs/PHASE22_5_RUNTIME_PATH_CLEANUP.md` intentionally records that Phase 22.5 retained:

```text
rust-web-check.yml
rust-web-release.yml
```

That is historical evidence, not an active workflow dependency.

## 9. Result

Phase 22.6 completes the repository-internal transition from Web-era workflow filenames to runtime terminology while preserving external compatibility identities and existing validation coverage.

After this phase, the current repository has:

```text
source directory:        rust-runtime/
PR/runtime workflow:     rust-runtime-check.yml
manual release workflow: rust-runtime-release.yml
```

and deliberately retains:

```text
Cargo package/binary compatibility name: stream-archive-server
```

Any future package/binary compatibility migration must remain a separate, explicitly bounded phase.
