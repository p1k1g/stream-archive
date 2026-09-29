# Phase 23.10 — Windows Branding / Release Polish

Phase 23.10 is the final Windows polish pass before the first public Stream Archive 0.5.2 release.
Baseline main commit: 72368bc71490f92dee2883050a90aaa4151bcfee.

## Scope

- Windows application/taskbar branding.
- LIVE Native UI cleanup and 저장 폴더 열기.
- FemtoVG-first rendering with software fallback.
- fresh yt-dlp/FFmpeg bundled-layout discovery on Diagnostics refresh.
- Native-only Windows release package; Unix CLI/headless remains supported.

No provider redesign, SQLite schema change, Queue/History redesign, backup-format change, tag, GitHub Release, signing/notarization, installer, or version bump is included.

## Windows branding

Canonical source artwork: rust-gui/assets/stream-archive-icon.png.

maintenance/Set-WindowsExecutableIcon.ps1 generates a multi-size Windows ICO during the package build at 16, 24, 32, 48, 64, 128 and 256 pixels, then embeds the icon group into the release GUI executable before it is copied as StreamArchive.exe. Slint uses the source image as the Window icon and build.rs explicitly selects EmbedFiles so the image bytes are compiled into the executable. The portable package does not require a standalone PNG/ICO at runtime, and the Windows package verifier rejects standalone icon assets.

Manual confirmation remains required for Explorer, title-bar, running taskbar, and taskbar pin/unpin/re-pin because Windows icon caching cannot be proven by CI.

## LIVE Native UI

The obsolete Phase 21 Slint native shell subtitle is removed.

The 저장 폴더 열기 action is shown for an active recording row and is disabled until that row has an actual output file path. The controller delegates to a GUI-only platform helper. Windows uses the Win32 ShellExecuteW open path for the directory; it does not assemble a shell command, use cmd /c, or execute the media file.

The helper preserves whitespace/Unicode paths, rejects empty or missing directories without crashing, and does not modify Stop / Resume / Recheck or recorder ownership.

## Windows renderer

The previous Windows GUI enabled only Slint renderer-software. Phase 23.10 enables renderer-femtovg and renderer-software with backend-winit. Skia is intentionally not added.

FemtoVG is the normal Windows candidate while software rendering remains a compatibility fallback. Manual comparison can use SLINT_BACKEND=winit-femtovg and SLINT_BACKEND=winit-software; this is QA-only and is not exposed in application Settings.

Manual repaint cases: overlap/uncover, Alt+Tab, minimize/restore, move, resize, maximize/restore, 100% scaling and, where available, 125%/150% and mixed-DPI monitor movement.

## Media-tool rediscovery

Discovery priority remains: explicit configured path -> bundled layout -> PATH -> common install location.

Windows bundled candidates remain backend\streamlink.exe, backend\vod\yt-dlp.exe and backend\vod\ffmpeg.exe.

An explicit Native Diagnostics refresh now uses the active-local Diagnostics path. It re-resolves the filesystem and performs local version probes, so a yt-dlp/FFmpeg executable copied into backend\vod after startup can be discovered without restarting the application. Runtime regression coverage verifies missing -> file created -> next resolution -> bundled for yt-dlp and FFmpeg.

Official archives still do not redistribute Streamlink, yt-dlp or FFmpeg.

## Native-only Windows package

The Windows release package no longer contains stream-archive-server.exe or RUN_HEADLESS.bat.

Expected Windows package surface:

    StreamArchive.exe
    RUN.bat
    BACKUP_DATA.bat
    RESTORE_DATA.bat
    RELEASE_INFO.txt
    SHA256SUMS.txt
    LICENSE
    THIRD_PARTY_NOTICES.md
    backend\
    backend\vod\
    data\
    maintenance\
    docs\

BUILD_PORTABLE.bat no longer performs a separate server-binary release build. The GUI build still compiles the shared Rust core dependency. Runtime/provider/unit/integration tests are not reduced. The Windows verifier explicitly rejects the removed server/headless files if they return.

Linux/macOS keep stream-archive-cli, stream-archive-server, CLI/headless operation, Unix ownership/signal handling, native secret-store boundaries and the Unix package contract.

## Release cleanliness

The official Windows package must keep data\ empty and exclude runtime DB/WAL/SHM, logs, backups, downloaded media, credentials/cookies, process state and bundled external media tools. Package-local and archive-level SHA-256 validation remain required.

## Automated validation

Status: PASS on Stream Archive check #277 for final code head `456d5a8e70a103d530b8860d8773ac2fc2ef7884`.

Windows, Linux and macOS core jobs passed. The final Windows job also passed RuntimeContracts, source metadata smoke, Windows portable packaging, package verification, offline backup/restore smoke, runtime-data leakage rejection, final release archive creation/verification and corrupted-archive rejection. The generated multi-size icon and PE resource embedding completed successfully during the Windows portable build.

The first #277 Windows-core attempt hit the pre-existing SOOP VOD offline provider E2E flake once (`FAILED` instead of `COMPLETED`); the same unchanged test passed on the immediate job rerun, after which the full cross-platform/final Windows pipeline passed. No test or guard was weakened to accommodate it.

The FemtoVG Cargo lockfile is generated by Cargo and committed; `--locked` validation is not relaxed.

## Size impact

Authoritative comparison uses the Phase 23.9 release-artifact workflow #1 Windows archive against Phase 23.10 check #277 packaging output:

- Phase 23.9 `StreamArchive.exe`: 27,432,448 bytes (26.16 MiB).
- Phase 23.10 `StreamArchive.exe`: 28,260,864 bytes (26.95 MiB), +828,416 bytes / +3.02%.
- Phase 23.9 Windows release ZIP: 14,391,518 bytes (13.72 MiB).
- Phase 23.10 Windows release ZIP: 11,512,587 bytes (10.98 MiB), -2,878,931 bytes / -20.00%.
- The Phase 23.9 package also carried `stream-archive-server.exe` at 7,942,656 bytes; Phase 23.10 no longer ships that duplicate Windows headless surface.

The small GUI increase is the expected FemtoVG/branding cost, while the final compressed Windows package is materially smaller because the standalone headless executable and launcher were removed.

## Manual RC remaining

- Explorer/title-bar/taskbar icon and pin/re-pin.
- FemtoVG repaint scenarios and software fallback launch.
- LIVE 저장 폴더 열기 with normal, whitespace, Unicode and missing paths.
- LIVE Stop/Resume/Recheck regression.
- yt-dlp/FFmpeg missing -> copy -> Diagnostics refresh.
- real SOOP LIVE/VOD and CHZZK LIVE/VOD.
- fresh install, representative upgrade and rollback.
- backup/restore and Windows DPAPI.
- exact publication artifact inspection.

Windows headless manual RC is no longer a release gate because that surface is no longer shipped in the Windows package.

## Release boundary

Phase 23.10 may reach Ready for review, but it must not merge itself and must not create v0.5.2, a GitHub Release or public release assets.
