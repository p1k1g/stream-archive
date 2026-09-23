# Repository Guide

이 저장소의 현재 제품 런타임은 Rust + SQLite 기반입니다.

## Canonical runtime

- `rust-runtime/src/main.rs`: HTTP/Web presentation이 없는 compatibility headless runtime entry. 실제 bootstrap/lifecycle은 `rust-runtime/src/headless.rs`의 shared runner에 위임한다.
- `rust-runtime/src/lib.rs`: shared Rust library boundary. source directory는 `rust-runtime`이고 Cargo package / Rust crate compatibility identity는 `stream-archive-server` / `stream_archive_server`로 유지한다.
- `rust-runtime/src/app_core.rs`: `StreamArchiveCore` service facade. SQLite/settings/secrets/channels/LIVE watcher/VOD/Queue/History/Backup/Storage/process lifecycle의 canonical application boundary.
- `rust-runtime/src/backup_service.rs`: reusable BackupManager/service boundary. managed backup policy/list/create/restore/integrity/retention을 presentation과 분리한다.
- `rust-gui/Cargo.toml`: Windows Slint desktop frontend crate.
- `rust-gui/src/main.rs`: Slint bootstrap/state binding. 반드시 `StreamArchiveCore`를 직접 호출하고 localhost HTTP, 직접 SQLite, 직접 child-process 제어를 추가하지 않는다.
- `rust-gui/ui/*.slint`: Windows native presentation/navigation. provider/storage/process 구현 로직을 넣지 않는다.
- Native `설정` 내부는 `일반`과 `관리`로 나뉜다. `일반`은 provider/runtime 설정, `관리`는 backup policy + Backup/Restore/Diagnostics/Logs를 담당한다. 모든 작업은 shared services를 사용한다.
- `rust-runtime/src/tool_discovery.rs`: cross-platform Streamlink/yt-dlp/FFmpeg discovery.
- `rust-runtime/src/bin/stream-archive-cli.rs`: Linux/macOS-oriented headless CLI entry. bootstrap(`init`/`tools`/`doctor`)과 Phase 23.5 daily-use management를 제공하며, management는 `rust-runtime/src/unix_cli.rs`를 통해 `StreamArchiveCore`를 사용한다. `serve`는 shared headless runner를 직접 사용하고 Web server를 시작하지 않는다.
- `rust-runtime/src/native_watcher.rs`: provider-neutral LIVE 상태 감시 orchestration.
- `rust-runtime/src/recorder.rs`: Streamlink/FFmpeg process ownership/lifecycle.
- `rust-runtime/src/platform/live.rs`: platform-neutral LIVE provider facade/session types.
- `rust-runtime/src/platform/vod.rs`: platform-neutral VOD lifecycle/dispatch.
- `rust-runtime/src/platform/soop/*`: SOOP provider implementation.
- `rust-runtime/src/platform/chzzk/*`: CHZZK provider implementation.
- `rust-runtime/src/security.rs`: secret protection boundary.
- `rust-runtime/src/store.rs`: canonical SQLite persistence/config/history.
- `rust-runtime/src/storage_service.rs`: shared storage-capacity diagnostics.
- `rust-runtime/src/history_service.rs`: shared History query service.
- `rust-runtime/src/queue_service.rs`: shared persistent VOD Queue service.
- `rust-runtime/src/backend.rs`: log buffer and backend-directory resolution.

Browser static UI, Axum routes, browser auth/session/CSRF, SSE, Web local picker and the Windows browser launcher were retired in Phase 22.3. Do not reintroduce them as a shortcut around `StreamArchiveCore`.

Provider-specific network/authentication/stream mechanics live under `rust-runtime/src/platform/<provider>/`. Common queue/history/runtime orchestration must depend on platform-neutral facades rather than provider implementations directly.

## Configuration source of truth

`data/stream-archive.db` is the canonical source of truth for settings, channels, encrypted/native-referenced secrets, LIVE/VOD history, queue state, and backup policy.

