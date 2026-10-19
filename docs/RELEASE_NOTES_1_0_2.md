# Stream Archive 1.0.2 릴리스 노트

상태: **안정화 패치 공개 준비 중**

1.0.1 이후 Phase 27.4의 LIVE 화면 model 수명, 이전 썸네일 정리 및 재표시 조회 순서 보완을 포함합니다. 제품·패키지 버전을 1.0.2로 맞추며 기존 1.0.0/1.0.1 Release·tag·패키지·체크섬을 보존합니다.

## 주요 변경사항

- **숨긴 LIVE 화면의 갱신 제한:** 트레이 또는 다른 탭에서는 중간 snapshot을 쌓지 않고 최신 하나만 보관합니다. model 행을 한 번 비워 이미지 참조를 해제하며, 녹화·감시와 필요한 썸네일 무효화 추적은 계속합니다.
- **표시 중 model 재사용:** 같은 VecModel에서 변경된 행만 갱신하고 채널 순서·개수가 바뀔 때만 전체 행을 교체합니다. Slint의 빈 Image 비교도 구분합니다.
- **이전 방송 썸네일 정리:** 녹화 완료 또는 비녹화 채널의 조회·인증 오류에서 이전 이미지를 무효화합니다. 늦은 응답으로 이전 썸네일이 되살아나지 않게 하며, 실제 recorder 소유 상태의 일시적인 조회 오류만으로 녹화를 중단하지 않습니다.
- **LIVE·저장공간 재표시:** 표시 세대 값으로 이전 요청의 응답을 구분합니다. 저장공간 조회가 이미 진행 중이어도 완료 직후 새 조회를 보내며, 새 데이터 확인 전에는 조회 중 상태를 표시합니다.
- **선택적 lifecycle 계측:** `STREAM_ARCHIVE_RESOURCE_TRACE=1`로 숫자·고정 event만 기록합니다. 주기 표본 병합, lifecycle 우선 보관, 64개 queue, 약 4MiB 회전 파일 및 유실 표시 `trace_gap`을 적용합니다. 기본값은 꺼짐입니다.

설정·사용법과 비교 측정 절차는 [Phase 27.4 문서](PHASE27_4_LIVE_RESOURCE_LIFECYCLE.md)를 참고하세요. 계측 로그는 메모리 Working Set/Private Bytes를 대신 측정하지 않습니다.

## 지원 범위

- Windows Native Slint GUI 및 Linux/macOS CLI·headless
- SOOP/CHZZK LIVE·VOD, KICK 공개 LIVE 및 공개·구독 VOD
- Queue / History, Backup / Restore, Diagnostics / Runtime Logs
- StreamArchiveCore·shared Rust runtime·SQLite 및 기존 owned-process lifecycle

새 기능·플랫폼을 추가하지 않습니다. renderer/FemtoVG, allocator, DB schema 및 미디어 도구·인증 처리 contract를 유지합니다.

## 패키지 및 체크섬

| 플랫폼 | 배포 파일 |
|---|---|
| Windows x64 | `stream-archive-windows-x64.zip` |
| Linux x64 | `stream-archive-linux-x64.tar.gz` |
| macOS arm64 | `stream-archive-macos-arm64.tar.gz` |

`RELEASE_INFO.txt`, Windows EXE `ProductVersion`, Unix CLI version은 1.0.2여야 합니다. package-local `SHA256SUMS.txt`, archive-level `.sha256`, 새 압축 해제 smoke 및 손상 archive 거부 검증을 유지합니다. 공식 패키지에는 runtime data·credentials/cookies·logs·backup DB·다운로드한 미디어·process-state 파일을 넣지 않습니다.

Streamlink, yt-dlp, FFmpeg는 별도로 설치하며 공식 패키지에 번들하지 않습니다. Windows는 Native-only 패키지이며 Linux/macOS CLI·headless를 유지합니다.

## 백업·보안 및 업그레이드 안내 (upgrade)

Windows DPAPI, Linux Secret Service, macOS Keychain의 기존 보호 방식을 유지하며 평문 비밀정보 fallback을 추가하지 않습니다.

1. 녹화·다운로드를 정리하고 Backup을 생성하여 무결성을 확인합니다.
2. 앱을 정상 종료하고 이전 패키지와 업그레이드 전 백업을 보관합니다.
3. 1.0.2 archive의 체크섬을 확인하고 새 디렉터리에 압축을 풉니다.
4. 기존 canonical 데이터 경로를 유지하여 실행하고 설정·Channels·Queue·History·인증정보 조회를 확인합니다. 새 디렉터리의 기본 `data`가 기존 데이터 경로와 다를 수 있으므로 [운영 가이드](OPERATIONS.md)의 패키지 교체 절차를 따릅니다.
5. 재실행 후 설정 유지와 Backup / Restore를 확인합니다.

rollback은 새 런타임을 종료한 뒤 이전 패키지를 사용하고, 필요한 경우 검증된 업그레이드 전 백업을 복구합니다. 임의 schema downgrade 호환성을 보장하지 않습니다.

## 검증 및 알려진 제한사항

Phase 27.4 [PR #132](https://github.com/p1k1g/stream-archive/pull/132)의 [최종 CI](https://github.com/p1k1g/stream-archive/actions/runs/39254608178)에서 Windows/Linux/macOS core-check와 windows-check가 통과했고 Windows GUI 테스트 69개가 성공했습니다. Codex 지적 3건을 수정한 최종 commit 재검토는 추가 지적 없이 완료됐습니다. 1.0.2 버전 전환 CI와 최종 공개 artifact 검증 결과는 [공개 체크리스트](RELEASE_CHECKLIST_1_0_2.md)에 별도로 기록합니다.

실제 provider 인증, Windows GUI/tray/notification 조작, OS별 secret store 및 며칠간의 메모리 비교는 자동 CI와 별개이며 미수행 항목을 완료로 표시하지 않습니다. 1.0.1 장시간 실행에서 관찰된 Private Bytes 증가 원인 전체가 해결됐다고 단정하지 않으며 메모리 누수가 전혀 없다고 보장하지 않습니다. Authenticode 및 macOS signing/notarization은 적용하지 않습니다.

## 릴리스 상태

공개 준비 중입니다. 최종 merge commit에서 canonical release artifact workflow를 실행하고 세 플랫폼 archive와 `.sha256`을 검증한 뒤 v1.0.2 tag 및 GitHub Release로 공개합니다. 공개 확인 전에는 README의 1.0.1 다운로드 링크를 유지합니다. 공개는 미완료 수동 QA의 성공을 뜻하지 않습니다.
