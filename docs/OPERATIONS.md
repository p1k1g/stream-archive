<a id="stream-archive-operations"></a>

# Stream Archive 운영 가이드

현재 Rust/SQLite 런타임의 운영, 백업·복구, 업그레이드 및 rollback 절차입니다.

<a id="source-of-truth"></a>

## 설정과 데이터의 기준

`STREAM_ARCHIVE_DATA_DIR`을 지정하지 않으면 기본 DB는 `data/stream-archive.db`입니다. 설정, Channels, 암호화된 비밀정보, VOD Queue 상태, LIVE/VOD History 및 백업 정책이 이 DB에 저장됩니다.

INI/TXT 설정 mirror는 폐기했습니다. `SOOP_LIVE_SETTING.ini`, `SOOP_LIVE_CHANNELS.txt`, `SOOP_VOD_SETTING.ini`를 애플리케이션 설정으로 만들거나 편집하지 않습니다.

이전 비공개 build에서 업그레이드할 때 시작 과정에서 제한된 DB 파일명 migration만 수행합니다. `stream-archive.db`가 없고 `soop.db`가 있으면 새 기준 파일명으로 복사하며, SQLite 백업이 성공한 뒤 기존 DB 파일을 삭제합니다.

<a id="online-backup"></a>

## 실행 중 백업

Slint Native UI의 관리는 **설정** 화면에 모여 있습니다. **설정 → 일반**에서 서비스·런타임 설정을 관리하고, **설정 → 관리**에서 자동 백업 정책, 관리형 백업 생성·목록·무결성·복구, Diagnostics 및 Runtime Logs를 사용합니다. 공유 `BackupManager` / `StreamArchiveCore` 서비스와 SQLite의 기준 정책을 사용합니다.

기본 정책은 다음과 같습니다.

- 백업 활성화
- 24시간 간격
- 관리형 백업 최대 10개 보관
- 3일이 지난 관리형 백업 삭제
- 교체하는 portable 패키지 밖의 형제 디렉터리 `stream-archive-backups` 사용

`STREAM_ARCHIVE_BACKUP_DIR`로 백업 디렉터리를 강제 지정할 수 있습니다. 환경변수가 지정되면 Native UI의 백업 경로 필드는 읽기 전용이 되며 보관 정책은 편집할 수 있습니다.

관리형 백업 파일명은 `stream_archive_*.db`이며 `.db.json` metadata가 함께 생성됩니다. 유효한 metadata가 없는 파일은 자동 정리하지 않습니다.

<a id="native-storage-status"></a>

## Native 저장공간 상태

LIVE 화면은 `StreamArchiveCore`와 공유 `storage_service`에서 저장공간 진단을 읽습니다. Slint가 localhost HTTP를 사용하거나 직접 filesystem을 조회하지 않습니다. 설정된 `OUTPUT_DIR`, 채널별 출력 경로와 기준 SQLite 데이터 경로가 포함되며, 같은 Windows volume의 경로는 한 행으로 묶습니다.

| 상태 | 남은 공간 |
|---|---|
| 정상 / OK | `MIN_FREE_SPACE_GB`의 2배보다 큼 |
| 주의 / WARN | 기준값보다 크고 2배 이하 |
| 공간 부족 / CRITICAL | `MIN_FREE_SPACE_GB` 이하 |

녹화기의 실제 공간 부족 시작·중지 기준은 `MIN_FREE_SPACE_GB` 그대로입니다. 주의 구간은 화면 표시용입니다. 최초 Native 로딩, LIVE 화면으로 복귀, 직접 새로고침 및 LIVE 화면에서만 동작하는 제한된 timer로 갱신합니다.

<a id="chzzk-destination-claim-sidecars"></a>

## CHZZK 출력 경로 claim sidecar

CHZZK VOD의 최종 파일 확정에는 목표 미디어 파일 옆의 `.stream-archive.claim`을 재사용 가능한 file-lock 기준으로 사용합니다. 작업 완료 후에도 이 경로를 유지합니다. unlock 직후 삭제하면 경쟁 작업이 서로 다른 파일을 잠가 덮어쓰기 방지 보장이 약해질 수 있습니다.

