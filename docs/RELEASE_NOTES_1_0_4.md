# Stream Archive 1.0.4 릴리스 노트

상태: **트레이/GPU 복구 안정화 및 메모리·성능 최적화 — 2026-11-19**

1.0.3 이후 Windows 트레이 대기 중 그래픽 드라이버 업데이트/리셋 시 앱이 종료되던 문제를 해결하고, 유휴 메모리·디스크·CPU 사용량 최적화, 런타임 프로세스·SQLite·로깅 동시성 보강, LIVE 필터 및 UI 상태 일관성 개선을 포함합니다.

## 주요 변경사항

- **Windows 트레이 GPU 드라이버 업데이트·리셋 복구 및 메모리 최적화:** 트레이 숨김 시 `SLINT_DESTROY_WINDOW_ON_HIDE=1`로 유휴 wgpu 장치·표면을 해제해 대기 메모리를 크게 절감하고, 드라이버 업데이트/TDR 이후 복원 시 자동 재시도 및 지연 복구 타이머로 녹화·감시를 중단 없이 유지합니다.
- **Win32 트레이 수명·알림·비차단 UI 개선:** 트레이 윈도우 `GWLP_USERDATA` 수명 관리와 복원 시 최소화 해제 순서를 보강하고, 다운로드 완료·실패 알림을 전용 비동기 리스너로 수신하도록 수정했으며, OS 파일/폴더 선택기와 탐색기 열기를 별도 스레드로 분리해 UI·워커 차단을 제거했습니다.
- **유휴 폴링 및 UI 상태 정합성 개선:** 창이 숨김·최소화·종료 중일 때 `LIVE`/`Storage`/`VOD`/`Queue`/`Logs` 폴링을 중단하고, 1초 주기 `VOD` 폴링의 불필요한 SQLite 쓰기를 제거했습니다. `LIVE` 필터링을 `LiveModel`로 이전해 빈 여백·카운트 불일치·`LOW_DISK`/`STALLED`/`AUTH` 라벨 표시를 수정하고, 설정 `ComboBox`·기록 달력 로컬 시간대·저장소 경로 표기·입력 검증을 보완했습니다.
- **런타임 프로세스·VOD·SQLite·로깅 안정화:** Streamlink 및 SOOP/CHZZK/KICK VOD 리더에 비UTF-8(CP949 등) 출력 내성과 UTF-8 CLI 환경을 적용하고, SOOP VOD `stdout` 종료 시 CPU 100% 스핀·지연 취소 덮어쓰기, Windows Job Object 종료 무한 대기, 감시자 `start`/`stop` 경합 및 초기 체크 집중·지수 백오프를 보완했습니다. 파일 로그 전용 백그라운드 스레드 + 20 MiB 로테이션, SQLite `busy_timeout(5s)` 및 `IMMEDIATE` 트랜잭션, 500건 초과 기록 검색, 백업 정리 복원력을 강화했습니다.

## 지원 범위

- Windows Native Slint GUI 및 Linux/macOS CLI·headless
- SOOP/CHZZK LIVE·VOD, KICK 공개 LIVE 및 공개·구독 VOD
- Queue / History, Backup / Restore, Diagnostics / Runtime Logs

## 패키지 및 외부 도구

| 플랫폼 | 배포 파일 |
|---|---|
| Windows x64 | `stream-archive-windows-x64.zip` |
| Linux x64 | `stream-archive-linux-x64.tar.gz` |
| macOS arm64 | `stream-archive-macos-arm64.tar.gz` |

`RELEASE_INFO.txt`, Windows EXE `ProductVersion`, Unix CLI version은 1.0.4여야 합니다. Streamlink, yt-dlp, FFmpeg는 별도로 설치하며 공식 패키지에 번들하지 않습니다.

## 업그레이드 안내 (upgrade)

1. 녹화·다운로드를 정리하고 Backup을 생성하여 무결성을 확인합니다.
2. 앱을 정상 종료하고 이전 패키지와 백업을 보관합니다.
3. 1.0.4 archive를 새 디렉터리에 압축 해제하고 기존 `data` 경로를 유지해 실행한 뒤 설정·Channels·Queue·History를 확인합니다.
