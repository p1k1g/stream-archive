# Stream Archive

**SOOP과 CHZZK의 LIVE 녹화 및 VOD 다운로드를 Windows Native UI에서 관리하는 로컬 미디어 아카이브입니다.**

Rust + Slint + SQLite 기반의 Native UI를 기본으로 사용하며, Windows portable 환경을 중심으로 지원합니다. Linux/macOS에서는 CLI/headless 경로를 유지합니다. LIVE/VOD 수명주기, Queue, 설정, History, 백업/복구를 Rust 런타임에서 관리하고 Streamlink · yt-dlp · FFmpeg를 외부 미디어 처리 도구로 사용합니다.

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

- Windows Slint Native UI 기반 관리
- LIVE watcher 및 녹화 상태 관리
- VOD Queue / History / 재시도 / 취소
- SQLite 기반 설정·채널·History 저장
- 애플리케이션 종료 시 소유한 LIVE/VOD child process 정리
- 데이터 백업 / 복구 / 진단 / Runtime Logs
- Windows portable package
- native secret 보호

## 지원 현황

| 기능 | SOOP | CHZZK |
|---|:---:|:---:|
| LIVE 녹화 | ✅ | ✅ |
| VOD 다운로드 | ✅ | ✅ |
| Native UI 관리 | ✅ | ✅ |
| Queue / History | ✅ | ✅ |
| 취소 / 재시도 | ✅ | ✅ |
| 클립 / CATCH / 기타 콘텐츠 | ❌ | ❌ |

| 운영체제 | 상태 |
|---|---|
| Windows | ✅ 현재 지원 |
| Linux | 🧪 Phase 20 — CI, process-group ownership, Secret Service 경계, CLI/tool discovery 구현. 실제 배포·통합 검증 진행 중 |
| macOS | 🧪 Phase 20 — CI, process-group ownership, Keychain 경계, CLI/tool discovery 구현. 실제 배포·통합 검증 진행 중 |

## 빠른 시작

### 1. 외부 도구 준비

다음 도구가 필요합니다.

- `streamlink`
- `yt-dlp`
- `ffmpeg`

Windows portable package에는 외부 미디어 도구가 포함되지 않습니다. Native UI의 설정 화면에서 실행 파일 경로를 지정하거나 `PATH`에서 찾을 수 있도록 구성하세요. Linux/macOS에서는 Phase 20의 `stream-archive-cli tools` / `tools configure`로 Unix 이름과 `PATH`를 기준으로 탐색하고 SQLite에 절대 경로를 저장할 수 있습니다.

각 외부 도구는 각 프로젝트의 라이선스와 배포 조건을 따릅니다. 자세한 내용은 `THIRD_PARTY_NOTICES.md`를 참고하세요.

### 2. 실행

Portable package에서는 `RUN.bat` 또는 `StreamArchive.exe`를 실행합니다.

```text
RUN.bat
   ↓
StreamArchive.exe
   ↓
StreamArchiveCore
   ↓
data\stream-archive.db
```

Explorer에서 `StreamArchive.exe`를 직접 더블클릭해도 portable root의 `backend\`와 `data\stream-archive.db`를 기준으로 동작합니다.

개발 환경에서 headless runtime을 직접 실행하려면 저장소 루트에서 `RUN_DEV.bat`을 사용하고, Native GUI는 Cargo로 직접 실행할 수 있습니다.

```powershell
.\RUN_DEV.bat
cargo run --locked --manifest-path .\rust-gui\Cargo.toml
```

### 3. 종료

Native 앱은 창을 정상 종료합니다. 선택적으로 headless runtime을 직접 실행한 경우 콘솔에서 `Ctrl+C`를 누릅니다.

Stream Archive는 종료 과정에서 자신이 소유한 LIVE/VOD child process를 정리합니다. 프로세스 이름 전체를 대상으로 하는 `taskkill /IM ffmpeg.exe`, `taskkill /IM streamlink.exe` 같은 방식은 사용하지 않습니다.

## 기본 사용 흐름

### LIVE

1. Native UI의 채널 화면에서 플랫폼과 채널을 등록합니다.
2. 저장 경로와 필요한 설정을 구성합니다.
3. Watcher를 시작합니다.
4. 방송이 시작되면 Recorder가 LIVE 녹화를 시작합니다.
5. 방송 종료, 수동 중지 또는 런타임 종료 시 소유한 녹화 프로세스를 정리합니다.

### VOD

1. SOOP 또는 CHZZK VOD URL을 입력합니다.
2. 콘텐츠 정보를 분석합니다.
3. 다운로드를 Queue에 등록합니다.
4. 진행 상태, 완료 내역, 실패/재시도 상태를 Native UI의 대기열/기록 화면에서 확인합니다.
5. 필요하면 실행 중 작업을 취소할 수 있습니다.

## 런타임 구조

```text
Windows Slint Native GUI ─┐
Unix CLI/headless ─────────┼──> StreamArchiveCore / shared Rust runtime
Headless runtime binary ───┘             │
                                         ├─ SQLite: data/stream-archive.db
                                         ├─ NativeWatcherManager / RecorderManager
                                         ├─ VOD / Queue / History / Backup
                                         └─ Streamlink / yt-dlp / FFmpeg
