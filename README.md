# SOOP Downloader

Rust/Axum 기반의 SOOP LIVE/VOD 다운로드 관리 서버입니다.

## 현재 구조

Phase 5.1부터 실행 경로는 Rust-only 입니다.

```text
Browser
  -> Rust Axum Web (soop-web)
     -> NativeWatcherManager
        -> RecorderManager -> streamlink
     -> VodManager -> yt-dlp / ffmpeg
     -> SQLite data/soop.db
```

기존 WinUI 3와 PowerShell LIVE/VOD 구현은 저장소에서 제거되었습니다.

설정/채널 파일은 Phase 5.2의 SQLite primary 전환 전까지 호환성 레이어로 유지합니다.

```text
backend/SOOP_LIVE_SETTING.ini
backend/SOOP_LIVE_CHANNELS.txt
backend/vod/SOOP_VOD_SETTING.ini
```

실제 런타임 파일은 Git에서 추적하지 않습니다. example 파일을 초기값으로 사용하고 Web UI에서 수정할 수 있습니다.

## 실행

개발/로컬 실행:

```powershell
.\RUN_RUST_WEB.bat
```

Release build:

```powershell
.\BUILD_RUST_WEB.bat
```

Portable package:

```powershell
.\PACKAGE_RUST_WEB.bat
```

패키지는 `dist\soop-recorder`에 생성됩니다.

서버는 OS 서비스로 등록되지 않으며 사용자가 직접 실행합니다. `Ctrl+C`로 종료하면 서버가 소유한 LIVE/VOD child process도 함께 정리합니다.

## 외부 도구

- streamlink
- yt-dlp
- ffmpeg

Portable package에는 외부 도구를 번들하지 않습니다. Web 설정에서 경로를 지정하거나 PATH를 사용하세요.

## 데이터

기본 SQLite 위치:

```text
data/soop.db
```

`SOOP_DATA_DIR` 환경변수로 위치를 변경할 수 있습니다.

## 다음 단계

Phase 5.2에서는 SQLite를 설정/채널의 primary source로 전환하고 INI/TXT는 migration/import 호환 경로로 축소할 예정입니다.
