# Repository Guide

이 저장소의 현재 제품 런타임은 Rust + SQLite 기반입니다.

## Canonical runtime

- `rust-web/src/main.rs`: 현재 Axum server/API presentation orchestration. Phase 21 동안 기존 Web UI의 regression reference로 유지하며, 새로운 제품 로직을 이 파일에 추가하지 않는다.
- `rust-web/src/lib.rs`: shared Rust library boundary. Phase 21부터 reusable runtime/service modules를 이 경계로 노출한다.
- `rust-web/src/app_core.rs`: `StreamArchiveCore` service facade. SQLite/settings/secrets/channels/LIVE watcher/VOD/process lifecycle을 HTTP와 분리해 Slint/CLI에서 직접 재사용하기 위한 canonical application boundary
- `rust-web/src/backup_service.rs`: reusable BackupManager/service boundary. managed backup policy/list/create/restore/integrity/retention을 Axum과 분리하고 Slint/Web이 같은 동작을 재사용한다.
- `rust-gui/Cargo.toml`: Windows Slint desktop frontend crate. 서버 패키지와 분리해 Slint dependency/빌드가 기존 Web 서버 배포를 임의로 바꾸지 않게 한다.
- `rust-gui/src/main.rs`: Slint bootstrap/state binding. 반드시 `StreamArchiveCore`를 직접 호출하고 localhost HTTP, 직접 SQLite, 직접 child-process 제어를 추가하지 않는다.
- `rust-gui/ui/*.slint`: Windows native presentation/navigation. provider/storage/process 구현 로직을 넣지 않는다.
- Phase 21.9에서 top-level `관리` navigation은 제거한다. Native `설정` 내부는 `일반`과 `관리`로 나눈다. `일반`은 provider/runtime 설정만 담당하고, `관리`는 backup policy + Backup/Restore/Diagnostics/Logs를 한곳에서 담당한다. 모든 작업은 `StreamArchiveCore`/`BackupManager`를 공유하며 DB copy/hash/restore, storage probing, 로그 파일 직접 읽기, process control을 Slint에 넣지 않는다.
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
- `rust-web/src/storage_service.rs`: shared storage-capacity diagnostics for Native/Web; canonical settings/channels are resolved here and presentation layers only render the result
- `rust-web/src/history_storage.rs`: current Web history/storage HTTP adapter; storage calculations delegate to `storage_service.rs`
- `rust-web/src/local_picker.rs`: localhost-only Windows file/folder picker bridge; retained only for the current Web UI until Slint native picker parity
- `rust-web/src/backend.rs`: log buffer and backend-directory resolution
- `rust-web/web/*`: current browser UI; retained while Phase 21 Slint parity is developed

Provider-specific network/authentication/stream mechanics live under `rust-web/src/platform/<provider>/`. Common queue/history/presentation orchestration must depend on the platform-neutral facades rather than a provider implementation directly.

## Configuration source of truth

`data/stream-archive.db` is the canonical source of truth for settings, channels, encrypted/native-referenced secrets, LIVE/VOD history, queue state, and backup policy.

Do not reintroduce runtime INI/TXT mirrors or one-off config-file readers. Product-wide runtime environment variables use the `STREAM_ARCHIVE_*` namespace. Provider-specific credentials such as `SOOP_USERNAME`, `SOOP_PASSWORD`, `CHZZK_NID_AUT`, and `CHZZK_NID_SES` remain provider-scoped settings.

The Phase 20 Unix CLI also writes canonical SQLite settings directly; it must not become a second configuration authority. Tool discovery persists only the existing runtime keys such as `STREAMLINK_PATH`, `YT_DLP_PATH`, and `FFMPEG_PATH`.

Phase 21 Slint code must use `StreamArchiveCore`/shared library services rather than opening a second SQLite connection as a new authority or reproducing settings/secret validation in UI callbacks.

The only retained transition bridge is the bounded `soop.db` -> `stream-archive.db` database filename migration. Do not add broader legacy compatibility layers without an explicit migration requirement.

## Process ownership rule

Never terminate tools globally with image-name/process-name commands such as `taskkill /IM ffmpeg.exe`, `taskkill /IM streamlink.exe`, `pkill ffmpeg`, or `killall streamlink`.

The runtime may terminate only process trees/groups it created and owns. Windows Job Object ownership and Unix process-group ownership live behind `platform_runtime` and are guarded by runtime-contract tests. Slint must call the shared runtime lifecycle instead of spawning or killing media tools directly.

## Build and test

Windows product/package flow:

```powershell
.\RUN_DEV.bat
.\BUILD_PORTABLE.bat
```

`BUILD_PORTABLE.bat` is the single Windows release/package entry point. Phase 21.8 builds both the Web compatibility binaries and the Slint frontend, then assembles `dist\stream-archive` with `StreamArchive.exe` as the default native entry point. `RUN_WEB.bat` retains the browser/Axum fallback. For compile-only developer checks, invoke Cargo directly instead of adding another build wrapper.

Phase 21 Windows Slint shell compile check:

```powershell
cargo check --locked --manifest-path rust-gui/Cargo.toml
```

The Slint crate remains a separate Windows frontend Cargo package, but Phase 21.8 includes its release binary in the Windows portable package as `StreamArchive.exe`. Portable startup must continue to use `StreamArchiveCore` directly; the Web server/launcher stay packaged only as an explicit compatibility fallback.

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

Phase 20 established the runtime-readiness baseline. Linux/macOS keep CLI/headless interfaces rather than receiving a second GUI implementation. Do not add a cross-platform native picker or Unix GUI launcher merely to mirror Windows.

Phase 21 introduces a Slint-based Windows native GUI. The intended dependency direction is:

```text
Slint UI --------┐
Unix CLI --------┼--> StreamArchiveCore / shared Rust library
Axum Web adapter ┘             |
                               +--> SQLite / Watcher / VOD / Security / Process ownership
```

Presentation layers must not call each other. In particular, Slint must not use localhost HTTP as its application API and must not duplicate provider, storage, process, security, queue, or backup logic. Browser authentication/session concerns remain Web-only. Keep the current browser UI until equivalent Slint functionality is validated; remove Web UI/Axum presentation routes incrementally only after parity and regression checks pass.