```

Windows portable의 기본 진입점은 Slint Native GUI이며 localhost HTTP를 애플리케이션 API로 사용하지 않습니다. Native UI와 headless runtime은 모두 `StreamArchiveCore`와 같은 SQLite/runtime 서비스를 직접 사용합니다.

Phase 22.3에서 기존 Axum/browser presentation, browser launcher와 Web fallback package 경로를 제거했습니다. 이전 WinUI/PowerShell 런타임과 INI/TXT 설정 mirror도 제거된 상태를 유지합니다.

Phase 20의 Linux/macOS 경로는 계속 `stream-archive-cli` 기반 headless 인터페이스를 사용합니다.

## 데이터와 보안

기본 SQLite 위치는 다음과 같습니다.

```text
data/stream-archive.db
```

`data/stream-archive.db`가 설정, 채널, Queue, History와 backup policy의 canonical source of truth입니다.

`STREAM_ARCHIVE_DATA_DIR` 환경변수로 데이터 디렉터리를 변경할 수 있습니다.

이전 개발 버전의 `data/soop.db`만 존재하고 `stream-archive.db`가 없는 경우에는 시작 시 새 파일명으로 한 번 이전합니다. INI/TXT 설정 파일을 런타임 원본이나 mirror로 사용하지 않습니다.

Windows에서는 SOOP 비밀번호, Cloudflare API key, CHZZK `NID_AUT` / `NID_SES` 같은 민감 정보가 CurrentUser DPAPI로 암호화된 형태로 SQLite에 저장됩니다. Linux/macOS에서는 SQLite에 평문 secret 대신 불투명한 `native-secret:v1:` 참조만 저장하고, 실제 secret은 각각 Linux Secret Service 또는 macOS Keychain에 저장합니다. Linux에서는 `secret-tool`과 사용 가능한 Secret Service 세션이 필요하며, native store를 사용할 수 없을 때 평문 저장으로 자동 fallback하지 않습니다.


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

`BUILD_PORTABLE.bat`은 shared/headless Rust runtime과 Slint Native GUI를 각각 tracked `Cargo.lock`으로 release build한 뒤 실행 가능한 portable 디렉터리를 조립합니다.

```text
shared/headless rust-runtime build + rust-gui release build
                    ↓
             dist\stream-archive
