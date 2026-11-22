# Stream Archive 1.0.7 릴리스 노트

상태: **LIVE 감시 성능·안정성 강화 및 GPUI 데스크톱 UX 고도화 — 2026-11-22**

1.0.6의 순수 Rust GPUI 전환 이후 실제 장기 운영 로그와 사용 피드백을 바탕으로 Watcher 폴링 성능·오류 복원력과 Windows Native GPUI 데스크톱 사용성을 대폭 개선했습니다.

## 주요 변경사항

- **오류 채널 지수 백오프 및 중복 경고 로그 억제:** 반복 실패(HTTP 500·네트워크 타임아웃 등)하는 채널에 최대 15분(`900s`) 지수 백오프를 적용하고, 동일 오류 `[RUST:WARN]` 로그는 1회차·오류 변경·10회 주기 요약만 기록해 로그 파일 및 인앱 200줄 뷰어 도배를 차단했습니다. 채널 복구·재확인·재개 시 즉시 초기화됩니다.
- **제한 병렬 채널 폴링 (`MAX_CONCURRENT_PROBES = 6`) 및 즉시 중지 취소:** 다수 채널 상태 확인을 최대 6개 동시 실행하여 폴링 지연을 단축하고, Watcher 중지/종료 시 진행 중인 프로브를 `stop_rx`로 즉시 취소합니다. 동일 배치 내 `AuthRequired` 인증 복구도 플랫폼당 1회로 중복 호출을 방지합니다.
- **기록(History) `1000건` 상한 일치:** 백엔드 `HISTORY_MAX_LIMIT`을 `500`에서 `1000`으로 상향해 UI의 `100건 / 300건 / 1000건` 선택기와 정확히 일치시켰습니다.
- **창 위치·크기·최대화 영속화 및 GPUI 다크 종료 모달:** 창 위치·크기·최대화 상태를 canonical SQLite(`STREAM_ARCHIVE_WINDOW_BOUNDS`)에 저장·복원하며(보조 모니터 분리 시 주 모니터로 자동 클램핑), 기존 Win32 흰색 `MessageBoxW` 대신 GPUI 다크 종료 확인 모달로 통일했습니다.
- **LIVE·채널 관리 검색/정렬 및 녹화 지표·`방송 보기`:** LIVE 탭에 실시간 검색, `상태순`/`이름순` 정렬, 녹화 경과 시간(`HH:MM:SS`), 실시간 쓰기 속도(`MB/s`·`KB/s`), 안전한 allowlist `https://` `방송 보기` 버튼을 추가했습니다. 채널 관리 탭에도 플랫폼 필터(`전체/SOOP/CHZZK/KICK`)와 실시간 검색창을 추가했습니다.
- **트레이 `LIVE 감시 시작/중지` 및 확장 트레이 알림:** 트레이 우클릭 메뉴에서 바로 LIVE 감시를 시작·중지할 수 있으며, 설정 > 일반에 `LIVE 녹화 시작 트레이 알림`(`GUI_NOTIFY_RECORD_START`)과 `저장 공간 부족·채널 오류 경고 알림`(`GUI_NOTIFY_WARNING`) 스위치를 추가했습니다.

## 지원 범위

- Windows Native GPUI GUI 및 Linux/macOS CLI·headless
- SOOP/CHZZK LIVE·VOD, KICK 공개 LIVE 및 공개·구독 VOD
- Queue / History, Backup / Restore, Diagnostics / Runtime Logs

## 패키지 및 외부 도구

| 플랫폼 | 배포 파일 |
|---|---|
| Windows x64 | `stream-archive-windows-x64.zip` |
| Linux x64 | `stream-archive-linux-x64.tar.gz` |
| macOS arm64 | `stream-archive-macos-arm64.tar.gz` |

`RELEASE_INFO.txt`, Windows EXE `ProductVersion`, Unix CLI version은 1.0.7여야 합니다. Streamlink, yt-dlp, FFmpeg는 별도로 설치하며 공식 패키지에 번들하지 않습니다.

## 업그레이드 안내 (upgrade)

1. 녹화·다운로드를 정리하고 Backup을 생성하여 무결성을 확인합니다.
2. 앱을 정상 종료하고 이전 패키지와 백업을 보관합니다.
3. 1.0.7 archive를 새 디렉터리에 압축 해제하고 기존 `data` 경로를 유지해 실행한 뒤 설정·Channels·Queue·History를 확인합니다.