Windows에서는 내부 claim 파일에 Hidden 속성을 지정해 Explorer에서 숨김 파일 표시를 끈 경우 보이지 않도록 합니다. 잠금 위치와 재사용 방식은 같습니다. 기존 claim 파일도 다음에 해당 출력 경로를 claim할 때 숨김 처리합니다. `.stream-archive.finalizing`은 임시 파일 확정용 artifact이며 기존 완료·취소·오래된 작업 복구 경로에서 정리합니다.

<a id="offline-manual-backup"></a>

## 종료 후 수동 백업

Windows 오프라인 유지보수 백업 전 `StreamArchive.exe`를 정상 종료합니다. 공식 Windows 패키지는 Native-only입니다. 개발용 또는 Unix 호환 headless 런타임을 따로 실행했다면 해당 소유자도 종료합니다. 관계없는 `streamlink`, `ffmpeg`, `yt-dlp` 프로세스를 종료하지 않습니다.

repository root 또는 portable package root에서 실행합니다.

```powershell
powershell -ExecutionPolicy Bypass -File .\maintenance\Backup-StreamArchiveData.ps1
```

스크립트는 다음을 수행합니다.

- `StreamArchive.exe` 또는 `stream-archive-server.exe` 실행 중에는 거부
- SQLite header 검증
- `stream-archive.db` 백업
- timestamp가 포함된 `stream_archive_manual_*.db` 생성
- 파일 크기·SHA-256을 포함한 JSON metadata 생성
- 기본 경로로 형제 디렉터리 `stream-archive-backups` 사용
- 기본적으로 최신 관리형 백업 10개 보관

보관 정책 지정:

```powershell
powershell -ExecutionPolicy Bypass -File .\maintenance\Backup-StreamArchiveData.ps1 -Keep 30 -RetentionDays 30
```

데이터 디렉터리 지정:

```powershell
powershell -ExecutionPolicy Bypass -File .\maintenance\Backup-StreamArchiveData.ps1 -DataDir D:\StreamArchiveData
```

<a id="restore"></a>

## 복구

복구는 기준 DB를 교체하므로 watcher, 실행 중인 VOD 작업 및 VOD Queue를 먼저 중지해야 합니다. `StreamArchiveCore::restore_backup`은 이 조건을 확인하고 먼저 `pre_restore` 안전 백업을 생성합니다.

오프라인 복구:

```powershell
powershell -ExecutionPolicy Bypass -File .\maintenance\Restore-StreamArchiveData.ps1 -BackupFile ..\stream-archive-backups\stream_archive_manual_YYYYMMDD_HHMMSS.db
```

복구 스크립트는 다음을 수행합니다.

- `StreamArchive.exe` 또는 `stream-archive-server.exe` 실행 중에는 거부
- SQLite header 검증
- 함께 제공된 metadata가 있으면 SHA-256 검증
- 현재 DB의 `pre_restore_*.db` 안전 사본 생성
- 오래된 `-wal`, `-shm` sidecar 제거
- 임시 파일을 거쳐 복사한 뒤 `stream-archive.db` 교체

복구 후 `StreamArchive.exe`를 실행해 설정, Channels, LIVE/VOD History 및 Queue 상태를 확인한 뒤 무인 운영을 재개합니다.

<a id="upgrade-procedure"></a>

## 업그레이드 절차

패키지를 교체하기 전에 다음을 수행합니다.

1. foreground watcher·런타임을 정상 종료합니다.
2. DB 백업을 생성합니다.
3. 새 버전을 충분히 확인할 때까지 이전 패키지·압축 파일을 보관합니다.
4. 새 압축 파일의 체크섬을 확인합니다.
5. 기존 런타임 데이터 경로와 교체할 패키지 파일을 분리합니다.
6. 새 패키지를 실행하고 Diagnostics를 확인한 뒤 무인 작업을 재개합니다.

<a id="windows-portable-upgrade"></a>

### Windows portable 업그레이드

