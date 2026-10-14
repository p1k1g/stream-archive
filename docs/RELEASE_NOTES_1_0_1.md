# Stream Archive 1.0.1 릴리스 노트

상태: **안정화 패치 준비 중 — 아직 공개하지 않음**

1.0.0 이후의 메모리·리소스 lifecycle 및 Windows tray 복구 보완을 포함하는 패치입니다. 제품과 패키지 버전을 1.0.1로 맞춥니다. 공개된 1.0.0 Release·tag·패키지·체크섬은 보존합니다.

## 주요 변경사항

- **27.2 background task 정리:** 자동 백업·VOD History 동기화의 중복 시작을 방지하고 core shutdown에서 stop·join합니다. 종료 대기 future가 취소되어도 JoinHandle을 보관합니다.
- **27.2 I/O와 로그 경계:** Unix control의 64KiB 제한을 buffer 확장 전에 적용하고 I/O timeout 및 shutdown을 처리합니다. 이미 수락한 stop/cancel 작업의 owned-process 정리는 완료까지 유지합니다.
- **27.2 외부 도구 출력:** 32KiB를 초과하는 줄은 안전하게 비우고 생략 안내로 대체합니다. LogBuffer는 기존 최대 400줄 외에 한 줄 8KiB·전체 문자열 내용 1MiB 제한을 적용합니다.
- **27.3 Windows tray 복구:** 연속 3회 등록 실패 때 숨겨진 창을 복원하고 redraw를 요청합니다. 자동 복구 안내는 확인을 기다리는 동기 팝업 대신 비차단 Slint banner로 표시합니다.

## 지원 범위

- Windows Native Slint GUI 및 Linux/macOS CLI·headless
- SOOP/CHZZK LIVE·VOD, KICK 공개 LIVE 및 공개·구독 VOD
- Queue / History, Backup / Restore, Diagnostics / Runtime Logs
- `StreamArchiveCore`와 shared Rust runtime, SQLite 및 기존 owned-process lifecycle

새로운 플랫폼이나 기능을 추가하지 않습니다. renderer/FemtoVG, allocator, DB schema 및 인증 처리의 contract를 유지합니다.

## 패키지 및 외부 도구

| 플랫폼 | 배포 파일 |
|---|---|
| Windows x64 | `stream-archive-windows-x64.zip` |
| Linux x64 | `stream-archive-linux-x64.tar.gz` |
| macOS arm64 | `stream-archive-macos-arm64.tar.gz` |

`RELEASE_INFO.txt`·Windows EXE `ProductVersion`·Unix CLI version은 1.0.1을 표시해야 합니다. 패키지 내부 `SHA256SUMS.txt`와 archive-level `.sha256`, 새 디렉터리 압축 해제 smoke 및 손상 archive 거부 검증을 유지합니다. runtime data·credentials/cookies·logs·backup DB·다운로드한 미디어·process-state 파일을 공식 패키지에 넣지 않습니다.

Streamlink, yt-dlp, FFmpeg는 별도로 설치하며 공식 패키지에 번들하지 않습니다. Windows는 Native-only 패키지이고 Linux/macOS의 CLI·headless surface를 유지합니다.

## 백업·보안 및 업그레이드 안내 (upgrade)

Windows DPAPI, Linux Secret Service, macOS Keychain의 기존 비밀정보 보호를 유지합니다. 인증정보를 평문 fallback으로 저장하지 않습니다.

1. 실행 중인 녹화·다운로드를 정리하고 Backup을 생성하여 무결성을 확인합니다.
2. 앱을 정상 종료하고 이전 패키지와 업그레이드 전 백업을 보관합니다.
3. 1.0.1 archive의 체크섬을 확인하고 별도 디렉터리에 풉니다.
4. 기존 canonical 데이터 디렉터리를 유지하여 실행합니다. 경로·설정·Channels·Queue·History와 비밀정보 조회를 확인합니다.
5. 다시 종료·실행하여 설정 유지와 Backup / Restore 동작을 확인합니다.

플랫폼별 경로와 offline 절차는 [운영 가이드](OPERATIONS.md)를 따릅니다. rollback은 새 런타임을 종료한 뒤 보관한 이전 패키지와 필요한 경우 검증된 업그레이드 전 백업을 사용합니다. 임의의 schema downgrade 호환성을 보장하지 않습니다.

## 검증 및 알려진 제한사항

27.2 PR #128과 27.3 PR #129의 Windows/Linux/macOS CI 및 windows-check는 통과했습니다. 1.0.1 버전 전환 자체는 해당 PR CI로 별도 검증하고, 공개 패키지는 merge 이후의 정확한 commit으로 release artifact workflow를 실행해 확인해야 합니다.

수동 Windows GUI/Explorer/tray/notification 및 실제 provider 인증·장시간 실행 검증은 자동 검증과 별개입니다. Phase 26 빌드에서 관찰된 약 5일 후 흰 화면과 Private Bytes 약 1.6GB의 원인 전체가 해결됐다고 주장하지 않습니다. 메모리 누수가 전혀 없다고 보장하지 않습니다. Authenticode 및 macOS signing/notarization은 적용하지 않습니다.

## 릴리스 상태

아직 v1.0.1 tag나 GitHub Release를 생성하지 않았습니다. 공개 절차와 미완료 수동 QA는 [1.0.1 공개 체크리스트](RELEASE_CHECKLIST_1_0_1.md)에서 관리합니다. 패키지 공개 전에 README의 기존 1.0.0 다운로드를 1.0.1로 바꾸지 않습니다.
