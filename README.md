# SOOP Downloader

Rust/Axum 기반의 SOOP LIVE/VOD 다운로드 관리 서버입니다.

## 현재 구조

Phase 6 기준 실행 경로와 설정 저장소는 Rust/SQLite이며, 배포/백업/복구 절차까지 portable package에 포함됩니다.

```text
Browser
  -> Rust Axum Web (soop-web)
     -> SQLite data/soop.db  [settings/channels/history source of truth]
     -> NativeWatcherManager
        -> RecorderManager -> streamlink
     -> VodManager -> yt-dlp / ffmpeg
```

기존 WinUI 3와 PowerShell LIVE/VOD 구현은 저장소에서 제거되었습니다.

## SQLite primary

기본 SQLite 위치:

```text
data/soop.db
```

`SOOP_DATA_DIR` 환경변수로 위치를 변경할 수 있습니다.

Phase 5.2 이후 `data/soop.db`가 source of truth입니다. `backend/SOOP_LIVE_SETTING.ini`, `backend/SOOP_LIVE_CHANNELS.txt`, `backend/vod/SOOP_VOD_SETTING.ini`는 native watcher/마이그레이션 호환을 위해 SQLite에서 생성되는 mirror이며 수동 편집 내용은 authoritative하지 않습니다.

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

Release build는 tracked `Cargo.lock`을 사용하는 `cargo build --locked --release`로 수행합니다.

Portable package:

```powershell
.\PACKAGE_RUST_WEB.bat
```

패키지는 `dist\soop-recorder`에 생성됩니다. Phase 6 패키지에는 다음이 포함됩니다.

```text
soop-server.exe
RUN.bat
BACKUP_DATA.bat
RESTORE_DATA.bat
RELEASE_INFO.txt
SHA256SUMS.txt
backend\...
maintenance\Backup-SoopData.ps1
maintenance\Restore-SoopData.ps1
docs\OPERATIONS.md
docs\REVERSE_PROXY.md
data\
```

서버는 OS 서비스로 등록되지 않으며 사용자가 직접 실행합니다. `Ctrl+C`로 종료하면 서버가 소유한 LIVE/VOD child process도 함께 정리합니다.

## 백업 / 복구

SQLite primary 전환 이후 백업 대상은 `data/soop.db`입니다. 일관된 백업을 위해 서버를 `Ctrl+C`로 종료한 뒤 실행하세요.

```powershell
.\BACKUP_DATA.bat
```

기본적으로 `data\backups` 아래에 timestamp DB와 SHA-256 metadata를 만들고 최신 10개를 유지합니다.

복구:

```powershell
.\RESTORE_DATA.bat -BackupFile .\data\backups\soop_YYYYMMDD_HHMMSS.db
```

복구 스크립트는 서버 실행 중에는 동작하지 않고, 기존 DB의 `pre_restore_*.db` 안전 복사본을 만든 뒤 교체합니다. 자세한 절차는 `docs/OPERATIONS.md`를 참고하세요.

## 외부 도구

- streamlink
- yt-dlp
- ffmpeg

Portable package에는 외부 도구를 번들하지 않습니다. Web 설정에서 경로를 지정하거나 PATH를 사용하세요.

## HTTPS / reverse proxy

권장 구조는 Rust 서버를 `127.0.0.1:8787`에 유지하고 Caddy/Nginx에서 HTTPS를 종료하는 방식입니다.

```text
Internet / LAN -> HTTPS reverse proxy -> 127.0.0.1:8787 -> SOOP Rust Web
```

Caddy/Nginx 예제와 운영 보안 주의사항은 `docs/REVERSE_PROXY.md`에 정리되어 있습니다.

## Release workflow

`.github/workflows/rust-web-release.yml`은 수동 `workflow_dispatch` 방식입니다. release job은 locked dependency unit test, Windows portable build, package 내부 `soop-server.exe` SHA-256 검증, ZIP 생성 및 ZIP checksum 생성을 수행합니다.

## 운영 문서

- `docs/OPERATIONS.md`: DB backup/restore, upgrade/rollback 절차
- `docs/REVERSE_PROXY.md`: Caddy/Nginx HTTPS reverse proxy 구성

Phase 6 이후에는 기능 migration보다 실제 장기 운영 중 발견되는 안정성/UX 개선을 작은 후속 릴리스로 처리하는 단계입니다.
