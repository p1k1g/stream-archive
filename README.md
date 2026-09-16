# Stream Archive

**SOOP과 CHZZK의 LIVE 녹화 및 VOD 다운로드를 하나의 Web UI에서 관리하는 로컬 미디어 아카이브입니다.**

Rust + Axum + SQLite 기반으로 동작하며, 현재는 Windows portable 환경을 중심으로 지원합니다. LIVE/VOD 수명주기, Queue, 설정, History, 백업/복구를 Rust 런타임에서 관리하고 Streamlink · yt-dlp · FFmpeg를 외부 미디어 처리 도구로 사용합니다.

> [!IMPORTANT]
> Stream Archive는 현재 **SOOP LIVE/VOD와 CHZZK LIVE/VOD만 지원**합니다. CATCH, 클립, 쇼츠/짧은 영상 및 기타 별도 콘텐츠 유형은 지원하지 않습니다.

## 프로젝트 성격 및 안정성 안내

Stream Archive는 개인 사용에서 출발해 빠르게 반복 개발하고 있는 오픈소스 프로젝트이며, 개발 과정에서 AI-assisted development를 적극적으로 활용하고 있습니다.

자동화된 테스트와 실제 사용 환경에서의 검증을 병행하고 있지만, 성숙한 상용 소프트웨어처럼 모든 운영체제·환경·예외 상황에서의 완전한 동작을 보장하지는 않습니다. 일부 기능과 UI, 내부 구조는 지속적으로 개선되고 있으며 변경될 수 있습니다.

중요한 녹화물과 설정 데이터는 별도로 백업해 두는 것을 권장합니다. 문제를 발견한 경우 재현 조건과 로그를 포함해 GitHub Issue로 알려주시면 개선에 도움이 됩니다.

## 지원 범위

현재 지원하는 콘텐츠 유형은 다음과 같습니다.

- SOOP LIVE 녹화
- SOOP VOD 분석 및 다운로드
- CHZZK LIVE 녹화
- CHZZK VOD 분석 및 다운로드

현재 지원하지 않는 콘텐츠 유형은 다음과 같습니다.

- SOOP/CHZZK 클립
- CATCH
- 쇼츠/짧은 영상 형식
- 별도 게시물/커뮤니티 콘텐츠
- 기타 LIVE/VOD 외 콘텐츠 유형

지원 대상은 SOOP 및 CHZZK의 일반 LIVE/VOD 흐름에 한정됩니다.

## 주요 기능

### SOOP

- LIVE 자동 녹화
- VOD 분석 및 다운로드
- 녹화 채널 관리
- 저장 경로 및 품질 설정

### CHZZK

- LIVE 자동 녹화
- VOD 분석 및 다운로드
- `NID_AUT` / `NID_SES` 기반 인증 정보 지원
- 공개/인증 필요 콘텐츠 처리

### 공통

- Web UI 기반 관리
- LIVE watcher 및 녹화 상태 관리
- VOD Queue / History / 재시도 / 취소
- SQLite 기반 설정·채널·History 저장
- 서버 종료 시 소유한 LIVE/VOD child process 정리
- 데이터 백업 / 복구
- Windows portable package
- 관리 토큰 및 secret 보호

## 지원 현황

| 기능 | SOOP | CHZZK |
|---|:---:|:---:|
| LIVE 녹화 | ✅ | ✅ |
| VOD 다운로드 | ✅ | ✅ |
| Web UI 관리 | ✅ | ✅ |
| Queue / History | ✅ | ✅ |
| 취소 / 재시도 | ✅ | ✅ |
| 클립 / CATCH / 기타 콘텐츠 | ❌ | ❌ |

| 운영체제 | 상태 |
|---|---|
| Windows | ✅ 현재 지원 |
| Linux | 🧪 Phase 20 진행 중 — CI 빌드/테스트 및 Unix process-group ownership 검증, 실제 제품 배포 검증 전 |
| macOS | 🧪 Phase 20 진행 중 — CI 빌드/테스트 및 Unix process-group ownership 검증, 실제 제품 배포 검증 전 |

## 빠른 시작

### 1. 외부 도구 준비

다음 도구가 필요합니다.

- `streamlink`
- `yt-dlp`
- `ffmpeg`

Portable package에는 외부 미디어 도구가 포함되지 않습니다. Web 설정에서 실행 파일 경로를 지정하거나 `PATH`에서 찾을 수 있도록 구성하세요.

