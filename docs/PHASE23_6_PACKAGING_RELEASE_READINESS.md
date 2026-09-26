# Phase 23.6 — Packaging / Release Readiness

Phase 23.6 turns the already-completed Windows Native and Unix CLI/headless
surfaces into verified portable release artifacts. It does not change provider,
SQLite, process-ownership, Queue, or runtime semantics.

## Verified artifact matrix

The Phase 23.6 PR CI uses native GitHub-hosted runners and currently verifies:

| Platform | Runner / Rust host | Archive |
|---|---|---|
| Windows | X64 / `x86_64-pc-windows-msvc` | `stream-archive-windows-x64.zip` |
| Linux | X64 / `x86_64-unknown-linux-gnu` | `stream-archive-linux-x64.tar.gz` |
| macOS | ARM64 / `aarch64-apple-darwin` | `stream-archive-macos-arm64.tar.gz` |

No architecture that is not built and exercised by CI is advertised.

## Canonical package layouts

### Windows

The existing portable layout remains the product contract:

~~~text
stream-archive/
├─ StreamArchive.exe
├─ stream-archive-server.exe
├─ RUN.bat
├─ RUN_HEADLESS.bat
├─ BACKUP_DATA.bat
├─ RESTORE_DATA.bat
├─ RELEASE_INFO.txt
├─ SHA256SUMS.txt
├─ LICENSE
├─ THIRD_PARTY_NOTICES.md
├─ backend/
│  └─ vod/
├─ data/
├─ maintenance/
└─ docs/
~~~

`StreamArchive.exe` remains the default Windows UI. The compatibility headless
server remains available. Browser/Web launcher and reverse-proxy artifacts do
not return.

### Linux / macOS

~~~text
stream-archive/
├─ bin/
│  ├─ stream-archive-cli
│  └─ stream-archive-server
├─ backend/
│  └─ vod/
├─ data/
├─ docs/
│  ├─ UNIX_CLI.md
│  └─ OPERATIONS.md
├─ LICENSE
├─ THIRD_PARTY_NOTICES.md
├─ RELEASE_INFO.txt
└─ SHA256SUMS.txt
~~~

The packaged `data/` directory is empty. No SQLite database, logs, backup,
runtime lock/socket, media output, or claim sidecar is shipped.

## Build entrypoints

Windows:

~~~powershell
.\BUILD_PORTABLE.bat
~~~

Unix:

~~~bash
./BUILD_UNIX_PACKAGE.sh
~~~

The Unix builder performs:

1. native OS/architecture detection;
2. `cargo build --locked --release`;
3. release binary checks;
4. clean package staging;
5. license/docs copy;
6. release metadata generation;
7. package-local SHA-256 generation;
8. package verification;
9. TAR.GZ creation;
10. archive-level checksum generation;
11. fresh-extraction verification.

## Verification entrypoints

Windows package tree:

~~~powershell
powershell -ExecutionPolicy Bypass -File .\maintenance\Verify-WindowsPackage.ps1 -Root .\dist\stream-archive
~~~

Official clean-package validation adds:

~~~powershell
-RequireCleanData
~~~

Windows archive creation:

~~~powershell
powershell -ExecutionPolicy Bypass -File .\maintenance\New-WindowsReleaseArchive.ps1 -PackageRoot .\dist\stream-archive -OutputDir .\dist\release
~~~

The archiver itself invokes the canonical Windows verifier with
`-RequireCleanData` before creating the ZIP. Preserved local SQLite/runtime
data therefore blocks archive creation even if the caller skipped a separate
pre-verification command.

Unix tree/archive:

~~~bash
./maintenance/Verify-UnixPackage.sh \
  ./dist/unix-<platform>-<arch>/stream-archive \
  ./dist/release/stream-archive-<platform>-<arch>.tar.gz
~~~

Package verification is intentionally separate from assembly.

## Release metadata

Every package contains:

~~~text
product=Stream Archive
version=<Cargo package version>
commit=<git commit or unknown>
built_at=<UTC/offset timestamp>
~~~

The canonical release version source is the Cargo package version for
`stream-archive-server`. The packaging runtime contract also reads
`rust-gui/Cargo.toml` through Cargo metadata and fails when the runtime and GUI
versions differ. The manual release-artifact workflow is gated by the same
runtime-contract entrypoint before any platform artifact job can run.

Current Phase 23.6 version: `0.5.2`.

Git metadata is optional. Source archives without `.git` use
`commit=unknown`. On Windows, an ambient GitHub Actions `GITHUB_SHA` is
accepted only when the explicit `RepositoryRoot` resolves to the actual
`GITHUB_WORKSPACE` checkout and that checkout's own Git `HEAD` matches the
workflow SHA. Even after that match, the canonical metadata writer still runs
`git status --porcelain --untracked-files=normal`; a modified workspace is
recorded as `<commit>-dirty`, and a failed status check does not emit a bare
reviewed SHA. Linked Git worktrees are supported: the worktree-local `HEAD`
is resolved together with refs and `packed-refs` from the shared Git
`commondir`. Passing an extracted/non-Git source directory as
`RepositoryRoot` therefore still records `commit=unknown`. A checkout or
linked worktree with tracked or untracked non-ignored changes records
`<commit>-dirty` so a locally generated artifact is not falsely attributed
to a reviewed clean commit. Generated `dist/` staging is ignored and
therefore does not mark a clean package build dirty. The PR/push packaging
workflow explicitly watches the root `.gitignore` so removing that exclusion
cannot silently bypass the packaging contract.

## Checksums

Two checksum layers are kept distinct.

### Package-local

`SHA256SUMS.txt` validates the shipped executables.

Windows:

~~~text
StreamArchive.exe
stream-archive-server.exe
~~~

Linux/macOS:

~~~text
bin/stream-archive-cli
bin/stream-archive-server
~~~

### Archive-level

Each final archive has a sibling checksum:

~~~text
stream-archive-windows-x64.zip.sha256
stream-archive-linux-x64.tar.gz.sha256
stream-archive-macos-arm64.tar.gz.sha256
~~~

The verifier checks the archive checksum before fresh extraction.

## Fresh-extraction smoke

Unix package verification extracts the TAR.GZ into a new path containing
whitespace and non-ASCII characters, then runs the packaged binary rather than
the Cargo target binary:

~~~bash
./bin/stream-archive-cli version
./bin/stream-archive-cli help
./bin/stream-archive-cli init
./bin/stream-archive-cli status --json
~~~

Windows archive verification extracts the ZIP into a fresh non-ASCII/whitespace
temporary directory and re-runs the package tree/checksum contract there.

## External media tools

The following executables are not bundled:

- Streamlink
- yt-dlp
- FFmpeg

The package verifiers explicitly reject these tool filenames. Tool discovery and
configuration remain the runtime responsibility.

Windows uses Native Settings/Diagnostics.

Unix uses:

~~~bash
./bin/stream-archive-cli tools
./bin/stream-archive-cli tools configure
./bin/stream-archive-cli doctor --active-tools
~~~

## Secret-storage boundary

Packaging does not weaken the native secret contract.

- Windows: CurrentUser DPAPI
- Linux: Secret Service, with `secret-tool` and an available session
- macOS: Keychain
- SQLite: protected/native-secret reference rather than plaintext where the
  native store contract applies
- native store unavailable: fail closed

No provider credential is needed by automated package smoke.

## Release workflow

`.github/workflows/rust-runtime-release.yml` remains manual
`workflow_dispatch`.

It contains a required `release-contracts` gate followed by native jobs for:

- Windows package
- Linux package
- macOS package