Do not reintroduce runtime INI/TXT mirrors or one-off config-file readers. Product-wide runtime environment variables use the `STREAM_ARCHIVE_*` namespace. Provider-specific credentials such as `SOOP_USERNAME`, `SOOP_PASSWORD`, `CHZZK_NID_AUT`, and `CHZZK_NID_SES` remain provider-scoped settings.

Unix CLI의 bootstrap `init`/`tools configure`는 기존 bounded SQLite bootstrap 경계를 유지한다. Phase 23.5 daily-use management는 `StreamArchiveCore`/shared services를 사용해야 하며 두 번째 configuration authority가 되어서는 안 된다. Tool discovery는 `STREAMLINK_PATH`, `YT_DLP_PATH`, `FFMPEG_PATH` 같은 기존 runtime key만 저장한다.

Slint code must use `StreamArchiveCore`/shared library services rather than opening a second SQLite connection or reproducing settings/secret validation in UI callbacks.

The only retained transition bridge is the bounded `soop.db` -> `stream-archive.db` database filename migration.

## Process ownership rule

Never terminate tools globally with image-name/process-name commands such as `taskkill /IM ffmpeg.exe`, `taskkill /IM streamlink.exe`, `pkill ffmpeg`, or `killall streamlink`.

The runtime may terminate only process trees/groups it created and owns. Windows Job Object ownership and Unix process-group ownership live behind `platform_runtime` and are guarded by runtime-contract tests. CHZZK job/destination OS locks remain the source of truth for active temporary/destination ownership.

## Build and test

Windows product/package flow:

```powershell
.\RUN_DEV.bat
.\BUILD_PORTABLE.bat
```

`RUN_DEV.bat` starts the compatible headless runtime. The Native GUI can be run directly through Cargo.

`BUILD_PORTABLE.bat` is the single Windows release/package entry point. It builds the retained shared/headless `rust-runtime` package plus the Slint frontend and assembles `dist\stream-archive` with `StreamArchive.exe` as the default entry point. `RUN_HEADLESS.bat` is the optional console/headless entry. Browser/Web fallback artifacts are not packaged.

Windows Slint compile check:

```powershell
cargo check --locked --manifest-path rust-gui/Cargo.toml
```

Unix/headless flow:

```bash
cargo build --locked --release --manifest-path rust-runtime/Cargo.toml
./rust-runtime/target/release/stream-archive-cli init
./rust-runtime/target/release/stream-archive-cli tools configure
./rust-runtime/target/release/stream-archive-cli doctor --active-tools
./rust-runtime/target/release/stream-archive-cli status --json
./rust-runtime/target/release/stream-archive-cli channels list --json
./rust-runtime/target/release/stream-archive-cli queue list --json
./rust-runtime/target/release/stream-archive-cli serve --watch
```

Rust checks:

```powershell
cargo fmt --manifest-path rust-runtime/Cargo.toml -- --check
cargo test --locked --manifest-path rust-runtime/Cargo.toml
cargo check --locked --manifest-path rust-runtime/Cargo.toml
cargo clippy --locked --manifest-path rust-runtime/Cargo.toml --all-targets --all-features -- -D warnings
```

Runtime contracts:

```powershell
.\maintenance\Test-RuntimeContracts.ps1
```

## Architecture boundary

The old WinUI/PowerShell runtime, browser/Axum presentation layer, browser launcher and INI/TXT configuration path are retired. New functionality and bug fixes must stay in the Rust/SQLite runtime and must not restore those legacy paths merely for compatibility.

Linux/macOS keep CLI/headless interfaces rather than receiving a second GUI implementation. Do not add a cross-platform native picker or Unix GUI launcher merely to mirror Windows.

The intended dependency direction is:

```text
Slint UI --------┐
Unix CLI --------┼--> StreamArchiveCore / shared Rust library
Headless runtime ┘             |
                               +--> SQLite / Watcher / VOD / Queue / Security / Process ownership
```

Presentation layers must not call each other. Slint must not use localhost HTTP as its application API and must not duplicate provider, storage, process, security, queue, backup or persistence logic.