각 외부 도구는 각 프로젝트의 라이선스와 배포 조건을 따릅니다. 자세한 내용은 `THIRD_PARTY_NOTICES.md`를 참고하세요.

### 2. 실행

Portable package에서는 `RUN.bat` 또는 `stream-archive-launcher.exe`를 실행합니다.

```text
RUN.bat / stream-archive-launcher.exe
        ↓
stream-archive-server.exe
        ↓
http://127.0.0.1:8787/
```

서버가 실행 중이 아니면 launcher가 서버를 시작하고 Web UI가 준비된 뒤 기본 브라우저를 엽니다. 이미 서버가 실행 중이면 기존 Web UI만 엽니다.

개발 환경에서 직접 실행하려면 저장소 루트에서 다음을 사용합니다.

```powershell
.\RUN_DEV.bat
```

### 3. 종료

서버 콘솔에서 `Ctrl+C`를 누릅니다.

Stream Archive는 종료 과정에서 자신이 소유한 LIVE/VOD child process를 정리합니다. 프로세스 이름 전체를 대상으로 하는 `taskkill /IM ffmpeg.exe`, `taskkill /IM streamlink.exe` 같은 방식은 사용하지 않습니다.

## 기본 사용 흐름

### LIVE

1. Web UI에서 플랫폼과 채널을 등록합니다.
2. 저장 경로와 필요한 설정을 구성합니다.
3. Watcher를 시작합니다.
4. 방송이 시작되면 Recorder가 LIVE 녹화를 시작합니다.
5. 방송 종료, 수동 중지 또는 서버 종료 시 소유한 녹화 프로세스를 정리합니다.

### VOD

1. SOOP 또는 CHZZK VOD URL을 입력합니다.
2. 콘텐츠 정보를 분석합니다.
3. 다운로드를 Queue에 등록합니다.
4. 진행 상태, 완료 내역, 실패/재시도 상태를 Web UI에서 확인합니다.
5. 필요하면 실행 중 작업을 취소할 수 있습니다.

## 런타임 구조

```text
Browser
  │
  ▼
Rust / Axum Web
  ├─ SQLite: data/stream-archive.db
  │    └─ settings / channels / queue / history source of truth
  │
  ├─ NativeWatcherManager
  │    └─ provider LIVE facade
  │         └─ RecorderManager
  │              └─ streamlink / ffmpeg
  │
  └─ provider VOD facade
       ├─ SOOP
       │    └─ yt-dlp / ffmpeg
       └─ CHZZK
            └─ API / streamlink / ffmpeg
```

현재 제품 런타임은 Rust/SQLite 중심입니다. 이전 WinUI/PowerShell 런타임과 INI/TXT 설정 mirror는 저장소에서 제거했습니다.

## 데이터와 보안

기본 SQLite 위치는 다음과 같습니다.

```text
data/stream-archive.db
```

`data/stream-archive.db`가 설정, 채널, Queue, History와 backup policy의 canonical source of truth입니다.

`STREAM_ARCHIVE_DATA_DIR` 환경변수로 데이터 디렉터리를 변경할 수 있습니다.

이전 개발 버전의 `data/soop.db`만 존재하고 `stream-archive.db`가 없는 경우에는 시작 시 새 파일명으로 한 번 이전합니다. INI/TXT 설정 파일을 런타임 원본이나 mirror로 사용하지 않습니다.

Windows에서는 SOOP 비밀번호, Cloudflare API key, CHZZK `NID_AUT` / `NID_SES` 같은 민감 정보가 CurrentUser DPAPI로 암호화된 형태로 SQLite에 저장됩니다. Linux/macOS에서는 SQLite에 평문 secret 대신 불투명한 `native-secret:v1:` 참조만 저장하고, 실제 secret은 각각 Linux Secret Service 또는 macOS Keychain에 저장합니다. Linux에서는 `secret-tool`과 사용 가능한 Secret Service 세션이 필요하며, native store를 사용할 수 없을 때 평문 저장으로 자동 fallback하지 않습니다. Web API는 평문 secret을 반환하지 않습니다.

기본 수신 주소는 로컬 loopback입니다.

```text
127.0.0.1:8787
```

## 관리 토큰

`STREAM_ARCHIVE_TOKEN`을 지정하지 않으면 서버가 관리 토큰을 자동 생성합니다.

생성된 토큰은 서버 시작 시 콘솔에 출력되고 다음 위치에 저장됩니다.

