# Stream Archive 1.1.1 릴리스 노트

상태: **CHZZK 2단계 경량 폴링·녹화 완료 알림·비동기 블로킹 I/O 분리 및 일상 UX 개선 — 2026-11-26**

1.1.0 이후 치지직(CHZZK) 감시 부하와 런타임 비동기 안정성을 보강하고, LIVE/대기열 바로 재생, 대기열 일괄 정리·재시도, 실전 런타임 로그 뷰어, 전역 단축키 및 고급 설정 UI를 추가한 안정화·사용성 릴리스입니다.

## 주요 변경사항

- **CHZZK LIVE 2단계 경량 폴링:**
  - 오프라인 채널 감시 시 먼저 경량 `polling/v2/channels/{channel_id}/live-status` 엔드포인트로 방송 상태만 확인하고, `OPEN` 상태이거나 폴링 API 응답이 비정상일 때만 `service/v2/channels/{channel_id}/live-detail`을 조회합니다. 다수 오프라인 채널을 감시할 때 네트워크 트래픽·JSON 파싱 비용과 429 위험을 크게 낮춥니다.
- **LIVE 녹화 완료 시스템 트레이 알림 (`GUI_NOTIFY_RECORD_FINISH`):**
  - **설정 → 일반**에 `LIVE 녹화 완료 알림` 스위치를 추가했습니다. 녹화가 정상 종료되면 경과 시간과 파일 크기를 포함한 트레이 알림을 표시하며, 오류/정체 전환 시에는 완료 알림 대신 경고 알림만 표시합니다.
- **블로킹 파일·SQLite I/O의 Tokio 워커 분리 (`spawn_blocking`):**
  - CHZZK VOD 최종 파일 이동/복사(`finalize_output` / `publish_by_copy`)와 SQLite 백업 생성·검증·복원·해시 계산(`BackupManager`)을 `tokio::task::spawn_blocking`으로 분리하여, 대용량 파일 이동이나 백업 중에도 LIVE 워처 폴링과 UI 응답이 지연되지 않습니다.
- **VOD 대기열 편의 기능 및 LIVE 녹화 중 바로 재생:**
  - **대기열(Queue)**에 완료 항목 `영상 재생`, `완료 항목 정리`(완료·취소 일괄 삭제), `실패 일괄 재시도`(실패·중단 일괄 재등록) 버튼을 추가했습니다.
  - **LIVE** 탭의 녹화 중 채널 행에도 `영상 재생` 버튼을 추가해 진행 중인 `.ts` 파일을 외부 플레이어에서 바로 타임시프트 시청할 수 있습니다.
- **실전형 런타임 로그 뷰어:**
  - **진단 → 런타임 로그**에 `Consolas` 고정폭 폰트와 `WARN`/`ERR` 색상 강조, 레벨 필터(`전체`/`경고·오류`/`오류만`), 키워드 검색, 표시 로그 일괄 클립보드 복사(`로그 복사`) 기능을 추가했습니다.
- **전역 키보드 단축키 및 접이식 고급 설정:**
  - `Ctrl+1~7`(탭 전환), `F5`/`Ctrl+R`(현재 탭 새로고침), `Ctrl+F`(현재 탭 검색창/URL 입력 포커스), `Esc`(모달·달력 닫기) 단축키를 지원합니다.
  - **설정 → 일반** 하단에 접이식 **고급 설정** 섹션을 추가해 `RECORD_STALL_TIMEOUT`, `RECORD_RETRY_INTERVAL`, `LOG_ENABLED`, `LOG_RETENTION_DAYS`를 UI에서 직접 조정할 수 있습니다.

## 지원 범위

- Windows Native GPUI GUI 및 Linux/macOS CLI·headless
- SOOP 일반·구독·구독플러스 LIVE 및 VOD, CHZZK LIVE·VOD (H.264 / HEVC), KICK 공개 LIVE 및 공개·구독 VOD
- Queue / History, Backup / Restore, Diagnostics / Runtime Logs

## 패키지 및 외부 도구

| 플랫폼 | 배포 파일 |
|---|---|
| Windows x64 | `stream-archive-windows-x64.zip` |
| Linux x64 | `stream-archive-linux-x64.tar.gz` |
| macOS arm64 | `stream-archive-macos-arm64.tar.gz` |

`RELEASE_INFO.txt`, Windows EXE `ProductVersion`, Unix CLI version은 1.1.1이어야 합니다. Streamlink, yt-dlp, FFmpeg는 별도로 설치하며 공식 패키지에는 번들하지 않습니다.

## 업그레이드 안내 (upgrade)

1. 녹화·다운로드를 정리하고 Backup을 생성하여 무결성을 확인합니다.
2. 앱을 정상 종료하고 이전 패키지와 백업을 보관합니다.
3. 1.1.1 archive를 새 디렉터리에 압축 해제하고 기존 `data` 경로를 유지해 실행한 뒤 설정·Channels·Queue·History를 확인합니다.
