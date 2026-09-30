# Phase 23.12 — 1.0.0 Release Closure

Status: PR preparation; first public stable release pending maintainer approval.

## Baseline and scope

Main baseline: `fbd4461d61dd9a79aef18c1a7b14a5562ae88e32`, Phase 23.11 / PR #106.
Main check #306 and release-artifacts #3 succeeded; #3 produced unexpired Windows,
Linux and macOS artifacts. These are 0.5.2 baseline evidence, not 1.0.0 validation.

Product versions come from rust-runtime/Cargo.toml and rust-gui/Cargo.toml.
Only product package entries in both lockfiles change; dependency versions and
historical Phase documents remain intact. RELEASE_INFO generation reads Cargo
metadata; Windows build.rs/winresource uses the GUI package version. Existing
0.5.2 notes are retained for historical links; current notes are RELEASE_NOTES_1_0_0.md.

No renderer, allocator, working-set trim, icon-cache workaround or architecture
change is included. Shared StreamArchiveCore and owned-process lifecycle remain.

## Automated validation

PR workflow rust-runtime-check.yml is authoritative for the final commit:
- Runtime fmt, unit tests, check and strict clippy on all three platforms.
- Windows Slint fmt, compile, adapter tests and strict clippy.
- RuntimeContracts, icon frame decoding and Native-only Windows package verifier.
- Windows offline backup/restore with corrupted-backup rejection.
- Runtime data leakage rejection, package-local and archive checksums,
  fresh extraction smoke and corrupted archive rejection.
- Linux/macOS canonical package builds and CLI/backup/replacement smokes.
- Explicit 1.0.0 Cargo, package metadata and Windows PE ProductVersion checks.

Local environment lacks cargo and PowerShell; no local Rust/Windows pass is claimed.
The release-artifacts workflow is workflow_dispatch with contents: read; it only
uploads Actions artifacts and does not publish Releases. After the PR CI passes,
the maintainer may dispatch it on this branch/final merged revision to obtain
1.0.0 Windows ZIP / Linux TAR.GZ / macOS TAR.GZ for manual RC. Verify all jobs and
download the three archives plus their sibling .sha256 files. This phase does
not dispatch publication or create a tag/Release.

## Manual RC validation — MANUAL TEST REQUIRED

The following checks remain pending on the final 1.0.0 artifacts. Credential-free
fixtures do not establish real provider or native secret-store success.

- [ ] Windows Explorer icon
- [ ] Windows title bar icon
- [ ] Windows taskbar icon
- [ ] Windows Task Manager icon
- [ ] Taskbar pin / unpin / re-pin
- [ ] Repaint / minimize / restore / resize
- [ ] SOOP LIVE real session smoke
- [ ] SOOP VOD real session smoke
- [ ] CHZZK LIVE real session smoke
- [ ] CHZZK VOD real session smoke
- [ ] Queue / History end-to-end interaction
- [ ] Fresh install on representative supported hosts
- [ ] Representative cross-version upgrade from retained RC data
- [ ] Backup / restore using representative user data
- [ ] Windows CurrentUser DPAPI secret write/read across restart
- [ ] Linux Secret Service secret write/read in a usable session
- [ ] macOS Keychain secret write/read across restart

Record artifact SHA256, OS, test date, observed result and limitations for each
manual check. Do not assume arbitrary schema downgrade compatibility; retain a
verified pre-upgrade backup.

Maintainer reports for 0.5.2: embedded icon resources in artifact #3 were valid;
the same binary displayed correctly on another PC/new path. Some existing hosts
retained stale icons. This is not completion of final 1.0.0 icon checks.
Maintainer memory observations were stable within each PC (home about 45 MB,
office about 75 MB for both renderers); private memory/handles/threads stabilized
without a persistent leak pattern. No numerical memory optimization is warranted.

## Release blockers and public release readiness

Failed required CI or failed manual gates block public release. Ready for review
means code review readiness, not manual RC completion or publication approval.
Maintainer alone decides merge, v1.0.0 tag and GitHub Release publication.