```text
backend/.stream-archive/web-token.txt
```

브라우저 자격 증명이나 세션을 잃은 경우 이 토큰으로 다시 인증할 수 있습니다.

`web-token.txt`는 관리 권한을 부여하는 secret이므로 공유하거나 Git에 커밋하지 마세요.

## 빌드

### 개발 / 로컬 실행

```powershell
.\RUN_DEV.bat
```

### Windows portable package

Windows에서 실제 실행·테스트·배포에 사용하는 단일 빌드 진입점은 다음입니다.

```powershell
.\BUILD_PORTABLE.bat
```

`BUILD_PORTABLE.bat`은 tracked `Cargo.lock`을 사용하는 release build를 내부에서 수행한 뒤 실행 가능한 portable 디렉터리를 조립합니다.

```text
cargo build --locked --release
        ↓
dist\stream-archive
```

기본 출력 위치:

```text
dist\stream-archive
```

주요 패키지 구성:

```text
stream-archive-server.exe
stream-archive-launcher.exe
RUN.bat
RUN_SERVER_CONSOLE.bat
BACKUP_DATA.bat
RESTORE_DATA.bat
RELEASE_INFO.txt
SHA256SUMS.txt
maintenance\Backup-StreamArchiveData.ps1
maintenance\Restore-StreamArchiveData.ps1
docs\...
data\
```

별도의 `BUILD_RELEASE.bat` wrapper는 사용하지 않습니다. 컴파일 결과만 확인해야 하는 개발 작업에서는 Cargo를 직접 실행할 수 있습니다.

```powershell
cargo build --locked --release --manifest-path .\rust-web\Cargo.toml
```

`rust-web\target\release`는 Cargo의 raw build output이며 배포 패키지 기준이 아닙니다. 실제 실행·배포 검증은 `dist\stream-archive`를 기준으로 합니다.

## 백업 / 복구

SQLite primary 전환 이후 핵심 백업 대상은 `data/stream-archive.db`입니다.

Web UI의 설정 → 백업에서 온라인 백업을 관리할 수 있습니다. 기본 관리형 백업 위치는 portable 디렉터리의 형제 폴더인 `stream-archive-backups`이며, `STREAM_ARCHIVE_BACKUP_DIR`로 위치를 고정할 수 있습니다.

오프라인 수동 백업은 서버를 `Ctrl+C`로 종료한 뒤 portable package의 다음 스크립트를 사용할 수 있습니다.

```powershell
.\BACKUP_DATA.bat
```

복구:

```powershell
.\RESTORE_DATA.bat -BackupFile ..\stream-archive-backups\stream_archive_manual_YYYYMMDD_HHMMSS.db
```

복구 스크립트는 서버 실행 중에는 동작하지 않으며, 기존 DB의 `pre_restore_*.db` 안전 복사본을 만든 뒤 교체합니다.

자세한 운영 절차는 `docs/OPERATIONS.md`를 참고하세요.

## 런타임 환경 변수

| 환경 변수 | 설명 |
|---|---|
| `STREAM_ARCHIVE_BIND` | 수신 주소. 기본값 `127.0.0.1:8787` |
| `STREAM_ARCHIVE_TOKEN` | 선택적 고정 관리 토큰 |
| `STREAM_ARCHIVE_START_WATCHER` | 서버 시작 후 watcher 자동 시작 여부 |
| `STREAM_ARCHIVE_BACKEND_DIR` | backend 디렉터리 override |
| `STREAM_ARCHIVE_DATA_DIR` | SQLite 데이터 디렉터리 |
| `STREAM_ARCHIVE_BACKUP_DIR` | Web 백업 디렉터리 override |

SOOP/CHZZK 계정 정보처럼 특정 provider에 속하는 설정 키는 provider namespace를 유지합니다.

## HTTPS / 원격 접근

원격 접근이 필요하면 Rust 서버는 loopback에 유지하고 Caddy/Nginx 같은 reverse proxy에서 HTTPS를 종료하는 구성을 권장합니다.

```text
Internet / LAN
      ↓
HTTPS reverse proxy
      ↓
127.0.0.1:8787
      ↓
Stream Archive
```

Launcher는 Windows Firewall, router port forwarding 또는 reverse proxy를 자동 구성하지 않습니다.

Caddy/Nginx 예제와 운영 보안 주의사항은 `docs/REVERSE_PROXY.md`를 참고하세요.

