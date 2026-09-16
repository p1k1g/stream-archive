# Repository Guide

이 저장소의 현재 제품 런타임은 Rust + SQLite 기반입니다.

## Canonical runtime

- `rust-web/src/main.rs`: Axum server/API orchestration
- `rust-web/src/lib.rs`: shared Rust library boundary; Phase 20 starts with reusable tool discovery and Phase 21 should move reusable core interfaces behind this boundary rather than duplicating runtime logic
- `rust-web/src/tool_discovery.rs`: cross-platform Streamlink/yt-dlp/FFmpeg discovery
- `rust-web/src/bin/stream-archive-cli.rs`: Linux/macOS-oriented headless CLI baseline
- `rust-web/src/native_watcher.rs`: provider-neutral LIVE 상태 감시 orchestration
- `rust-web/src/recorder.rs`: Streamlink/FFmpeg process ownership/lifecycle
- `rust-web/src/platform/live.rs`: platform-neutral LIVE provider facade/session types
- `rust-web/src/platform/vod.rs`: platform-neutral VOD lifecycle/dispatch
- `rust-web/src/platform/soop/*`: SOOP provider implementation
- `rust-web/src/platform/chzzk/*`: CHZZK provider implementation
- `rust-web/src/security.rs`: secret protection boundary
- `rust-web/src/store.rs`: canonical SQLite persistence/config/history
- `rust-web/src/history_storage.rs`: history queries and storage diagnostics APIs
- `rust-web/src/local_picker.rs`: localhost-only Windows file/folder picker bridge; retained for the current Web UI but not a cross-platform Phase 20 target
- `rust-web/src/backend.rs`: log buffer and backend-directory resolution
- `rust-web/web/*`: current browser UI; retained through Phase 20 while Phase 21 Slint parity is developed

Provider-specific network/authentication/stream mechanics live under `rust-web/src/platform/<provider>/`. Common queue/history/API orchestration must depend on the platform-neutral facades rather than a provider implementation directly.

## Configuration source of truth

`data/stream-archive.db` is the canonical source of truth for settings, channels, encrypted/native-referenced secrets, LIVE/VOD history, queue state, and backup policy.

Do not reintroduce runtime INI/TXT mirrors or one-off config-file readers. Product-wide runtime environment variables use the `STREAM_ARCHIVE_*` namespace. Provider-specific credentials such as `SOOP_USERNAME`, `SOOP_PASSWORD`, `CHZZK_NID_AUT`, and `CHZZK_NID_SES` remain provider-scoped settings.

The Phase 20 Unix CLI also writes canonical SQLite settings directly; it must not become a second configuration authority. Tool discovery persists only the existing runtime keys such as `STREAMLINK_PATH`, `YT_DLP_PATH`, and `FFMPEG_PATH`.

The only retained transition bridge is the bounded `soop.db` -> `stream-archive.db` database filename migration. Do not add broader legacy compatibility layers without an explicit migration requirement.

## Process ownership rule

Never terminate tools globally with image-name/process-name commands such as `taskkill /IM ffmpeg.exe`, `taskkill /IM streamlink.exe`, `pkill ffmpeg`, or `killall streamlink`.

The server may terminate only process trees/groups it created and owns. Windows Job Object ownership and Unix process-group ownership live behind `platform_runtime` and are guarded by runtime-contract tests.

## Build and test

Windows product/package flow:

```powershell
.\RUN_DEV.bat
.\BUILD_PORTABLE.bat
```

`BUILD_PORTABLE.bat` is the single Windows release/package entry point. It performs the locked release build and assembles the runnable `dist\stream-archive` package. For compile-only developer checks, invoke Cargo directly instead of adding another build wrapper.

Unix/headless Phase 20 flow:

```bash
cargo build --locked --release --manifest-path rust-web/Cargo.toml
./rust-web/target/release/stream-archive-cli init
./rust-web/target/release/stream-archive-cli tools configure
./rust-web/target/release/stream-archive-cli doctor
./rust-web/target/release/stream-archive-cli serve --watch
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

Phase 20 is a runtime-readiness phase. Linux/macOS should use CLI/headless interfaces rather than receiving a second GUI implementation. Do not add a cross-platform native picker or Unix GUI launcher merely to mirror Windows.

Phase 21 will introduce a Slint-based Windows native GUI. Slint should consume shared Rust library/service interfaces instead of reimplementing provider, storage, process, security, queue, or backup logic. Keep the current browser UI until equivalent Slint functionality is validated; remove Web UI/Axum presentation routes incrementally only after parity and regression checks pass.
