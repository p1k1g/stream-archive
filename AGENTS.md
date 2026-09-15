# Repository Guide

이 저장소의 현재 제품 런타임은 Rust + SQLite 기반입니다.

## Canonical runtime

- `rust-web/src/main.rs`: Axum server/API orchestration
- `rust-web/src/native_watcher.rs`: provider-neutral LIVE 상태 감시 orchestration
- `rust-web/src/recorder.rs`: Streamlink/FFmpeg process ownership/lifecycle
- `rust-web/src/platform/live.rs`: platform-neutral LIVE provider facade/session types
- `rust-web/src/platform/vod.rs`: platform-neutral VOD lifecycle/dispatch
- `rust-web/src/platform/soop/*`: SOOP provider implementation
- `rust-web/src/platform/chzzk/*`: CHZZK provider implementation
- `rust-web/src/security.rs`: secret protection boundary
- `rust-web/src/store.rs`: canonical SQLite persistence/config/history
- `rust-web/src/history_storage.rs`: history queries and storage diagnostics APIs
- `rust-web/src/local_picker.rs`: localhost-only Windows file/folder picker bridge
- `rust-web/src/backend.rs`: log buffer and backend-directory resolution
- `rust-web/web/*`: browser UI

Provider-specific network/authentication/stream mechanics live under `rust-web/src/platform/<provider>/`. Common queue/history/API orchestration must depend on the platform-neutral facades rather than a provider implementation directly.

## Configuration source of truth

`data/stream-archive.db` is the canonical source of truth for settings, channels, encrypted secrets, LIVE/VOD history, queue state, and backup policy.

Do not reintroduce runtime INI/TXT mirrors or one-off config-file readers. Product-wide runtime environment variables use the `STREAM_ARCHIVE_*` namespace. Provider-specific credentials such as `SOOP_USERNAME`, `SOOP_PASSWORD`, `CHZZK_NID_AUT`, and `CHZZK_NID_SES` remain provider-scoped settings.

The only retained transition bridge is the bounded `soop.db` -> `stream-archive.db` database filename migration. Do not add broader legacy compatibility layers without an explicit migration requirement.

## Process ownership rule

Never terminate tools globally with image-name commands such as `taskkill /IM ffmpeg.exe`, `taskkill /IM streamlink.exe`, or `taskkill /IM yt-dlp.exe`.

The server may terminate only process trees it created and owns. Windows ownership/termination rules live behind `platform_runtime` and are guarded by runtime-contract tests.

## Build and test

```powershell
.\RUN_DEV.bat
.\BUILD_RELEASE.bat
.\BUILD_PORTABLE.bat
```

Rust checks:

```powershell
cargo fmt --manifest-path rust-web/Cargo.toml -- --check
cargo test --locked --manifest-path rust-web/Cargo.toml
cargo check --locked --manifest-path rust-web/Cargo.toml
```

Runtime contracts:

```powershell
.\maintenance\Test-RuntimeContracts.ps1
```

## Architecture boundary

The old WinUI/PowerShell runtime and INI/TXT configuration path are retired. New functionality and bug fixes must stay in the Rust/SQLite runtime and must not restore those legacy paths merely for compatibility.

Windows-specific behavior should remain isolated behind platform/runtime boundaries so Linux/macOS support can be added without provider or orchestration rewrites.
