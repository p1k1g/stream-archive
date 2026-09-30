# Stream Archive 1.0.0 릴리스 노트

상태: **첫 공개 안정 버전 준비 중 — 아직 공개하지 않음**

Stream Archive 1.0.0은 첫 공개 안정 버전으로 준비하고 있습니다. 버전과 릴리스 문서 정리가 완료되어도 공개 승인을 뜻하지는 않습니다. 최종 merge, `v1.0.0` tag 생성과 GitHub Release 공개는 운영자가 결정합니다.

## 주요 변경사항

Rust + SQLite 기반 런타임을 중심으로 Windows Slint Native GUI와 Linux/macOS CLI·headless 실행 환경을 제공합니다. 두 환경은 같은 `StreamArchiveCore`, SQLite 저장소, 서비스별 처리 로직, Queue, History, Backup, 저장공간 진단 및 애플리케이션이 소유한 프로세스의 수명주기 관리를 사용합니다.

Phase 23.10에서는 Windows 아이콘, LIVE 화면의 개발용 부제 제거, 녹화 중 `저장 폴더 열기`, FemtoVG 및 software renderer fallback을 반영했습니다. Phase 23.11에서는 실제 Windows RC에서 확인한 아이콘 잘림 문제를 수정했습니다. Phase 23.12에서는 제품 버전과 릴리스 문서를 1.0.0으로 정리하고 패키지 버전 검증을 강화했습니다.

## 지원 범위

- Windows Slint Native GUI
- Linux/macOS CLI 및 headless 런타임
- SOOP LIVE / SOOP VOD
- CHZZK LIVE / CHZZK VOD
- Queue / History
- Backup / Restore
- Diagnostics / Runtime Logs
- OS별 비밀정보 보호

인증정보 없이 실행하는 CI는 오프라인 서비스·미디어 도구 fixture로 명령 구성, 결과 매핑, timeout, 취소 및 소유한 하위 프로세스 정리를 검증합니다. 실제 서비스와 로그인 세션 검증은 공개 전 수동 QA로 남아 있습니다.

## Windows Native

Windows portable 패키지는 다음을 포함합니다.

- Native GUI 실행 파일 `StreamArchive.exe`
- Native 실행용 `RUN.bat`
- 프로그램 종료 후 사용하는 오프라인 백업·복구 PowerShell 스크립트
- 릴리스 metadata와 SHA-256 체크섬 목록

Windows 공식 패키지에는 `stream-archive-server.exe`, `RUN_HEADLESS.bat`, browser/Web launcher 및 reverse-proxy 구성물이 포함되지 않습니다. Linux/macOS의 CLI·headless 실행 경로는 유지합니다.

Phase 23.11 아이콘 수정은 `rust-gui/assets/stream-archive-icon.png`를 기준으로 합니다. `build.rs`에서 16/24/32/48/64/128/256 크기의 ICO frame을 만들고 `winresource`를 통해 PE resource에 넣습니다. 패키지 검증기는 모든 예상 `RT_GROUP_ICON` / `RT_ICON` frame을 직접 디코딩합니다. 기존 PC의 아이콘 cache와 최종 배포 파일의 수동 아이콘 검증은 별도로 확인해야 합니다.

## Linux / macOS

다음 실행 파일을 제공합니다.

- `bin/stream-archive-cli`
- `bin/stream-archive-server`

CLI에서 초기화, 상태, 설정, 서비스 준비 상태, 미디어 도구, doctor, Channels, watcher, VOD, Queue, History, Backup, 저장공간, 로그 및 foreground serve를 사용할 수 있습니다.

CI에서 검증하는 파일은 `stream-archive-linux-x64.tar.gz`와 `stream-archive-macos-arm64.tar.gz`입니다. 검증하지 않은 아키텍처의 지원은 주장하지 않습니다.

## 패키지 및 체크섬

| 플랫폼 | 배포 파일 |
|---|---|
| Windows x64 | `stream-archive-windows-x64.zip` |
| Linux x64 | `stream-archive-linux-x64.tar.gz` |
| macOS arm64 | `stream-archive-macos-arm64.tar.gz` |

모든 공식 패키지에는 `RELEASE_INFO.txt`와 패키지 내부 `SHA256SUMS.txt`가 들어갑니다. 최종 압축 파일에는 별도의 archive-level `.sha256` 파일이 함께 제공됩니다. 제품 및 패키지 metadata 버전은 1.0.0으로 검증하며, Windows EXE의 `ProductVersion`도 확인합니다.

공식 패키지의 런타임 `data/` 디렉터리는 비어 있어야 합니다. SQLite DB/WAL/SHM, 로그, 인증정보·cookies, 백업 DB, 다운로드한 미디어, process-state 파일은 배포 대상이 아닙니다. 손상된 압축 파일은 검증 단계에서 반드시 거부해야 합니다.

## 백업 및 복구

