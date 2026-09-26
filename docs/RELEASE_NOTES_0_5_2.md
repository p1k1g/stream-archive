# Stream Archive 0.5.2 — Release Notes Draft

Status: **Release Candidate draft — not a published release**

This document describes the currently verified product surface for the 0.5.2
release candidate. Phase 23.7 does not create a tag or GitHub Release.

## Highlights

Stream Archive 0.5.2 consolidates the application around the Rust + SQLite
runtime and provides two supported presentation surfaces:

- **Windows Native** Slint desktop application;
- Linux/macOS CLI and headless runtime.

Both surfaces use the same `StreamArchiveCore`, SQLite store, provider services,
Queue, History, Backup, storage diagnostics and owned-process lifecycle.

## Provider support

The retained provider paths are:

- SOOP LIVE
- SOOP VOD
- CHZZK LIVE
- CHZZK VOD

Credential-free CI uses offline provider/media-tool fixtures to verify command
construction, result mapping, timeouts, cancellation and owned-descendant
cleanup. A real provider/session smoke remains required before public release.

## Windows Native

The Windows portable package contains:

- `StreamArchive.exe` as the default Native UI;
- `stream-archive-server.exe` as the optional compatibility headless runtime;
- `RUN.bat` and `RUN_HEADLESS.bat`;
- packaged offline backup/restore maintenance scripts;
- release metadata and SHA-256 manifests.

The retired browser/Web launcher and reverse-proxy surface are not included.

## Linux and macOS

Linux and macOS use:

- `bin/stream-archive-cli`
- `bin/stream-archive-server`

The CLI includes initialization, status, settings, provider readiness, media
tools, doctor, channels, watcher, VOD, Queue, History, Backup, storage, logs and
foreground serve operations.

CI-verified archive names are:

- `stream-archive-linux-x64.tar.gz`
- `stream-archive-macos-arm64.tar.gz`

No untested architecture is advertised.

## Packaging and checksums

The Windows RC artifact is:

- `stream-archive-windows-x64.zip`

Every canonical artifact contains `RELEASE_INFO.txt` and package-local
`SHA256SUMS.txt`, and every final archive has a sibling archive-level SHA-256
file.

Official packages ship an empty runtime `data/` directory. Runtime SQLite,
logs, provider credentials, backups, downloads and process-state files are not
release payload.

Phase 23.7 also requires corrupted archive copies to be rejected by the
verifiers.

## Backup and restore

The shared BackupManager provides managed SQLite backup/restore and retention.
The Windows package also contains offline PowerShell backup/restore scripts for
maintenance while Stream Archive is stopped.

The RC suite covers managed backup round-trip behavior, runtime-owner restore
safety, Windows offline backup metadata/hash validation, and restore rejection
for a corrupted backup.

## Diagnostics and media tools

Streamlink, yt-dlp and FFmpeg are **external dependencies** and are not bundled
in the release archives.

Tool discovery supports canonical configuration, application/backend layouts,
PATH and supported common installation locations. Windows exposes the Native
Settings/Diagnostics surface; Linux/macOS provide `tools` and
`doctor --active-tools`.

## Secrets

Secrets are not intentionally stored as plaintext fallbacks.

- Windows: CurrentUser DPAPI
- Linux: Secret Service through `secret-tool`
- macOS: Keychain Services

Linux requires a usable Secret Service session for secret writes. Real provider
credentials are not injected into CI.

## Upgrade guidance

For an upgrade:

1. stop the active runtime cleanly;
2. create and verify a backup;
3. keep the previous package available for rollback;
4. extract the new package separately rather than overwriting it in place;
5. reuse the existing canonical data directory;
6. start the new package and verify settings, channels, history, Queue and
   diagnostics;
7. restart once more to confirm persistence.

Phase 23.7 automatically verifies package replacement against preserved data on
Unix RC artifacts. A representative **cross-version upgrade** remains a manual
release-candidate check.

Do not assume arbitrary schema downgrade compatibility. If rollback requires a
database change, restore a verified pre-upgrade backup only when appropriate to
the observed database state.

## Known limitations

- Windows artifacts are not Authenticode signed.
- macOS artifacts are not code-signed or notarized.
- There is no MSI, deb/rpm, Homebrew, Snap/Flatpak/AppImage package.
- There is no systemd/launchd installer.
- Real SOOP/CHZZK sessions remain manual final QA.
- Windows Native GUI interaction remains manual final QA.

## Release status

This file is a release-notes draft. Public tag/Release creation, signing
decisions and final go/no-go remain user-controlled after Phase 23.7 automated
and manual RC validation.
