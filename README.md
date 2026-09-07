# SOOP Downloader

Rust/Axum 기반의 SOOP LIVE/VOD 다운로드 관리 서버입니다.

## 현재 구조

Phase 5.2부터 실행 경로와 설정 저장소 모두 Rust/SQLite 기준입니다.

```text
Browser
  -> Rust Axum Web (soop-web)
     -> SQLite data/soop.db  [settings/channels/history source of truth]
     -> NativeWatcherManager
        -> RecorderManager -> streamlink
     -> VodManager -> yt-dlp / ffmpeg
```

기존 WinUI 3와 PowerShell LIVE/VOD 구현은 저장소에서 제거되었습니다.

## SQLite primary cutover

기본 SQLite 위치:

```text
data/soop.db
```

`SOOP_DATA_DIR` 환경변수로 위치를 변경할 수 있습니다.

Phase 5.2를 처음 실행할 때 기존 설정/채널 파일을 SQLite로 한 번 가져옵니다.

```text
backend/SOOP_LIVE_SETTING.ini
backend/SOOP_LIVE_CHANNELS.txt
backend/vod/SOOP_VOD_SETTING.ini
           |
           | one-time import
           v
data/soop.db
```

가져오기가 끝난 뒤에는 `data/soop.db`가 source of truth입니다. 위 INI/TXT는 native watcher 및 마이그레이션 호환을 위해 SQLite에서 자동 생성되는 mirror이며 수동 편집 내용은 authoritative하지 않습니다.

SOOP 비밀번호와 Cloudflare API key는 Windows CurrentUser DPAPI로 암호화한 문자열만 SQLite에 저장합니다. Web API는 평문 secret을 반환하지 않습니다.

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

## 데이터와 복구

SQLite에는 설정, 채널, LIVE history, VOD history가 저장됩니다. 서버 재시작 시 완료되지 않은 recording/job은 `INTERRUPTED` 상태로 복구합니다.

SQLite DB를 삭제한 경우 다음 실행 시 남아 있는 compatibility mirror/example을 이용해 새 DB를 초기화할 수 있습니다.

## 다음 단계

Phase 6에서는 release hardening, DB backup/restore 및 retention, 배포 패키지와 reverse proxy 문서를 최종 정리합니다.
