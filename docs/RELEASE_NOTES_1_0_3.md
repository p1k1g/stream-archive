# Stream Archive 1.0.3 릴리스 노트

상태: **안정화 및 UI/로깅 개선 — 2026-11-13**

1.0.2 이후 Windows Native Slint GUI의 wgpu 렌더러 전환 및 트레이 복원 안정화, 전체 탭 UI 개선, 오프라인 채널 저장 폴더 열기, 영구 파일 로깅(`.\log\runtime-YYYY-MM-DD.log`) 및 로그 관리 기능을 포함합니다.

## 주요 변경사항

- **wgpu 렌더러 전환 및 트레이 복원 안정화:** OpenGL 대신 `renderer-femtovg-wgpu`(+ `renderer-software` 폴백)를 사용하고, 트레이 복원 시 표시·최소화 해제 순서와 비동기 폴링 경합을 보완했습니다.
- **전체 탭 UI 개선:** `LIVE`, `채널`, `VOD 다운로드`, `대기열`, `기록`, `설정`, `진단` 화면의 정보 밀도와 정렬을 개선했습니다.
- **오프라인 채널 저장 폴더 열기:** `LIVE` 탭에서 오프라인 채널이라도 기존 저장 경로가 존재하면 `저장 폴더 열기` 버튼을 표시합니다.
- **영구 파일 로깅 및 로그 관리:** `.\log\runtime-YYYY-MM-DD.log` 및 `.\log\panic.log`에 로그를 기록하고 Streamlink `stdout`/`stderr`를 모두 수집하며, `진단 → 로그` 탭에 `로그 폴더 열기`와 `로그 내용 삭제` 버튼을 추가했습니다.

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

`RELEASE_INFO.txt`, Windows EXE `ProductVersion`, Unix CLI version은 1.0.3여야 합니다. Streamlink, yt-dlp, FFmpeg는 별도로 설치하며 공식 패키지에 번들하지 않습니다.

## 업그레이드 안내 (upgrade)

1. 녹화·다운로드를 정리하고 Backup을 생성하여 무결성을 확인합니다.
2. 앱을 정상 종료하고 이전 패키지와 백업을 보관합니다.
3. 1.0.3 archive를 새 디렉터리에 압축 해제하고 기존 `data` 경로를 유지해 실행한 뒤 설정·Channels·Queue·History를 확인합니다.