공식 Windows ZIP은 비어 있는 `data\` 디렉터리를 포함하는 깨끗한 패키지입니다. 실행 중 DB의 유일한 사본 위에 덮어 풀면 안 됩니다.

1. `StreamArchive.exe`를 종료합니다.
2. `data\stream-archive.db`를 백업합니다.
3. 새 ZIP을 새 패키지 디렉터리에 풉니다.
4. 기존 데이터를 보존하거나 `STREAM_ARCHIVE_DATA_DIR`로 해당 경로를 명시합니다.
5. `StreamArchive.exe` / `RUN.bat`를 실행합니다.
6. Diagnostics, Channels, LIVE 시작·종료, VOD 분석·다운로드, Queue, History를 확인합니다.

개발용 `BUILD_PORTABLE.bat` 재빌드는 기존 `dist\stream-archive\data`를 보존할 수 있습니다. 이 편의 기능과 런타임 데이터가 비어 있어야 하는 공식 CI·배포 artifact contract는 구분합니다.

<a id="linuxmacos-portable-upgrade"></a>

### Linux/macOS portable 업그레이드

Phase 23.6에서 정립한 Unix 압축 파일은 `bin/`, `backend/`, 비어 있는 `data/`, 문서, 릴리스 metadata 및 체크섬을 포함합니다.

1. `stream-archive-cli serve --watch` 또는 `stream-archive-server`를 종료합니다.
2. 기준 SQLite DB를 백업합니다.
3. 압축 파일의 `.sha256`을 확인합니다.
4. 기존 패키지에 덮어쓰지 않고 새 디렉터리에 풉니다.
5. `STREAM_ARCHIVE_DATA_DIR`로 기존 데이터 경로를 재사용합니다. 복사할 경우 먼저 백업을 검증합니다.
6. `./bin/stream-archive-cli doctor --active-tools`를 실행합니다.
7. 런타임을 시작하고 상태·Queue·History를 확인한 뒤 무인 운영을 재개합니다.

패키지 기본 경로를 사용할 때는 압축 해제한 `stream-archive/` root에서 명령을 실행해야 `backend/`와 `data/`가 해당 패키지 기준으로 해석됩니다. 장기 운영에서는 환경변수로 binary·런타임 데이터·백업 디렉터리를 명시적으로 분리하는 편이 안전합니다.

<a id="rollback"></a>

### rollback

rollback 전에 새 런타임을 종료합니다. 먼저 이전 패키지로 돌아가고, 실제 DB 상태에서 필요한 경우에만 업그레이드 전 백업을 복구합니다. 임의 schema downgrade 호환성을 가정하지 않습니다.

[1.0.0 최종 수동 검증 체크리스트](https://github.com/p1k1g/stream-archive/blob/main/docs/MANUAL_RC_1_0_0.md)에 이전 버전 실행 가능 여부, 새 버전 실행 후 DB 호환 상태 및 필요한 백업 복구 증빙을 기록합니다. 확인하지 않은 rollback을 성공으로 표시하지 않습니다.

<a id="portable-package-replacement"></a>

## portable 패키지 교체

`BUILD_PORTABLE.bat`은 `dist\stream-archive`에 패키지를 만듭니다. Windows는 Native 애플리케이션 `StreamArchive.exe`와 launcher `RUN.bat`를 포함합니다. Windows 공식 배포에는 `stream-archive-server.exe`, `RUN_HEADLESS.bat`, browser launcher, Web static asset 및 reverse-proxy artifact가 포함되지 않습니다. 로컬 재빌드는 기존 `data`를 보존하지만 GitHub Actions build는 깨끗한 패키지를 사용합니다.

Explorer에서 직접 실행할 수 있습니다. backend 탐색은 `StreamArchive.exe` 옆의 `backend`를 우선하며 기본 SQLite 경로는 형제 디렉터리의 `data\stream-archive.db`입니다. 정의된 환경변수가 있으면 우선 적용합니다.

Linux/macOS는 `BUILD_UNIX_PACKAGE.sh`로 만들고 `bin/stream-archive-cli`, `bin/stream-archive-server`를 포함합니다. systemd/launchd 서비스나 package manager 항목은 설치하지 않습니다.

이전 INI/TXT 설정 파일을 새 패키지로 복사하지 않습니다. SQLite가 유일한 런타임 설정 기준입니다.

<a id="release-package-verification"></a>

## 릴리스 패키지 검증

Phase 23.6부터 패키지 조립과 검증을 분리합니다.

Windows:

```powershell
.\BUILD_PORTABLE.bat
powershell -ExecutionPolicy Bypass -File .\maintenance\Verify-WindowsPackage.ps1 -Root .\dist\stream-archive -RequireCleanData
powershell -ExecutionPolicy Bypass -File .\maintenance\New-WindowsReleaseArchive.ps1 -PackageRoot .\dist\stream-archive -OutputDir .\dist\release
```

`New-WindowsReleaseArchive.ps1`도 ZIP을 열기 전에 같은 clean-data verifier를 수행합니다. 기존 DB를 보존한 로컬 `BUILD_PORTABLE.bat` 결과는 staging에서 런타임 데이터를 분리하기 전에는 공식 배포 형태의 압축 파일로 만들 수 없습니다. 운영 데이터 원본을 삭제하는 절차가 아닙니다. CI·배포 검증은 생성한 ZIP과 archive-level `.sha256`도 확인합니다.

Linux/macOS:

```bash
./BUILD_UNIX_PACKAGE.sh
./maintenance/Verify-UnixPackage.sh ./dist/unix-<platform>-<arch>/stream-archive ./dist/release/stream-archive-<platform>-<arch>.tar.gz
```

검증기는 필수 파일, 실행 권한, 비어 있는 런타임 데이터, 내부 `SHA256SUMS.txt`, 압축 파일 체크섬 및 금지한 legacy·runtime artifact를 확인합니다. 공백·비ASCII 문자가 포함된 새 압축 해제 경로에서 CLI도 실행합니다.

Streamlink, yt-dlp, FFmpeg는 외부 의존성이며 공식 패키지에 재배포하지 않습니다. 최종 공개 artifact의 정확한 파일 점검은 [수동 RC 체크리스트](https://github.com/p1k1g/stream-archive/blob/main/docs/MANUAL_RC_1_0_0.md)를 따릅니다.

<a id="runtime-environment-overrides"></a>

## 런타임 환경변수

| 환경변수 | 용도 |
|---|---|
| `STREAM_ARCHIVE_START_WATCHER` | headless 런타임의 선택적 watcher 자동 시작 |
| `STREAM_ARCHIVE_BACKEND_DIR` | backend 디렉터리 지정 |
| `STREAM_ARCHIVE_DATA_DIR` | SQLite 데이터 디렉터리 지정 |
| `STREAM_ARCHIVE_BACKUP_DIR` | 관리형 백업 디렉터리 지정 |

`SOOP_USERNAME`, `SOOP_PASSWORD`, `CHZZK_NID_AUT`, `CHZZK_NID_SES`는 서비스별 설정으로 SQLite에 저장합니다.

<a id="incident-notes"></a>

## 장애 대응 시 주의사항

애플리케이션은 자신이 생성하고 소유한 child-process tree만 종료할 수 있습니다. `taskkill /IM ffmpeg.exe`, `taskkill /IM streamlink.exe`, `taskkill /IM yt-dlp.exe` 또는 같은 방식의 process-name 기반 일괄 종료를 사용하지 않습니다.

패키지 재빌드·복구 중 파일 잠금으로 실패하면 관계없는 미디어 도구를 종료하지 말고 Stream Archive를 정상 종료한 뒤 다시 시도합니다.

Linux/macOS Restore는 SOOP·CHZZK·KICK·Worker 인증정보와 정리 대기 기록을 현재 상태로 유지합니다. SOOP 사용자명·Worker URL도 현재 값을 유지하며, 다른 설정·채널·기록은 백업에서 복원합니다. 삭제·미설정 인증정보를 옛 백업에서 되살리지 않습니다. Windows DPAPI 복원 방식은 그대로입니다.
