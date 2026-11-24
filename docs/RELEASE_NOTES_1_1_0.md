# Stream Archive 1.1.0 릴리스 노트

상태: **자동 실행·파일명 패턴 UI·기록 바로 재생·업데이트 확인 및 내부 최적화 — 2026-11-24**

1.0.9 이후 무인 자동 녹화 워크플로를 완성하는 데스크톱 자동화 기능, LIVE 파일명 패턴 UI 선택, 기록/VOD 바로 재생, GitHub 최신 릴리스 확인 기능과 함께 런타임 캐싱·HTTP 응답 크기 제한·GUI 모듈 분할을 포함합니다.

## 주요 변경사항

- **무인 자동화(앱 시작 시 감시 자동 시작 · 트레이 최소화 시작 · Windows 시작프로그램 등록):**
  - **설정 → 일반**에서 `앱 실행 시 LIVE 감시 자동 시작`(`GUI_AUTO_START_WATCHER`), `앱 실행 시 시스템 트레이로 최소화 시작`(`GUI_START_MINIMIZED_TRAY`), 그리고 Windows 현재 사용자 시작프로그램(`HKCU Run`) 즉시 등록/해제 스위치를 제공합니다.
- **LIVE 녹화 파일명 패턴 UI 선택 (`FILE_NAME_PATTERN`):**
  - 런타임에 내장되어 있던 `LEGACY`, `TIME_TITLE`, `BJ_TITLE`, `TITLE_NUMBER` 4종 파일명 형식을 **설정 → 일반**에서 예시 파일명과 함께 직접 선택·저장할 수 있습니다.
- **기록 · VOD 결과 파일 바로 재생 (`영상 재생`):**
  - **기록(History)** 및 **VOD** 화면에 허용된 미디어 파일(`.ts`, `.mp4`, `.mkv`, `.m4v`, `.webm`, `.flv`, `.mov`)을 OS 기본 플레이어로 즉시 여는 `영상 재생` 버튼을 추가했습니다.
- **GitHub Releases 최신 버전 확인:**
  - 앱 실행 시 비차단 백그라운드 확인과 **진단 → 진단 점검**의 `업데이트 확인` 버튼으로 새 릴리스 공개 여부를 확인하고 GitHub 릴리스 페이지를 바로 열 수 있습니다(자동 강제 다운로드/설치는 수행하지 않음).
- **런타임 최적화 및 GUI 모듈 분할:**
  - `ChzzkAuth::load()` 메모리 캐시(`PRAGMA data_version` 변경 감지 포함), 공용 `OnceLock<reqwest::Client>` / `LazyLock<Regex>` 재사용, SOOP·CHZZK HTTP 응답 크기 상한(`read_bounded_*`) 통일, GPUI `controller/`·`core_worker/` 도메인별 서브모듈 분할을 완료했습니다.

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

`RELEASE_INFO.txt`, Windows EXE `ProductVersion`, Unix CLI version은 1.1.0이어야 합니다. Streamlink, yt-dlp, FFmpeg는 별도로 설치하며 공식 패키지에는 번들하지 않습니다.

## 업그레이드 안내 (upgrade)

1. 녹화·다운로드를 정리하고 Backup을 생성하여 무결성을 확인합니다.
2. 앱을 정상 종료하고 이전 패키지와 백업을 보관합니다.
3. 1.1.0 archive를 새 디렉터리에 압축 해제하고 기존 `data` 경로를 유지해 실행한 뒤 설정·Channels·Queue·History를 확인합니다.