The contract gate runs `maintenance/Test-RuntimeContracts.ps1`, including
runtime/gui version parity and packaging/release-safety checks. Only after that
passes do the platform jobs build, verify, archive, verify again, and upload
GitHub Actions artifacts with a 7-day retention period.

The workflow has `contents: read` permission and does not:

- create a Git tag;
- create/publish a GitHub Release;
- upload public Release assets;
- publish a package registry artifact.

PR CI deliberately reuses the same package builders/verifiers so packaging
regressions are caught before the manual release-artifact workflow is used.

## CI evidence

Phase 23.6 PR check #166 completed successfully for commit
`67e4c4bf7ad3316c426fa1a094a9d7184bbe2e5f`.

Verified there:

- Windows core check: PASS
- Linux core check: PASS
- macOS core check: PASS
- Windows final check: PASS
- Linux Unix release package smoke: PASS
- macOS Unix release package smoke: PASS
- Windows portable package build: PASS
- Windows portable package verification: PASS
- Windows release archive build: PASS
- Windows release archive fresh-extraction verification: PASS
- runtime contract guard: PASS
- source-archive metadata smoke: PASS

The subsequent documentation commits must pass the same PR workflow before the
PR is considered ready.

The manual `workflow_dispatch` release-artifact workflow is not a public
release operation. Running it is appropriate for RC artifact collection, but
Phase 23.6 does not publish those artifacts as GitHub Release assets.

## Runtime contracts

`maintenance/guards/Packaging.ps1` protects:

- canonical Windows and Unix package entrypoints;
- Windows Native GUI retention;
- compatibility headless server retention;
- CLI/server inclusion in Unix archives;
- license and third-party notices;
- metadata/checksum generation;
- dirty-worktree provenance;
- archive-level checksum generation;
- clean-data enforcement inside the Windows archiver;
- reusable package verification;
- empty-data official package contract;
- media-tool non-bundling;
- native release workflow runners;
- manual workflow dispatch;
- release-contract gate before platform artifact jobs;
- no automatic tag/GitHub Release publishing;
- runtime/gui Cargo version equality.

`maintenance/guards/ReleaseSafety.ps1` also scans the new packaging scripts
and keeps current-tree plus reachable-history secret/privacy protection.

## Upgrade / rollback

Official artifacts are clean packages. They must not be treated as a backup of
runtime data.

Before upgrade:

1. stop the active Stream Archive runtime;
2. create a verified database backup;
3. verify the new archive checksum;
4. extract the new package separately;
5. preserve/reuse the existing canonical data directory;
6. run diagnostics;
7. start and verify the new runtime.

Rollback should restore the previous package first. Restore the database backup
only when required by the actual database state. Phase 23.6 does not claim
arbitrary schema downgrade compatibility.

See `docs/OPERATIONS.md` for platform-specific details.

## Signing and installer status

Phase 23.6 artifacts are checksum-verified but are not presented as:

- Windows Authenticode signed;
- macOS code-signed;
- macOS notarized;
- MSI installers;
- Homebrew packages;
- deb/rpm packages;
- Snap/Flatpak/AppImage packages;
- systemd/launchd installers.

No Gatekeeper bypass procedure is added.

## Phase 23.7 handoff — Release Candidate / Final QA

Phase 23.7 should consume these artifacts rather than changing packaging
architecture unless a real RC defect requires it.

Manual RC checks:

- real Windows ZIP extraction and Native UI smoke;
- real Linux TAR.GZ extraction and CLI/headless smoke;
- real macOS TAR.GZ extraction and CLI/headless smoke;
- real SOOP LIVE;
- real CHZZK LIVE;
- real SOOP VOD;
- real CHZZK VOD;
- fresh-install behavior;
- upgrade with preserved runtime data;
- backup / restore;
- archive checksum verification;
- known-issues review;
- release notes;
- final version/tag decision;
- signing/notarization decision;
- final RC go/no-go.

Phase 23.7, not Phase 23.6, owns the public release decision.