```

기본 출력 위치:

```text
dist\stream-archive
```

주요 패키지 구성:

```text
StreamArchive.exe
RUN.bat
stream-archive-server.exe
RUN_HEADLESS.bat
BACKUP_DATA.bat
RESTORE_DATA.bat
RELEASE_INFO.txt
SHA256SUMS.txt
backend\
data\
maintenance\...
docs\...
```

`RUN.bat`과 `StreamArchive.exe`가 기본 Native 경로이며, `RUN_HEADLESS.bat`은 선택적인 headless runtime 경로입니다.

별도의 `BUILD_RELEASE.bat` wrapper는 사용하지 않습니다. 컴파일 결과만 확인해야 하는 개발 작업에서는 Cargo를 직접 실행할 수 있습니다.

```powershell
cargo build --locked --release --manifest-path .\rust-runtime\Cargo.toml
cargo build --locked --release --manifest-path .\rust-gui\Cargo.toml
```

각 Cargo `target\release` 디렉터리는 raw build output이며 배포 패키지 기준이 아닙니다. 실제 실행·배포 검증은 `dist\stream-archive`를 기준으로 합니다.

### Linux / macOS CLI baseline

Phase 20의 Unix 경로는 GUI launcher가 아니라 `stream-archive-cli`를 사용합니다.

```bash
cargo build --locked --release --manifest-path rust-runtime/Cargo.toml
./rust-runtime/target/release/stream-archive-cli init
./rust-runtime/target/release/stream-archive-cli tools
./rust-runtime/target/release/stream-archive-cli tools configure
./rust-runtime/target/release/stream-archive-cli doctor
./rust-runtime/target/release/stream-archive-cli serve --watch
```

`tools configure`는 Streamlink/yt-dlp/FFmpeg를 기존 SQLite 설정 → backend layout → `PATH` → 일반적인 Unix 설치 경로 순서로 찾고, 발견된 절대 경로를 canonical SQLite 설정에 원자적으로 기록합니다. 별도 INI/TXT 설정 파일은 만들지 않습니다.

`stream-archive-cli doctor`와 `doctor --json`은 Windows Native Diagnostics와 같은 shared runtime preflight를 사용합니다. Required Error만 실행 차단으로 취급하고 optional/provider warning은 별도 주의 상태로 표시하며, provider 네트워크 접속·실제 다운로드·설정 변경은 수행하지 않습니다.

현재 CLI는 Phase 20의 headless/tool-discovery baseline이며 전체 채널·VOD·secret 관리 명령은 Linux/macOS 통합 검증과 함께 확장할 예정입니다. 자세한 내용은 `docs/UNIX_CLI.md`를 참고하세요.

## 백업 / 복구

SQLite primary 전환 이후 핵심 백업 대상은 `data/stream-archive.db`입니다.

Native UI의 설정 → 관리에서 백업 정책, 온라인 백업 생성/무결성 확인/복원을 shared backup service를 통해 함께 관리할 수 있습니다. 기본 관리형 백업 위치는 portable 디렉터리의 형제 폴더인 `stream-archive-backups`이며, `STREAM_ARCHIVE_BACKUP_DIR`로 위치를 고정할 수 있습니다.

오프라인 수동 백업은 `StreamArchive.exe`를 닫고 선택적인 headless runtime도 중지한 뒤 portable package의 다음 스크립트를 사용할 수 있습니다.

```powershell
.\BACKUP_DATA.bat
```

복구:

```powershell
.\RESTORE_DATA.bat -BackupFile ..\stream-archive-backups\stream_archive_manual_YYYYMMDD_HHMMSS.db
```

백업/복구 스크립트는 Native 앱 또는 headless runtime 실행 중에는 동작하지 않으며, 복구 시 기존 DB의 `pre_restore_*.db` 안전 복사본을 만든 뒤 교체합니다.

자세한 운영 절차는 `docs/OPERATIONS.md`를 참고하세요.

## 런타임 환경 변수

| 환경 변수 | 설명 |
|---|---|
| `STREAM_ARCHIVE_START_WATCHER` | headless runtime 시작 후 watcher 자동 시작 여부 |
| `STREAM_ARCHIVE_BACKEND_DIR` | backend 디렉터리 override |
| `STREAM_ARCHIVE_DATA_DIR` | SQLite 데이터 디렉터리 |
| `STREAM_ARCHIVE_BACKUP_DIR` | 관리형 백업 디렉터리 override |

SOOP/CHZZK 계정 정보처럼 특정 provider에 속하는 설정 키는 provider namespace를 유지합니다.


## 프로젝트 및 서비스 관련 안내

Stream Archive는 독립적인 오픈소스 프로젝트이며 SOOP, NAVER, CHZZK, Streamlink, FFmpeg, yt-dlp와 제휴·승인·후원 관계가 없습니다. 각 명칭과 상표는 해당 권리자에게 귀속됩니다.

사용자는 Stream Archive를 사용하는 과정에서 적용되는 법률, 저작권 규정 및 각 서비스의 이용약관을 확인하고 준수할 책임이 있습니다. 본 프로젝트는 콘텐츠에 대한 권리를 부여하거나 서비스 측 접근 제한을 우회할 권리를 제공하지 않습니다.

보안 취약점 제보 방법은 `SECURITY.md`, 기여 방법은 `CONTRIBUTING.md`를 참고하세요.

## 라이선스

Stream Archive 자체 코드는 **GNU Affero General Public License v3.0 or later (`AGPL-3.0-or-later`)**로 공개합니다.

Streamlink, FFmpeg, yt-dlp는 Stream Archive에 포함된 코드가 아니라 별도로 설치·탐지·실행되는 외부 도구이며 각 프로젝트의 라이선스를 따릅니다. 자세한 내용은 `THIRD_PARTY_NOTICES.md`를 참고하세요.

## 개발 / CI

Pull Request runtime validation은 `.github/workflows/rust-runtime-check.yml`에서 수행합니다.

주요 검증 항목:

- Windows / Linux / macOS whole-crate `cargo fmt --check`
- Windows / Linux / macOS Rust unit tests, including deterministic media-tool subprocess harness coverage
- Windows / Linux / macOS native compile check
- Windows / Linux / macOS strict Clippy (`-D warnings`)
- Windows Runtime contract guard
- Source archive release-metadata smoke test
- Windows portable package smoke test
- Windows portable package verification

Release workflow는 `.github/workflows/rust-runtime-release.yml`의 수동 `workflow_dispatch` 방식입니다.

Phase 20의 Windows/Linux/macOS GitHub-hosted CI baseline은 구성되어 있습니다. Linux/macOS는 CI 빌드·단위 테스트, Unix process-group ownership, native secret-storage 경계와 cross-platform tool-discovery/CLI 코드까지 검증하고 있으며 실제 로그인 세션·미디어 도구를 이용한 end-to-end integration은 아직 진행 중입니다.

## 문서

| 문서 | 내용 |
|---|---|
| `docs/PHASE21_NATIVE_PORTABLE.md` | Phase 21.8 portable 구조 및 Windows manual QA |
| `docs/PHASE23_1_NATIVE_UX_CLOSURE.md` | Phase 23.1 Windows Native daily-use UX closure 및 portable manual QA checklist |
| `docs/PHASE23_2_RUNTIME_PREFLIGHT.md` | Phase 23.2 shared Diagnostics / CLI runtime preflight contract |
| `docs/PHASE23_3_MEDIA_TOOL_HARNESS.md` | Phase 23.3 deterministic media-tool subprocess integration harness |
| `docs/PHASE21_NATIVE_UX_POLISH.md` | Phase 21.9 Native storage/backup IA/claim sidecar UX 및 manual QA |
| `docs/UNIX_CLI.md` | Linux/macOS headless CLI, tool discovery, first-run layout |
| `docs/OPERATIONS.md` | DB backup/restore, upgrade/rollback 절차 |
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

### Phase 20 🚧 Cross-platform Runtime Readiness

- ✅ Windows/Linux/macOS GitHub-hosted CI matrix baseline
- ✅ Unix process-group ownership / termination
- ✅ Linux Secret Service / macOS Keychain native secret-storage boundary
- ✅ Unix/headless CLI + cross-platform Streamlink/yt-dlp/FFmpeg discovery baseline
- 🚧 Linux/macOS CLI runtime/configuration commands 확장
- 🚧 Linux/macOS real-session / real-tool integration coverage
- 🚧 Unix packaging / install guidance

Phase 20에서는 cross-platform native picker나 Linux/macOS GUI launcher를 추가하지 않습니다. Unix 계열은 CLI/headless 경로를 명확히 하고, Windows GUI 교체는 Phase 21로 분리합니다.

### Phase 21 ✅ Slint Native GUI

- ✅ Settings / native picker / diagnostics
- ✅ Channels + LIVE watcher/recording
- ✅ SOOP/CHZZK VOD analyze/download
- ✅ VOD Queue + LIVE/VOD History
- ✅ Native Backup/Restore + Diagnostics/Runtime Logs
- ✅ Windows Native portable packaging / startup 전환
- ✅ Native UX polish: LIVE 저장공간, 설정 내부 일반/관리 정리 및 Backup 관리 통합, 내부 claim sidecar 노출 개선 — Phase 23.1에서 daily-use UX closure 완료
- Slint GUI는 shared Rust core를 직접 호출하며 localhost HTTP, direct SQLite, direct process control을 사용하지 않음
- ✅ Phase 22.3에서 legacy browser/Web UI, Axum presentation, Web launcher/fallback 제거
- Linux/macOS는 GUI를 복제하지 않고 Phase 20의 CLI/headless 인터페이스 유지

### Phase 22 ✅ Runtime / Legacy Architecture Closure

- ✅ legacy dependency / warning cleanup
- ✅ browser/Web presentation 및 Axum application layer 제거
- ✅ runtime/core compatibility boundary audit
- ✅ shared runtime source path를 `rust-runtime/`로 통일
- ✅ CI/release workflow 이름을 `rust-runtime-*`로 통일
- ✅ final legacy/compatibility audit 및 CHZZK transient runtime path 정리
- `stream-archive-server` package/headless binary 이름은 외부 호환성 경계로 의도적으로 유지

### Phase 23 🚧 Product Readiness / Integration

- ✅ 23.1 Native Daily-use UX Closure
- ✅ 23.2 Diagnostics / Runtime Preflight
- ⏳ 23.3 Media-tool Integration Harness
- ⏳ 23.4 Provider E2E Validation
- ⏳ 23.5 Unix CLI Completion
- ⏳ 23.6 Packaging / Release Readiness
- ⏳ 23.7 Release Candidate / Final QA

---

**Stream Archive**는 Phase 20에서 Windows 런타임의 안정성을 유지하면서 Linux/macOS headless 기반을 완성하고, Phase 21부터 Windows 사용자 경험을 Slint native GUI로 전환하는 것을 목표로 합니다.