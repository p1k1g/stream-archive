# Repository Guide

이 저장소의 현재 제품 런타임은 Rust-only 입니다.

## Canonical runtime

- `rust-web/src/main.rs`: Axum server/API orchestration
- `rust-web/src/native_watcher.rs`: LIVE 상태 감시
- `rust-web/src/recorder.rs`: streamlink process ownership/lifecycle
- `rust-web/src/vod.rs`: VOD analyze/download/merge lifecycle
- `rust-web/src/security.rs`: Windows DPAPI secret protection
- `rust-web/src/store.rs`: SQLite persistence/history
- `rust-web/src/backend.rs`: INI/TXT compatibility layer
- `rust-web/web/*`: browser UI

## Runtime compatibility files

Phase 5.2 전까지 다음 파일 형식은 유지한다.

- `backend/SOOP_LIVE_SETTING.ini`
- `backend/SOOP_LIVE_CHANNELS.txt`
- `backend/vod/SOOP_VOD_SETTING.ini`

저장소에는 example 파일만 추적한다. 비밀번호/API key 같은 secret은 커밋하지 않는다.

## Process ownership rule

절대로 `taskkill /IM ffmpeg.exe`, `taskkill /IM streamlink.exe`, `taskkill /IM yt-dlp.exe`처럼 프로세스 이름 전체를 종료하지 않는다.

서버가 직접 생성하고 소유한 PID/process tree만 종료한다. Windows에서는 exact owned PID tree 방식만 사용한다.

## Build and test

```powershell
.\RUN_RUST_WEB.bat
.\BUILD_RUST_WEB.bat
.\PACKAGE_RUST_WEB.bat
```

Rust checks:

```powershell
cargo test --manifest-path rust-web/Cargo.toml
cargo check --manifest-path rust-web/Cargo.toml
```

## Architecture boundary

WinUI/PowerShell 구현은 Phase 5.1에서 제거되었다. 새 기능이나 bug fix를 위해 legacy GUI/backend를 다시 추가하지 않는다.

INI/TXT를 제거하거나 DB-only로 전환하는 작업은 Phase 5.2 범위로 취급한다.