## 프로젝트 및 서비스 관련 안내

Stream Archive는 독립적인 오픈소스 프로젝트이며 SOOP, NAVER, CHZZK, Streamlink, FFmpeg, yt-dlp와 제휴·승인·후원 관계가 없습니다. 각 명칭과 상표는 해당 권리자에게 귀속됩니다.

사용자는 Stream Archive를 사용하는 과정에서 적용되는 법률, 저작권 규정 및 각 서비스의 이용약관을 확인하고 준수할 책임이 있습니다. 본 프로젝트는 콘텐츠에 대한 권리를 부여하거나 서비스 측 접근 제한을 우회할 권리를 제공하지 않습니다.

보안 취약점 제보 방법은 `SECURITY.md`, 기여 방법은 `CONTRIBUTING.md`를 참고하세요.

## 라이선스

Stream Archive 자체 코드는 **GNU Affero General Public License v3.0 or later (`AGPL-3.0-or-later`)**로 공개합니다.

Streamlink, FFmpeg, yt-dlp는 Stream Archive에 포함된 코드가 아니라 별도로 설치·탐지·실행되는 외부 도구이며 각 프로젝트의 라이선스를 따릅니다. 자세한 내용은 `THIRD_PARTY_NOTICES.md`를 참고하세요.

## 개발 / CI

Pull Request runtime validation은 `.github/workflows/rust-web-check.yml`에서 수행합니다.

주요 검증 항목:

- Windows / Linux / macOS JavaScript syntax check
- Windows / Linux / macOS whole-crate `cargo fmt --check`
- Windows / Linux / macOS Rust unit tests
- Windows / Linux / macOS native compile check
- Windows / Linux / macOS Clippy advisory
- Windows Runtime contract guard
- Source archive release-metadata smoke test
- Windows portable package smoke test
- Windows portable package verification

Release workflow는 `.github/workflows/rust-web-release.yml`의 수동 `workflow_dispatch` 방식입니다.

Phase 20의 Windows/Linux/macOS GitHub-hosted CI baseline은 구성되어 있습니다. Linux/macOS는 현재 CI 빌드·단위 테스트와 Unix process-group ownership 경계까지 검증되었으며 실제 제품 배포 및 사용자 흐름 검증은 아직 진행 중입니다.

## 문서

| 문서 | 내용 |
|---|---|
| `docs/LOCAL_LAUNCHER.md` | Windows local launcher 사용 방법 |
| `docs/OPERATIONS.md` | DB backup/restore, upgrade/rollback 절차 |
| `docs/REVERSE_PROXY.md` | Caddy/Nginx HTTPS reverse proxy 구성 |
| `docs/PHASE19_AUDIT.md` | Runtime hardening, ownership, architecture audit 및 Phase 20 경계 |
| `THIRD_PARTY_NOTICES.md` | 외부 도구 및 라이선스 안내 |
| `SECURITY.md` | 보안 취약점 제보 정책 |
| `CONTRIBUTING.md` | 기여 및 PR 가이드 |

## Roadmap

### Phase 19 ✅ Runtime Hardening / Cleanup

- Runtime resource ownership 및 cleanup 강화
- LIVE process-tree ownership 강화
- CHZZK VOD bounded reader / cancellation cleanup
- SQLite source-of-truth 정리
- OS-specific runtime boundary 통합
- Runtime contract / CI consolidation

### Phase 19.5 ✅ Namespace / Legacy Cleanup

- 제품 namespace를 `Stream Archive`로 통일
- app-wide 환경변수와 runtime 파일명을 `STREAM_ARCHIVE_*` / `stream-archive-*`로 정리
- INI/TXT compatibility path와 dead code 제거
- portable/build/release 이름 정리

### Phase 20 🚧 Cross-platform Readiness

진행 상황 및 예정 작업:

- ✅ Windows/Linux/macOS GitHub-hosted CI matrix baseline
- ✅ Unix process-group ownership / termination
- 🚧 Linux/macOS secret storage
- Native cross-platform picker
- Cross-platform launcher / packaging / tool discovery
- Linux/macOS integration coverage

---

**Stream Archive**는 현재 Windows에서 SOOP과 CHZZK의 LIVE/VOD를 안정적으로 관리하는 것을 우선 목표로 하며, 이후 Linux/macOS 지원을 위한 경계를 단계적으로 확장할 예정입니다.