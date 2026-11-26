# Stream Archive 1.2.0 릴리스 노트

상태: **무손실 자동 Remux · 외부 알림 웹훅(Discord/Telegram) · 다중 VOD URL 일괄 대기열 등록 및 내부 안정화 — 2026-11-26**

1.1.1 이후 LIVE 녹화 후처리 자동화, 원격 모니터링용 외부 알림 연동, 다중 VOD URL 일괄 등록 UX를 추가하고 테스트·런타임 내부 구조를 안정화한 마이너 기능 릴리스입니다.

## 주요 변경사항

- **LIVE 녹화 후 무손실 자동 Remux (`LIVE_AUTO_REMUX`, `LIVE_REMUX_DELETE_SOURCE`):**
  - **설정 → 일반**에서 LIVE 녹화 종료 후 `.ts` 파일을 FFmpeg 무손실 스트림 복사(`-map 0:v? -map 0:a? -c copy`, MP4의 경우 `-movflags +faststart`)로 `.mp4` 또는 `.mkv`로 자동 변환할 수 있습니다(기본값 `OFF`).
  - 백그라운드 소유 서브프로세스(`media_process`, 최대 동시 2건)로 실행되어 앱 종료 시 안전하게 중단·정리되며, 임시 스테이징 후 원자적 이름 변경(충돌 방지 `_02`~`_9999`)으로 저장됩니다.
  - 변환 완료 시 SQLite 기록(`live_recordings`)의 경로와 크기를 자동 갱신하며, `원본 .ts 자동 삭제` 옵션을 켠 경우에도 Remux 성공과 출력 파일 검증이 끝난 뒤에만 원본을 삭제합니다. 실패·취소 시 원본 `.ts`는 항상 보존됩니다.
- **외부 알림 웹훅 (Discord · Telegram):**
  - Windows GPUI와 Linux/macOS headless 공통 런타임에서 **LIVE 녹화 시작·종료**, **VOD 다운로드 완료·실패**, **채널 경고(디스크 부족·정체·인증·오류, 채널별 중복 억제)** 이벤트를 Discord Webhook 및 Telegram Bot API로 비동기 발송합니다.
  - **설정 → 일반**에서 전체 활성화(`WEBHOOK_ENABLED`)와 이벤트별 알림(`WEBHOOK_NOTIFY_LIVE`, `WEBHOOK_NOTIFY_VOD`, `WEBHOOK_NOTIFY_WARNING`)을 개별 제어할 수 있으며, `DISCORD_WEBHOOK_URL`과 `TELEGRAM_BOT_TOKEN`은 OS 비밀정보 저장소(Windows DPAPI / Linux Secret Service / macOS Keychain)에 암호화 저장되고 로그·오류 메시지에 노출되지 않습니다.
  - **`웹훅 테스트 발송`** 버튼과 **`웹훅 인증정보 삭제`** 버튼을 제공합니다.
- **다중 VOD URL 일괄 대기열 등록:**
  - **VOD** 탭에 접이식 **`다중 URL 일괄 등록`** 패널을 추가했습니다. SOOP · CHZZK · KICK VOD URL을 줄바꿈으로 여러 개 붙여넣으면 `#` 주석·공백·중복 URL을 정리하고 플랫폼별 요청 형식(SOOP 전체 PART, CHZZK/KICK PART 1, KICK 최고 화질 정규화)으로 자동 변환해 대기열에 일괄 등록합니다.
  - 잘못된 URL이 일부 섞여 있어도 나머지 정상 URL은 중단 없이 등록되며, 등록 건수·중복 제외·실패 사유 요약을 표시합니다.
- **유지보수 · 내부 안정화:**
  - macOS CI 대용량 manifest 테스트에서 발생하던 루프백 송신 데드락(`LocalManifestServer`)을 송신 타임아웃·청크 분할 전송 및 Windows `accept()` 일시 오류 복구로 해소했습니다.
  - 테스트 런타임 간 `reqwest::Client` 전역 커넥션 풀 충돌(`platform/soop/vod.rs`, `webhook_service.rs`) 및 로컬 `PATH` 도구 설치 여부에 따른 `tool_discovery` 테스트 환경 의존성을 제거했습니다.
  - 대형 런타임 핵심 파일 5개(`store.rs`, `app_core.rs`, `native_watcher.rs`, `platform/soop/vod.rs`, `platform/chzzk/vod.rs`)의 쿠키·파서·헬퍼·테스트 코드를 응집도 높은 서브모듈로 분리했습니다.

## 지원 범위

- Windows Native GPUI GUI 및 Linux/macOS CLI·headless
- SOOP 일반·구독·구독플러스 LIVE 및 VOD, CHZZK LIVE·VOD (H.264 / HEVC), KICK 공개 LIVE 및 공개·구독 VOD
- LIVE 무손실 자동 Remux, Discord/Telegram 외부 알림 웹훅, 다중 VOD URL 일괄 대기열 등록
- Queue / History, Backup / Restore, Diagnostics / Runtime Logs

## 패키지 및 외부 도구

| 플랫폼 | 배포 파일 |
|---|---|
| Windows x64 | `stream-archive-windows-x64.zip` |
| Linux x64 | `stream-archive-linux-x64.tar.gz` |
| macOS arm64 | `stream-archive-macos-arm64.tar.gz` |

`RELEASE_INFO.txt`, Windows EXE `ProductVersion`, Unix CLI version은 1.2.0이어야 합니다. Streamlink, yt-dlp, FFmpeg는 별도로 설치하며 공식 패키지에는 번들하지 않습니다.

## 업그레이드 안내 (upgrade)

1. 녹화·다운로드·Remux 작업을 정리하고 Backup을 생성하여 무결성을 확인합니다.
2. 앱을 정상 종료하고 이전 패키지와 백업을 보관합니다.
3. 1.2.0 archive를 새 디렉터리에 압축 해제하고 기존 `data` 경로를 유지해 실행한 뒤 설정·Channels·Queue·History를 확인합니다.
