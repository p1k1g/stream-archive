# Stream Archive 1.0.9 릴리스 노트

상태: **SOOP 구독·구독플러스 전용 LIVE 녹화 지원 — 2026-11-22**

SOOP에서 구독 전용(`P_MIN_TIER=1`) 및 구독플러스 전용(`P_MIN_TIER=2`) 방송을 구독 중인 계정으로 녹화할 때 발생하던 `400 Bad Request` / `401 Unauthorized` / `403 Forbidden` 오류를 해결했습니다.

## 주요 변경사항

- **SOOP 구독·구독플러스 전용 CDN 자동 선택:** `player_live_api.php`의 `P_MIN_TIER` / `TIER_TYPE` / `CDN` 필드를 감지해 일반 CDN 대신 구독 전용 CDN(`lg_cdn_pc_web_subscribe`, `gcp_cdn_subscribe`, `azure_cdn_subscribe`, `aws_cf_subscribe`, `gs_cdn_pc_web_subscribe`)과 Worker 해외 `aid`를 조합해 1080p 원본 스트림을 자동 선택합니다. 미구독 계정으로 구독 전용 방송을 감지한 경우(`RESULT=-14`) 명확한 안내 메시지를 표시합니다.
- **서명 쿠키 자동 갱신 로컬 HLS 프록시 (`SoopSignedHlsProxy`):** SOOP 구독 방송에 필요한 `private_auth.php`(`type=subs_live`) 서명 쿠키는 90초마다 만료(`Max-Age=90`)되므로, `127.0.0.1` 루프백 전용 프록시가 45초 주기 자동 갱신 및 `401/403` 즉시 재발급·재시도를 수행해 장시간 녹화가 끊김 없이 이어지도록 구현했습니다. 녹화 종료 시 포트와 갱신 태스크를 즉시 회수하며 `aid`와 쿠키는 디스크나 Streamlink 인자에 노출하지 않습니다.
- **Cloudflare Worker 하위 호환 및 업데이트 (`backend/worker.js`):** Worker에도 `subscribe` 요청 시 `*_subscribe` CDN 및 `live-sub-global-cdn` 호스트 지원을 추가했습니다. 기존에 배포한 Worker를 즉시 재배포하지 않아도 앱이 로컬에서 구독 CDN과 서명 쿠키를 자동 처리합니다.

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

`RELEASE_INFO.txt`, Windows EXE `ProductVersion`, Unix CLI version은 1.0.9여야 합니다. Streamlink, yt-dlp, FFmpeg는 별도로 설치하며 공식 패키지에 번들하지 않습니다.

## 업그레이드 안내 (upgrade)

1. 녹화·다운로드를 정리하고 Backup을 생성하여 무결성을 확인합니다.
2. 앱을 정상 종료하고 이전 패키지와 백업을 보관합니다.
3. 1.0.9 archive를 새 디렉터리에 압축 해제하고 기존 `data` 경로를 유지해 실행한 뒤 설정·Channels·Queue·History를 확인합니다.