공유 `BackupManager`가 SQLite 백업·복구와 보관 정책을 관리합니다. Windows 패키지에는 Stream Archive를 종료한 상태에서 사용하는 오프라인 백업·복구 스크립트도 포함합니다.

자동 검증은 관리형 백업의 생성·복구, 런타임 소유자 및 복구 안전 조건, Windows 오프라인 백업 metadata·hash 검증, 손상된 백업 복구 거부를 확인합니다. 실제 사용자 데이터와 Native GUI 조작을 통한 최종 확인은 수동 QA가 필요합니다.

## 진단 및 외부 미디어 도구

Streamlink, yt-dlp, FFmpeg는 **별도로 설치하는 외부 의존성**이며 공식 압축 파일에 번들하지 않습니다.

도구 탐색은 저장된 설정, 애플리케이션/backend 배치 구조, `PATH` 및 지원하는 일반 설치 경로를 사용합니다. Windows Native Diagnostics를 새로고침하면 로컬 탐색과 버전 확인을 다시 수행합니다. 실행 후 사용자가 지원하는 도구 배치 경로에 yt-dlp 또는 FFmpeg를 추가한 경우 재시작 없이 탐지할 수 있습니다. 이 탐색 기능은 공식 패키지에 도구를 번들한다는 의미가 아닙니다. Linux/macOS에서는 `tools`와 `doctor --active-tools`를 제공합니다.

## 보안 및 비밀정보 저장

비밀정보를 평문 fallback으로 저장하지 않습니다.

| 플랫폼 | 보호 방식 |
|---|---|
| Windows | CurrentUser DPAPI |
| Linux | `secret-tool`을 통한 Secret Service |
| macOS | Keychain Services |

Linux의 비밀정보 저장에는 사용 가능한 Secret Service 세션이 필요합니다. 실제 서비스 인증정보는 CI에 주입하지 않습니다. 비밀정보 저장·조회, 재시작 후 유지 및 실패 시 안전하게 거부하는 동작은 해당 OS 환경에서 수동 검증해야 합니다.

## 업그레이드 안내

1. 실행 중인 런타임을 정상 종료합니다.
2. 백업을 만들고 무결성을 확인합니다.
3. rollback에 사용할 이전 패키지를 보관합니다.
4. 새 압축 파일의 체크섬을 확인하고 별도 디렉터리에 풉니다.
5. 기존의 기준 데이터 디렉터리를 유지합니다.
6. 새 패키지를 실행해 설정, Channels, History, Queue와 Diagnostics를 확인합니다.
7. 다시 종료·실행해 데이터가 유지되는지 확인합니다.

Unix 자동 RC 검증은 데이터를 보존한 상태의 패키지 교체를 확인합니다. 서로 다른 버전 간 업그레이드(cross-version upgrade)는 대표적인 기존 데이터로 수동 검증해야 합니다.

rollback 시 새 런타임을 종료하고 이전 패키지로 돌아갑니다. **임의의 schema downgrade 호환성을 보장하지 않습니다.** 실제 DB 상태에서 필요한 경우에만 검증한 업그레이드 전 백업을 복구합니다. 자세한 플랫폼별 절차는 [운영 가이드](OPERATIONS.md)를 참고하세요.

## 알려진 제한사항

- Windows 파일은 Authenticode 서명되어 있지 않습니다.
- macOS 파일은 code signing 및 notarization을 적용하지 않았습니다.
- MSI, deb/rpm, Homebrew, Snap/Flatpak/AppImage 패키지는 제공하지 않습니다.
- systemd/launchd 설치 기능은 제공하지 않습니다.
- 실제 SOOP/CHZZK 세션은 최종 수동 QA가 필요합니다.
- Windows Native GUI 조작은 최종 수동 QA가 필요합니다.

## 릴리스 상태

자동 검증 범위는 Windows/Linux/macOS의 unit·integration fixture, 런타임 contract, Native compile, 패키지 체크섬, 새 디렉터리 압축 해제 smoke, 오프라인 백업·복구, 런타임 데이터 유출 거부 및 손상된 압축 파일 거부입니다. 1.0.0 revision의 근거는 해당 commit의 PR CI 결과이며, 이전 RC의 성공만으로 새 revision 검증을 대신하지 않습니다.

[1.0.0 릴리스 마무리 문서](PHASE23_12_1_0_0_RELEASE_CLOSURE.md)와 [최종 수동 검증 체크리스트](MANUAL_RC_1_0_0.md)에 남은 작업을 기록합니다. 실제 서비스 세션, GUI 조작, 대표 데이터의 업그레이드·rollback, OS별 비밀정보 저장 및 공개할 정확한 artifact 점검은 수동 검증 항목입니다. 수행하지 않은 수동 QA는 완료로 표시하지 않습니다. 공개 승인, `v1.0.0` 및 GitHub Release 공개는 운영자가 결정합니다.
